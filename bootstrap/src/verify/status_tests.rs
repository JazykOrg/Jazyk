use super::tests::fixture;
use super::*;
use crate::gen::{mark as gen_mark, test_name, GenSettings};

// A row whose requirement left the graph is not actionable work: the queue
// excludes it, and the next record buries it.
// Mirrors docs/consumers/gen.md#deletion-prunes-the-ledger.
#[test]
fn requirement_gone_rows_are_not_actionable() {
    let out = std::env::temp_dir().join(format!("jazyk-verify-gone-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, gs) = fixture(&out);
    // The row exists; then the requirement is deleted from the graph.
    s.graph.requirements.remove("req:shop-1");
    let listed = pending(&s, &gs, Some("all"), None);
    assert!(
        !listed.iter().any(|p| p["requirement"] == "req:shop-1"),
        "{:?}",
        listed
    );
}

#[test]
fn status_matrix_and_cascade() {
    let out = std::env::temp_dir().join(format!("jazyk-verify-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, gs) = fixture(&out);

    // Fresh row: unverified, never-run.
    let p = pending(&s, &gs, None, None);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0]["status"], "unverified");
    assert_eq!(p[0]["reason"], "never-run");

    // A pass verdict rebaselines and verifies.
    mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
    assert!(pending(&s, &gs, None, None).is_empty());

    // Hand-editing an implementing file flips stale-code.
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// edited product").unwrap();
    let p = pending(&s, &gs, None, None);
    assert_eq!(p[0]["status"], "stale-code");
    mark(&s, "req:shop-1", "pass", None, None, None, &gs).unwrap();

    // Editing the test artifact flips stale-test.
    let name = test_name("req:shop-1", "The Cart shall hold items.");
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        format!("// changed\nfn {}() {{}}\n", name),
    )
    .unwrap();
    assert_eq!(pending(&s, &gs, None, None)[0]["status"], "stale-test");
    mark(&s, "req:shop-1", "pass", None, None, None, &gs).unwrap();

    // Rewording the requirement flips stale-requirement; task refuses; test refuses.
    s.graph
        .requirements
        .get_mut("req:shop-1")
        .unwrap()
        .statement = "The Cart shall hold many items.".into();
    let p = pending(&s, &gs, None, None);
    assert_eq!(p[0]["status"], "stale-requirement");
    assert!(task(&s, "req:shop-1", &gs)
        .unwrap_err()
        .contains("stale-requirement"));

    // A fail verdict surfaces as failing.
    s.graph
        .requirements
        .get_mut("req:shop-1")
        .unwrap()
        .statement = "The Cart shall hold items.".into();
    mark(&s, "req:shop-1", "fail", None, Some("boom"), Some(1), &gs).unwrap();
    // A failing row is a finding, never a verify goal: the default listing leaves
    // it alone; a rerun a person asked for still selects it
    // (docs/compiler/tools.md#verification-tools).
    assert!(pending(&s, &gs, None, None).is_empty());
    let runnable = pending(&s, &gs, Some("runnable"), None);
    assert_eq!(runnable[0]["status"], "failing");
    assert!(runnable[0]["repair"].as_str().unwrap().contains("finding"));
    assert_eq!(select_rows(&s, &gs, &[], None, false).len(), 1);

    // Programmatic run: `true` exits 0.
    let pass = run_programmatic(&s, "req:shop-1", &gs).unwrap().pass;
    assert!(pass);
}

fn scoped_fixture() -> (Store, GenSettings) {
    let (store, mut gs) = fixture(std::path::Path::new("/project/.jazyk/ds_main"));
    let old = gs.deliverable.clone();
    gs.deliverable = "/project".into();
    gs.code = vec!["src/**".into(), "tests/**".into()];
    for file in ["src/cart.rs", "tests/cart.rs"] {
        let path = gs.deliverable.join(file);
        crate::vfs::create_dir_all(path.parent().unwrap()).unwrap();
        crate::vfs::write(path, crate::vfs::read(old.join(file)).unwrap()).unwrap();
    }
    scoped_mapping(&[]);
    mark(&store, "req:shop-1", "pass", None, None, Some(0), &gs).unwrap();
    (store, gs)
}

fn scoped_mapping(extra_docs: &[&str]) {
    let docs: Vec<_> = std::iter::once("docs/**")
        .chain(extra_docs.iter().copied())
        .collect();
    crate::vfs::write("/project/.jazyk/jazyk.toml", format!(
        "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Main'\ndocs = {}\nsource = ['src/**', 'tests/**']\n",
        serde_json::to_string(&docs).unwrap()
    )).unwrap();
}

#[test]
fn document_reclassification_reopens_binding_and_generation_without_touching_files() {
    let _vfs = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let (store, gs) = scoped_fixture();
    assert!(pending(&store, &gs, None, None).is_empty());
    assert!(crate::bind::pending(&store, &gs).is_empty());
    assert!(crate::gen::pending(&store, &gs).is_empty());
    let before = crate::vfs::read(store.out.join("gen/ledger.yaml")).unwrap();
    let content = crate::vfs::read(gs.deliverable.join("src/cart.rs")).unwrap();
    scoped_mapping(&["src/cart.rs"]);
    let ledger = Ledger::load(&store.out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(
        status_of(&store, "req:shop-1", row, &gs),
        ("missing".into(), "source-selection-changed".into())
    );
    assert_eq!(
        crate::bind::pending(&store, &gs)[0]["reason"],
        "source-selection-changed"
    );
    assert_eq!(
        crate::gen::pending(&store, &gs)[0]["reason"],
        "source-selection-changed"
    );
    assert!(crate::bind::gate(&store, &gs, "req:shop-1")
        .unwrap_err()
        .contains("current Source"));
    assert!(gate(&store, &gs, "req:shop-1").is_err());
    assert!(crate::gen::gate(&store, &gs, "ent:cart", false)
        .unwrap_err()
        .contains("current Source"));
    assert!(task(&store, "req:shop-1", &gs)
        .unwrap_err()
        .contains("source-selection-changed"));
    assert!(mark(&store, "req:shop-1", "pass", None, None, Some(0), &gs)
        .unwrap_err()
        .contains("source-selection-changed"));
    assert!(run_programmatic(&store, "req:shop-1", &gs)
        .err()
        .unwrap()
        .contains("source-selection-changed"));
    assert!(run_criterion_tests(&store, "req:shop-1", &gs)[0]["note"]
        .as_str()
        .unwrap()
        .contains("source-selection-changed"));
    let test = json!({"kind":"programmatic","artifact":row.test.artifact,"name":row.test.name,"run":"true"});
    assert!(crate::bind::record(
        &store,
        "req:shop-1",
        &["src/cart.rs".into()],
        &test,
        &Value::Null,
        "pass",
        None,
        &gs
    )
    .is_err());
    let stale_manifest = json!({"files":["src/cart.rs"],"tests":[]});
    assert!(gen_mark(&store, "ent:cart", None, &stale_manifest, &gs).is_err());
    assert!(crate::gen::confine_in(&gs, "src/cart.rs", &mut Vec::new()).is_err());
    assert_eq!(
        crate::vfs::read(store.out.join("gen/ledger.yaml")).unwrap(),
        before
    );
    assert_eq!(
        crate::vfs::read(gs.deliverable.join("src/cart.rs")).unwrap(),
        content
    );
    // Binding honestly records no implementation, then generation writes inside
    // Source. The file assigned to Docs is preserved through that repair.
    crate::bind::record(
        &store,
        "req:shop-1",
        &[],
        &test,
        &Value::Null,
        "fail",
        None,
        &gs,
    )
    .unwrap();
    let ledger = Ledger::load(&store.out);
    assert!(ledger.requirements["req:shop-1"].files.is_empty());
    assert_eq!(
        status_of(
            &store,
            "req:shop-1",
            &ledger.requirements["req:shop-1"],
            &gs
        )
        .0,
        "unimplemented"
    );
    assert!(crate::bind::pending(&store, &gs).is_empty());
    assert!(!crate::gen::pending(&store, &gs).is_empty());
    assert_eq!(
        crate::vfs::read(gs.deliverable.join("src/cart.rs")).unwrap(),
        content
    );
    crate::vfs::write(
        gs.deliverable.join("src/cart-new.rs"),
        "// replacement product",
    )
    .unwrap();
    gen_mark(
        &store,
        "ent:cart",
        None,
        &json!({
            "files":["src/cart-new.rs"], "tests":[{
                "requirement":"req:shop-1", "kind":"programmatic", "artifact":row.test.artifact,
                "name":row.test.name, "run":"true", "files":["src/cart-new.rs"]
            }]
        }),
        &gs,
    )
    .unwrap();
    assert!(crate::gen::pending(&store, &gs).is_empty());
    assert_eq!(
        crate::vfs::read(gs.deliverable.join("src/cart.rs")).unwrap(),
        content
    );
}

#[test]
fn test_reclassification_reopens_development_without_treating_tests_as_implementation() {
    let _vfs = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let (store, gs) = scoped_fixture();
    scoped_mapping(&["tests/cart.rs"]);
    let ledger = Ledger::load(&store.out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(
        status_of(&store, "req:shop-1", row, &gs).1,
        "source-selection-changed"
    );
    assert_eq!(crate::bind::pending(&store, &gs).len(), 1);
    // Development can repair the missing current test binding. Its implementing
    // file remains Source, so this is incomplete coverage rather than lost code.
    assert_eq!(
        crate::gen::pending(&store, &gs),
        vec![json!({
            "entity":"ent:cart", "reason":"incomplete-bindings", "changed":["req:shop-1"]
        })]
    );
    assert_eq!(row.files, ["src/cart.rs"]);
    scoped_mapping(&[]);
    assert_eq!(status_of(&store, "req:shop-1", row, &gs).0, "verified");
    assert!(crate::gen::pending(&store, &gs).is_empty());
    // Internal LLM criteria are metadata, not an implementing path that must be
    // added to Source. Criterion artifacts outside it still need rebinding.
    let mut internal = row.clone();
    internal.test.kind = "llm".into();
    internal.test.artifact = "criteria/cart.md".into();
    crate::vfs::create_dir_all(store.out.join("gen/criteria")).unwrap();
    crate::vfs::write(store.out.join("gen/criteria/cart.md"), "metadata").unwrap();
    assert!(crate::gen::binding_source_current(
        &store.out, &gs, &ledger, &internal
    ));
    internal.tests.insert(
        "crit:cart-1".into(),
        crate::gen::CritTest {
            test: crate::gen::TestRef {
                artifact: "docs/test.rs".into(),
                ..row.test.clone()
            },
            criterion_hash: String::new(),
            test_hash: String::new(),
            verdict: "none".into(),
            last_run: None,
            exit_code: None,
            evidence: None,
        },
    );
    assert!(!crate::gen::binding_source_current(
        &store.out, &gs, &ledger, &internal
    ));
}

// A rebuilt artifact among a row's files is not a code change: the stale-code hash
// covers implementing files alone. Mirrors docs/consumers/gen.md#status-is-derived-never-stored.
#[test]
fn a_rebuilt_artifact_never_reopens_the_row() {
    let out = std::env::temp_dir().join(format!(
        "jazyk-verify-artifact-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    let mut ledger = crate::gen::Ledger::load(&out);
    ledger.medium = Some(crate::gen::Medium {
        form: "deck".into(),
        produced: "built".into(),
        toolchain: "python3".into(),
        artifact: "dist/deck.pptx".into(),
    });
    ledger
        .requirements
        .get_mut("req:shop-1")
        .unwrap()
        .files
        .push("dist/deck.pptx".into());
    ledger.save(&out).unwrap();
    crate::vfs::create_dir_all(gs.deliverable.join("dist")).unwrap();
    crate::vfs::write(gs.deliverable.join("dist/deck.pptx"), "zip v1").unwrap();
    mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
    assert!(pending(&s, &gs, None, None).is_empty());
    // The build rewrites the artifact with fresh bytes: nothing reopens.
    crate::vfs::write(
        gs.deliverable.join("dist/deck.pptx"),
        "zip v2 fresh timestamps",
    )
    .unwrap();
    assert!(
        pending(&s, &gs, None, None).is_empty(),
        "{:?}",
        pending(&s, &gs, None, None)
    );
    // An implementing file edit still does.
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// edited").unwrap();
    assert_eq!(pending(&s, &gs, None, None)[0]["status"], "stale-code");
    crate::vfs::remove_dir_all(&out).ok();
}

// The verify gate holds exactly when a verdict stands on a current row; every
// other state names the call that lands it. Mirrors docs/compiler/goals/verify.md#gate.
#[test]
fn the_verify_gate_reads_the_landed_verdict() {
    let out = std::env::temp_dir().join(format!("jazyk-verify-gate-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, gs) = fixture(&out);
    let err = gate(&s, &gs, "req:shop-1").unwrap_err();
    assert!(
        err.contains("no verdict") && err.contains("run_tests"),
        "{}",
        err
    );
    mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
    gate(&s, &gs, "req:shop-1").unwrap();
    mark(&s, "req:shop-1", "fail", None, Some("boom"), Some(1), &gs).unwrap();
    gate(&s, &gs, "req:shop-1").unwrap();
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// moved").unwrap();
    let err = gate(&s, &gs, "req:shop-1").unwrap_err();
    assert!(err.contains("stale-code"), "{}", err);
    mark(&s, "req:shop-1", "pass", None, None, None, &gs).unwrap();
    s.graph
        .requirements
        .get_mut("req:shop-1")
        .unwrap()
        .statement = "Reworded.".into();
    let err = gate(&s, &gs, "req:shop-1").unwrap_err();
    assert!(
        err.contains("moved since") && err.contains("jazyk gen"),
        "{}",
        err
    );
    assert!(gate(&s, &gs, "req:none").unwrap_err().contains("bind work"));
    crate::vfs::remove_dir_all(&out).ok();
}
