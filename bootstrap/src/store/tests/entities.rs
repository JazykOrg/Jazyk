// Entity mutations: natural keys, minting, merges, moves, gates, and reparent flips.
use super::*;

// The natural key: name plus scope, and parent when supplied. Two same-named
// children under different parents stay apart; an upsert that names neither is
// an ambiguity error naming the candidates.
// Mirrors docs/compiler/concepts/identity.md#the-natural-key-under-containment.
#[test]
fn natural_key_with_and_without_parent() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nTwo modules each carry a Config.\n");
    s.apply(
        vec![create("ent:auth", "Auth"), create("ent:billing", "Billing")],
        &session(),
    );
    let under = |id: &str, parent: &str| Op::CreateEntity {
        id: id.into(),
        entity: Entity {
            parent: Some(parent.into()),
            ..entity("Config")
        },
    };
    let r = s.apply(
        vec![
            under("ent:config", "ent:auth"),
            under("ent:config-b", "ent:billing"),
        ],
        &session(),
    );
    assert_eq!(r.applied, 2, "{:?}", r.skipped);
    assert_eq!(s.graph.entities.len(), 4);
    assert_eq!(
        s.graph.entities["ent:config"].parent.as_deref(),
        Some("ent:auth")
    );
    assert_eq!(
        s.graph.entities["ent:config-b"].parent.as_deref(),
        Some("ent:billing")
    );
    // With the parent, the key lands on that child alone.
    assert_eq!(
        s.find_natural("config", "public", Some("ent:billing")),
        Ok(Some("ent:config-b".into()))
    );
    // A parent nobody has: no match, never a wrong merge.
    assert_eq!(
        s.find_natural("config", "public", Some("ent:auth-2")),
        Ok(None)
    );
    // Without the parent: two candidates is an error naming them.
    assert_eq!(
        s.find_natural("Config", "public", None),
        Err(vec!["ent:config".to_string(), "ent:config-b".to_string()])
    );
    let r = s.apply(vec![create("ent:config-c", "Config")], &session());
    assert_eq!(r.applied, 0);
    assert!(r.skipped[0].contains("ambiguous name"), "{:?}", r.skipped);
    assert!(r.skipped[0].contains("ent:config-b"));
    // A single match lands whatever its parent.
    assert_eq!(
        s.find_natural("auth", "public", None),
        Ok(Some("ent:auth".into()))
    );
    let r = s.apply(vec![create("ent:auth-x", "auth")], &session());
    assert_eq!(r.applied, 1);
    assert_eq!(s.graph.entities.len(), 4);
    assert_eq!(s.find_natural("auth", "other", None), Ok(None));
}

#[test]
fn mint_and_create_and_natural_key_reconcile() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
    let e = Entity {
        name: "Cart".into(),
        mentions: vec![mention("t.md", "/t", "The Cart holds items.")],
        ..Default::default()
    };
    let r = s.apply(
        vec![Op::CreateEntity {
            id: "ent:cart".into(),
            entity: e.clone(),
        }],
        &wi().commit(1, Some(10)),
    );
    assert_eq!(r.applied, 1);
    assert!(s.graph.entities.contains_key("ent:cart"));
    // A second create with the same natural key becomes an update, not a duplicate.
    let e2 = Entity {
        name: "cart".into(),
        mentions: vec![mention("t.md", "/t", "holds items")],
        ..Default::default()
    };
    s.apply(
        vec![Op::CreateEntity {
            id: "ent:cart-x".into(),
            entity: e2,
        }],
        &wi().commit(1, Some(10)),
    );
    assert_eq!(s.graph.entities.len(), 1);
    assert_eq!(s.graph.entities["ent:cart"].mentions.len(), 2);
    assert_eq!(
        s.mint_view_id("use-case", "Checkout", &BTreeSet::new()),
        "view:usecase/checkout"
    );
}

