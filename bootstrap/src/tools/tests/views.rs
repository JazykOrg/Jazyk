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
        &json!({"id": "view:class/cart-parts", "add_members": ["ent:cart"]}),
    )
    .unwrap();
    // Empty means absent: members [] is a filled-in blank, not "replace the
    // membership with nothing".
    t.dispatch(
        "update_view",
        &json!({"id": "view:class/cart-parts", "members": [], "add_members": ["ent:customer"]}),
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
            &json!({"id": "view:class/cart-parts", "add_members": ["req:shop-1"]}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-member");
    let err = t
        .dispatch(
            "update_view",
            &json!({"id": "view:class/nope", "title": "x"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unknown-id");
    let err = t
        .dispatch(
            "delete_view",
            &json!({"id": "view:class/public", "reason": "noise"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "default-view");
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
