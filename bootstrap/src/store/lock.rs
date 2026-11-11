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
            };
        }
        let path = out.join(".lock");
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
                    };
                }
            };
            let started = std::time::Instant::now();
            let mut warned = false;
            loop {
                if fs2::FileExt::try_lock_exclusive(&file).is_ok() {
                    return FileLock {
                        _file: Some(file),
                        marker: None,
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
            };
        }
        SCOPED_LOCKS.with(|held| held.borrow_mut().insert(path.clone()));
        FileLock {
            #[cfg(not(target_arch = "wasm32"))]
            _file: None,
            marker: Some(path),
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
        let path = out.join(COMMIT_MARKER);
        let outer = MARKING.with(|m| m.borrow_mut().insert(path.clone()));
        if !outer {
            return CommitMarker { path: None };
        }
        if let Err(e) = crate::vfs::create_dir_all(out)
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
    if !out.join(COMMIT_MARKER).vexists() {
        return false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        // A shared probe of the writers' advisory lock: it fails only while a writer
        // holds it, and it creates nothing.
        let Ok(file) = std::fs::File::open(out.join(".lock")) else {
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

impl Drop for FileLock {
    fn drop(&mut self) {
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
