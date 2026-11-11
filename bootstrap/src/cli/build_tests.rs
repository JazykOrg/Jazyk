use super::build::{watch_incomplete, watch_stores};
use super::*;
use crate::store::Store;

// `check` and `watch` take no paths, `compile` drops the inert `--dirty`, and a
// compile path selects exactly the sets claiming it
// (docs/frontends/cli.md#jazyk-compile, #project-discovery).
#[test]
fn compile_paths_select_claiming_sets_and_other_builds_take_none() {
    use serde_json::json;
    for cmd in ["check", "watch"] {
        assert_eq!(
            crate::launcher::command_shape(cmd).unwrap().0,
            Some(0),
            "{}",
            cmd
        );
        assert!(
            !crate::launcher::cmd_usage(cmd).unwrap().contains("path..."),
            "{}",
            cmd
        );
    }
    let (_, extra) = crate::launcher::command_shape("compile").unwrap();
    assert!(!extra.contains(&"--dirty"));
    assert!(!crate::launcher::cmd_usage("compile")
        .unwrap()
        .contains("--dirty"));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs/orders")).unwrap();
    std::fs::write(root.join("docs/orders/a.md"), "# A").unwrap();
    std::fs::write(root.join("docs/billing.md"), "# B").unwrap();
    for (id, docs) in [
        ("ds_orders", "docs/orders/**/*.md"),
        ("ds_billing", "docs/billing.md"),
    ] {
        let w = crate::document_sets::Workspace::load(root).unwrap();
        crate::document_sets::edit(
            root,
            &json!({"baseHash": w.hash, "set": {"id": id, "name": id, "docs": [docs]}}),
            false,
        )
        .unwrap();
    }
    let w = crate::document_sets::Workspace::load(root).unwrap();
    let claims = |path: &str| -> Vec<String> {
        w.sets
            .iter()
            .filter(|s| super::set_claims(&w, s, root, path))
            .map(|s| s.id.clone())
            .collect()
    };
    assert_eq!(claims("docs/orders/a.md"), vec!["ds_orders"]);
    assert_eq!(claims("docs/orders"), vec!["ds_orders"]);
    assert_eq!(claims("docs/billing.md"), vec!["ds_billing"]);
    assert!(claims("README.md").is_empty());
    assert_eq!(
        super::set_claims(&w, &w.sets[0], &root.join("docs"), "orders/a.md"),
        w.sets[0].id == "ds_orders"
    );
}

// Without `--set`, a watch retries when any set it compiles ended incomplete; the
// shared `.jazyk/` directory holds no set's verdict.
#[test]
fn watch_retries_follow_the_resolved_sets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs/orders")).unwrap();
    std::fs::write(root.join("docs/orders/a.md"), "# A").unwrap();
    std::fs::write(root.join("docs/billing.md"), "# B").unwrap();
    for (id, docs) in [
        ("ds_orders", "docs/orders/**/*.md"),
        ("ds_billing", "docs/billing.md"),
    ] {
        let w = crate::document_sets::Workspace::load(root).unwrap();
        crate::document_sets::edit(
            root,
            &serde_json::json!({"baseHash": w.hash, "set": {"id": id, "name": id, "docs": [docs]}}),
            false,
        )
        .unwrap();
    }
    let opts = Options::default();
    let shared = root.join(".jazyk");
    let stores = watch_stores(root, root, &[], &opts, &shared);
    assert_eq!(stores.len(), 2, "{:?}", stores);
    assert!(!stores.contains(&shared));
    assert!(!watch_incomplete(&stores));
    let billing = root.join(".jazyk/ds_billing");
    assert!(stores.contains(&billing), "{:?}", stores);
    let mut s = Store::load(&billing);
    s.status.verdict.state = "incomplete".into();
    s.write_status("test").unwrap();
    assert!(watch_incomplete(&stores));
    let orders_only = watch_stores(root, root, &["docs/orders".to_string()], &opts, &shared);
    assert_eq!(orders_only, vec![root.join(".jazyk/ds_orders")]);
    assert!(!watch_incomplete(&orders_only));
    let named = Options {
        set: Some("ds_orders".into()),
        ..Default::default()
    };
    assert_eq!(
        watch_stores(root, root, &[], &named, &billing),
        vec![billing.clone()]
    );
}

// The verdict line the build prints last carries the counts, in both shapes.
// Mirrors docs/frontends/cli.md#jazyk-compile.
#[test]
fn the_verdict_line_renders_its_counts() {
    let v = crate::model::Verdict {
        state: "converged".into(),
        blocked: 2,
        optional: 1,
        ..Default::default()
    };
    let r = reconcile::BuildReport {
        verdict: v.to_string(),
        ..Default::default()
    };
    assert_eq!(verdict_line(&r), "converged, 2 blocked, 1 optional advised");

    let v = crate::model::Verdict {
        state: "incomplete".into(),
        open: 3,
        failed: 1,
        blocked: 2,
        optional: 5,
    };
    let r = reconcile::BuildReport {
        verdict: v.to_string(),
        ..Default::default()
    };
    assert_eq!(
        verdict_line(&r),
        "incomplete: 3 open, 1 failed, 2 blocked, 5 optional advised"
    );
}
