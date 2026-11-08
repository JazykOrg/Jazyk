// The project edit log an MCP serving reads: native file events on the project root,
// filtered to `.jazyk/jazyk.toml` and every set's `docs` and `source` files, each
// fingerprint change one numbered edit. One log per project root per process, started
// on first use. Mirrors docs/frontends/mcp.md#watching-edits.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub seq: u64,
    pub path: String,
    pub removed: bool,
}

#[derive(Default)]
struct State {
    seq: u64,
    edits: Vec<Edit>,
    fingerprints: BTreeMap<String, String>,
}

pub struct EditLog {
    root: PathBuf,
    state: Mutex<State>,
    changed: Condvar,
    watching: std::sync::atomic::AtomicBool,
}

// The watched files and their fingerprints (size and modification time).
fn fingerprints(root: &Path) -> BTreeMap<String, String> {
    let Ok(w) = crate::document_sets::Workspace::load(root) else {
        return BTreeMap::new();
    };
    let print = |rel: &str| {
        crate::vfs::entry_metadata(&root.join(rel))
            .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
            .ok()
    };
    let mut values = BTreeMap::new();
    if let Some(fp) = print(crate::document_sets::CONFIG) {
        values.insert(crate::document_sets::CONFIG.to_string(), fp);
    }
    for f in w.files() {
        if !w.memberships(&f).is_empty() {
            if let Some(fp) = print(&f) {
                values.insert(f, fp);
            }
        }
    }
    values
}

