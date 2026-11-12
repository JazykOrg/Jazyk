//! Where every file of a project's `.jazyk` folder lives: the committed set files, the
//! set's `local/` and `derived/` folders, the project's `local/` folder, and the log
//! directory outside the project. One place names each path, so a layout change is one
//! edit. Mirrors docs/compiler/jazyk-folder.md.
use std::path::{Path, PathBuf};

// ---- committed set files ----

/// `store.yaml`: the store version.
pub fn store_file(out: &Path) -> PathBuf {
    out.join("store.yaml")
}

/// The version 4 status file a version 4 store kept at the top of the set.
pub fn legacy_status_file(out: &Path) -> PathBuf {
    out.join("status.yaml")
}

pub fn graph_dir(out: &Path) -> PathBuf {
    out.join("graph")
}

/// One authored node kind's directory, e.g. `graph/entities`.
pub fn node_dir(out: &Path, kind: &str) -> PathBuf {
    graph_dir(out).join(kind)
}

/// One decision kind's directory, e.g. `graph/decisions/pairs`.
pub fn decisions_dir(out: &Path, kind: &str) -> PathBuf {
    graph_dir(out).join("decisions").join(kind)
}

/// `sources/`: each document's section tree as the graph reflects it.
pub fn sources_dir(out: &Path) -> PathBuf {
    out.join("sources")
}

/// One document's source file, `sources/<doc path>.yaml`.
pub fn source_file(out: &Path, doc: &str) -> PathBuf {
    sources_dir(out).join(format!("{}.yaml", doc))
}

/// `gen/`: the ledger.
pub fn gen_dir(out: &Path) -> PathBuf {
    out.join("gen")
}

// ---- local set files ----

/// `local/`: what this machine's runs leave behind.
pub fn local_dir(out: &Path) -> PathBuf {
    out.join("local")
}

pub fn build_file(out: &Path) -> PathBuf {
    local_dir(out).join("build.yaml")
}

/// An interrupted build's progress, present only while a build runs.
pub fn run_file(out: &Path) -> PathBuf {
    local_dir(out).join("run.yaml")
}

pub fn control_file(out: &Path) -> PathBuf {
    local_dir(out).join("control.yaml")
}

pub fn journal_dir(out: &Path) -> PathBuf {
    local_dir(out).join("journal")
}

pub fn journal_file(out: &Path, generation: u64) -> PathBuf {
    journal_dir(out).join(format!("g{}.yaml", generation))
}

pub fn workers_dir(out: &Path) -> PathBuf {
    local_dir(out).join("workers")
}

pub fn leases_dir(out: &Path) -> PathBuf {
    local_dir(out).join("leases")
}

/// The commit lock.
pub fn lock_file(out: &Path) -> PathBuf {
    local_dir(out).join(".lock")
}

pub fn leases_lock_file(out: &Path) -> PathBuf {
    local_dir(out).join(".leases.lock")
}

/// The ledger's writer lock.
pub fn ledger_lock_file(out: &Path) -> PathBuf {
    local_dir(out).join(".ledger.lock")
}

/// The marker present while a commit writes.
pub fn committing_file(out: &Path) -> PathBuf {
    local_dir(out).join(".committing")
}

/// Source sections a git merge left for the next build: `local/merged.yaml`.
pub fn merged_file(out: &Path) -> PathBuf {
    local_dir(out).join("merged.yaml")
}

pub fn decompile_dir(out: &Path) -> PathBuf {
    local_dir(out).join("decompile")
}

pub fn deliverable_baseline_dir(out: &Path) -> PathBuf {
    local_dir(out).join("deliverable-baseline")
}

/// GUI Stop's fence on automatic builds: `local/jobs-stopped`.
pub fn jobs_stopped_file(out: &Path) -> PathBuf {
    local_dir(out).join("jobs-stopped")
}

