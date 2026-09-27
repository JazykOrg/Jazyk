// The filesystem seam. Every module that the wasm backend shares with the native
// binary reads and writes through here instead of `std::fs`, so the same store,
// reconciler, and project loader run against a real directory on a machine and
// against an in-memory tree in a browser tab. See docs/frontends/wasm.md#filesystem.
//
// The surface is exactly the `std::fs` calls the crate makes, with the same
// signatures and the same `io::Result`, so a call site reads the same on both
// targets. `Path::exists`, `is_dir`, and `is_file` are inherent methods and cannot
// be intercepted, so they travel as `VPathExt::vexists`, `vis_dir`, and `vis_file`.

use std::io;
use std::path::{Path, PathBuf};

// Path predicates. Inherent `Path` methods answer against the real filesystem on
// every target, so the crate asks these instead.
pub trait VPathExt {
    fn vexists(&self) -> bool;
    fn vis_dir(&self) -> bool;
    fn vis_file(&self) -> bool;
    fn vcanonicalize(&self) -> io::Result<PathBuf>;
}

impl VPathExt for Path {
    fn vexists(&self) -> bool {
        exists(self)
    }
    fn vis_dir(&self) -> bool {
        is_dir(self)
    }
    fn vis_file(&self) -> bool {
        is_file(self)
    }
    fn vcanonicalize(&self) -> io::Result<PathBuf> {
        canonicalize(self)
    }
}

impl VPathExt for PathBuf {
    fn vexists(&self) -> bool {
        exists(self.as_path())
    }
    fn vis_dir(&self) -> bool {
        is_dir(self.as_path())
    }
    fn vis_file(&self) -> bool {
        is_file(self.as_path())
    }
    fn vcanonicalize(&self) -> io::Result<PathBuf> {
        canonicalize(self.as_path())
    }
}

// One directory entry. Mirrors the `std::fs::DirEntry` methods the crate calls.
pub struct DirEntry {
    path: PathBuf,
    is_dir: bool,
}

impl DirEntry {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
    pub fn file_name(&self) -> std::ffi::OsString {
        self.path
            .file_name()
            .map(|s| s.to_os_string())
            .unwrap_or_default()
    }
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }
    // Size and mtime, the two facts the crate asks an entry for. The in-memory
    // tree keeps no timestamps, so there `modified` is always the epoch.
    pub fn metadata(&self) -> io::Result<Metadata> {
        imp::entry_metadata(&self.path)
    }
}

// The slice of `std::fs::Metadata` the crate reads.
pub struct Metadata {
    pub(crate) len: u64,
    pub(crate) modified: std::time::SystemTime,
}

impl Metadata {
    pub fn len(&self) -> u64 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn modified(&self) -> io::Result<std::time::SystemTime> {
        Ok(self.modified)
    }
}

