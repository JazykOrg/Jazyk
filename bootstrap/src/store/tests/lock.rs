// The store lock, the commit marker, and readers waiting on a commit.
use super::*;

// A status write outside a changeset waits for the lock and is refused like a
// commit when another writer landed first. Mirrors docs/compiler/graph.md#concurrency.
#[test]
fn a_status_write_takes_the_lock_and_is_refused_when_the_store_moved() {
    let out = own_dir("status-write");
    let mut a = Store {
        out: out.clone(),
        ..Default::default()
    };
    assert_eq!(a.apply(vec![create("ent:a", "A")], &session()).applied, 1);
    let held = FileLock::acquire(&out);
    let mut fresh = Store::load(&out);
    fresh.status.roots = vec!["main.md".into()];
    let writer = std::thread::spawn(move || fresh.write_status("roots"));
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(
        !writer.is_finished(),
        "a status write ran under a held lock"
    );
    drop(held);
    assert!(writer.join().unwrap().is_ok());
    assert_eq!(Store::load(&out).status.roots, vec!["main.md".to_string()]);
    let mut b = Store::load(&out);
    assert_eq!(b.apply(vec![create("ent:b", "B")], &session()).applied, 1);
    a.status.roots = vec!["stale.md".into()];
    let why = a.write_status("roots").unwrap_err();
    assert!(why.starts_with("store-moved:"), "{}", why);
    let disk = Store::load(&out);
    assert_eq!(disk.status.roots, vec!["main.md".to_string()]);
    assert_eq!(disk.status.generation, 2);
}

// A reader never accepts a store a commit is rewriting: it waits while the marker
// stands under a held lock and reads the finished commit. A marker no writer holds
// is a leftover and is read past at once. Mirrors docs/compiler/graph.md#concurrency.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_reader_waits_for_a_commit_in_progress() {
    let out = own_dir("reader-wait");
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    assert_eq!(s.apply(vec![create("ent:a", "A")], &session()).applied, 1);
    // A writer mid-commit: the lock held, the marker down, one node file rewritten,
    // the commit point not yet moved.
    let held = FileLock::acquire(&out);
    crate::vfs::write(crate::layout::committing_file(&out), "1").unwrap();
    let mut next = Store::load(&out);
    next.graph
        .entities
        .insert("ent:b".into(), next.graph.entities["ent:a"].clone());
    next.status.generation = 2;
    next.save_nodes();
    let reader = {
        let out = out.clone();
        std::thread::spawn(move || Store::load(&out))
    };
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(
        !reader.is_finished(),
        "a reader accepted a half-committed store"
    );
    next.save_status_files();
    crate::vfs::remove_file(crate::layout::committing_file(&out)).unwrap();
    drop(held);
    let read = reader.join().unwrap();
    assert!(read.load_errors.is_empty(), "{:?}", read.load_errors);
    assert_eq!(read.status.generation, 2);
    assert!(read.graph.entities.contains_key("ent:b"));
    // A marker left by a writer that stopped: no lock is held, nothing waits.
    crate::vfs::write(crate::layout::committing_file(&out), "1").unwrap();
    let started = std::time::Instant::now();
    let read = Store::load(&out);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert!(read.load_errors.is_empty(), "{:?}", read.load_errors);
    // The next commit replaces the leftover and removes it.
    let mut w = Store::load(&out);
    assert_eq!(w.apply(vec![create("ent:c", "C")], &session()).applied, 1);
    assert!(!crate::layout::committing_file(&out).vexists());
}

// A store that never holds still within the wait loads read-only with `store-busy`
// instead of a mix of two commits. Mirrors docs/compiler/graph.md#concurrency.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_reader_that_never_reads_a_still_store_reports_store_busy() {
    let out = own_dir("reader-busy");
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    assert_eq!(s.apply(vec![create("ent:a", "A")], &session()).applied, 1);
    let held = FileLock::acquire(&out);
    crate::vfs::write(crate::layout::committing_file(&out), "1").unwrap();
    let reader = {
        let out = out.clone();
        std::thread::spawn(move || {
            READ_WAIT_OVERRIDE.with(|o| o.set(Some(std::time::Duration::from_millis(200))));
            Store::load(&out)
        })
    };
    let mut busy = reader.join().unwrap();
    drop(held);
    assert!(
        busy.load_errors[0].starts_with("store-busy:"),
        "{:?}",
        busy.load_errors
    );
    let r = busy.apply(vec![create("ent:b", "B")], &session());
    assert_eq!(r.applied, 0);
    assert!(r.skipped[0].starts_with("store-busy:"), "{:?}", r.skipped);
    crate::vfs::remove_file(crate::layout::committing_file(&out)).unwrap();
}

