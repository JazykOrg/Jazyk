use super::tests::{fixture, multi_llm_fixture};
use super::*;
use crate::model::*;

// A directory link back to an ancestor is skipped, not followed in a loop.
#[cfg(unix)]
#[test]
fn file_walk_skips_symlinked_directories() {
    let root =
        std::env::temp_dir().join(format!("jazyk-verify-symlink-{}", crate::sys::process_id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(root.join("tests/a.rs"), "fn a() {}").unwrap();
    std::os::unix::fs::symlink(&root, root.join("tests/loop")).unwrap();
    let mut v = Vec::new();
    super::audit::collect_files(&root, &mut v, false);
    assert_eq!(v, vec![(root.join("tests/a.rs"), false)]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn audit_clears_all_criterion_verdicts_when_implementing_files_change() {
    let _scope = crate::vfs::Scope::enter(&Default::default());
    let out = std::path::Path::new("/project/.jazyk");
    let (store, gs) = multi_llm_fixture(out);
    let mut ledger = Ledger::load(out);
    let row = ledger.requirements.get_mut("req:shop-1").unwrap();
    // Audit recognizes a criteria headline by the requirement statement hash.
    let path = artifact_path(out, &gs, &row.test);
    let mut text = crate::vfs::read_to_string(&path).unwrap();
    text.push_str(&format!("\n{}\n", row.hashes.requirement));
    crate::vfs::write(path, &text).unwrap();
    row.hashes.test = hash_hex(&text);
    row.tests.get_mut("crit:first").unwrap().test_hash = row.hashes.test.clone();
    ledger.save(out).unwrap();
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// source changed").unwrap();
    audit(&store, &gs);
    assert!(Ledger::load(out).requirements["req:shop-1"]
        .tests
        .values()
        .all(|t| t.verdict == "none"));
}

#[test]
fn audit_never_launders_stale_requirement() {
    let out = std::env::temp_dir().join(format!("jazyk-audit-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, gs) = fixture(&out);
    mark(&s, "req:shop-1", "pass", None, None, None, &gs).unwrap();
    // Reword the requirement: stale-requirement.
    s.graph
        .requirements
        .get_mut("req:shop-1")
        .unwrap()
        .statement = "The Cart shall hold many items.".into();
    assert_eq!(
        pending(&s, &gs, None, None)[0]["status"],
        "stale-requirement"
    );
    // Audit must not launder it into verified: the artifact carries the OLD name.
    audit(&s, &gs);
    assert_eq!(
        pending(&s, &gs, None, None)[0]["status"],
        "stale-requirement"
    );
}

// A lost programmatic row is reported, never recreated: the harness cannot know
// the command that runs the test, and a row with none is a verify goal that can
// never resolve. The requirement stays missing and binds. A lost llm row is
// recreated from its criteria file, matched on the front matter the built-in
// worker writes (the requirement id and the full statement hash), since the test
// name is not in it. Mirrors docs/consumers/gen.md#runners.
#[test]
fn missing_rows_surface_and_audit_rebuilds() {
    let out = std::env::temp_dir().join(format!("jazyk-rebuild-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, gs) = fixture(&out);
    // Wipe the ledger: the row must surface as missing/not-generated with a repair.
    for f in crate::gen::Ledger::files(&out) {
        crate::vfs::remove_file(f).unwrap();
    }
    let p = pending(&s, &gs, None, None);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0]["status"], "missing");
    assert_eq!(p[0]["reason"], "not-generated");
    assert!(p[0]["repair"]
        .as_str()
        .unwrap()
        .contains("jazyk gen ent:cart"));
    // Audit finds the programmatic test (tests/cart.rs carries the name) but
    // records no row for it: binding records the command.
    let r = audit(&s, &gs);
    assert!(r["rebuilt"].as_array().unwrap().is_empty(), "{}", r);
    assert_eq!(r["found"].as_array().unwrap().len(), 1, "{}", r);
    assert_eq!(r["found"][0]["artifact"], "tests/cart.rs");
    let p2 = pending(&s, &gs, None, None);
    assert_eq!(p2[0]["status"], "missing");
    assert!(crate::bind::pending(&s, &gs)
        .iter()
        .any(|b| b["requirement"] == "req:shop-1" && b["reason"] == "unbound"));

    // An llm row is rebuilt from its criteria file.
    s.graph.requirements.insert(
        "req:shop-2".into(),
        Requirement {
            statement: "The Cart shall look tidy.".into(),
            entities: vec!["ent:cart".into()],
            ..Default::default()
        },
    );
    let crit = out.join("gen").join("criteria");
    crate::vfs::create_dir_all(&crit).unwrap();
    crate::vfs::write(
        crit.join("req-shop-2.md"),
        format!(
            "---\nrequirement: req:shop-2\nhash: {}\n---\n\n# Verify req:shop-2\nInspect the Cart. PASS if tidy. FAIL otherwise.\n",
            hash_hex("The Cart shall look tidy.")
        ),
    )
    .unwrap();
    let r = audit(&s, &gs);
    assert_eq!(
        r["rebuilt"].as_array().unwrap(),
        &vec![json!("req:shop-2")],
        "{}",
        r
    );
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-2"];
    assert_eq!(row.test.kind, "llm");
    assert_eq!(row.test.artifact, "criteria/req-shop-2.md");
    assert_eq!(status_of(&s, "req:shop-2", row, &gs).0, "unverified");
    // A second audit refreshes it in place instead of losing it.
    let r = audit(&s, &gs);
    assert_eq!(r["refreshed"], 1, "{}", r);
    crate::vfs::remove_dir_all(&out).ok();
}

// An audit rebaselines hashes, never verdicts: a verified row whose implementing
// file moved comes out unverified, not verified over bytes nothing reran.
// Mirrors docs/consumers/gen.md#runners.
#[test]
fn audit_drops_a_verdict_whose_bytes_moved() {
    let out = std::env::temp_dir().join(format!("jazyk-audit-reset-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
    assert!(pending(&s, &gs, None, None).is_empty());
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// edited by hand").unwrap();
    assert_eq!(pending(&s, &gs, None, None)[0]["status"], "stale-code");
    let r = audit(&s, &gs);
    assert_eq!(r["verdictReset"].as_array().unwrap().len(), 1, "{}", r);
    let p = pending(&s, &gs, None, None);
    assert_eq!(p[0]["status"], "unverified", "{:?}", p);
    assert_eq!(p[0]["reason"], "never-run");
    // Unchanged bytes keep their verdict across an audit.
    mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
    let r = audit(&s, &gs);
    assert!(r["verdictReset"].as_array().unwrap().is_empty(), "{}", r);
    assert!(pending(&s, &gs, None, None).is_empty());
    crate::vfs::remove_dir_all(&out).ok();
}