/// The ACP proxy's edit socket: `local/acp-edit-sink-<pid>.sock`. A path past the
/// platform's socket path limit moves to the user's runtime or temporary directory
/// under the same name.
pub fn edit_sink_socket(out: &Path, pid: u32) -> PathBuf {
    let name = format!("acp-edit-sink-{}.sock", pid);
    let path = local_dir(out).join(&name);
    if path.as_os_str().len() <= SOCKET_PATH_MAX {
        return path;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(dir) = dirs::runtime_dir() {
        return dir.join(format!("jazyk-{}", name));
    }
    std::env::temp_dir().join(format!("jazyk-{}", name))
}

// The longest socket path every supported platform binds: macOS allows 103 bytes
// (`sun_path` is 104 with its terminator), Linux 107.
const SOCKET_PATH_MAX: usize = 100;

// ---- derived set files ----

/// `derived/`: rebuilt from the committed files, safe to delete.
pub fn derived_dir(out: &Path) -> PathBuf {
    out.join("derived")
}

/// One derived node kind's directory, e.g. `derived/graph/relationships`.
pub fn derived_node_dir(out: &Path, kind: &str) -> PathBuf {
    derived_dir(out).join("graph").join(kind)
}

pub fn diagrams_dir(out: &Path) -> PathBuf {
    derived_dir(out).join("diagrams")
}

pub fn docsgen_dir(out: &Path) -> PathBuf {
    derived_dir(out).join("docsgen")
}

pub fn last_build_dir(out: &Path) -> PathBuf {
    derived_dir(out).join("last-build")
}

pub fn preview_dir(out: &Path) -> PathBuf {
    derived_dir(out).join("preview")
}

// ---- project files ----

/// The project's `.jazyk` folder for a set directory `.jazyk/<set-id>`, when the set
/// sits in one.
pub fn jazyk_dir_of(out: &Path) -> Option<&Path> {
    out.parent()
        .filter(|p| p.file_name().is_some_and(|n| n == ".jazyk"))
}

/// The project's `local/` folder: `.jazyk/local`.
pub fn project_local_dir(jazyk: &Path) -> PathBuf {
    jazyk.join("local")
}

pub fn project_lock_file(jazyk: &Path) -> PathBuf {
    project_local_dir(jazyk).join("project.lock")
}

pub fn removed_sets_dir(jazyk: &Path) -> PathBuf {
    project_local_dir(jazyk).join("removed-sets")
}

/// Task runs on this machine: `.jazyk/local/runtime/`.
pub fn runtime_dir(jazyk: &Path) -> PathBuf {
    project_local_dir(jazyk).join("runtime")
}

/// GUI jobs' stop flags: `.jazyk/local/job-cancellation/`.
pub fn job_cancellation_dir(jazyk: &Path) -> PathBuf {
    project_local_dir(jazyk).join("job-cancellation")
}

// ---- machine state earlier versions kept outside `local/` ----

/// `.jazyk/runtime/`, now [`runtime_dir`].
pub fn legacy_runtime_dir(jazyk: &Path) -> PathBuf {
    jazyk.join("runtime")
}

/// `.jazyk/job-cancellation/`, now [`job_cancellation_dir`].
pub fn legacy_job_cancellation_dir(jazyk: &Path) -> PathBuf {
    jazyk.join("job-cancellation")
}

/// `<out>/jobs-stopped`, now [`jobs_stopped_file`].
pub fn legacy_jobs_stopped_file(out: &Path) -> PathBuf {
    out.join("jobs-stopped")
}

/// Whether `name` is an edit socket an earlier version left at the top of `<out>`, and
/// the process id it names.
pub fn legacy_edit_sink_pid(name: &str) -> Option<u32> {
    name.strip_prefix("acp-edit-sink-")?
        .strip_suffix(".sock")?
        .parse()
        .ok()
}

/// Move what an earlier version kept at `old` to `new`, once: a file or folder `new`
/// lacks moves whole, a folder present at both merges entry by entry with `new`
/// winning, and `old` is gone afterwards. Best effort: what cannot move stays.
pub fn adopt_legacy(old: &Path, new: &Path) {
    use crate::vfs::VPathExt;
    if !old.vexists() {
        return;
    }
    if let Some(parent) = new.parent() {
        crate::vfs::create_dir_all(parent).ok();
    }
    if !new.vexists() {
        crate::vfs::rename(old, new).ok();
    } else if old.vis_dir() && new.vis_dir() {
        if let Ok(entries) = crate::vfs::read_dir(old) {
            for entry in entries.flatten() {
                adopt_legacy(&entry.path(), &new.join(entry.file_name()));
            }
        }
        crate::vfs::remove_dir(old).ok();
    } else if !old.vis_dir() {
        crate::vfs::remove_file(old).ok();
    }
}

/// Where a build archives a store of another version: `.jazyk/local/archive/<set>-v<N>`,
/// or a `<out>.v<N>` sibling for a store outside a `.jazyk` folder.
pub fn archive_dir(out: &Path, version: u32) -> PathBuf {
    let set = out
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "store".to_string());
    match jazyk_dir_of(out) {
        Some(jazyk) => project_local_dir(jazyk)
            .join("archive")
            .join(format!("{}-v{}", set, version)),
        None => out.with_file_name(format!("{}.v{}", set, version)),
    }
}

