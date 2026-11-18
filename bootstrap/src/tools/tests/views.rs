use super::*;

// The view tools gate kind, membership, uniqueness, collapse, and defaults.
// Mirrors docs/compiler/tools.md#view-tools.
#[test]
fn view_gates_name_their_rules() {
    let mut t = session();
    t.snapshot
        .graph
        .entities
        .insert("ent:cart".into(), plain("Cart"));
    t.snapshot
        .graph
        .entities
        .insert("ent:item".into(), under("Item", "ent:cart"));
    t.snapshot.graph.requirements.insert(
        "req:shop-1".into(),
        quoted_req("The Cart holds items.", &["ent:cart"], "holds items"),
    );
    t.snapshot.graph.views.insert(
        "view:class/public".into(),
        View {
            kind: "class".into(),
            title: "Public".into(),
            members: vec!["ent:cart".into()],
            default: true,
            ..Default::default()
        },
    );
    let call = |members: Value, collapse: Value| json!({"kind": "class", "title": "Cart parts", "members": members, "collapse": collapse, "reasoning": "the cart's parts"});
    let err = t
        .dispatch(
            "upsert_view",
            &json!({"kind": "blueprint", "title": "X", "reasoning": "r"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-kind");
    let err = t
        .dispatch("upsert_view", &call(json!(["ent:ghost"]), json!([])))
        .unwrap_err();
    assert_eq!(err.rule, "unknown-id");
    let err = t
        .dispatch(
            "upsert_view",
            &call(json!(["ent:cart", "ent:cart"]), json!([])),
        )
        .unwrap_err();
    assert_eq!(err.rule, "duplicate-member");
    let err = t
        .dispatch("upsert_view", &call(json!(["req:shop-1"]), json!([])))
        .unwrap_err();
    assert_eq!(err.rule, "bad-member");
    let err = t
        .dispatch(
            "upsert_view",
            &call(json!(["ent:item"]), json!(["ent:customer"])),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-collapse");
    // An ancestor of a member collapses; a repeated upsert lands on the staged view.
    let v = t
        .dispatch(
            "upsert_view",
            &call(json!(["ent:item"]), json!(["ent:cart"])),
        )
        .unwrap();
    assert_eq!(v["id"], "view:class/cart-parts");
    assert_eq!(v["created"], true);
    let again = t
        .dispatch(
            "upsert_view",
            &call(json!(["ent:item"]), json!(["ent:cart"])),
        )
        .unwrap();
    assert_eq!(again["id"], "view:class/cart-parts");
    assert_eq!(again["created"], false);
    t.dispatch(
        "update_view",
        &json!({ "reasoning": "Fixture diagram authoring","id": "view:class/cart-parts", "add_members": ["ent:cart"]}),
    )
    .unwrap();
    // Empty means absent: members [] is a filled-in blank, not "replace the
    // membership with nothing".
    t.dispatch(
        "update_view",
        &json!({ "reasoning": "Fixture diagram authoring","id": "view:class/cart-parts", "members": [], "add_members": ["ent:customer"]}),
    )
    .unwrap();
    let staged = t
        .staged_views
        .get("view:class/cart-parts")
        .expect("staged view");
    assert!(staged.members.contains(&"ent:item".to_string()));
    assert!(staged.members.contains(&"ent:customer".to_string()));
    let err = t
        .dispatch(
            "update_view",
            &json!({ "reasoning": "Fixture diagram authoring","id": "view:class/cart-parts", "add_members": ["req:shop-1"]}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-member");
    let err = t
        .dispatch(
            "update_view",
            &json!({ "reasoning": "Fixture diagram authoring","id": "view:class/nope", "title": "x"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unknown-id");
    let deleted = t
        .dispatch(
            "delete_view",
            &json!({"id":"view:class/public","reason":"noise"}),
        )
        .unwrap();
    assert_eq!(deleted["deleted"], true);
    // The default's natural key lands on it; the store clears default at commit.
    let v = t
        .dispatch("upsert_view", &json!({"kind": "class", "title": "Public", "members": ["ent:cart"], "reasoning": "curated"}))
        .unwrap();
    assert_eq!(v["id"], "view:class/public");
    assert_eq!(v["created"], false);
    let err = t
        .dispatch("upsert_view", &json!({"kind": "sequence", "title": "Flow", "members": ["ent:cart"], "reasoning": "r"}))
        .unwrap_err();
    assert_eq!(err.rule, "bad-member");
    let v = t
        .dispatch("upsert_view", &json!({"kind": "sequence", "title": "Flow", "members": ["req:shop-1"], "excluded": [{"id": "ent:cart", "note": "structure, not flow"}], "reasoning": "r"}))
        .unwrap();
    assert_eq!(v["id"], "view:sequence/flow");
    t.dispatch(
        "delete_view",
        &json!({"id": "view:sequence/flow", "reason": "no"}),
    )
    .unwrap();
    assert!(matches!(t.staged.last(), Some(Op::DeleteView { .. })));
}

#[test]
fn every_catalog_kind_can_be_authored_edited_and_deleted_through_gated_tools() {
    for kind in VIEW_KINDS {
        let dir = tempfile::tempdir().unwrap();
        let mut t = session();
        t.snapshot.out = dir.path().to_path_buf();
        t.snapshot.graph.requirements.insert(
            "req:step".into(),
            quoted_req("The Customer buys.", &["ent:customer"], "Customer"),
        );
        let members = match kind {
            "class" | "object" | "package" | "component" | "composite" | "deployment" | "state" => {
                json!(["ent:customer"])
            }
            "timing" => json!(["ent:customer", "req:step"]),
            _ => json!(["req:step"]),
        };
        let created=t.dispatch("upsert_view",&json!({"kind":kind,"title":format!("Test {kind}"),"purpose":"Explain a sourced example","area":{"target":"ent:customer","extent":"selection"},"members":members,"reasoning":"The fixture statement supplies the members"})).unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        t.dispatch("update_view",&json!({"id":id,"description":"An edited explanation","reasoning":"Clarify the explanation"})).unwrap();
        assert!(crate::goals::kind("diagram-coverage")
            .unwrap()
            .gates(&t.snapshot, &t.staged)
            .is_empty());
        let mut store = t.snapshot.clone();
        let result = store.apply(t.staged.clone(), &crate::store::Commit::store("session"));
        assert_eq!(result.applied, 2, "{kind}: {:?}", result.skipped);
        assert_eq!(
            store.graph.views[&id].authored["description"],
            "An edited explanation"
        );
        let scope = WorkScope::for_batch("b1-1", &[]);
        let mut next = ToolSession::new(store, scope, 64, 24000);
        next.dispatch(
            "delete_view",
            &json!({"id":id,"reason":"The owner no longer needs this example"}),
        )
        .unwrap();
        let mut store = next.snapshot.clone();
        let result = store.apply(next.staged.clone(), &crate::store::Commit::store("session"));
        assert_eq!(result.applied, 1, "{kind}: {:?}", result.skipped);
        assert!(!store.graph.views.contains_key(&id));
        crate::derive::recompute(&mut store, "g3", &mut crate::store::RecordBatch::new(3));
        assert!(store.graph.views.is_empty());
    }
}

#[test]
fn a_diagram_review_suppression_commits_and_reopens_after_cumulative_growth() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store {
        out: dir.path().to_path_buf(),
        ..Default::default()
    };
    for i in 0..10 {
        let mut e = plain(&format!("Part {i}"));
        e.provenance = Some(Provenance::Decree {
            author: "Fixture owner".into(),
            at: "fixture".into(),
            note: Some("Independent glossary concept".into()),
        });
        store.graph.entities.insert(format!("ent:part-{i}"), e);
    }
    let gen = crate::gen::GenSettings::resolve(&crate::project::Project::default());
    let kind = crate::goals::kind("diagram-coverage").unwrap();
    let goal = kind.derive_goals(&store, &gen).remove(0);
    let mut t = ToolSession::new(
        store.clone(),
        WorkScope::for_batch("b1-1", std::slice::from_ref(&goal)),
        64,
        24000,
    );
    t.dispatch("suppress_diagram_coverage",&json!({"id":goal.target,"reason":"These independent glossary entries are clearer as an alphabetical list","expectedAreaHash":goal.change["areaHash"]})).unwrap();
    t.dispatch("mark_goal_done",&json!({"goal":goal.id,"justification":"Saved a reasoned suppression of this glossary area's structural suggestion"})).unwrap();
    t.dispatch("done", &json!({"summary":"Retain the glossary as a list"}))
        .unwrap();
    assert!(t.done.is_some());
    let commit = t.commit(1, None);
    let result = store.apply(t.staged, &commit);
    assert!(result.skipped.is_empty(), "{:?}", result.skipped);
    assert_eq!(Store::load(dir.path()).decisions.diagram_coverage.len(), 1);
    assert!(kind.derive_goals(&store, &gen).is_empty());
    store
        .graph
        .entities
        .insert("ent:part-10".into(), plain("Part 10"));
    assert!(
        kind.derive_goals(&store, &gen).is_empty(),
        "One new entry is below a fifth of the baseline"
    );
    store
        .graph
        .entities
        .insert("ent:part-11".into(), plain("Part 11"));
    assert_eq!(kind.derive_goals(&store, &gen).len(), 1);
    assert_eq!(
        store.decisions.diagram_coverage.len(),
        1,
        "Expired decisions remain inspectable"
    );
}