// `read_dir` hands back a vector of results rather than a lazy iterator: the
// in-memory tree has nothing to stream, and every call site either iterates,
// flattens, or counts.
pub type ReadDir = std::vec::IntoIter<io::Result<DirEntry>>;

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::{DirEntry, ReadDir};
    use std::io;
    use std::path::{Path, PathBuf};

    pub fn read_to_string(p: impl AsRef<Path>) -> io::Result<String> {
        std::fs::read_to_string(p)
    }
    pub fn read(p: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        std::fs::read(p)
    }
    pub fn write(p: impl AsRef<Path>, c: impl AsRef<[u8]>) -> io::Result<()> {
        std::fs::write(p, c)
    }
    pub fn create_dir(p: impl AsRef<Path>) -> io::Result<()> {
        std::fs::create_dir(p)
    }
    pub fn create_dir_all(p: impl AsRef<Path>) -> io::Result<()> {
        std::fs::create_dir_all(p)
    }
    pub fn remove_file(p: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_file(p)
    }
    pub fn remove_dir(p: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_dir(p)
    }
    pub fn remove_dir_all(p: impl AsRef<Path>) -> io::Result<()> {
        std::fs::remove_dir_all(p)
    }
    pub fn rename(a: impl AsRef<Path>, b: impl AsRef<Path>) -> io::Result<()> {
        std::fs::rename(a, b)
    }
    pub fn copy(a: impl AsRef<Path>, b: impl AsRef<Path>) -> io::Result<u64> {
        std::fs::copy(a, b)
    }
    pub fn canonicalize(p: impl AsRef<Path>) -> io::Result<PathBuf> {
        std::fs::canonicalize(p)
    }
    pub fn exists(p: &Path) -> bool {
        p.exists()
    }
    pub fn is_dir(p: &Path) -> bool {
        p.is_dir()
    }
    pub fn is_file(p: &Path) -> bool {
        p.is_file()
    }
    pub fn read_dir(p: impl AsRef<Path>) -> io::Result<ReadDir> {
        let mut out: Vec<io::Result<DirEntry>> = Vec::new();
        for e in std::fs::read_dir(p)? {
            out.push(e.map(|e| DirEntry {
                is_dir: e.path().is_dir(),
                path: e.path(),
            }));
        }
        Ok(out.into_iter())
    }
    // Create a file only if it does not exist, writing `contents`. This is the
    // exclusive create a lock file is built on: it must fail when the file is there.
    pub fn create_new(p: impl AsRef<Path>, contents: &str) -> io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(p)?;
        f.write_all(contents.as_bytes())
    }

    // Append bytes to a file, creating it.
    pub fn append(p: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)?;
        f.write_all(bytes)
    }

    // Append one line to a file, creating it. The crate's logs are append-only.
    pub fn append_line(p: impl AsRef<Path>, line: &str) -> io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)?;
        writeln!(f, "{}", line)?;
        f.flush()
    }

    pub fn entry_metadata(p: &Path) -> io::Result<super::Metadata> {
        let md = std::fs::metadata(p)?;
        Ok(super::Metadata {
            len: md.len(),
            modified: md.modified().unwrap_or(std::time::UNIX_EPOCH),
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::{DirEntry, ReadDir};
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::io;
    use std::path::{Component, Path, PathBuf};

    // One tree per tab. The browser runs the backend on a single thread, so a
    // RefCell is the whole of the locking story.
    thread_local! {
        static FILES: RefCell<BTreeMap<PathBuf, Vec<u8>>> = RefCell::new(BTreeMap::new());
        static DIRS: RefCell<BTreeSet<PathBuf>> = RefCell::new(BTreeSet::new());
    }

    // Paths arrive from a project root the host chose, so they are already
    // absolute; this only folds away `.` and `..` so two spellings of one file
    // are one key.
    fn norm(p: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for c in p.components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir => {
                    out.pop();
                }
                c => out.push(c.as_os_str()),
            }
        }
        out
    }

    fn not_found(p: &Path) -> io::Error {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{}: no such file or directory", p.display()),
        )
    }

    pub fn read(p: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        let p = norm(p.as_ref());
        FILES.with(|f| f.borrow().get(&p).cloned()).ok_or_else(|| not_found(&p))
    }

    pub fn read_to_string(p: impl AsRef<Path>) -> io::Result<String> {
        let bytes = read(p)?;
        String::from_utf8(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    pub fn write(p: impl AsRef<Path>, c: impl AsRef<[u8]>) -> io::Result<()> {
        let p = norm(p.as_ref());
        if let Some(parent) = p.parent() {
            create_dir_all(parent)?;
        }
        FILES.with(|f| f.borrow_mut().insert(p.clone(), c.as_ref().to_vec()));
        crate::wasm::fs_bridge::wrote(&p, c.as_ref());
        Ok(())
    }

    pub fn create_dir(p: impl AsRef<Path>) -> io::Result<()> {
        DIRS.with(|d| d.borrow_mut().insert(norm(p.as_ref())));
        Ok(())
    }

    pub fn create_dir_all(p: impl AsRef<Path>) -> io::Result<()> {
        let p = norm(p.as_ref());
        DIRS.with(|d| {
            let mut d = d.borrow_mut();
            let mut cur = PathBuf::new();
            for c in p.components() {
                cur.push(c.as_os_str());
                d.insert(cur.clone());
            }
        });
        Ok(())
    }

    pub fn remove_file(p: impl AsRef<Path>) -> io::Result<()> {
        let p = norm(p.as_ref());
        FILES
            .with(|f| f.borrow_mut().remove(&p))
            .map(|_| ())
            .ok_or_else(|| not_found(&p))?;
        crate::wasm::fs_bridge::removed(&p, false);
        Ok(())
    }

    pub fn remove_dir(p: impl AsRef<Path>) -> io::Result<()> {
        let p = norm(p.as_ref());
        DIRS.with(|d| d.borrow_mut().remove(&p));
        crate::wasm::fs_bridge::removed(&p, true);
        Ok(())
    }

    pub fn remove_dir_all(p: impl AsRef<Path>) -> io::Result<()> {
        let p = norm(p.as_ref());
        FILES.with(|f| f.borrow_mut().retain(|k, _| !k.starts_with(&p)));
        DIRS.with(|d| {
            let keep: Vec<PathBuf> = d.borrow().iter().filter(|k| !k.starts_with(&p)).cloned().collect();
            *d.borrow_mut() = keep.into_iter().collect();
        });
        crate::wasm::fs_bridge::removed(&p, true);
        Ok(())
    }

    pub fn rename(a: impl AsRef<Path>, b: impl AsRef<Path>) -> io::Result<()> {
        let (a, b) = (norm(a.as_ref()), norm(b.as_ref()));
        // A file move, or a directory move carrying everything under it.
        let moved: Vec<(PathBuf, Vec<u8>)> = FILES.with(|f| {
            f.borrow()
                .iter()
                .filter(|(k, _)| *k == &a || k.starts_with(&a))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        });
        if moved.is_empty() {
            return Err(not_found(&a));
        }
        // The host mirrors this tree (a folder on disk, browser storage), so each
        // source leaves the way a removed file does, or the mirror keeps both.
        for (k, v) in moved {
            let rest = k.strip_prefix(&a).ok();
            let dest = match rest {
                Some(r) if !r.as_os_str().is_empty() => b.join(r),
                _ => b.clone(),
            };
            write(&dest, &v)?;
            FILES.with(|f| f.borrow_mut().remove(&k));
            crate::wasm::fs_bridge::removed(&k, false);
        }
        let dirs: Vec<PathBuf> = DIRS.with(|d| d.borrow().iter().filter(|k| k.starts_with(&a)).cloned().collect());
        if !dirs.is_empty() {
            DIRS.with(|d| {
                let mut d = d.borrow_mut();
                for k in &dirs {
                    d.remove(k);
                    if let Ok(rest) = k.strip_prefix(&a) {
                        d.insert(b.join(rest));
                    }
                }
            });
            crate::wasm::fs_bridge::removed(&a, true);
        }
        Ok(())
    }

    pub fn copy(a: impl AsRef<Path>, b: impl AsRef<Path>) -> io::Result<u64> {
        let v = read(a)?;
        let n = v.len() as u64;
        write(b, &v)?;
        Ok(n)
    }

    pub fn canonicalize(p: impl AsRef<Path>) -> io::Result<PathBuf> {
        Ok(norm(p.as_ref()))
    }

    pub fn exists(p: &Path) -> bool {
        is_file(p) || is_dir(p)
    }

    pub fn is_file(p: &Path) -> bool {
        let p = norm(p);
        FILES.with(|f| f.borrow().contains_key(&p))
    }

    pub fn is_dir(p: &Path) -> bool {
        let p = norm(p);
        if DIRS.with(|d| d.borrow().contains(&p)) {
            return true;
        }
        // A directory is also implied by any file under it, so a tree seeded as a
        // flat file list needs no directory records.
        FILES.with(|f| f.borrow().keys().any(|k| k.starts_with(&p) && k != &p))
    }

    pub fn read_dir(p: impl AsRef<Path>) -> io::Result<ReadDir> {
        let dir = norm(p.as_ref());
        if !is_dir(&dir) {
            return Err(not_found(&dir));
        }
        // One entry per immediate child, files and directories alike.
        let mut seen: BTreeMap<PathBuf, bool> = BTreeMap::new();
        let mut note = |key: &PathBuf, is_dir_key: bool| {
            let Ok(rest) = key.strip_prefix(&dir) else { return };
            let mut it = rest.components();
            let Some(first) = it.next() else { return };
            let child = dir.join(first.as_os_str());
            let child_is_dir = it.next().is_some() || is_dir_key;
            let e = seen.entry(child).or_insert(child_is_dir);
            *e = *e || child_is_dir;
        };
        FILES.with(|f| {
            for k in f.borrow().keys() {
                note(k, false);
            }
        });
        DIRS.with(|d| {
            for k in d.borrow().iter() {
                note(k, true);
            }
        });
        let out: Vec<io::Result<DirEntry>> = seen
            .into_iter()
            .map(|(path, is_dir)| Ok(DirEntry { path, is_dir }))
            .collect();
        Ok(out.into_iter())
    }

    pub fn create_new(p: impl AsRef<Path>, contents: &str) -> io::Result<()> {
        let p = p.as_ref();
        if is_file(p) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists", p.display()),
            ));
        }
        write(p, contents)
    }

    pub fn append(p: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
        let p = p.as_ref();
        let mut buf = read(p).unwrap_or_default();
        buf.extend_from_slice(bytes);
        write(p, buf)
    }

    pub fn append_line(p: impl AsRef<Path>, line: &str) -> io::Result<()> {
        let p = p.as_ref();
        let mut buf = read(p).unwrap_or_default();
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
        write(p, buf)
    }

    pub fn entry_metadata(p: &Path) -> io::Result<super::Metadata> {
        Ok(super::Metadata {
            len: read(p)?.len() as u64,
            modified: std::time::UNIX_EPOCH,
        })
    }

    // Seeding and draining the tree. The wasm backend fills it from the bundled
    // demo project on boot and hands edited documents back to the page.
    pub fn seed(path: &Path, bytes: Vec<u8>) {
        let p = norm(path);
        if let Some(parent) = p.parent() {
            let _ = create_dir_all(parent);
        }
        FILES.with(|f| f.borrow_mut().insert(p, bytes));
    }

    pub fn list_all() -> Vec<PathBuf> {
        FILES.with(|f| f.borrow().keys().cloned().collect())
    }

    // Empty the tree, so a project seeded after another starts from nothing rather
    // than over the first one's files.
    pub fn reset() {
        FILES.with(|f| f.borrow_mut().clear());
        DIRS.with(|d| d.borrow_mut().clear());
    }
}

pub use imp::*;
