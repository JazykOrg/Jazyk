// The project file walk (document membership, the GUI's docs watcher fingerprints)
// visits each directory once: a symlink loop ends, and a file reachable through a
// link inside the root is listed under each path once.
// Mirrors docs/compiler/document-sets.md.
use super::*;

#[cfg(unix)]
#[test]
fn the_project_walk_ends_on_symlink_loops_and_lists_each_path_once() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("docs/deep")).unwrap();
    std::fs::write(root.join("docs/a.md"), "# A\n").unwrap();
    std::fs::write(root.join("docs/deep/b.md"), "# B\n").unwrap();
    std::os::unix::fs::symlink(&root, root.join("docs/deep/up")).unwrap();
    std::os::unix::fs::symlink(root.join("docs/deep"), root.join("docs/deep/self")).unwrap();
    let files = files_under(&root);
    assert_eq!(files, vec!["docs/a.md", "docs/deep/b.md"], "{files:?}");
}
