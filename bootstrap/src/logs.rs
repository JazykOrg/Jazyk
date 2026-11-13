//! The project's logs outside the repository: whether this process records replay
//! snapshots, and the retention sweep a build runs at its start.
//! Mirrors docs/compiler/jazyk-folder.md#logs and docs/compiler/sessions.md#snapshots.
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

// Days a log file is kept before a build's sweep removes it.
pub const RETENTION_DAYS: u64 = 30;

static RECORDING: AtomicBool = AtomicBool::new(false);

/// `jazyk compile --record`: this process writes a graph snapshot per batch.
pub fn set_recording(on: bool) {
    RECORDING.store(on, Ordering::Relaxed);
}

/// Whether batches write replay snapshots. Off unless `--record` asked for them.
pub fn recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

/// Remove log files older than [`RETENTION_DAYS`] under the project's log directory.
/// Best effort, native only: a scoped (hosted or browser) file system keeps no logs of
/// its own to prune. Returns the number of files removed.
pub fn prune(out: &Path) -> usize {
    if !crate::vfs::host_logs() {
        return 0;
    }
    let dir = crate::layout::project_log_dir(out);
    let cutoff =
        std::time::SystemTime::now() - std::time::Duration::from_secs(RETENTION_DAYS * 24 * 3600);
    prune_dir(&dir, cutoff)
}

fn prune_dir(dir: &Path, cutoff: std::time::SystemTime) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for e in rd.flatten() {
        let p = e.path();
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            removed += prune_dir(&p, cutoff);
            // An emptied directory goes with its files.
            if std::fs::read_dir(&p)
                .map(|mut r| r.next().is_none())
                .unwrap_or(false)
            {
                std::fs::remove_dir(&p).ok();
            }
        } else if md.modified().map(|m| m < cutoff).unwrap_or(false)
            && std::fs::remove_file(&p).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    // An in-memory workspace writes no log files to a host directory: opening a
    // serving (its transcript), feedback, pruning and snapshots all skip the log
    // directory. The browser has no host directory at all, and resolving one there
    // reaches std::env::temp_dir, which panics on wasm32.
    // Mirrors docs/compiler/jazyk-folder.md#logs.
    #[test]
    fn in_memory_workspaces_write_no_host_log_files() {
        assert!(crate::vfs::host_logs() || crate::vfs::is_scoped());
        let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        assert!(!crate::vfs::host_logs());
        let root = Path::new("/project");
        crate::vfs::create_dir_all(root.join(".jazyk")).unwrap();
        crate::vfs::write(
            root.join(".jazyk/jazyk.toml"),
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        let out = root.join(".jazyk/ds_main");
        let host = crate::layout::project_log_dir(&out);
        assert!(!std::path::Path::new(&host).exists());
        let project = crate::document_sets::Workspace::load(root)
            .unwrap()
            .files_project();
        let _serving = crate::mcp::McpServer::new(project, out.clone(), vec!["chat".into()], false);
        crate::feedback::append(
            &out,
            &crate::feedback::Entry {
                at: "2026-11-13T00:00:00Z".into(),
                kind: "other".into(),
                message: "probe".into(),
                ..Default::default()
            },
        );
        assert_eq!(prune(&out), 0);
        crate::store::snapshot_graph(&out, "probe");
        assert!(!std::path::Path::new(&host).exists(), "{}", host.display());
        assert!(!crate::vfs::exists(&crate::layout::feedback_file(&out)));
    }

    #[test]
    fn prune_removes_only_files_past_retention() {
        let base =
            std::env::temp_dir().join(format!("jazyk-logs-prune-{}", crate::sys::process_id()));
        std::fs::remove_dir_all(&base).ok();
        let old = base.join("set/trace/old.jsonl");
        let new = base.join("set/trace/new.jsonl");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "x").unwrap();
        std::fs::write(&new, "x").unwrap();
        let stale = std::time::SystemTime::now()
            - std::time::Duration::from_secs((RETENTION_DAYS + 1) * 24 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(stale)
            .unwrap();
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(RETENTION_DAYS * 24 * 3600);
        assert_eq!(prune_dir(&base, cutoff), 1);
        assert!(!old.exists() && new.exists());
        std::fs::remove_dir_all(&base).ok();
    }
}
