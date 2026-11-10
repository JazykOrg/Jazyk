use super::*;

#[test]
fn relationships_group_per_direction_and_type() {
    let mut s = Store::default();
    for id in ["ent:a", "ent:b"] {
        s.graph.entities.insert(
            id.into(),
            Entity {
                name: id.into(),
                ..Default::default()
            },
        );
    }
    let req = |edges: Vec<ReqEdge>| Requirement {
        statement: "x".into(),
        entities: vec!["ent:a".into(), "ent:b".into()],
        edges,
        source: Some(src("t.md", "/t", "x")),
        ..Default::default()
    };
    s.graph.requirements.insert(
        "req:t-1".into(),
        req(vec![ReqEdge {
            a: "ent:a".into(),
            b: "ent:b".into(),
            rel_type: None,
            cardinality: None,
        }]),
    );
    s.graph.requirements.insert(
        "req:t-2".into(),
        req(vec![edge("ent:b", "ent:a", "association", Some("1"))]),
    );
    s.graph.requirements.insert(
        "req:t-3".into(),
        req(vec![edge("ent:a", "ent:b", "dependency", None)]),
    );
    s.graph.requirements.insert(
        "req:t-4".into(),
        req(vec![edge("ent:b", "ent:a", "association", Some("*"))]),
    );
    recompute_relationships(&mut s);
    assert_eq!(s.graph.relationships.len(), 1);
    let rel = &s.graph.relationships["rel:a~b"];
    assert_eq!(rel.contributions.len(), 2);
    let ab = rel.contributions.iter().find(|c| c.a == "ent:a").unwrap();
    assert_eq!(ab.r#type, "dependency");
    assert_eq!(
        ab.requirements,
        vec!["req:t-1".to_string(), "req:t-3".to_string()]
    );
    let ba = rel.contributions.iter().find(|c| c.a == "ent:b").unwrap();
    assert_eq!(ba.r#type, "association");
    // Disagreeing cardinalities leave the group without one.
    assert_eq!(ba.cardinality, None);
    assert_eq!(rel.strongest(), "association");
}

#[test]
fn state_machine_derives_initial_and_keeps_a_nondeterministic_pair() {
    let mut s = showcase_store();
    recompute_relationships(&mut s);
    recompute_state_machines(&mut s);
    let m = &s.graph.state_machines["sm:order"];
    assert_eq!(m.subject, "ent:order");
    assert_eq!(m.states, vec!["placed", "paid", "held"]);
    assert_eq!(m.initial.as_deref(), Some("placed"));
    assert_eq!(m.transitions.len(), 2);
    assert_eq!(
        m.transitions[0].requirements,
        vec!["req:shop-7".to_string()]
    );
    assert_eq!(m.transitions[1].to, "held");
    // A second transition out of placed on the same trigger, spelled differently,
    // lands on the same machine as a nondeterministic pair for the checks.
    s.graph.requirements.insert(
        "req:shop-99".into(),
        Requirement {
            statement: "When payment succeeds twice, the order is refunded.".into(),
            entities: vec!["ent:order".into()],
            transition: Some(Transition {
                subject: "ent:order".into(),
                from: " Placed".into(),
                to: "refunded".into(),
                trigger: Some("payment succeeds".into()),
                guard: None,
            }),
            source: Some(src("shop.md", "/shop/orders", "twice")),
            ..Default::default()
        },
    );
    recompute_state_machines(&mut s);
    let m = &s.graph.state_machines["sm:order"];
    assert_eq!(m.states, vec!["placed", "paid", "held", "refunded"]);
    let same_trigger: Vec<&StateTransition> = m
        .transitions
        .iter()
        .filter(|t| t.from == "placed" && t.trigger.as_deref() == Some("payment succeeds"))
        .collect();
    assert_eq!(same_trigger.len(), 2);
    // Two states nobody enters: no initial.
    s.graph
        .requirements
        .get_mut("req:shop-99")
        .unwrap()
        .transition = Some(Transition {
        subject: "ent:order".into(),
        from: "draft".into(),
        to: "paid".into(),
        trigger: None,
        guard: None,
    });
    recompute_state_machines(&mut s);
    assert_eq!(s.graph.state_machines["sm:order"].initial, None);
    // The last transition gone removes the machine.
    for id in ["req:shop-7", "req:shop-8", "req:shop-99"] {
        s.graph.requirements.remove(id);
    }
    recompute_state_machines(&mut s);
    assert!(s.graph.state_machines.is_empty());
}

// A restated transition draws one arrow listing its contributors; distinct triggers
// or guards stay distinct arrows. Mirrors docs/compiler/model/state-machine.md#derivation.
#[test]
fn state_machine_merges_a_restated_transition_into_one_arrow() {
    let mut s = showcase_store();
    let restate =
        |statement: &str, from: &str, to: &str, trigger: Option<&str>, guard: Option<&str>| {
            Requirement {
                statement: statement.into(),
                entities: vec!["ent:order".into()],
                transition: Some(Transition {
                    subject: "ent:order".into(),
                    from: from.into(),
                    to: to.into(),
                    trigger: trigger.map(str::to_string),
                    guard: guard.map(str::to_string),
                }),
                source: Some(src("shop.md", "/shop/orders", statement)),
                ..Default::default()
            }
        };
    // The same arrow as req:shop-7, the trigger spelled differently.
    s.graph.requirements.insert(
        "req:shop-92".into(),
        restate(
            "The order is paid once  Payment Succeeds.",
            "Placed",
            "paid",
            Some("Payment  Succeeds"),
            None,
        ),
    );
    // The same arrow as req:shop-8 with no trigger of its own: it takes the one
    // req:shop-8 names.
    s.graph.requirements.insert(
        "req:shop-93".into(),
        restate(
            "A held order is one whose payment did not go through.",
            "placed",
            "held",
            None,
            None,
        ),
    );
    // Two guards on one arrow stay two arrows.
    s.graph.requirements.insert(
        "req:shop-94".into(),
        restate(
            "A paid order ships when the item is in stock.",
            "paid",
            "shipped",
            Some("dispatch"),
            Some("in stock"),
        ),
    );
    s.graph.requirements.insert(
        "req:shop-95".into(),
        restate(
            "A paid order ships when the item is backordered and arrives.",
            "paid",
            "shipped",
            Some("dispatch"),
            Some("backordered"),
        ),
    );
    recompute_state_machines(&mut s);
    let m = &s.graph.state_machines["sm:order"];
    assert_eq!(m.states, vec!["placed", "paid", "held", "shipped"]);
    assert_eq!(m.initial.as_deref(), Some("placed"));
    assert_eq!(m.transitions.len(), 4, "{:?}", m.transitions);
    // Arrows by lowest requirement id, requirements within an arrow by id.
    let paid = &m.transitions[0];
    assert_eq!((paid.from.as_str(), paid.to.as_str()), ("placed", "paid"));
    assert_eq!(paid.trigger.as_deref(), Some("payment succeeds"));
    assert_eq!(
        paid.requirements,
        vec!["req:shop-7".to_string(), "req:shop-92".to_string()]
    );
    let held = &m.transitions[1];
    assert_eq!(held.to, "held");
    assert_eq!(held.trigger.as_deref(), Some("payment declined"));
    assert_eq!(
        held.requirements,
        vec!["req:shop-8".to_string(), "req:shop-93".to_string()]
    );
    let shipped: Vec<&StateTransition> =
        m.transitions.iter().filter(|t| t.to == "shipped").collect();
    assert_eq!(shipped.len(), 2);
    assert!(shipped.iter().all(|t| t.requirements.len() == 1));
    // The checks read the merged machine: the two restatements trip nothing, and a
    // conflicting arrow out of placed names every contributor of the merged one.
    let rule = |f: &[crate::reconcile::Finding], rule: &str| -> Vec<(Vec<String>, String)> {
        f.iter()
            .filter(|x| x.0 == rule)
            .map(|x| (x.1.clone(), x.3.clone()))
            .collect()
    };
    let f = crate::reconcile::checks(&s, &crate::project::Project::default());
    assert!(
        rule(&f, "nondeterministic-transition").is_empty(),
        "{:?}",
        f
    );
    assert_eq!(rule(&f, "unhandled-event"), vec![(vec!["ent:order".to_string()], "ent:order: no transition for placed on dispatch; paid on payment declined; paid on payment succeeds".to_string())]);
    assert_eq!(rule(&f, "dead-end-state").len(), 1);
    assert!(
        rule(&f, "dead-end-state")[0].1.contains("held, shipped"),
        "{:?}",
        rule(&f, "dead-end-state")
    );
    s.graph.requirements.insert(
        "req:shop-99".into(),
        restate(
            "When payment succeeds twice, the order is refunded.",
            "placed",
            "refunded",
            Some("payment succeeds"),
            None,
        ),
    );
    recompute_state_machines(&mut s);
    let f = crate::reconcile::checks(&s, &crate::project::Project::default());
    let hits = rule(&f, "nondeterministic-transition");
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(
        hits[0].0,
        vec![
            "req:shop-7".to_string(),
            "req:shop-92".to_string(),
            "req:shop-99".to_string()
        ]
    );
}

#[test]
fn document_order_takes_link_levels_ahead_of_path() {
    let mut s = Store::default();
    let a = "# A\n\nalpha\n";
    let z = "# Z\n\nsee [a](./a.md)\n";
    for (doc, text) in [("a.md", a), ("z.md", z)] {
        s.docs.insert(
            doc.into(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
    }
    let req = |doc: &str, section: &str| Requirement {
        statement: format!("{} states a fact.", doc),
        source: Some(src(doc, section, "")),
        ..Default::default()
    };
    s.graph
        .requirements
        .insert("req:a-1".into(), req("a.md", "/a"));
    s.graph
        .requirements
        .insert("req:z-1".into(), req("z.md", "/z"));
    // z.md is the stamped root and links a.md: link level orders ahead of path.
    s.status.roots = vec!["z.md".into()];
    assert_eq!(
        document_order(&s),
        vec!["req:z-1".to_string(), "req:a-1".to_string()]
    );
    // No stamped roots: every document is its own level in path order.
    s.status.roots.clear();
    assert_eq!(
        document_order(&s),
        vec!["req:a-1".to_string(), "req:z-1".to_string()]
    );
}

// The indices hold what the scans they replace compute: the document order, the
// children of every parent, the requirements referencing every entity, and the
// level of every view as the per-target lookups find it.
// Mirrors docs/compiler/graph.md#derived-data.
#[test]
fn indices_match_the_scans_they_replace() {
    let mut synthetic = synthetic_store(120, 300);
    let mut batch = RecordBatch::new(1);
    recompute(&mut synthetic, "g1", &mut batch);
    for s in &[crate::board::tests::settled_store(), synthetic] {
        let ix = Indices::build(s);
        assert_eq!(ix.document_order, document_order(s));
        assert_eq!(ix.doc_levels, doc_levels_from(s, &s.status.roots));
        for (i, rid) in ix.document_order.iter().enumerate() {
            assert_eq!(ix.requirement_position[rid], i);
        }
        let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (id, e) in &s.graph.entities {
            if let Some(p) = &e.parent {
                children.entry(p.clone()).or_default().push(id.clone());
            }
        }
        assert_eq!(ix.children, children);
        for id in s.graph.entities.keys() {
            assert_eq!(
                ix.requirements_referencing(id),
                s.requirements_referencing(id).as_slice(),
                "{}",
                id
            );
            assert_eq!(
                level_view_id_with(s, &ix, id),
                level_view_id(s, id),
                "{}",
                id
            );
            assert_eq!(
                level_view_members_with(s, &ix, id),
                level_view_members(s, id)
            );
        }
        // The level of every view, as the per-target lookups answer it: the
        // entities by id, then the scope roots.
        let mut targets: Vec<String> = s.graph.entities.keys().cloned().collect();
        let mut scopes: Vec<String> = s
            .graph
            .entities
            .values()
            .map(|e| scope_root_target(&e.scope))
            .collect();
        scopes.sort();
        scopes.dedup();
        targets.extend(scopes);
        let ids: Vec<(String, Option<String>)> = targets
            .iter()
            .map(|t| (t.clone(), level_view_id(s, t)))
            .collect();
        let flow_ids: Vec<String> = level_flow_views(s, &ix)
            .iter()
            .map(|(_, w)| w.id.clone())
            .collect();
        let distinct: BTreeSet<&String> = flow_ids.iter().collect();
        assert_eq!(distinct.len(), flow_ids.len(), "one level per flow view id");
        let flows = flow_view_levels(s);
        let levels = view_levels(s, &ix);
        for vid in s.graph.views.keys() {
            let expected = flows.get(vid).cloned().or_else(|| {
                ids.iter()
                    .find(|(_, id)| id.as_deref() == Some(vid.as_str()))
                    .map(|(t, _)| t.clone())
            });
            assert_eq!(levels.get(vid), expected.as_ref(), "{}", vid);
        }
        assert!(levels.len() >= flows.len());
    }
}
