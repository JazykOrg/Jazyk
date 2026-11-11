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

/// The marker present while a commit writes.
pub fn committing_file(out: &Path) -> PathBuf {
    local_dir(out).join(".committing")
}

pub fn decompile_dir(out: &Path) -> PathBuf {
    local_dir(out).join("decompile")
}

pub fn deliverable_baseline_dir(out: &Path) -> PathBuf {
    local_dir(out).join("deliverable-baseline")
}

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
/// the operating system's cache directory, then `<project>-<key>`. A scoped (hosted or
/// browser) file system keeps its logs inside its own snapshot under
/// `.jazyk/local/records/`. Mirrors docs/compiler/jazyk-folder.md#logs.
pub fn project_log_dir(out: &Path) -> PathBuf {
    let jazyk = jazyk_dir_of(out).map(Path::to_path_buf);
    if crate::vfs::is_scoped() {
        return match &jazyk {
            Some(j) => project_local_dir(j).join("records"),
            None => out.join("local").join("records"),
        };
    }
    let root = jazyk
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| out.to_path_buf());
    native_log_dir(&root)
}

/// The log directory of the project at `root` (the folder holding `.jazyk/`).
pub fn log_dir_for_root(root: &Path) -> PathBuf {
    if crate::vfs::is_scoped() {
        return project_local_dir(&root.join(".jazyk")).join("records");
    }
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
