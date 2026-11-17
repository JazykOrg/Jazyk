// The single-writer lock, the read wait, and the commit marker.
// Mirrors docs/compiler/graph.md#concurrency.
use super::*;

// Cross-process single-writer lock: an advisory lock on `.lock` the operating system
// releases when its holder exits, so a crashed writer leaves nothing stale and a waiter
// never overrides a live holder. Two handles in one process exclude each other too.
// A scoped (in-memory) store belongs to one process and keeps a marker file instead.
// Mirrors docs/compiler/graph.md#concurrency.
pub(super) struct FileLock {
    #[cfg(not(target_arch = "wasm32"))]
    _file: Option<std::fs::File>,
    marker: Option<PathBuf>,
    // The wait passed STORE_LOCK_WAIT: nothing is held and the writer refuses
    // `store-busy`. Mirrors docs/compiler/graph.md#concurrency.
    busy: bool,
    #[cfg(not(target_arch = "wasm32"))]
    taken: Option<std::time::Instant>,
}

thread_local! {
    // Whether this thread's latest acquire gave up waiting: the writer's refusal check
    // runs right after its acquire on the same thread.
    static BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// The lock wait this thread gave up on, as the refusal every writer reports.
pub(super) fn busy_refusal(out: &Path) -> Option<String> {
    BUSY.with(|b| b.get()).then(|| {
        format!(
            "store-busy: another writer held {} past {} seconds; nothing changed, retry",
            crate::layout::lock_file(out).display(),
            crate::limits::STORE_LOCK_WAIT_SECS
        )
    })
}

#[cfg(test)]
thread_local! {
    pub(super) static LOCK_WAIT_OVERRIDE: std::cell::Cell<Option<std::time::Duration>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(not(target_arch = "wasm32"))]
fn lock_wait() -> std::time::Duration {
    #[cfg(test)]
    if let Some(w) = LOCK_WAIT_OVERRIDE.with(|o| o.get()) {
        return w;
    }
    std::time::Duration::from_secs(crate::limits::STORE_LOCK_WAIT_SECS)
}

impl FileLock {
    pub(super) fn acquire(out: &Path) -> FileLock {
        // A newer store takes no lock file: every writer refuses it after the
        // acquire, so nothing lands in its directory (docs/compiler/graph.md#store-version).
        if newer_version(out).is_some() {
            return FileLock {
                #[cfg(not(target_arch = "wasm32"))]
                _file: None,
                marker: None,
                busy: false,
                #[cfg(not(target_arch = "wasm32"))]
                taken: None,
            };
        }
        let path = crate::layout::lock_file(out);
        if let Some(dir) = path.parent() {
            crate::vfs::create_dir_all(dir).ok();
        }
        crate::vfs::create_dir_all(out).ok();
        #[cfg(not(target_arch = "wasm32"))]
        if !crate::vfs::is_scoped() {
            let opened = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path);
            let file = match opened {
                Ok(f) => f,
                Err(e) => {
                    eprintln!(
                        "[jazyk] warning: cannot open {}: {}; writing without the lock",
                        path.display(),
                        e
                    );
                    return FileLock {
                        _file: None,
                        marker: None,
                        busy: false,
                        taken: None,
                    };
                }
            };
            let started = std::time::Instant::now();
            let mut warned = false;
            let wait = lock_wait();
            loop {
                if fs2::FileExt::try_lock_exclusive(&file).is_ok() {
                    BUSY.with(|b| b.set(false));
                    return FileLock {
                        _file: Some(file),
                        marker: None,
                        busy: false,
                        taken: Some(std::time::Instant::now()),
                    };
                }
                if started.elapsed() >= wait {
                    eprintln!(
                        "[jazyk] error: gave up waiting for the writer holding {} after {:?}",
                        path.display(),
                        wait
                    );
                    BUSY.with(|b| b.set(true));
                    return FileLock {
                        _file: None,
                        marker: None,
                        busy: true,
                        taken: None,
                    };
                }
                if !warned && started.elapsed() >= std::time::Duration::from_secs(10) {
                    eprintln!("[jazyk] waiting for the writer holding {}", path.display());
                    warned = true;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        // An in-memory tree belongs to one thread, so no other party can hold its
        // marker and waiting never frees it. A marker this thread holds is a nested
        // acquire: it proceeds without taking the marker, which stays its holder's. Any
        // other marker has no live holder (a snapshot carried it in) and is taken over.
        let nested = SCOPED_LOCKS.with(|held| held.borrow().contains(&path));
        if nested {
            return FileLock {
                #[cfg(not(target_arch = "wasm32"))]
                _file: None,
                marker: None,
                busy: false,
                #[cfg(not(target_arch = "wasm32"))]
                taken: None,
            };
        }
        if let Err(e) = crate::vfs::write(&path, crate::sys::process_id().to_string()) {
            eprintln!(
                "[jazyk] warning: cannot write {}: {}; writing without the lock",
                path.display(),
                e
            );
            return FileLock {
                #[cfg(not(target_arch = "wasm32"))]
                _file: None,
                marker: None,
                busy: false,
                #[cfg(not(target_arch = "wasm32"))]
                taken: None,
            };
        }
        SCOPED_LOCKS.with(|held| held.borrow_mut().insert(path.clone()));
        BUSY.with(|b| b.set(false));
        FileLock {
            #[cfg(not(target_arch = "wasm32"))]
            _file: None,
            marker: Some(path),
            busy: false,
            #[cfg(not(target_arch = "wasm32"))]
            taken: None,
        }
    }
}

// How long a reader waits for a consistent read before it reports `store-busy`.
const READ_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(test)]
thread_local! {
    pub(super) static READ_WAIT_OVERRIDE: std::cell::Cell<Option<std::time::Duration>> =
        const { std::cell::Cell::new(None) };
}

pub(super) fn read_wait() -> std::time::Duration {
    #[cfg(test)]
    if let Some(w) = READ_WAIT_OVERRIDE.with(|o| o.get()) {
        return w;
    }
    READ_WAIT
}

// The commit marker: present while a writer rewrites the store's files, removed once
// the commit point is written. Mirrors docs/compiler/graph.md#concurrency.
pub const COMMIT_MARKER: &str = ".committing";

pub(super) struct CommitMarker {
    path: Option<PathBuf>,
}

impl CommitMarker {
    // A nested begin (a save inside a save) leaves the marker to the outer one.
    pub(super) fn begin(out: &Path) -> CommitMarker {
        let path = crate::layout::committing_file(out);
        let outer = MARKING.with(|m| m.borrow_mut().insert(path.clone()));
        if !outer {
            return CommitMarker { path: None };
        }
        if let Err(e) = crate::vfs::create_dir_all(path.parent().unwrap_or(out))
            .and_then(|()| write_durable(&path, crate::sys::process_id().to_string().as_bytes()))
        {
            eprintln!("[jazyk] error: cannot write {}: {}", path.display(), e);
        }
        CommitMarker { path: Some(path) }
    }
}

impl Drop for CommitMarker {
    fn drop(&mut self) {
        if let Some(p) = self.path.take() {
            if let Err(e) = crate::vfs::remove_file(&p) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("[jazyk] error: cannot remove {}: {}", p.display(), e);
                }
            }
            MARKING.with(|m| m.borrow_mut().remove(&p));
        }
    }
}

