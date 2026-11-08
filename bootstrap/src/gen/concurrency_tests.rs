// Ledger writers racing on one out directory: every load, change, and save runs
// under the ledger lock, so no writer loses another's change, a refused change
// persists nothing, and a writer that dies mid-change frees the lock.
// Mirrors docs/consumers/gen.md#the-ledger.
use super::*;

fn fresh_out(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let out = std::env::temp_dir().join(format!(
        "jazyk-ledger-race-{}-{}-{}",
        tag,
        crate::sys::process_id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    crate::vfs::remove_dir_all(&out).ok();
    Ledger::default().save(&out).unwrap();
    out
}

const WRITERS: usize = 16;

// Many writers released at once, each touching two fields: every change lands.
#[test]
fn many_concurrent_updates_keep_every_change() {
    let out = fresh_out("many");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITERS));
    let writers: Vec<_> = (0..WRITERS)
        .map(|n| {
            let out = out.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Ledger::update(&out, |ledger| {
                    ledger.support.push(format!("support/{n:02}.css"));
                    ledger
                        .contradicted
                        .insert(format!("req:w{n:02}"), vec![format!("diag:d{n}")]);
                    Ok(())
                })
                .unwrap();
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    let ledger = Ledger::load(&out);
    let mut support = ledger.support.clone();
    support.sort();
    assert_eq!(
        support,
        (0..WRITERS)
            .map(|n| format!("support/{n:02}.css"))
            .collect::<Vec<_>>()
    );
    assert_eq!(ledger.contradicted.len(), WRITERS);
    assert!(ledger.unreadable.is_none());
    crate::vfs::remove_dir_all(&out).ok();
}

// A change that refuses persists nothing and keeps the lock for no one: writers
// racing with it still land, and its half-made edit never reaches the disk.
#[test]
fn a_refused_change_persists_nothing_while_others_land() {
    let out = fresh_out("refused");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITERS));
    let writers: Vec<_> = (0..WRITERS)
        .map(|n| {
            let out = out.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Ledger::update(&out, |ledger| {
                    ledger.support.push(format!("w{n:02}"));
                    if n % 2 == 1 {
                        return Err(format!("w{n:02} refused"));
                    }
                    Ok(n)
                })
            })
        })
        .collect();
    let results: Vec<Result<usize, String>> =
        writers.into_iter().map(|w| w.join().unwrap()).collect();
    for (n, r) in results.iter().enumerate() {
        if n % 2 == 1 {
            assert_eq!(r, &Err(format!("w{n:02} refused")));
        } else {
            assert_eq!(r, &Ok(n));
        }
    }
    let mut support = Ledger::load(&out).support;
    support.sort();
    assert_eq!(
        support,
        (0..WRITERS)
            .filter(|n| n % 2 == 0)
            .map(|n| format!("w{n:02}"))
            .collect::<Vec<_>>()
    );
    crate::vfs::remove_dir_all(&out).ok();
}

// A writer that panics inside its change releases the lock as it unwinds: the next
// writer is not blocked, and the dead writer's edit never landed.
#[test]
fn a_writer_that_panics_mid_change_frees_the_lock() {
    let out = fresh_out("panic");
    let dying = {
        let out = out.clone();
        std::thread::spawn(move || {
            let _: Result<(), String> = Ledger::update(&out, |ledger| {
                ledger.support.push("never.css".into());
                panic!("the writer died mid-change");
            });
        })
    };
    assert!(dying.join().is_err());
    let next = {
        let out = out.clone();
        std::thread::spawn(move || {
            Ledger::update(&out, |ledger| {
                ledger.support.push("after.css".into());
                Ok(())
            })
        })
    };
    next.join().unwrap().unwrap();
    assert_eq!(Ledger::load(&out).support, vec!["after.css".to_string()]);
    crate::vfs::remove_dir_all(&out).ok();
}