impl EditLog {
    fn new(root: &Path) -> EditLog {
        EditLog {
            root: root.to_path_buf(),
            state: Mutex::new(State {
                fingerprints: fingerprints(root),
                ..Default::default()
            }),
            changed: Condvar::new(),
            watching: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn watching(&self) -> bool {
        self.watching.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn cursor(&self) -> u64 {
        self.state.lock().unwrap().seq
    }

    // Re-fingerprint and record one edit per changed path. Returns how many landed.
    pub fn rescan(&self) -> usize {
        let now = fingerprints(&self.root);
        let mut st = self.state.lock().unwrap();
        let changed: Vec<(String, bool)> = now
            .keys()
            .chain(st.fingerprints.keys())
            .filter(|k| now.get(*k) != st.fingerprints.get(*k))
            .map(|k| (k.clone(), !now.contains_key(k)))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for (path, removed) in &changed {
            st.seq += 1;
            let seq = st.seq;
            st.edits.push(Edit {
                seq,
                path: path.clone(),
                removed: *removed,
            });
        }
        st.fingerprints = now;
        drop(st);
        if !changed.is_empty() {
            self.changed.notify_all();
        }
        changed.len()
    }

    // The edits after `since`, one per path (its latest), in sequence order.
    pub fn since(&self, since: u64) -> Vec<Edit> {
        let st = self.state.lock().unwrap();
        let mut latest: BTreeMap<&str, &Edit> = BTreeMap::new();
        for e in st.edits.iter().filter(|e| e.seq > since) {
            latest.insert(&e.path, e);
        }
        let mut v: Vec<Edit> = latest.into_values().cloned().collect();
        v.sort_by_key(|e| e.seq);
        v
    }

    // Block until an edit lands after `after` or the timeout passes; the cursor then.
    pub fn wait(&self, after: u64, timeout: Duration) -> u64 {
        let st = self.state.lock().unwrap();
        let (st, _) = self
            .changed
            .wait_timeout_while(st, timeout, |s| s.seq <= after)
            .unwrap();
        st.seq
    }
}

// Paths whose events cannot change a watched fingerprint: Jazyk's own state (except
// the project file) and git's.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn ignorable(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let rel = rel.to_string_lossy().replace('\\', "/");
    (rel.starts_with(".jazyk/") && rel != crate::document_sets::CONFIG)
        || rel == ".git"
        || rel.starts_with(".git/")
}

// The process-wide log for the project at `root`, watching from the first call.
pub fn for_root(root: &Path) -> Arc<EditLog> {
    static LOGS: std::sync::OnceLock<Mutex<BTreeMap<PathBuf, Arc<EditLog>>>> =
        std::sync::OnceLock::new();
    let key = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut logs = LOGS.get_or_init(Default::default).lock().unwrap();
    if let Some(log) = logs.get(&key) {
        return log.clone();
    }
    let log = Arc::new(EditLog::new(root));
    logs.insert(key.clone(), log.clone());
    drop(logs);
    start(log.clone(), key);
    log
}

#[cfg(target_arch = "wasm32")]
fn start(_log: Arc<EditLog>, _key: PathBuf) {}

#[cfg(not(target_arch = "wasm32"))]
fn start(log: Arc<EditLog>, key: PathBuf) {
    use notify::Watcher;
    // A scoped (in-memory) filesystem has no native events.
    if crate::vfs::is_scoped() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let watch_root = key.clone();
    let mut watcher =
        match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if event.paths.is_empty() || !event.paths.iter().all(|p| ignorable(&watch_root, p))
                {
                    tx.send(()).ok();
                }
            }
        }) {
            Ok(w) => w,
            Err(_) => return,
        };
    if watcher
        .watch(&key, notify::RecursiveMode::Recursive)
        .is_err()
    {
        return;
    }
    log.watching
        .store(true, std::sync::atomic::Ordering::SeqCst);
    std::thread::spawn(move || {
        let _watcher = watcher;
        while rx.recv().is_ok() {
            // Debounce the burst, then compare fingerprints.
            std::thread::sleep(Duration::from_millis(300));
            while rx.try_recv().is_ok() {}
            log.rescan();
        }
    });
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".jazyk")).unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join(crate::document_sets::CONFIG),
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nsource = [\"src/**\"]\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("docs/a.md"), "# A\n\nThe order ships.\n").unwrap();
        dir
    }

    #[test]
    fn rescan_records_selected_files_only_once_per_change() {
        let dir = project();
        let log = EditLog::new(dir.path());
        assert_eq!(log.rescan(), 0);
        std::fs::write(
            dir.path().join("docs/a.md"),
            "# A\n\nThe order ships today.\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "unselected\n").unwrap();
        assert_eq!(log.rescan(), 2);
        assert_eq!(log.rescan(), 0);
        let edits = log.since(0);
        let paths: Vec<_> = edits.iter().map(|e| e.path.as_str()).collect();
        assert!(
            paths.contains(&"docs/a.md") && paths.contains(&"src/main.rs"),
            "{paths:?}"
        );
        let cursor = log.cursor();
        std::fs::remove_file(dir.path().join("src/main.rs")).unwrap();
        log.rescan();
        assert_eq!(
            log.since(cursor),
            vec![Edit {
                seq: cursor + 1,
                path: "src/main.rs".into(),
                removed: true
            }]
        );
        assert_eq!(log.wait(cursor, Duration::from_millis(10)), cursor + 1);
    }

    #[test]
    fn the_watcher_notices_an_edit_from_any_writer() {
        let dir = project();
        let log = for_root(dir.path());
        if !log.watching() {
            return;
        }
        let cursor = log.cursor();
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(dir.path().join("docs/b.md"), "# B\n\nA refund is issued.\n").unwrap();
        let mut now = cursor;
        for _ in 0..50 {
            now = log.wait(cursor, Duration::from_millis(200));
            if now > cursor {
                break;
            }
        }
        assert!(now > cursor);
        assert!(log.since(cursor).iter().any(|e| e.path == "docs/b.md"));
    }

    #[test]
    fn own_state_and_git_are_ignorable() {
        let root = Path::new("/p");
        assert!(ignorable(root, Path::new("/p/.jazyk/ds_main/status.yaml")));
        assert!(!ignorable(root, Path::new("/p/.jazyk/jazyk.toml")));
        assert!(ignorable(root, Path::new("/p/.git/index")));
        assert!(!ignorable(root, Path::new("/p/docs/a.md")));
    }
}
