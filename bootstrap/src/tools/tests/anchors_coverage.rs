use super::*;

// A section that yielded a statement cannot also state none of them.
#[test]
fn non_normative_contradicts_an_extracted_statement() {
    let mut t = session();
    t.dispatch(
        "upsert_requirement",
        &json!({
            "facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}],
            "statement": "The Shopping Cart shall hold items a Customer intends to buy.",
            "entities": ["ent:customer"],
            "section": "/shop/cart",
            "quote": "The Shopping Cart holds items a Customer intends to buy."
        }),
    )
    .unwrap();
    let err = t
        .dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "non-normative", "note": "just facts"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "contradicts-extraction");
    assert!(err.message.contains("covered"), "{}", err.message);
    // The honest mark still goes through.
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered"}),
    )
    .unwrap();
    // A section that yielded nothing is still free to be non-normative.
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop", "state": "non-normative", "note": "navigation only"}),
    )
    .unwrap();
}

// The listing renders the committed snapshot; a line a staged mutation touches
// says so (docs/compiler/context.md#the-loaded-set).
#[test]
fn staged_statement_is_noted_on_the_listing() {
    let mut t = session();
    t.dispatch("load", &json!({"target": "ent:customer"}))
        .unwrap();
    t.dispatch(
        "update_entity",
        &json!({"id": "ent:customer", "definition": "A Customer is a person who buys."}),
    )
    .unwrap();
    let status = t.dispatch("graph_status", &json!({})).unwrap()["status"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(status.contains("(staged: revised)"), "{}", status);
}

// Passing the statement statement as the quote is the common miscall; the rejection has to
// point at the existing anchor, not just repeat that the quote is absent.
#[test]
fn reanchor_rejection_names_the_existing_anchor() {
    let mut t = session();
    let id = t
        .dispatch(
            "upsert_requirement",
            &json!({
                "facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}],
                "statement": "The Shopping Cart shall hold items a Customer intends to buy.",
                "entities": ["ent:customer"],
                "section": "/shop/cart",
                "quote": "The Shopping Cart holds items a Customer intends to buy."
            }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let err = t
        .dispatch(
            "update_requirement",
            &json!({"id": id, "entities": ["ent:customer"], "section": "shop.md#/shop/cart",
                    "quote": "The Shopping Cart shall hold items a Customer intends to buy."}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "quote-not-found");
    assert!(
        err.message.contains("omit `section` and `quote`"),
        "{}",
        err.message
    );
    // Only-entities is the call that was meant, and it is accepted.
    t.dispatch(
        "update_requirement",
        &json!({"id": id, "entities": ["ent:customer"]}),
    )
    .unwrap();
}

#[test]
fn coverage_restage_replaces_earlier_mark() {
    let mut t = session();
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "non-normative", "note": "just prose"}),
    )
    .unwrap();
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered"}),
    )
    .unwrap();
    let marks: Vec<&Op> = t
        .staged
        .iter()
        .filter(|o| matches!(o, Op::SetCoverage { .. }))
        .collect();
    assert_eq!(marks.len(), 1);
    match marks[0] {
        Op::SetCoverage { state, .. } => assert_eq!(state, "covered"),
        _ => unreachable!(),
    }
}

#[test]
fn done_rejects_untouched_stale_anchor() {
    let mut t = session_with_stale_anchor();
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered"}),
    )
    .unwrap();
    let goal = "g:reconcile-section:shop.md#/shop/cart";
    let err = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": goal, "justification": "Covered."}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "stale-anchor");
    assert!(
        err.message.contains("req:shop-1"),
        "names the anchor: {}",
        err.message
    );
    // Failing the goal does not let the batch commit around the anchor.
    t.dispatch(
        "mark_goal_failed",
        &json!({"goal": goal, "reason": "cannot decide the anchor"}),
    )
    .unwrap();
    let err = t
        .dispatch("done", &json!({"summary": "covered around the anchor"}))
        .unwrap_err();
    assert_eq!(err.rule, "stale-anchor");
}