// Every committed file reaches the disk before its rename: no temporary file is
// left beside it, and the directories the renames landed in are flushed.
#[test]
fn a_commit_leaves_no_temporary_files_and_no_marker() {
    let out = own_dir("durable");
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    assert_eq!(s.apply(vec![create("ent:a", "A")], &session()).applied, 1);
    let mut files = Vec::new();
    fn walk(d: &Path, files: &mut Vec<String>) {
        for e in crate::vfs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.vis_dir() {
                walk(&p, files);
            } else {
                files.push(e.file_name().to_string_lossy().to_string());
            }
        }
    }
    walk(&out, &mut files);
    assert!(
        !files
            .iter()
            .any(|f| f.ends_with(".tmp") || f == COMMIT_MARKER),
        "{:?}",
        files
    );
    assert!(
        TOUCHED_DIRS.with(|t| t.borrow().is_empty()),
        "every touched directory was flushed"
    );
}

// An in-memory store's marker lock: a stale marker is taken over at once, a nested
// acquire never removes its holder's marker, and the holder's release does.
// Mirrors docs/compiler/graph.md#concurrency.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_scoped_lock_takes_over_a_stale_marker_and_keeps_a_live_one() {
    let mut files = BTreeMap::new();
    files.insert(".jazyk/ds/.lock".to_string(), b"999".to_vec());
    let _scope = crate::vfs::Scope::enter(&files);
    let out = PathBuf::from("/project/.jazyk/ds");
    let marker = crate::layout::lock_file(&out);
    let started = std::time::Instant::now();
    let outer = FileLock::acquire(&out);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "a stale marker is not waited on"
    );
    assert!(marker.vexists());
    drop(FileLock::acquire(&out));
    assert!(
        marker.vexists(),
        "a nested release removed the holder's marker"
    );
    drop(outer);
    assert!(!marker.vexists());
    drop(FileLock::acquire(&out));
    assert!(!marker.vexists());
}

// A spend stamp rewrites a committed journal entry: it waits for the store lock
// a writer holds and lands through the atomic write.
// Mirrors docs/compiler/graph.md#concurrency.
#[test]
fn spend_stamps_wait_for_the_store_lock() {
    let out = own_dir("stamp-lock");
    crate::vfs::create_dir_all(crate::layout::journal_dir(&out)).unwrap();
    for g in [1, 2] {
        let entry = JournalEntry {
            build: format!("g{}", g),
            generation: g,
            kind: "session".to_string(),
            ..Default::default()
        };
        crate::vfs::write(
            crate::layout::journal_dir(&out).join(format!("g{}.yaml", g)),
            serde_norway::to_string(&entry).unwrap(),
        )
        .unwrap();
    }
    let read = |g: u64| -> JournalEntry {
        serde_norway::from_str(
            &crate::vfs::read_to_string(
                crate::layout::journal_dir(&out).join(format!("g{}.yaml", g)),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let held = FileLock::acquire(&out);
    let (a, b) = (out.clone(), out.clone());
    let reported = std::thread::spawn(move || Store::stamp_reported(&a, 2, 500, None));
    let measured = std::thread::spawn(move || Store::stamp_tokens(&b, 0, 1, 700, 0));
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert_eq!(read(1).tokens, None, "a stamp wrote under a held lock");
    assert_eq!(read(2).tokens, None, "a stamp wrote under a held lock");
    drop(held);
    assert_eq!(reported.join().unwrap().unwrap().tokens, Some(500));
    measured.join().unwrap();
    assert_eq!(read(1).tokens, Some(700));
    assert_eq!(read(2).tokens, Some(500));
    let leftovers: Vec<String> = crate::vfs::read_dir(crate::layout::journal_dir(&out))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{:?}", leftovers);
}