// A merge rewires every reference: parent, transition subject, view members,
// provenance from, and refuses to make the survivor its own ancestor.
#[test]
fn merge_rewires_every_reference_and_redirects() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nbuyer and customer\n");
    s.apply(
        vec![
            create("ent:buyer", "buyer"),
            create("ent:customer", "Customer"),
            Op::CreateEntity {
                id: "ent:wishlist".into(),
                entity: Entity {
                    parent: Some("ent:buyer".into()),
                    ..entity("Wishlist")
                },
            },
            Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    statement: "The buyer pays.".into(),
                    entities: vec!["ent:buyer".into()],
                    transition: Some(Transition {
                        subject: "ent:buyer".into(),
                        from: "browsing".into(),
                        to: "paid".into(),
                        trigger: None,
                        guard: None,
                    }),
                    source: Some(mention("t.md", "/t", "buyer and customer")),
                    ..Default::default()
                },
            },
            Op::CreateRequirement {
                id: "req:x-1".into(),
                requirement: Requirement {
                    statement: "The buyer is split into tiers.".into(),
                    entities: vec!["ent:buyer".into()],
                    provenance: Some(derived(&["ent:buyer"])),
                    ..Default::default()
                },
            },
            Op::CreateView {
                id: "view:class/people".into(),
                view: View {
                    kind: "class".into(),
                    title: "People".into(),
                    members: vec!["ent:buyer".into(), "ent:customer".into()],
                    collapse: vec!["ent:buyer".into()],
                    provenance: Some(derived(&["ent:buyer", "ent:customer"])),
                    ..Default::default()
                },
            },
        ],
        &session(),
    );
    assert!(s.graph.state_machines.contains_key("sm:buyer"));
    let r = s.apply(
        vec![Op::MergeEntities {
            keep: "ent:customer".into(),
            absorb: "ent:buyer".into(),
            reason: "same concept".into(),
        }],
        &session(),
    );
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    assert!(!s.graph.entities.contains_key("ent:buyer"));
    assert_eq!(s.graph.redirects["ent:buyer"], "ent:customer");
    assert_eq!(
        s.graph.requirements["req:t-1"].entities,
        vec!["ent:customer".to_string()]
    );
    assert_eq!(
        s.graph.requirements["req:t-1"]
            .transition
            .as_ref()
            .unwrap()
            .subject,
        "ent:customer"
    );
    assert_eq!(
        s.graph.entities["ent:wishlist"].parent.as_deref(),
        Some("ent:customer")
    );
    assert!(matches!(
        s.graph.requirements["req:x-1"].provenance.as_ref(),
        Some(Provenance::Derived { from, .. }) if from == &vec!["ent:customer".to_string()]
    ));
    let v = &s.graph.views["view:class/people"];
    assert_eq!(v.members, vec!["ent:customer"]);
    assert_eq!(v.collapse, vec!["ent:customer"]);
    assert!(matches!(
        v.provenance.as_ref(),
        Some(Provenance::Derived { from, .. }) if from == &vec!["ent:customer".to_string()]
    ));
    assert!(s.graph.entities["ent:customer"]
        .aliases
        .contains(&"buyer".to_string()));
    assert_eq!(s.resolve_id("ent:buyer"), "ent:customer");
    assert!(s.graph.state_machines.contains_key("sm:customer"));
    assert!(!s.graph.state_machines.contains_key("sm:buyer"));
    assert_eq!(
        s.status.change_ids(&[CHANGE_ENTITY], "ent:customer").len(),
        1
    );
    // Merging a child into its ancestor is refused: it would make the survivor its
    // own ancestor.
    let r = s.apply(
        vec![Op::MergeEntities {
            keep: "ent:wishlist".into(),
            absorb: "ent:customer".into(),
            reason: "bad idea".into(),
        }],
        &session(),
    );
    assert_eq!(r.applied, 0);
    assert!(r.skipped[0].contains("own ancestor"), "{:?}", r.skipped);
}

