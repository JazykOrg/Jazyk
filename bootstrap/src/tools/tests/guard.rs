use super::*;

#[test]
fn runtime_observation_is_not_a_repeated_graph_mutation() {
    let files = std::collections::BTreeMap::from([(
        ".jazyk/jazyk.toml".into(),
        b"version=1\nsets=[]\n".to_vec(),
    )]);
    let _scope = crate::vfs::Scope::enter(&files);
    let mut tool = session();
    tool.gen.deliverable = "/project".into();
    tool.snapshot.out = "/project/.jazyk/ds_main".into();
    for _ in 0..5 {
        let result = tool.dispatch("task_context", &json!({})).unwrap();
        assert!(result["tasks"].is_array());
        let error = tool
            .dispatch("wait_run", &json!({"run_id":"pending","wait_ms":1}))
            .unwrap_err();
        assert_ne!(error.rule, "repeated-call");
    }
}

// The guard: the second identical call is marked, the third refused; a load of a
// loaded target counts; done and the goal claims are exempt.
// Mirrors docs/compiler/sessions.md#repeated-calls.
#[test]
fn repeated_calls_are_marked_then_refused_with_exemptions() {
    let mut t = session();
    let args = json!({"query": "cart"});
    assert!(t.dispatch("search", &args).unwrap().get("repeat").is_none());
    assert!(t.dispatch("search", &args).unwrap()["repeat"].is_string());
    let err = t.dispatch("search", &args).unwrap_err();
    assert_eq!(err.rule, "repeated-call");
    // A load of an already loaded target is a repeat whatever its depth.
    t.dispatch("load", &json!({"target": "ent:customer"}))
        .unwrap();
    let v = t
        .dispatch("load", &json!({"target": "ent:customer", "depth": 2}))
        .unwrap();
    assert!(v["repeat"].is_string());
    let err = t
        .dispatch("load", &json!({"target": "ent:customer"}))
        .unwrap_err();
    assert_eq!(err.rule, "repeated-call");
    // done, mark_goal_done, and mark_goal_failed are exempt: a repaired claim
    // legitimately repeats.
    for _ in 0..4 {
        let err = t.dispatch("done", &json!({"summary": "x"})).unwrap_err();
        assert_ne!(err.rule, "repeated-call");
    }
    // A repeat whose answer changed is no repeat: the state moved between them.
    let args = json!({"query": "wishlist"});
    t.dispatch("search", &args).unwrap();
    t.snapshot
        .graph
        .entities
        .insert("ent:wishlist".into(), plain("Wishlist"));
    assert!(t.dispatch("search", &args).unwrap().get("repeat").is_none());
    assert!(t.dispatch("search", &args).unwrap()["repeat"].is_string());
}

// A refused load never counts, an alias and its id are one load, and a third
// identical call whose state moved answers instead of being refused.
// Mirrors docs/compiler/sessions.md#repeated-calls.
#[test]
fn repeated_call_guard_follows_state_not_raw_arguments() {
    let mut t = session();
    for _ in 0..4 {
        let err = t
            .dispatch("load", &json!({"target": "ent:nope"}))
            .unwrap_err();
        assert_ne!(err.rule, "repeated-call");
    }
    t.dispatch("load", &json!({"target": "Customer"})).unwrap();
    let v = t
        .dispatch("load", &json!({"target": "ent:customer"}))
        .unwrap();
    assert!(v["repeat"].is_string(), "{}", v);
    // A read whose answer moved after its marked repeat answers on the third call.
    let args = json!({"query": "gizmo"});
    t.dispatch("search", &args).unwrap();
    assert!(t.dispatch("search", &args).unwrap()["repeat"].is_string());
    t.snapshot
        .graph
        .entities
        .insert("ent:gizmo".into(), plain("Gizmo"));
    let v = t.dispatch("search", &args).unwrap();
    assert!(v.get("repeat").is_none(), "{}", v);
    // A write refused twice answers again once the session staged a mutation.
    let args = json!({"id": "ent:sprocket", "reason": "unused"});
    t.dispatch("delete_entity", &args).unwrap_err();
    t.dispatch("delete_entity", &args).unwrap_err();
    let up = json!({"name": "Sprocket", "definition": "a toothed wheel", "mention": {"section": "shop.md#/shop/cart", "quote": "a Sprocket"}});
    t.dispatch("upsert_entity", &up).unwrap();
    let third = t.dispatch("delete_entity", &args);
    assert!(
        third.as_ref().err().map(|e| e.rule.as_str()) != Some("repeated-call"),
        "{:?}",
        third.err()
    );
    // With nothing moved, the third call is refused.
    let args = json!({"id": "ent:ghost", "reason": "unused"});
    t.dispatch("delete_entity", &args).unwrap_err();
    t.dispatch("delete_entity", &args).unwrap_err();
    assert_eq!(
        t.dispatch("delete_entity", &args).unwrap_err().rule,
        "repeated-call"
    );
    // A commit between per-call sessions moves the state as well.
    let args = json!({"id": "ent:phantom", "reason": "unused"});
    t.dispatch("delete_entity", &args).unwrap_err();
    t.dispatch("delete_entity", &args).unwrap_err();
    t.snapshot.status.generation += 1;
    assert_ne!(
        t.dispatch("delete_entity", &args).unwrap_err().rule,
        "repeated-call"
    );
}

// A refused write stages nothing and leaves no lookup entry behind.
// Mirrors docs/compiler/tools.md#write-tools.
#[test]
fn a_refused_write_leaves_no_staged_lookup_entries() {
    let mut t = session();
    t.snapshot
        .graph
        .entities
        .insert("ent:gadget".into(), plain("Gadget"));
    t.mutation_limit = 0;
    let up = json!({"name": "Sprocket", "definition": "a toothed wheel", "mention": {"section": "shop.md#/shop/cart", "quote": "a Sprocket"}});
    assert_eq!(
        t.dispatch("upsert_entity", &up).unwrap_err().rule,
        "mutation-budget"
    );
    let err = t
        .dispatch(
            "update_entity",
            &json!({"id": "ent:customer", "parent": "ent:gadget"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "mutation-budget");
    assert!(t.staged.is_empty());
    assert!(t.staged_entities.is_empty());
    assert!(t.staged_parents.is_empty());
    assert!(t.taken_ids.is_empty());
    // With budget again, the same create mints the same id it would have.
    t.mutation_limit = 64;
    let v = t.dispatch("upsert_entity", &up).unwrap();
    assert_eq!(v["id"], "ent:sprocket", "{}", v);
}