#[test]
fn stale_anchor_satisfied_by_delete() {
    let mut t = session_with_stale_anchor();
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered"}),
    )
    .unwrap();
    t.dispatch(
        "delete_requirement",
        &json!({"id": "req:shop-1", "reason": "the document dropped the statement"}),
    )
    .unwrap();
    t.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "The fact is gone; deleted the anchor."}),
    )
    .unwrap();
    t.dispatch("done", &json!({"summary": "anchor deleted"}))
        .unwrap();
    assert!(t.done.is_some());
}

#[test]
fn stale_anchor_satisfied_by_same_statement_reupsert() {
    let mut t = session_with_stale_anchor();
    // Same statement, fresh verbatim quote: the natural key resolves at stage time
    // and the model sees the anchor's id, not a misleading fresh one.
    let v = t
        .dispatch(
            "upsert_requirement",
            &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The Shopping Cart shall hold items a Customer intends to buy.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "The Shopping Cart keeps items a Customer intends to buy."}),
        )
        .unwrap();
    assert_eq!(v["id"], "req:shop-1");
    assert_eq!(v["updated"], true);
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered"}),
    )
    .unwrap();
    t.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "Re-recorded on the anchor."}),
    )
    .unwrap();
    t.dispatch("done", &json!({"summary": "re-anchored"}))
        .unwrap();
    assert!(t.done.is_some());
}

#[test]
fn stale_anchor_reworded_statement_resolves_to_the_anchor() {
    let mut t = session_with_stale_anchor();
    // The statement was reworded (it subsumes the anchor's) and the old quote no
    // longer locates: the upsert lands on the anchor's id, updating in place.
    let v = t
        .dispatch(
            "upsert_requirement",
            &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The Shopping Cart shall hold items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "The Shopping Cart keeps items a Customer intends to buy."}),
        )
        .unwrap();
    assert_eq!(v["id"], "req:shop-1");
    assert_eq!(v["updated"], true);
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered"}),
    )
    .unwrap();
    t.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "Reworded onto the anchor."}),
    )
    .unwrap();
    t.dispatch("done", &json!({"summary": "re-anchored reworded"}))
        .unwrap();
    assert!(t.done.is_some());
}

fn align_session() -> ToolSession {
    let mut s = Store::default();
    let text = "# Shop\nintro\n\n## Basket\nThe Basket keeps items a Customer intends to buy.\nItems stay until checkout.\n";
    s.docs.insert(
        "shop.md".into(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: BTreeMap::new(),
        },
    );
    s.graph.entities.insert(
        "ent:basket".into(),
        Entity {
            name: "Basket".into(),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:shop-1".into(),
        Requirement {
            statement: "The Basket shall keep items until checkout.".into(),
            entities: vec!["ent:basket".into()],
            edges: Vec::new(),
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop/cart".into(),
                quote: "Items stay until checkout.".into(),
            }),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:shop-2".into(),
        Requirement {
            statement: "The Basket shall hold items a Customer intends to buy.".into(),
            entities: vec!["ent:basket".into()],
            edges: Vec::new(),
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop/cart".into(),
                quote: "The Cart holds items a Customer intends to buy.".into(),
            }),
            ..Default::default()
        },
    );
    let candidate = |locates: bool| crate::model::AnchorCandidate {
        section: "shop.md#/shop/basket".into(),
        similarity: if locates { 1.0 } else { 0.8 },
        quote_locates: locates,
        nearest: (!locates)
            .then(|| "The Basket keeps items a Customer intends to buy.".to_string()),
        excerpt: String::new(),
    };
    s.status.alignment.push(crate::model::DocAlignment {
        doc: "shop.md".into(),
        changes: vec![],
        proposals: vec![
            crate::model::AnchorProposal {
                anchor: "req:shop-1".into(),
                from: "shop.md#/shop/cart".into(),
                quote: "Items stay until checkout.".into(),
                excerpt: String::new(),
                candidates: vec![candidate(true)],
            },
            crate::model::AnchorProposal {
                anchor: "req:shop-2".into(),
                from: "shop.md#/shop/cart".into(),
                quote: "The Cart holds items a Customer intends to buy.".into(),
                excerpt: String::new(),
                candidates: vec![candidate(false)],
            },
        ],
    });
    let goal = Goal {
        id: "g:place-anchors:shop.md".into(),
        kind: "place-anchors".into(),
        mandatory: true,
        target: "shop.md".into(),
        change: json!({"anchors": ["req:shop-1", "req:shop-2"]}),
        ..Default::default()
    };
    ToolSession::new(
        s,
        WorkScope::for_batch("b0-1", std::slice::from_ref(&goal)),
        64,
        24_000,
    )
}