// A rewire or resolve that makes two ids equal anywhere in a list leaves one, in
// first-occurrence order, not only when the two sat side by side.
#[test]
fn merge_and_resolve_drop_non_adjacent_duplicates() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nbuyer, cart, and customer\n");
    s.apply(
        vec![
            create("ent:buyer", "Buyer"),
            create("ent:cart", "Cart"),
            create("ent:customer", "Customer"),
            Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    statement: "The buyer fills the cart for the customer.".into(),
                    entities: vec!["ent:buyer".into(), "ent:cart".into(), "ent:customer".into()],
                    source: Some(mention("t.md", "/t", "buyer, cart, and customer")),
                    ..Default::default()
                },
            },
            Op::CreateView {
                id: "view:class/shop".into(),
                view: View {
                    kind: "class".into(),
                    title: "Shop".into(),
                    members: vec!["ent:buyer".into(), "ent:cart".into(), "ent:customer".into()],
                    provenance: Some(derived(&["ent:buyer", "ent:cart", "ent:customer"])),
                    ..Default::default()
                },
            },
        ],
        &session(),
    );
    let r = s.apply(
        vec![Op::MergeEntities {
            keep: "ent:buyer".into(),
            absorb: "ent:customer".into(),
            reason: "same concept".into(),
        }],
        &session(),
    );
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    assert_eq!(
        s.graph.requirements["req:t-1"].entities,
        vec!["ent:buyer", "ent:cart"]
    );
    assert_eq!(
        s.graph.views["view:class/shop"].members,
        vec!["ent:buyer", "ent:cart"]
    );
    // A staged list naming the absorbed id and its survivor apart resolves to one.
    let r = s.apply(
        vec![Op::CreateView {
            id: "view:class/again".into(),
            view: View {
                kind: "class".into(),
                title: "Again".into(),
                members: vec!["ent:customer".into(), "ent:cart".into(), "ent:buyer".into()],
                collapse: vec!["ent:customer".into(), "ent:cart".into(), "ent:buyer".into()],
                provenance: Some(derived(&["ent:buyer"])),
                ..Default::default()
            },
        }],
        &session(),
    );
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    let v = s.graph.views.values().find(|v| v.title == "Again").unwrap();
    assert_eq!(v.members, vec!["ent:buyer", "ent:cart"]);
    assert_eq!(v.collapse, vec!["ent:buyer", "ent:cart"]);
}

// A merge carries the absorbed entity's `not_about`, `distinct_from`, and `limits`
// (the higher bump on a clash) and drops the pair's own note on each other.
// Mirrors docs/compiler/graph.md#mutations.
#[test]
fn merge_unions_not_about_distinct_from_and_limits() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nbuyer and customer\n");
    let with = |name: &str,
                not_about: &[&str],
                distinct: &[(&str, &str)],
                limits: &[(&str, u64)]| Entity {
        not_about: not_about.iter().map(|x| x.to_string()).collect(),
        distinct_from: distinct
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        limits: limits
            .iter()
            .map(|(k, v)| (k.to_string(), LimitBump { value: *v }))
            .collect(),
        ..entity(name)
    };
    s.apply(
        vec![
            create("ent:payment", "Payment"),
            Op::CreateEntity {
                id: "ent:customer".into(),
                entity: with(
                    "Customer",
                    &["req:a-1"],
                    &[("ent:buyer", "told apart")],
                    &[
                        ("requirements-per-entity", 70),
                        ("attributes-per-entity", 40),
                    ],
                ),
            },
            Op::CreateEntity {
                id: "ent:buyer".into(),
                entity: with(
                    "Buyer",
                    &["req:b-1", "req:a-1"],
                    &[("ent:payment", "the payer, not the payment")],
                    &[("requirements-per-entity", 90), ("children-per-entity", 30)],
                ),
            },
        ],
        &session(),
    );
    let r = s.apply(
        vec![Op::MergeEntities {
            keep: "ent:customer".into(),
            absorb: "ent:buyer".into(),
            reason: "same concept".into(),
        }],
        &session(),
    );
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    let k = &s.graph.entities["ent:customer"];
    assert_eq!(k.not_about, vec!["req:a-1", "req:b-1"]);
    assert_eq!(
        k.distinct_from.keys().collect::<Vec<_>>(),
        vec!["ent:payment"],
        "the pair's own note goes, the absorbed one's other notes stay"
    );
    assert_eq!(k.limits["requirements-per-entity"].value, 90);
    assert_eq!(k.limits["attributes-per-entity"].value, 40);
    assert_eq!(k.limits["children-per-entity"].value, 30);
}

// Redirects stay one hop: absorbing a survivor repoints the redirects into it, and
// a chain an older store left behind still resolves past eight hops.
// Mirrors docs/compiler/graph.md#storage-layout.
#[test]
fn merges_compact_redirect_chains() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nshop\n");
    s.apply(
        vec![
            create("ent:a", "A"),
            create("ent:b", "B"),
            create("ent:c", "C"),
        ],
        &session(),
    );
    for (keep, absorb) in [("ent:b", "ent:a"), ("ent:c", "ent:b")] {
        let r = s.apply(
            vec![Op::MergeEntities {
                keep: keep.into(),
                absorb: absorb.into(),
                reason: "same".into(),
            }],
            &session(),
        );
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
    }
    assert_eq!(s.graph.redirects["ent:a"], "ent:c");
    assert_eq!(s.graph.redirects["ent:b"], "ent:c");
    let mut old = Store::default();
    old.graph.entities.insert("ent:end".into(), entity("End"));
    for i in 0..12 {
        let to = if i == 11 {
            "ent:end".to_string()
        } else {
            format!("ent:x{}", i + 1)
        };
        old.graph.redirects.insert(format!("ent:x{}", i), to);
    }
    assert_eq!(old.resolve_id("ent:x0"), "ent:end");
    // A cycle stops instead of spinning.
    old.graph.redirects.insert("ent:p".into(), "ent:q".into());
    old.graph.redirects.insert("ent:q".into(), "ent:p".into());
    let _ = old.resolve_id("ent:p");
}