thread_local! {
    static MARKING: std::cell::RefCell<BTreeSet<PathBuf>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

// Whether a writer is rewriting the store now: the marker is present and a writer holds
// `.lock`. A marker no writer holds is left by a writer that stopped mid-commit; the
// next commit replaces it, and reads proceed on what the disk holds. An in-memory tree
// belongs to one thread, which cannot read while it writes, so its marker is always a
// leftover. Mirrors docs/compiler/graph.md#concurrency.
pub(super) fn commit_in_progress(out: &Path) -> bool {
    if !crate::layout::committing_file(out).vexists() {
        return false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        // A shared probe of the writers' advisory lock: it fails only while a writer
        // holds it, and it creates nothing.
        let Ok(file) = std::fs::File::open(crate::layout::lock_file(out)) else {
            return false;
        };
        return match fs2::FileExt::try_lock_shared(&file) {
            Ok(()) => {
                fs2::FileExt::unlock(&file).ok();
                false
            }
            Err(_) => true,
        };
    }
    false
}

// The markers this thread holds on in-memory trees.
thread_local! {
    static SCOPED_LOCKS: std::cell::RefCell<BTreeSet<PathBuf>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

impl FileLock {
    pub(super) fn busy(&self) -> bool {
        self.busy
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        if self.busy {
            BUSY.with(|b| b.set(false));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(taken) = self.taken {
            let held = taken.elapsed();
            if held >= std::time::Duration::from_secs(crate::limits::STORE_LOCK_HOLD_SECS) {
                eprintln!(
                    "[jazyk] warning: a commit held the store lock for {:?} (over {} seconds)",
                    held,
                    crate::limits::STORE_LOCK_HOLD_SECS
                );
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(f) = &self._file {
            fs2::FileExt::unlock(f).ok();
        }
        if let Some(p) = &self.marker {
            crate::vfs::remove_file(p).ok();
            SCOPED_LOCKS.with(|held| held.borrow_mut().remove(p));
        }
    }
}
