// The existing-test search walks the deliverable without following a symlinked
// directory: a link back up the tree lists each file once and ends.
// Mirrors docs/compiler/goals/bind.md#gate.
use super::*;

#[cfg(unix)]
#[test]
fn the_existing_test_search_lists_each_file_once_through_a_symlink_loop() {
    let root = tempfile::tempdir().unwrap();
    let deliverable = root.path();
    std::fs::create_dir_all(deliverable.join("tests")).unwrap();
    std::fs::write(
        deliverable.join("tests/cart_test.rs"),
        "// req:cart-1 abcd1234\nfn cart() {}\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(deliverable, deliverable.join("tests/loop")).unwrap();
    std::os::unix::fs::symlink(deliverable.join("tests"), deliverable.join("alias")).unwrap();
    let hits = existing_tests(deliverable, &["req:cart-1".to_string()]);
    let files: Vec<&str> = hits.iter().filter_map(|h| h["file"].as_str()).collect();
    assert_eq!(files, vec!["tests/cart_test.rs"], "{hits:?}");
}