// Clearing a parent is a move: the journal records the prior parent, the parent
// left gets its `parent` record, the child no `fields` record, and the reparent
// history reads the clear. Mirrors docs/compiler/graph.md#journal.
#[test]
fn clearing_a_parent_is_journaled_as_a_move() {
    let mut s = Store {
        out: own_dir("clear-parent"),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nshop\n");
    s.apply(
        vec![
            create("ent:orders", "Orders"),
            Op::CreateEntity {
                id: "ent:parcel".into(),
                entity: Entity {
                    parent: Some("ent:orders".into()),
                    ..entity("Parcel")
                },
            },
        ],
        &session(),
    );
    let r = s.apply(
        vec![Op::UpdateEntity {
            id: "ent:parcel".into(),
            name: None,
            definition: None,
            add_aliases: Vec::new(),
            remove_aliases: Vec::new(),
            clear_parent: true,
            not_about_add: Vec::new(),
            add_mention: None,
            stereotype: None,
            parent: None,
            set_attributes: None,
            add_attributes: Vec::new(),
            provenance: None,
        }],
        &session(),
    );
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    assert_eq!(s.graph.entities["ent:parcel"].parent, None);
    let records: Vec<(String, String)> = r
        .changes
        .iter()
        .filter(|c| c.kind == CHANGE_ENTITY)
        .map(|c| (c.subject.clone(), c.via.clone()))
        .collect();
    assert!(
        records.contains(&("ent:orders".into(), "parent".into())),
        "{:?}",
        records
    );
    assert!(
        !records.iter().any(|(s, _)| s == "ent:parcel"),
        "{:?}",
        records
    );
    let entry = journal_entry(&s, r.generation);
    let moves = parent_moves_in(std::slice::from_ref(&entry));
    assert_eq!(moves.len(), 1, "{:?}", entry.mutations);
    assert_eq!(moves[0].child, "ent:parcel");
    assert_eq!(moves[0].from.as_deref(), Some("ent:orders"));
    assert_eq!(moves[0].to, None);
}

// A move alone writes `parent` on the parent left and the one joined and no
// `fields` record on the child; a move with a definition still reviews the child.
// Mirrors docs/compiler/goals/review-entity.md#created-when.
#[test]
fn a_move_alone_records_parent_on_both_parents_and_nothing_on_the_child() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nshop\n");
    s.apply(
        vec![
            create("ent:orders", "Orders"),
            create("ent:shipping", "Shipping"),
            Op::CreateEntity {
                id: "ent:parcel".into(),
                entity: Entity {
                    parent: Some("ent:orders".into()),
                    ..entity("Parcel")
                },
            },
        ],
        &session(),
    );
    let update = |id: &str, parent: &str, definition: Option<&str>| Op::UpdateEntity {
        id: id.into(),
        name: None,
        definition: definition.map(String::from),
        add_aliases: Vec::new(),
        remove_aliases: Vec::new(),
        clear_parent: false,
        not_about_add: Vec::new(),
        add_mention: None,
        stereotype: None,
        parent: Some(parent.into()),
        set_attributes: None,
        add_attributes: Vec::new(),
        provenance: None,
    };
    let r = s.apply(vec![update("ent:parcel", "ent:shipping", None)], &session());
    let entity_records: Vec<(String, String)> = r
        .changes
        .iter()
        .filter(|c| c.kind == CHANGE_ENTITY)
        .map(|c| (c.subject.clone(), c.via.clone()))
        .collect();
    assert!(
        entity_records.contains(&("ent:orders".into(), "parent".into())),
        "{:?}",
        entity_records
    );
    assert!(
        entity_records.contains(&("ent:shipping".into(), "parent".into())),
        "{:?}",
        entity_records
    );
    assert!(
        !entity_records.iter().any(|(s, _)| s == "ent:parcel"),
        "a move alone never reviews the child: {:?}",
        entity_records
    );
    // Moved back with a new definition: the child's own facts changed, so it
    // gets its `fields` record beside the parents' `parent` records.
    let r = s.apply(
        vec![update("ent:parcel", "ent:orders", Some("The package."))],
        &session(),
    );
    assert!(r
        .changes
        .iter()
        .any(|c| c.kind == CHANGE_ENTITY && c.subject == "ent:parcel" && c.via == "fields"));
}