// ---- logs ----

/// The log directory of the project whose set directory is `out`: `JAZYK_LOG_DIR`, else
/// the operating system's cache directory, then `<project>-<key>`. A hosted run keeps
/// no logs in its snapshot: its records go to the host's record store.
/// Mirrors docs/compiler/jazyk-folder.md#logs.
pub fn project_log_dir(out: &Path) -> PathBuf {
    let root = jazyk_dir_of(out)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| out.to_path_buf());
    native_log_dir(&root)
}

/// The log directory of the project at `root` (the folder holding `.jazyk/`).
pub fn log_dir_for_root(root: &Path) -> PathBuf {
    native_log_dir(root)
}

fn native_log_dir(root: &Path) -> PathBuf {
    let root = root.to_path_buf();
    let base = match crate::sys::env_var("JAZYK_LOG_DIR")
        .ok()
        .filter(|v| !v.is_empty())
    {
        Some(dir) => PathBuf::from(dir),
        None => cache_base(),
    };
    let abs = std::path::absolute(&root).unwrap_or(root.clone());
    let name = abs
        .file_name()
        .map(|n| crate::ids::slug(&n.to_string_lossy()))
        .unwrap_or_else(|| "project".to_string());
    let key = format!(
        "{:016x}",
        crate::ids::fnv1a64(abs.to_string_lossy().as_bytes())
    );
    base.join(format!("{}-{}", name, &key[..8]))
}

fn cache_base() -> PathBuf {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(dir) = dirs::cache_dir() {
        return dir.join("jazyk");
    }
    std::env::temp_dir().join("jazyk")
}

/// One set's logs: `<log dir>/<set-id>`.
pub fn set_log_dir(out: &Path) -> PathBuf {
    let set = out
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "store".to_string());
    project_log_dir(out).join(set)
}

pub fn trace_dir(out: &Path) -> PathBuf {
    set_log_dir(out).join("trace")
}

pub fn sessions_dir(out: &Path) -> PathBuf {
    set_log_dir(out).join("sessions")
}

pub fn feedback_file(out: &Path) -> PathBuf {
    set_log_dir(out).join("feedback.jsonl")
}

pub fn snapshots_dir(out: &Path) -> PathBuf {
    set_log_dir(out).join("snapshots")
}

