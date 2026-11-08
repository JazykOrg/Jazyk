// A skill folder's file listing does not follow a symlinked directory: a link back
// into the folder lists each file once. Mirrors docs/frontends/agent-context.md#skills.
use super::*;

#[cfg(unix)]
#[test]
fn a_skill_folder_with_a_symlink_loop_lists_each_file_once() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join(".agents/skills/cart");
    std::fs::create_dir_all(folder.join("refs")).unwrap();
    std::fs::write(
        folder.join("SKILL.md"),
        "---\nname: cart\ndescription: Cart rules.\n---\nUse the cart.\n",
    )
    .unwrap();
    std::fs::write(folder.join("refs/rules.md"), "rules\n").unwrap();
    std::os::unix::fs::symlink(&folder, folder.join("refs/loop")).unwrap();
    let (found, warnings) = skills(root.path(), None);
    assert!(warnings.is_empty(), "{warnings:?}");
    let reply: Value = serde_json::from_str(&read_skill(&found, "cart").unwrap()).unwrap();
    assert_eq!(reply["files"], json!(["refs/rules.md"]), "{reply}");
}