// The commit-time gates: a parent cycle, an unknown view member, and deleting a
// parent are refused with the rule spelled out. Mirrors docs/compiler/graph.md#validation-gates.
#[test]
fn commit_gates_refuse_cycles_unknown_members_and_deleting_a_parent() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nshop\n");
    s.apply(
        vec![
            create("ent:shop", "Shop"),
            Op::CreateEntity {
                id: "ent:orders".into(),
                entity: Entity {
                    parent: Some("ent:shop".into()),
                    ..entity("Orders")
                },
            },
        ],
        &session(),
    );
    let update_parent = |id: &str, parent: &str| Op::UpdateEntity {
        id: id.into(),
        name: None,
        definition: None,
        add_aliases: Vec::new(),
        remove_aliases: Vec::new(),
        clear_parent: false,
        not_about_add: Vec::new(),
        add_mention: None,
        stereotype: None,
        parent: Some(parent.into()),
        set_attributes: None,
        add_attributes: Vec::new(),
        provenance: None,
    };
    let r = s.apply(
        vec![
            update_parent("ent:shop", "ent:orders"),
            update_parent("ent:shop", "ent:shop"),
            update_parent("ent:shop", "ent:nowhere"),
            Op::CreateView {
                id: "view:class/x".into(),
                view: View {
                    kind: "class".into(),
                    title: "X".into(),
                    members: vec!["ent:shop".into(), "ent:ghost".into()],
                    provenance: Some(derived(&["ent:shop"])),
                    ..Default::default()
                },
            },
            Op::CreateView {
                id: "view:mindmap/x".into(),
                view: View {
                    kind: "mindmap".into(),
                    title: "X".into(),
                    provenance: Some(derived(&["ent:shop"])),
                    ..Default::default()
                },
            },
            Op::DeleteEntity {
                id: "ent:shop".into(),
                reason: "trying".into(),
            },
        ],
        &session(),
    );
    assert_eq!(r.applied, 0, "{:?}", r.skipped);
    assert_eq!(r.skipped.len(), 6);
    assert!(
        r.skipped[0].contains("parent cycle through ent:orders > ent:shop"),
        "{:?}",
        r.skipped
    );
    assert!(r.skipped[1].contains("parent cycle"));
    assert!(r.skipped[2].contains("unknown parent ent:nowhere"));
    assert!(r.skipped[3].contains("unknown member ent:ghost"));
    assert!(r.skipped[4].contains("unknown kind"));
    assert!(r.skipped[5].contains("still a parent of ent:orders"));
    assert!(s.graph.entities["ent:shop"].parent.is_none());
    // Deleting the child first frees the parent, and the delete tombstones.
    let r = s.apply(
        vec![
            Op::DeleteEntity {
                id: "ent:orders".into(),
                reason: "gone".into(),
            },
            Op::DeleteEntity {
                id: "ent:shop".into(),
                reason: "gone".into(),
            },
        ],
        &session(),
    );
    assert_eq!(r.applied, 2, "{:?}", r.skipped);
    assert_eq!(s.graph.redirects["ent:shop"], "");
    assert!(s.status.has_change(CHANGE_ENTITY_DELETED, "ent:shop"));
    // A prose edit staged alone is refused.
    let r = s.apply(
        vec![Op::EditDocProse {
            doc: "t.md".into(),
            section: "/t".into(),
            old_text: "shop".into(),
            new_text: "store".into(),
            text: "# T\nstore\n".into(),
        }],
        &Commit::store("dual-write"),
    );
    assert_eq!(r.applied, 0);
    assert!(r.skipped[0].contains("edit-needs-mutation"));
}