#[test]
fn align_done_rejects_an_undecided_proposal() {
    let mut s = align_session();
    s.dispatch(
        "place_anchor",
        &json!({"id": "req:shop-1", "section": "/shop/basket", "reevaluate": false}),
    )
    .unwrap();
    let err = s
        .dispatch(
            "mark_goal_done",
            &json!({"goal": "g:place-anchors:shop.md", "justification": "Both proposals decided."}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "undecided-proposal");
    assert!(err.message.contains("req:shop-2"));
    let err = s.dispatch("done", &json!({"summary": "x"})).unwrap_err();
    assert_eq!(err.rule, "open-goal");
    s.dispatch("orphan_anchor", &json!({"id": "req:shop-2"}))
        .unwrap();
    s.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:place-anchors:shop.md", "justification": "Both proposals decided."}),
    )
    .unwrap();
    assert!(s.dispatch("done", &json!({"summary": "x"})).is_ok());
    assert_eq!(s.staged.len(), 2);
}

#[test]
fn place_anchor_gates_quote_and_section_and_rejects_strangers() {
    let mut s = align_session();
    let err = s
        .dispatch(
            "place_anchor",
            &json!({"id": "ent:basket", "section": "/shop/basket", "reevaluate": false}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unknown-anchor");
    let err = s
        .dispatch(
            "place_anchor",
            &json!({"id": "req:shop-2", "section": "/shop/nowhere", "reevaluate": false}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unknown-section");
    let err = s
        .dispatch("place_anchor", &json!({"id": "req:shop-2", "section": "/shop/basket", "quote": "not in the text", "reevaluate": false}))
        .unwrap_err();
    assert_eq!(err.rule, "quote-not-found");
    // Without a quote the old one rides along; the reply says it will not locate.
    let v = s
        .dispatch(
            "place_anchor",
            &json!({"id": "req:shop-2", "section": "/shop/basket", "reevaluate": false}),
        )
        .unwrap();
    assert_eq!(v["reevaluate"], true);
    // With the new sentence verbatim it is a clean relocation.
    let v = s
        .dispatch("place_anchor", &json!({"id": "req:shop-1", "section": "shop.md#/shop/basket", "quote": "Items stay until checkout.", "reevaluate": false}))
        .unwrap();
    assert_eq!(v["reevaluate"], false);
    assert!(matches!(&s.staged[1], Op::PlaceAnchor { to, .. } if to.section == "/shop/basket"));
}

// A relocated anchor flagged for re-evaluation owes the reconcile turn a decision
// even though its quote locates.
#[test]
fn reconcile_done_holds_a_flagged_anchor_whose_quote_locates() {
    let mut s = session();
    s.snapshot.graph.requirements.insert(
        "req:shop-9".into(),
        Requirement {
            statement: "The Shopping Cart shall hold items a Customer intends to buy.".into(),
            entities: vec!["ent:shopping-cart".into()],
            edges: Vec::new(),
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop/cart".into(),
                quote: "The Shopping Cart holds items a Customer intends to buy.".into(),
            }),
            ..Default::default()
        },
    );
    s.snapshot.status.reevaluate.push("req:shop-9".into());
    s.scope
        .goals
        .retain(|g| g.sections == vec!["/shop/cart".to_string()]);
    s.scope.goals[0].stale_anchors.push("req:shop-9".into());
    s.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered"}),
    )
    .unwrap();
    let goal = "g:reconcile-section:shop.md#/shop/cart";
    let err = s
        .dispatch(
            "mark_goal_done",
            &json!({"goal": goal, "justification": "Covered."}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "stale-anchor");
    s.dispatch(
        "delete_requirement",
        &json!({"id": "req:shop-9", "reason": "meaning changed"}),
    )
    .unwrap();
    s.dispatch(
        "mark_goal_done",
        &json!({"goal": goal, "justification": "Re-judged and removed."}),
    )
    .unwrap();
    assert!(s.dispatch("done", &json!({"summary": "x"})).is_ok());
}
