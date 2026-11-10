use super::*;

// delete_entity's parent gate and merge_entities' ancestor gate hold when the
// call is staged, staged moves counted, so the reply never says done for a
// mutation the commit would skip. Mirrors docs/compiler/tools.md#write-tools.
#[test]
fn delete_and_merge_gates_hold_at_staging_with_staged_moves_counted() {
    let mut t = session();
    let ents = &mut t.snapshot.graph.entities;
    ents.insert("ent:cart".into(), plain("Cart"));
    ents.insert("ent:item".into(), under("Item", "ent:cart"));
    ents.insert("ent:order".into(), plain("Order"));
    // A parent cannot go while a child names it.
    let err = t
        .dispatch(
            "delete_entity",
            &json!({"id": "ent:cart", "reason": "noise"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "still-a-parent");
    assert!(err.message.contains("ent:item"), "{}", err.message);
    // A staged move of the child away frees the parent.
    t.dispatch(
        "update_entity",
        &json!({"id": "ent:item", "parent": "ent:order"}),
    )
    .unwrap();
    let v = t
        .dispatch(
            "delete_entity",
            &json!({"id": "ent:cart", "reason": "noise"}),
        )
        .unwrap();
    assert_eq!(v["deleted"], true);
    // A staged create under an entity binds it.
    t.dispatch(
        "upsert_entity",
        &json!({"name": "Line", "parent": "ent:order", "mention": {"section": "/shop/cart", "quote": "A Gadget and a Line sit beside"}}),
    )
    .unwrap();
    let err = t
        .dispatch(
            "delete_entity",
            &json!({"id": "ent:order", "reason": "noise"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "still-a-parent");
    assert!(err.message.contains("ent:line"), "{}", err.message);

    // Merging a container into something it contains would make the survivor
    // its own ancestor; the other direction is fine.
    let mut t = session();
    let ents = &mut t.snapshot.graph.entities;
    ents.insert("ent:cart".into(), plain("Cart"));
    ents.insert("ent:item".into(), under("Item", "ent:cart"));
    ents.insert("ent:sku".into(), under("Sku", "ent:item"));
    let err = t
        .dispatch(
            "merge_entities",
            &json!({"keep": "ent:sku", "absorb": "ent:cart", "reason": "same"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-merge");
    assert!(
        err.message.contains("ent:sku > ent:item > ent:cart"),
        "{}",
        err.message
    );
    // The staged parent counts: an item moved out from under the cart merges fine.
    let mut t = session();
    let ents = &mut t.snapshot.graph.entities;
    ents.insert("ent:cart".into(), plain("Cart"));
    ents.insert("ent:item".into(), under("Item", "ent:cart"));
    ents.insert("ent:basket".into(), plain("Basket"));
    t.dispatch(
        "update_entity",
        &json!({"id": "ent:item", "parent": "ent:basket"}),
    )
    .unwrap();
    let v = t
        .dispatch(
            "merge_entities",
            &json!({"keep": "ent:item", "absorb": "ent:cart", "reason": "same"}),
        )
        .unwrap();
    assert_eq!(v["kept"], "ent:item");
}

#[test]
fn entity_and_requirement_gates_name_their_rules() {
    let mut t = session();
    let ents = &mut t.snapshot.graph.entities;
    ents.insert("ent:cart".into(), plain("Cart"));
    ents.insert("ent:order".into(), plain("Order"));
    ents.insert("ent:item".into(), under("Item", "ent:cart"));
    ents.insert("ent:item-2".into(), under("Item", "ent:order"));
    let mention = json!({"section": "/shop/cart", "quote": "holds items"});
    let err = t
        .dispatch(
            "upsert_entity",
            &json!({"name": "Item", "mention": mention}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "ambiguous-name");
    assert!(
        err.message.contains("ent:item-2") && err.message.contains("parent"),
        "{}",
        err.message
    );
    let v = t
        .dispatch(
            "upsert_entity",
            &json!({"name": "Item", "parent": "ent:order", "mention": mention}),
        )
        .unwrap();
    assert_eq!(v["id"], "ent:item-2");
    assert_eq!(v["created"], false);
    let err = t
        .dispatch(
            "upsert_entity",
            &json!({"name": "Widget", "parent": "ent:nope", "mention": mention}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unknown-parent");
    let err = t
        .dispatch(
            "update_entity",
            &json!({"id": "ent:cart", "parent": "ent:item"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "parent-cycle");
    assert!(err.message.contains("ent:item"), "{}", err.message);
    let err = t
        .dispatch("upsert_entity", &json!({"name": "Widget"}))
        .unwrap_err();
    assert_eq!(err.rule, "provenance-required");
    let v = t
        .dispatch("upsert_entity", &json!({"name": "Pricing", "parent": "ent:cart", "stereotype": "module", "provenance": {"derived": {"from": ["ent:cart"], "reasoning": "split out"}}}))
        .unwrap();
    assert_eq!(v["created"], true);
    assert!(matches!(
        t.staged.last(),
        Some(Op::CreateEntity { entity, .. })
            if entity.parent.as_deref() == Some("ent:cart")
                && entity.stereotype.as_deref() == Some("module")
                && matches!(entity.provenance, Some(Provenance::Derived { .. }))
    ));
    // A staged parent counts for the cycle gate: cart under pricing, which is under cart.
    let err = t
        .dispatch(
            "update_entity",
            &json!({"id": "ent:cart", "parent": "ent:pricing"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "parent-cycle");
    // Attributes are unique by name and take the call's quote.
    let err = t
        .dispatch(
            "update_entity",
            &json!({"id": "ent:cart", "attributes": [{"name": "total"}, {"name": "total"}]}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-attribute");
    t.dispatch("upsert_entity", &json!({"name": "Cart", "mention": mention, "attributes": [{"name": "total", "type": "money"}]}))
        .unwrap();
    assert!(matches!(
        t.staged.last(),
        Some(Op::UpdateEntity { add_attributes, .. })
            if add_attributes.len() == 1
                && matches!(add_attributes[0].provenance, Provenance::Quote(ref q) if q.quote == "holds items")
    ));

    let base = json!({"statement": "The Cart holds items.", "entities": ["ent:cart", "ent:order"], "section": "/shop/cart", "quote": "holds items"});
    let with = |extra: Value| {
        let mut b = base.clone();
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        b
    };
    let err = t
        .dispatch(
            "upsert_requirement",
            &with(json!({"transition": {"subject": "ent:ghost", "from": "open", "to": "paid"}})),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unknown-id");
    let err = t
        .dispatch(
            "upsert_requirement",
            &with(json!({"transition": {"subject": "ent:customer", "from": "open", "to": "paid"}})),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-transition");
    let err = t
        .dispatch(
            "upsert_requirement",
            &with(json!({"edges": [{"a": "ent:cart", "b": "ent:order", "cardinality": "many"}]})),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-cardinality");
    let err = t
        .dispatch(
            "upsert_requirement",
            &with(json!({"facets": [{"facet": "speed", "reasoning": "x"}]})),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-facet");
    let err = t
        .dispatch("upsert_requirement", &with(json!({"facets": [{"facet": "behavior", "reasoning": "x", "measure": "2 seconds"}]})))
        .unwrap_err();
    assert_eq!(err.rule, "bad-facet");
    let err = t
        .dispatch(
            "upsert_requirement",
            &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The Cart holds items.", "entities": ["ent:cart"]}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "provenance-required");
    let err = t
        .dispatch("report_diagnostic", &json!({"rule": "decision", "severity": "warning", "subjects": ["ent:cart"], "message": "which bound?"}))
        .unwrap_err();
    assert_eq!(err.rule, "decision-needs-prompt");
    // The whole shape lands: edges deduplicated on (a, b, type), transition, facets.
    let v = t
        .dispatch("upsert_requirement", &with(json!({
            "edges": [{"a": "ent:cart", "b": "ent:order", "type": "composition", "cardinality": "1..*"}, {"a": "ent:cart", "b": "ent:order", "type": "composition"}],
            "transition": {"subject": "ent:cart", "from": "open", "to": "paid", "trigger": "payment"},
            "facets": [{"facet": "quality", "reasoning": "bounded", "measure": "2 seconds"}]
        })))
        .unwrap();
    assert_eq!(v["created"], true);
    match t.staged.last().unwrap() {
        Op::CreateRequirement { requirement, .. } => {
            assert_eq!(requirement.edges.len(), 1);
            assert_eq!(requirement.edges[0].cardinality.as_deref(), Some("1..*"));
            assert_eq!(requirement.transition.as_ref().unwrap().subject, "ent:cart");
            assert_eq!(requirement.facets[0].measure.as_deref(), Some("2 seconds"));
        }
        other => panic!("unexpected {:?}", other),
    }
    // A derived requirement mints req:x-<n> and keys on the statement within its from set.
    let derived = |s: &str| json!({"statement": s, "entities": ["ent:cart"], "provenance": {"derived": {"from": ["ent:cart"], "reasoning": "too dense"}}});
    let v = t
        .dispatch(
            "upsert_requirement",
            &derived("The Cart is split by category."),
        )
        .unwrap();
    let x1 = v["id"].as_str().unwrap().to_string();
    assert!(x1.starts_with("req:x-"), "{}", v);
    let again = t
        .dispatch(
            "upsert_requirement",
            &derived("The Cart is split by category"),
        )
        .unwrap();
    assert_eq!(again["id"], x1.as_str());
    assert_eq!(again["updated"], true);
}

#[test]
fn junk_names_rejected_with_repair_hint() {
    let mut t = session();
    let err = t
        .dispatch("upsert_entity", &json!({"name": "--api-key", "mention": {"section": "/shop/cart", "quote": "holds items"}}))
        .unwrap_err();
    assert_eq!(err.rule, "junk-name");
    assert!(err.message.contains("note"));
    // Operation identifiers are requirement detail, not entities.
    let err2 = t
        .dispatch("upsert_entity", &json!({"name": "createUser", "mention": {"section": "/shop/cart", "quote": "holds items"}}))
        .unwrap_err();
    assert_eq!(err2.rule, "junk-name");
    assert!(err2.message.contains("operation"), "{}", err2.message);
}

#[test]
fn upsert_reuses_existing_natural_key() {
    let mut t = session();
    let v = t
        .dispatch("upsert_entity", &json!({"name": "customer", "mention": {"section": "/shop/cart", "quote": "a Customer intends to buy"}}))
        .unwrap();
    assert_eq!(v["id"], "ent:customer");
    assert_eq!(v["created"], false);
}

// Mirrors docs/compiler/tools.md#write-tools: remove_aliases takes a wording back
// in the same session, and the read-back shows the list as it will commit.
#[test]
fn update_entity_remove_aliases_takes_a_wording_back() {
    let mut t = session();
    t.dispatch(
        "update_entity",
        &json!({"id": "ent:customer", "add_aliases": ["Buyer", "Shopper"]}),
    )
    .unwrap();
    t.dispatch(
        "update_entity",
        &json!({"id": "ent:customer", "remove_aliases": ["Buyer"]}),
    )
    .unwrap();
    let v = t
        .dispatch("get_entity", &json!({"id": "ent:customer"}))
        .unwrap();
    assert_eq!(v["aliases"], json!(["Shopper"]), "{}", v);
}

// Mirrors docs/compiler/tools.md#write-tools: remove_parent takes an entity out of
// the containment tree in the same session.
#[test]
fn update_entity_remove_parent_clears_the_parent() {
    let mut t = session();
    let id = t
        .dispatch(
            "upsert_entity",
            &json!({"name": "Shopping Cart", "parent": "ent:customer", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let v = t.dispatch("get_entity", &json!({"id": id})).unwrap();
    assert_eq!(v["parent"], "ent:customer", "{}", v);
    t.dispatch("update_entity", &json!({"id": id, "remove_parent": true}))
        .unwrap();
    let v = t.dispatch("get_entity", &json!({"id": id})).unwrap();
    assert!(v["parent"].is_null(), "{}", v);
}

// Mirrors docs/compiler/tools.md#write-tools: a name starting with an article is
// a proper name when a document writes it capitalized mid-sentence.
#[test]
fn a_proper_name_with_an_article_passes_the_junk_name_rule() {
    let mut t = session();
    let err = t
        .dispatch(
            "upsert_entity",
            &json!({"name": "The Lantern Room", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "junk-name");
    let text = "# Shop\nintro text\n\n## Cart\nThe Shopping Cart holds items a Customer intends to buy.\nShe reads upstairs at The Lantern Room.\n";
    t.snapshot.docs.insert(
        "shop.md".into(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: BTreeMap::new(),
        },
    );
    let v = t
        .dispatch(
            "upsert_entity",
            &json!({"name": "The Lantern Room", "mention": {"section": "/shop/cart", "quote": "She reads upstairs at The Lantern Room."}}),
        )
        .unwrap();
    assert_eq!(v["created"], true, "{}", v);
}

#[test]
fn prefixed_case_variant_resolves() {
    let mut t = session();
    let v = t
        .dispatch(
            "update_entity",
            &json!({"id": "ent:Customer", "add_aliases": ["Buyer"]}),
        )
        .unwrap();
    assert_eq!(v["id"], "ent:customer");
}

#[test]
fn lenient_entity_refs_resolve_unambiguously() {
    let mut t = session();
    // Prefix-less id and exact display name both resolve to ent:customer.
    let v = t
        .dispatch(
            "upsert_requirement",
            &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The Customer shall buy items.", "entities": ["customer"], "section": "/shop/cart", "quote": "a Customer intends to buy"}),
        )
        .unwrap();
    assert_eq!(v["created"], true);
    let v2 = t
        .dispatch(
            "upsert_requirement",
            &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The Customer shall intend to buy.", "entities": ["Customer"], "section": "/shop/cart", "quote": "intends to buy"}),
        )
        .unwrap();
    assert_eq!(v2["created"], true);
    match t
        .staged
        .iter()
        .find(|o| matches!(o, Op::CreateRequirement { .. }))
        .unwrap()
    {
        Op::CreateRequirement { requirement, .. } => {
            assert_eq!(requirement.entities, vec!["ent:customer".to_string()])
        }
        _ => unreachable!(),
    }
}

// Mirrors docs/compiler/graph.md#validation-gates: a plural is a variant, a
// compound extending a single name is not, and a child declared under its
// lookalike passes without a note.
#[test]
fn name_variant_gate_folds_plurals_and_lets_compounds_and_declared_children_pass() {
    let mut t = session();
    t.snapshot
        .graph
        .entities
        .insert("ent:employee".into(), plain("Employee"));
    t.snapshot
        .graph
        .entities
        .insert("ent:department".into(), plain("Department"));
    let quote: String = t.snapshot.docs["shop.md"].sections["/shop/cart"]
        .raw
        .lines()
        .find(|l| l.contains("Departments"))
        .unwrap()
        .trim()
        .to_string();
    let call = move |name: &str, parent: Option<&str>| {
        let mut v = json!({"name": name, "definition": "d", "mention": {"section": "shop.md#/shop/cart", "quote": quote}});
        if let Some(p) = parent {
            v["parent"] = json!(p);
        }
        v
    };
    // An entity named by the plural passes: the area and the thing are two concepts.
    let r = t.dispatch("upsert_entity", &call("Departments", None));
    assert!(r.is_ok(), "{:?}", r.err().map(|e| (e.rule, e.message)));
    // A grouping named by the plural is a twin of the stated singular.
    assert!(t.near_name("Employees", "public", false).is_some());
    assert!(t.near_name("Employees", "public", true).is_none());
    let r = t.dispatch("upsert_entity", &call("Employee Handbook", None));
    assert!(r.is_ok(), "{:?}", r.err().map(|e| (e.rule, e.message)));
    let r = t.dispatch(
        "upsert_entity",
        &call("Employee Badge", Some("ent:employee")),
    );
    assert!(r.is_ok(), "{:?}", r.err().map(|e| (e.rule, e.message)));
}

// A new entity enters with a sentence that names it; an alias counts.
// Mirrors docs/compiler/graph.md#validation-gates.
#[test]
fn a_new_entity_whose_quote_never_names_it_is_refused() {
    let mut t = session();
    let q = "The Shopping Cart holds items a Customer intends to buy.";
    let err = t
        .dispatch(
            "upsert_entity",
            &json!({"name": "Warehouse", "definition": "d", "stereotype": "system",
            "mention": {"section": "/shop/cart", "quote": q}}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "mention-unnamed", "{}", err.message);
    assert!(t
        .dispatch(
            "upsert_entity",
            &json!({"name": "Basket", "definition": "d", "aliases": ["Shopping Cart"],
            "mention": {"section": "/shop/cart", "quote": q}})
        )
        .is_ok());
    assert!(t
        .dispatch(
            "upsert_entity",
            &json!({"name": "Items", "definition": "d",
            "mention": {"section": "/shop/cart", "quote": q}})
        )
        .is_ok());
}

// An entity quoting a heading hears it is not minted from one.
// Mirrors docs/compiler/graph.md#validation-gates.
#[test]
fn an_entity_quoting_a_heading_hears_it_is_not_minted_from_one() {
    let mut t = session();
    let err = t
        .dispatch(
            "upsert_entity",
            &json!({"name": "Shop", "definition": "d", "stereotype": "system",
            "mention": {"section": "/shop", "quote": "Shop"}}),
        )
        .unwrap_err();
    assert!(
        err.message.contains("not minted from its heading"),
        "{}",
        err.message
    );
}

// A section is no node: derived.from naming one is refused with the quote repair.
// Mirrors docs/compiler/model.md#provenance.
#[test]
fn derived_from_a_section_is_refused_toward_a_mention() {
    let mut t = session();
    let err = t
        .dispatch(
            "upsert_entity",
            &json!({"name": "Warehouse", "definition": "The system.",
                    "provenance": {"derived": {"from": ["shop.md#/shop"], "reasoning": "the page"}}}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unknown-id");
    assert!(
        err.message.contains("a section") && err.message.contains("mention"),
        "{}",
        err.message
    );
}

// The fan-out variant faces its level's count at mark_goal_done even when the
// changeset left the level alone: a done claim over an untouched level over soft is
// refused; a move under an existing sibling that brings the count to soft passes.
// Mirrors docs/compiler/goals/abstract-entity.md#the-fan-out-gate.
#[test]
fn mark_goal_done_on_a_fan_out_goal_faces_the_untouched_level() {
    use crate::limits::{threshold, CHILDREN_PER_ENTITY};
    let (soft, hard) = threshold(CHILDREN_PER_ENTITY, None).unwrap();
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:backend".into(), plain("Backend"));
    let n = soft as usize + 1;
    for i in 0..n {
        s.graph.entities.insert(
            format!("ent:c{}", i),
            under(&format!("C{}", i), "ent:backend"),
        );
    }
    s.status.changes.push(
        ChangeRecord::new(
            1,
            0,
            0,
            crate::store::CHANGE_THRESHOLD_CROSSED,
            "ent:backend",
            "limits",
        )
        .with_detail(json!({
            "limit": CHILDREN_PER_ENTITY, "count": n, "soft": soft, "hard": hard,
            "level": "soft", "goal": "abstract-entity",
        })),
    );
    let goal = Goal {
        id: "g:abstract-entity:ent:backend".into(),
        kind: "abstract-entity".into(),
        mandatory: false,
        target: "ent:backend".into(),
        change: json!({"fan_out": n, "limit": {"soft": soft, "hard": hard}, "candidates": []}),
        ..Default::default()
    };
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &[goal]), 64, 24_000);
    let err = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": "g:abstract-entity:ent:backend", "justification": "The level is fine."}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "fan-out-over-limit");
    assert!(err.message.contains("ent:backend"), "{}", err.message);
    t.dispatch(
        "update_entity",
        &json!({"id": "ent:c1", "parent": "ent:c0"}),
    )
    .unwrap();
    t.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:abstract-entity:ent:backend", "justification": "C1 belongs under C0, which already contains it conceptually."}),
    )
    .unwrap();
}

// A level under `ent:backend`: four stated children, a derived grouping
// `ent:messaging` holding two of them, a stated sibling area whose name a grouping
// could look like, and a parentless entity on another level. The session runs a
// fan-out goal on the backend.
fn level_session() -> ToolSession {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:backend".into(), plain("Backend"));
    for i in 0..4 {
        s.graph.entities.insert(
            format!("ent:c{}", i),
            under(&format!("C{}", i), "ent:backend"),
        );
    }
    s.graph.entities.insert(
        "ent:messaging".into(),
        Entity {
            name: "Messaging".into(),
            definition: Some("moves events between the services".into()),
            parent: Some("ent:backend".into()),
            provenance: Some(Provenance::Derived {
                from: vec!["ent:c2".into(), "ent:c3".into()],
                reasoning: "the documents treat the queue and the bus as one area".into(),
            }),
            ..Default::default()
        },
    );
    for c in ["ent:c2", "ent:c3"] {
        s.graph.entities.get_mut(c).unwrap().parent = Some("ent:messaging".into());
    }
    s.graph.entities.insert(
        "ent:storage-layer".into(),
        under("Storage Layer", "ent:backend"),
    );
    s.graph.entities.insert("ent:other".into(), plain("Other"));
    let goal = Goal {
        id: "g:abstract-entity:ent:backend".into(),
        kind: "abstract-entity".into(),
        mandatory: false,
        target: "ent:backend".into(),
        change: json!({"fan_out": 5, "limit": {"soft": 9, "hard": 15}, "candidates": []}),
        ..Default::default()
    };
    ToolSession::new(s, WorkScope::for_batch("b0-1", &[goal]), 64, 24_000)
}

// group_entities stages one derived entity from the members and one parent move
// per member, and refuses a lone member, a cross-level set, and a lookalike name.
// Mirrors docs/compiler/tools.md#grouping-tools.
#[test]
fn group_entities_accepts_a_valid_grouping_and_rejects_bad_ones() {
    let mut t = level_session();
    let grouping = json!({
        "name": "Compute",
        "definition": "Runs the request handlers.",
        "members": ["ent:c0", "c1"],
        "stereotype": "component",
        "reasoning": "the compute page describes both as one tier",
    });
    let r = t.dispatch("group_entities", &grouping).unwrap();
    assert_eq!(r["id"], "ent:compute");
    assert_eq!(r["moved"], json!(["ent:c0", "ent:c1"]));
    assert_eq!(t.staged.len(), 3, "{:?}", t.staged);
    match &t.staged[0] {
        Op::CreateEntity { id, entity } => {
            assert_eq!(id, "ent:compute");
            assert_eq!(entity.parent.as_deref(), Some("ent:backend"));
            assert_eq!(entity.stereotype.as_deref(), Some("component"));
            assert_eq!(
                entity.provenance,
                Some(Provenance::Derived {
                    from: vec!["ent:c0".into(), "ent:c1".into()],
                    reasoning: "the compute page describes both as one tier".into(),
                })
            );
        }
        other => panic!("expected a create, got {:?}", other),
    }
    for (op, member) in t.staged[1..].iter().zip(["ent:c0", "ent:c1"]) {
        match op {
            Op::UpdateEntity { id, parent, .. } => {
                assert_eq!(id, member);
                assert_eq!(parent.as_deref(), Some("ent:compute"));
            }
            other => panic!("expected a move, got {:?}", other),
        }
    }
    // The session's own gates see the moves: the grouping now holds the level.
    assert_eq!(t.level_after("ent:compute"), vec!["ent:c0", "ent:c1"]);
    assert_eq!(
        t.level_view_of("ent:compute").as_deref(),
        Some("view:component/compute")
    );
    // The abstract-entity gate accepts the grouping as staged.
    t.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:abstract-entity:ent:backend", "justification": "Compute groups the two handlers the compute page describes."}),
    )
    .unwrap();

    let mut t = level_session();
    let err = t
        .dispatch(
            "group_entities",
            &json!({"name": "Solo", "definition": "One.", "members": ["ent:c0"], "reasoning": "why"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "too-few-members");
    assert!(err.message.contains("update_entity"), "{}", err.message);
    let err = t
        .dispatch(
            "group_entities",
            &json!({"name": "Mixed", "definition": "Two levels.", "members": ["ent:c0", "ent:other"], "reasoning": "why"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "cross-level");
    assert!(
        err.message.contains("ent:other") && err.message.contains("the scope root"),
        "{}",
        err.message
    );
    // A staged move counts as the current parent: c2 moved beside c0 groups with it.
    let err = t
        .dispatch(
            "group_entities",
            &json!({"name": "Mixed", "definition": "Two levels.", "members": ["ent:c0", "ent:c2"], "reasoning": "why"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "cross-level");
    t.dispatch(
        "update_entity",
        &json!({"id": "ent:c2", "parent": "ent:backend"}),
    )
    .unwrap();
    let before = t.staged.len();
    t.dispatch(
        "group_entities",
        &json!({"name": "Pair", "definition": "Two handlers.", "members": ["ent:c0", "ent:c2"], "reasoning": "why"}),
    )
    .unwrap();
    assert_eq!(t.staged.len(), before + 3);
    // A lookalike of an existing area reuses it; an exact name is that entity.
    let err = t
        .dispatch(
            "group_entities",
            &json!({"name": "Storage", "definition": "Keeps the data.", "members": ["ent:c1", "ent:messaging"], "reasoning": "why"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "near-duplicate");
    assert!(err.message.contains("ent:storage-layer"), "{}", err.message);
    // The lookalike sits beside the members at this level: a peer carrying the
    // area's word, never their parent. Mirrors docs/compiler/concepts/levels.md#naming.
    assert!(
        err.message.contains("a peer of the members at this level")
            && err.message.contains("never becomes the members' parent"),
        "{}",
        err.message
    );
    // A sibling that already holds children is the area: reuse it.
    let err = t
        .dispatch(
            "group_entities",
            &json!({"name": "Messaging Hub", "definition": "Moves messages.", "members": ["ent:c1", "ent:storage-layer"], "reasoning": "why"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "near-duplicate");
    assert!(
        err.message.contains("ent:messaging") && err.message.contains("already holds children"),
        "{}",
        err.message
    );
    let err = t
        .dispatch(
            "group_entities",
            &json!({"name": "Storage Layer", "definition": "Keeps the data.", "members": ["ent:c1", "ent:storage-layer"], "reasoning": "why"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "near-duplicate");
    assert!(err.message.contains("already exists"), "{}", err.message);
    // A lookalike at another level names that level's concept: qualify the name.
    t.dispatch(
        "update_entity",
        &json!({"id": "ent:storage-layer", "parent": "ent:messaging"}),
    )
    .unwrap();
    let err = t
        .dispatch(
            "group_entities",
            &json!({"name": "Storage", "definition": "Keeps the data.", "members": ["ent:c1", "ent:messaging"], "reasoning": "why"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "near-duplicate");
    assert!(
        err.message.contains("another level") && err.message.contains("qualify"),
        "{}",
        err.message
    );
    // definition and reasoning are non-empty; a member must resolve.
    for (args, rule) in [
        (
            json!({"name": "Bare", "definition": " ", "members": ["ent:c1", "ent:messaging"], "reasoning": "why"}),
            "definition-required",
        ),
        (
            json!({"name": "Bare", "definition": "Keeps the data.", "members": ["ent:c1", "ent:messaging"], "reasoning": ""}),
            "reasoning-required",
        ),
        (
            json!({"name": "Bare", "definition": "Keeps the data.", "members": ["ent:c1", "ent:nope"], "reasoning": "why"}),
            "unknown-id",
        ),
    ] {
        assert_eq!(t.dispatch("group_entities", &args).unwrap_err().rule, rule);
    }
}

// dissolve_entity refuses an entity the documents state and stages the dissolve
// of a derived grouping, its children landing on the grouping's parent for the
// session's own gates. Mirrors docs/compiler/tools.md#grouping-tools.
#[test]
fn dissolve_entity_refuses_a_stated_entity_and_dissolves_a_grouping() {
    let mut t = level_session();
    let err = t
        .dispatch(
            "dissolve_entity",
            &json!({"id": "ent:backend", "reason": "flatten"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "stated-entity");
    assert!(
        err.message.contains("Revise the documents"),
        "{}",
        err.message
    );
    // A derived entity holding requirements is a sub-entity, not a grouping.
    t.snapshot.graph.requirements.insert(
        "req:shop-1".into(),
        quoted_req(
            "Messaging retries once.",
            &["ent:messaging"],
            "retries once",
        ),
    );
    let err = t
        .dispatch(
            "dissolve_entity",
            &json!({"id": "ent:messaging", "reason": "flatten"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "not-a-grouping");
    assert!(err.message.contains("req:shop-1"), "{}", err.message);
    t.snapshot.graph.requirements.clear();
    let r = t
        .dispatch(
            "dissolve_entity",
            &json!({"id": "messaging", "reason": "the two belong beside the cache"}),
        )
        .unwrap();
    assert_eq!(r["dissolved"], "ent:messaging");
    assert_eq!(r["parent"], "ent:backend");
    assert_eq!(r["children"], json!(["ent:c2", "ent:c3"]));
    assert!(matches!(
        &t.staged[0],
        Op::DissolveEntity { id, reason, parent: None, children }
            if id == "ent:messaging" && reason == "the two belong beside the cache" && children.is_empty()
    ));
    // The session reads the children's moves: they sit under the backend now,
    // and the dissolved grouping is no level.
    assert_eq!(t.parent_of("ent:c2").as_deref(), Some("ent:backend"));
    assert_eq!(
        t.level_after("ent:backend"),
        vec!["ent:c0", "ent:c1", "ent:c2", "ent:c3", "ent:storage-layer"]
    );
    assert!(t.level_after("ent:messaging").is_empty());
    let err = t
        .dispatch(
            "dissolve_entity",
            &json!({"id": "ent:messaging", "reason": "again"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "already-dissolved");
    // A grouping staged this session is not in the graph yet: the call withdraws
    // it, the create and both moves dropped from the changeset.
    let before = t.staged.len();
    t.dispatch(
        "group_entities",
        &json!({"name": "Compute", "definition": "Runs the handlers.", "members": ["ent:c0", "ent:c1"], "reasoning": "why"}),
    )
    .unwrap();
    assert_eq!(t.staged.len(), before + 3);
    let r = t
        .dispatch(
            "dissolve_entity",
            &json!({"id": "ent:compute", "reason": "undo"}),
        )
        .unwrap();
    assert_eq!(r["withdrawn"], "ent:compute");
    assert_eq!(r["dropped"], 3);
    assert_eq!(t.staged.len(), before);
    assert!(!t.staged_entities.contains_key("ent:compute"));
}

// A caps split session: a service over its requirement limit with two statements
// naming other entities beside it. Mirrors docs/compiler/goals/abstract-entity.md#gate.
fn caps_session() -> ToolSession {
    let mut s = Store::default();
    s.graph.entities.insert("ent:svc".into(), plain("Service"));
    s.graph.entities.insert("ent:db".into(), plain("Database"));
    s.graph
        .entities
        .insert("ent:session".into(), plain("Session"));
    for (i, ents) in [
        vec!["ent:svc", "ent:session", "ent:db"],
        vec!["ent:svc", "ent:db"],
    ]
    .iter()
    .enumerate()
    {
        s.graph.requirements.insert(
            format!("req:svc-{}", i),
            Requirement {
                statement: format!("The Service persists record {} in the Database.", i),
                entities: ents.iter().map(|e| e.to_string()).collect(),
                source: Some(SourceRef {
                    doc: "svc.md".into(),
                    section: "/svc".into(),
                    quote: "persists".into(),
                }),
                ..Default::default()
            },
        );
    }
    let goal = Goal {
        id: "g:abstract-entity:ent:svc".into(),
        kind: "abstract-entity".into(),
        mandatory: true,
        target: "ent:svc".into(),
        change: json!({"limits": [{"limit": "requirements-per-entity", "count": 96, "soft": 50, "hard": 80, "level": "hard"}]}),
        ..Default::default()
    };
    ToolSession::new(s, WorkScope::for_batch("b0-1", &[goal]), 64, 24_000)
}

// Mirrors docs/compiler/sessions.md#budgets: a caps split earns one move per
// grouped statement over the registry constant.
#[test]
fn caps_split_grows_the_mutation_budget_by_the_crossed_count() {
    let t = caps_session();
    assert_eq!(t.mutation_limit, crate::limits::SESSION_MUTATIONS + 96);
    assert_eq!(
        level_session().mutation_limit,
        crate::limits::SESSION_MUTATIONS
    );
}

// Mirrors docs/compiler/goals/abstract-entity.md#gate: a re-point swaps the target
// for the sub-entity and keeps the other entities; a list that drops one is
// refused with the list to send.
#[test]
fn caps_repoint_is_a_swap_that_keeps_the_other_entities() {
    let mut t = caps_session();
    t.dispatch(
        "upsert_entity",
        &json!({"name": "Data Model", "parent": "ent:svc", "definition": "The Service's data model is the set of records it persists.", "provenance": {"derived": {"from": ["ent:svc", "req:svc-0"], "reasoning": "the data section"}}}),
    )
    .unwrap();
    let err = t
        .dispatch(
            "update_requirement",
            &json!({"id": "req:svc-0", "entities": ["ent:data-model"]}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "entities-dropped");
    assert!(
        err.message
            .contains("send entities [ent:data-model, ent:session, ent:db]"),
        "{}",
        err.message
    );
    t.dispatch(
        "update_requirement",
        &json!({"id": "req:svc-0", "entities": ["ent:data-model", "ent:session", "ent:db"]}),
    )
    .unwrap();
    // Keeping the target while adding the sub-entity drops nothing either.
    t.dispatch(
        "update_requirement",
        &json!({"id": "req:svc-1", "entities": ["ent:svc", "ent:db", "ent:data-model"]}),
    )
    .unwrap();
    // Withdrawing the sub-entity drops its create and both re-points.
    let r = t
        .dispatch(
            "dissolve_entity",
            &json!({"id": "ent:data-model", "reason": "not filled"}),
        )
        .unwrap();
    assert_eq!(r["dropped"], 3);
    assert!(t.staged.is_empty(), "{:?}", t.staged);
}

// A dissolve that lands a level past the fan-out threshold previews the goal it
// opens. Mirrors docs/compiler/reconciler.md#bubbling.
#[test]
fn dissolve_previews_the_fan_out_goal_it_opens() {
    use crate::limits::{threshold, CHILDREN_PER_ENTITY};
    let (soft, _) = threshold(CHILDREN_PER_ENTITY, None).unwrap();
    let mut t = level_session();
    // The backend holds five after the dissolve; pad it to the soft threshold first.
    for i in 0..(soft as usize - 4) {
        t.snapshot.graph.entities.insert(
            format!("ent:pad{}", i),
            under(&format!("Pad{}", i), "ent:backend"),
        );
    }
    let r = t
        .dispatch(
            "dissolve_entity",
            &json!({"id": "ent:messaging", "reason": "flatten"}),
        )
        .unwrap();
    let opens = r["opens"].as_array().cloned().unwrap_or_default();
    assert!(
        opens.iter().any(|o| {
            o.as_str()
                .is_some_and(|s| s.starts_with("abstract-entity ent:backend (fan-out"))
        }),
        "{:?}",
        opens
    );
}