// children-per-entity: the record lands on the node when its direct children cross
// soft, and on `scope:<scope>` when the parentless entities do; a count back under
// soft clears it without a session. Mirrors docs/compiler/reconciler.md#fan-out.
#[test]
fn threshold_crossed_on_children_per_entity_and_cleared_on_dropping_back() {
    let mut s = Store {
        out: own_dir("fanout"),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nthe hub\n");
    let (soft, hard) = crate::limits::threshold("children-per-entity", None).unwrap();
    let part = |i: u64| Op::CreateEntity {
        id: format!("ent:part-{}", i),
        entity: stated(&format!("Part {}", i), "the hub", Some("ent:hub")),
    };
    let mut ops = vec![Op::CreateEntity {
        id: "ent:hub".into(),
        entity: stated("Hub", "the hub", None),
    }];
    ops.extend((0..soft).map(part));
    s.apply(ops, &session());
    assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:hub"));
    let r = s.apply(vec![part(soft)], &session());
    let crossed = r
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_THRESHOLD_CROSSED && c.subject == "ent:hub")
        .expect("threshold-crossed on the node");
    assert_eq!(crossed.via, "limits");
    assert_eq!(crossed.detail["limit"], "children-per-entity");
    assert_eq!(crossed.detail["count"], soft + 1);
    assert_eq!(crossed.detail["soft"], soft);
    assert_eq!(crossed.detail["hard"], hard);
    assert_eq!(crossed.detail["level"], "soft");
    assert_eq!(crossed.detail["goal"], "abstract-entity");
    // Dropping back under soft clears the record without a session.
    s.apply(
        vec![Op::DeleteEntity {
            id: format!("ent:part-{}", soft),
            reason: "test".into(),
        }],
        &session(),
    );
    assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:hub"));
    // The scope root: the hub plus soft more parentless entities cross on scope:public.
    let top = |i: u64| Op::CreateEntity {
        id: format!("ent:top-{}", i),
        entity: stated(&format!("Top {}", i), "the hub", None),
    };
    let r = s.apply((0..soft).map(top).collect(), &session());
    let crossed = r
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_THRESHOLD_CROSSED && c.subject == "scope:public")
        .expect("threshold-crossed on the scope root");
    assert_eq!(crossed.detail["limit"], "children-per-entity");
    assert_eq!(crossed.detail["count"], soft + 1);
    assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:hub"));
    // An unrelated commit keeps the root's record standing.
    s.apply(
        vec![Op::CreateRequirement {
            id: "req:t-1".into(),
            requirement: Requirement {
                statement: "The hub holds.".into(),
                entities: vec!["ent:hub".into()],
                source: Some(mention("t.md", "/t", "the hub")),
                ..Default::default()
            },
        }],
        &session(),
    );
    assert!(s
        .status
        .has_change(CHANGE_THRESHOLD_CROSSED, "scope:public"));
    // One parentless entity moved under a sibling drops the root back under soft.
    s.apply(vec![move_to("ent:top-0", "ent:top-1")], &session());
    assert!(!s
        .status
        .has_change(CHANGE_THRESHOLD_CROSSED, "scope:public"));
    assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:hub"));
}