pub fn benchmark_dir(out: &Path) -> PathBuf {
    set_log_dir(out).join("benchmark")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_paths_follow_the_documented_layout() {
        let out = Path::new("/p/.jazyk/my-set-ks8mf");
        assert_eq!(
            store_file(out),
            Path::new("/p/.jazyk/my-set-ks8mf/store.yaml")
        );
        assert_eq!(
            build_file(out),
            Path::new("/p/.jazyk/my-set-ks8mf/local/build.yaml")
        );
        assert_eq!(
            journal_file(out, 3),
            Path::new("/p/.jazyk/my-set-ks8mf/local/journal/g3.yaml")
        );
        assert_eq!(
            derived_node_dir(out, "relationships"),
            Path::new("/p/.jazyk/my-set-ks8mf/derived/graph/relationships")
        );
        assert_eq!(
            diagrams_dir(out),
            Path::new("/p/.jazyk/my-set-ks8mf/derived/diagrams")
        );
        assert_eq!(
            decisions_dir(out, "pairs"),
            Path::new("/p/.jazyk/my-set-ks8mf/graph/decisions/pairs")
        );
        assert_eq!(
            source_file(out, "docs/a.md"),
            Path::new("/p/.jazyk/my-set-ks8mf/sources/docs/a.md.yaml")
        );
        assert_eq!(
            archive_dir(out, 3),
            Path::new("/p/.jazyk/local/archive/my-set-ks8mf-v3")
        );
        assert_eq!(
            archive_dir(Path::new("/tmp/store"), 3),
            Path::new("/tmp/store.v3")
        );
    }

    // Machine state lives in a `local/` folder, never at the top of `.jazyk/` or of a set
    // directory. Mirrors docs/compiler/jazyk-folder.md#local.
    #[test]
    fn machine_state_lives_under_local() {
        let jazyk = Path::new("/p/.jazyk");
        let out = Path::new("/p/.jazyk/my-set-ks8mf");
        assert_eq!(runtime_dir(jazyk), Path::new("/p/.jazyk/local/runtime"));
        assert_eq!(
            job_cancellation_dir(jazyk),
            Path::new("/p/.jazyk/local/job-cancellation")
        );
        assert_eq!(
            jobs_stopped_file(out),
            Path::new("/p/.jazyk/my-set-ks8mf/local/jobs-stopped")
        );
        assert_eq!(
            jobs_stopped_file(jazyk),
            Path::new("/p/.jazyk/local/jobs-stopped")
        );
        assert_eq!(
            edit_sink_socket(jazyk, 42),
            Path::new("/p/.jazyk/local/acp-edit-sink-42.sock")
        );
        assert_eq!(
            edit_sink_socket(out, 42),
            Path::new("/p/.jazyk/my-set-ks8mf/local/acp-edit-sink-42.sock")
        );
        assert_eq!(legacy_edit_sink_pid("acp-edit-sink-42.sock"), Some(42));
        assert_eq!(legacy_edit_sink_pid("acp-edit-sink-x.sock"), None);
        assert_eq!(legacy_edit_sink_pid("store.yaml"), None);
    }

    // A socket path past the platform limit binds outside the project instead of failing.
    #[test]
    fn a_long_edit_socket_path_moves_out_of_the_project() {
        let deep = PathBuf::from("/").join("d".repeat(120)).join(".jazyk");
        let path = edit_sink_socket(&deep, 42);
        assert!(!path.starts_with(&deep), "{}", path.display());
        assert!(
            path.ends_with("jazyk-acp-edit-sink-42.sock"),
            "{}",
            path.display()
        );
    }

    // What an earlier version kept outside `local/` moves once and leaves nothing behind;
    // what is already at the new place wins.
    #[test]
    fn legacy_state_moves_once() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        let new = dir.path().join("local").join("new");
        std::fs::create_dir_all(old.join("runs/a")).unwrap();
        std::fs::write(old.join("runs/a/state.json"), "old a").unwrap();
        std::fs::write(old.join("only-old"), "kept").unwrap();
        std::fs::create_dir_all(new.join("runs/b")).unwrap();
        std::fs::write(new.join("runs/b/state.json"), "new b").unwrap();
        std::fs::write(new.join("only-old"), "newer").unwrap();
        adopt_legacy(&old, &new);
        assert!(!old.exists());
        assert_eq!(
            std::fs::read_to_string(new.join("runs/a/state.json")).unwrap(),
            "old a"
        );
        assert_eq!(
            std::fs::read_to_string(new.join("runs/b/state.json")).unwrap(),
            "new b"
        );
        assert_eq!(
            std::fs::read_to_string(new.join("only-old")).unwrap(),
            "newer"
        );
        let flag = dir.path().join("jobs-stopped");
        let moved = dir.path().join("local").join("jobs-stopped");
        std::fs::write(&flag, "stopped").unwrap();
        adopt_legacy(&flag, &moved);
        assert!(!flag.exists());
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "stopped");
        adopt_legacy(&flag, &moved);
        assert!(moved.exists(), "a second call changes nothing");
    }

    #[test]
    fn logs_live_outside_the_project_keyed_by_its_path() {
        let a = set_log_dir(Path::new("/work/shop/.jazyk/main-4kq7z"));
        let b = set_log_dir(Path::new("/other/shop/.jazyk/main-4kq7z"));
        assert!(!a.starts_with("/work/shop"), "{}", a.display());
        assert_ne!(a, b, "two projects of one name keep separate logs");
        assert!(a.ends_with("main-4kq7z"));
        let project = a
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(
            project.starts_with("shop-") && project.len() == "shop-".len() + 8,
            "{project}"
        );
        assert_eq!(
            trace_dir(Path::new("/work/shop/.jazyk/main-4kq7z")),
            a.join("trace")
        );
    }
}