// The reparent flip: the first move of a child records nothing; the move back
// between the same two parents writes `reparent-flip` on the child with the pair
// in `between`; a move to a third parent is a first move again; and a grouping
// dissolved and re-minted under a new id counts as the same parent.
// Mirrors docs/compiler/reconciler.md#flip-detection.
#[test]
fn reparent_flip_recorded_on_the_second_alternation_not_the_first() {
    let mut s = Store {
        out: own_dir("flip"),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nthe backend\n");
    s.apply(
        vec![
            Op::CreateEntity {
                id: "ent:backend".into(),
                entity: stated("Backend", "the backend", None),
            },
            Op::CreateEntity {
                id: "ent:storage".into(),
                entity: stated("Storage", "the backend", None),
            },
            Op::CreateEntity {
                id: "ent:cache".into(),
                entity: stated("Cache", "the backend", Some("ent:backend")),
            },
        ],
        &session(),
    );
    let r = s.apply(vec![move_to("ent:cache", "ent:storage")], &session());
    assert!(r.changes.iter().all(|c| c.kind != CHANGE_REPARENT_FLIP));
    let entry = journal_entry(&s, r.generation);
    assert_eq!(entry.mutations[0]["parent"], "ent:storage");
    assert_eq!(entry.mutations[0]["prior"]["parent"], "ent:backend");
    let r = s.apply(vec![move_to("ent:cache", "ent:backend")], &session());
    let flip = r
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_REPARENT_FLIP)
        .expect("reparent-flip");
    assert_eq!(flip.subject, "ent:cache");
    assert_eq!(flip.via, "parent");
    assert_eq!(flip.mutation, 1);
    assert_eq!(
        flip.detail["between"],
        serde_json::json!(["ent:storage", "ent:backend"])
    );
    assert!(s.status.has_change(CHANGE_REPARENT_FLIP, "ent:cache"));
    // A third parent is a first move again.
    let r = s.apply(
        vec![
            Op::CreateEntity {
                id: "ent:edge".into(),
                entity: stated("Edge", "the backend", None),
            },
            move_to("ent:cache", "ent:edge"),
        ],
        &session(),
    );
    assert!(r.changes.iter().all(|c| c.kind != CHANGE_REPARENT_FLIP));
    // A grouping over the child, dissolved, then re-minted under a collision
    // suffix: the move back under it matches the dead grouping by natural key.
    let tier = |id: &str| Op::CreateEntity {
        id: id.into(),
        entity: grouping("Tier", &["ent:cache", "ent:other"], Some("ent:edge")),
    };
    s.apply(
        vec![
            Op::CreateEntity {
                id: "ent:other".into(),
                entity: stated("Other", "the backend", Some("ent:edge")),
            },
            tier("ent:tier"),
            move_to("ent:cache", "ent:tier"),
            move_to("ent:other", "ent:tier"),
        ],
        &session(),
    );
    assert_eq!(
        s.graph.entities["ent:cache"].parent.as_deref(),
        Some("ent:tier")
    );
    let r = s.apply(
        vec![Op::DissolveEntity {
            id: "ent:tier".into(),
            reason: "test".into(),
            parent: None,
            children: Vec::new(),
        }],
        &session(),
    );
    // Grouping and dissolving in consecutive generations is itself the alternation.
    assert!(r
        .changes
        .iter()
        .any(|c| c.kind == CHANGE_REPARENT_FLIP && c.subject == "ent:cache"));
    // Both members move back under the re-minted grouping: the sweep behind the
    // commit dissolves a grouping left with one child.
    let r = s.apply(
        vec![
            tier("ent:tier"),
            move_to("ent:cache", "ent:tier"),
            move_to("ent:other", "ent:tier"),
        ],
        &session(),
    );
    let (new_id, _) = s
        .graph
        .entities
        .iter()
        .find(|(_, e)| e.name == "Tier")
        .expect("re-minted grouping");
    assert_ne!(new_id, "ent:tier");
    assert_eq!(
        s.graph.entities["ent:cache"].parent.as_deref(),
        Some(new_id.as_str())
    );
    let flip = r
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_REPARENT_FLIP && c.subject == "ent:cache")
        .expect("reparent-flip across the re-mint");
    assert_eq!(
        flip.detail["between"],
        serde_json::json!(["ent:edge", new_id])
    );
}

#[test]
fn create_under_existing_id_folds_in_place_and_reports_change() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(
        &mut s,
        "t.md",
        "# T\nThe Cart holds items a Customer intends to buy. It keeps items.\n",
    );
    s.graph.entities.insert("ent:cart".into(), entity("Cart"));
    s.graph.requirements.insert(
        "req:t-1".into(),
        Requirement {
            statement: "The Cart holds items a Customer intends to buy.".into(),
            entities: vec!["ent:cart".into()],
            source: Some(mention(
                "t.md",
                "/t",
                "holds items a Customer intends to buy",
            )),
            ..Default::default()
        },
    );
    // Stage-time resolution staged a create under the anchor's id with a reworded,
    // subsuming statement and a fresh quote; commit folds it in place.
    let report = s.apply(
        vec![Op::CreateRequirement {
            id: "req:t-1".into(),
            requirement: Requirement {
                statement: "The Cart holds items.".into(),
                entities: vec!["ent:cart".into()],
                source: Some(mention("t.md", "/t", "keeps items")),
                ..Default::default()
            },
        }],
        &session(),
    );
    assert_eq!(s.graph.requirements.len(), 1);
    let r = &s.graph.requirements["req:t-1"];
    assert_eq!(r.statement, "The Cart holds items.");
    assert_eq!(r.source.as_ref().unwrap().quote, "keeps items");
    assert!(
        report.changed_requirements.contains("req:t-1"),
        "{:?}",
        report.changed_requirements
    );
}
