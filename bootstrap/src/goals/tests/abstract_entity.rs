use super::*;

// A service over a requirement record with soft 2 and hard 3, four statements on it.
fn caps_store() -> Store {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:svc".into(), child("Service", None, "svc.md"));
    for i in 1..=4 {
        statement(
            &mut s,
            &format!("req:svc-{}", i),
            &format!("The Service persists record {} in the database.", i),
            &["ent:svc"],
            "/svc/data",
        );
    }
    s.status.changes.push(
        ChangeRecord::new(
            7,
            0,
            0,
            store::CHANGE_THRESHOLD_CROSSED,
            "ent:svc",
            "limits",
        )
        .with_detail(json!({
            "limit": "requirements-per-entity", "count": 4, "soft": 2, "hard": 3,
            "level": "hard", "goal": "abstract-entity",
        })),
    );
    s
}

fn repoint(id: &str, to: &str) -> Op {
    Op::UpdateRequirement {
        id: id.into(),
        statement: None,
        entities: Some(vec![to.into()]),
        edges: None,
        transition: None,
        facets: None,
        source: None,
        provenance: None,
    }
}

// Mirrors docs/compiler/goals/abstract-entity.md#gate: the count after the
// changeset is within soft, so a split that only de-escalates is refused.
#[test]
fn caps_gate_refuses_a_count_still_over_soft_and_accepts_one_within() {
    let s = caps_store();
    let v = caps_goal_gate(&s, &[], "ent:svc").expect("four over soft two");
    assert_eq!(v.rule, "count-over-limit");
    assert!(v
        .message
        .contains("4 requirements after the changeset, over 2"));
    let one = [repoint("req:svc-1", "ent:data")];
    let v = caps_goal_gate(&s, &one, "ent:svc").expect("three is still over two");
    assert!(v.message.contains("3 requirements"));
    let two = [
        repoint("req:svc-1", "ent:data"),
        repoint("req:svc-2", "ent:data"),
    ];
    assert!(caps_goal_gate(&s, &two, "ent:svc").is_none());
    // A record on another limit is not this gate's.
    let mut other = Store::default();
    other
        .graph
        .entities
        .insert("ent:svc".into(), child("Service", None, "svc.md"));
    fan_out_record(&mut other, "ent:svc", 12);
    assert!(caps_goal_gate(&other, &[], "ent:svc").is_none());
}

// Mirrors docs/compiler/goals/abstract-entity.md#hints: a group hint shows the
// word the statements use, and function words are never groups.
#[test]
fn cohesion_groups_drop_function_words_and_show_the_written_word() {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:svc".into(), child("Service", None, "svc.md"));
    for (i, t) in [
        "Every Service handler persists a session from the request.",
        "Every Service handler persists an audit entry from the request.",
        "Every Service handler persists an invoice from the queue.",
    ]
    .iter()
    .enumerate()
    {
        statement(
            &mut s,
            &format!("req:svc-{}", i),
            t,
            &["ent:svc"],
            "/svc/data",
        );
    }
    let groups = cohesion_groups(&s, "ent:svc");
    let words: Vec<&str> = groups.iter().map(|(w, _, _)| w.as_str()).collect();
    assert!(words.contains(&"persists"), "{:?}", words);
    assert!(words.contains(&"handler"), "{:?}", words);
    // "request" recurs in two statements only; "every" and "from" recur in three
    // and are function words; "persist" is the stem, never shown.
    assert!(
        !words
            .iter()
            .any(|w| *w == "request" || *w == "every" || *w == "from" || *w == "persist"),
        "{:?}",
        words
    );
    let persists = groups.iter().find(|(w, _, _)| w == "persists").unwrap();
    assert_eq!((persists.1, persists.2.as_str()), (3, "/svc/data"));
}

// Mirrors docs/compiler/goals/abstract-entity.md#hints: the defining section is
// the root of the document headed with the entity's name, never the front door's
// list item that mentions it first.
#[test]
fn defining_section_prefers_the_document_headed_with_the_name() {
    let mut s = Store::default();
    let mut e = child("API Server", None, "README.md");
    e.mentions.push(SourceRef {
        doc: "api-server.md".into(),
        section: "/api-server/routing".into(),
        quote: "API Server".into(),
    });
    s.graph.entities.insert("ent:api-server".into(), e);
    let root = |title: &str| Section {
        reflected: None,
        title: title.into(),
        kind: "root".into(),
        order: 0,
        parent: None,
        raw: String::new(),
        hash: String::new(),
        lines: [1, 1],
    };
    let mut readme = DocRecord::default();
    readme.sections.insert("/trellis".into(), root("Trellis"));
    s.docs.insert("README.md".into(), readme);
    let mut api = DocRecord::default();
    api.sections
        .insert("/api-server".into(), root("API Server"));
    s.docs.insert("api-server.md".into(), api);
    assert_eq!(
        defining_section(&s, "ent:api-server"),
        Some(("api-server.md".into(), "/api-server".into()))
    );
    // Without a namesake document, the mention section quoting most of its
    // requirements; with nothing quoted, the first mention.
    s.docs.clear();
    assert_eq!(
        defining_section(&s, "ent:api-server"),
        Some(("README.md".into(), "/x".into()))
    );
    let quoted = |s: &mut Store, id: &str, doc: &str, section: &str| {
        s.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: "The API Server routes.".into(),
                entities: vec!["ent:api-server".into()],
                source: Some(SourceRef {
                    doc: doc.into(),
                    section: section.into(),
                    quote: "routes".into(),
                }),
                ..Default::default()
            },
        );
    };
    quoted(&mut s, "req:a-1", "README.md", "/x");
    quoted(&mut s, "req:a-2", "api-server.md", "/api-server/routing");
    quoted(&mut s, "req:a-3", "api-server.md", "/api-server/routing");
    quoted(&mut s, "req:a-4", "other.md", "/elsewhere");
    assert_eq!(
        defining_section(&s, "ent:api-server"),
        Some(("api-server.md".into(), "/api-server/routing".into()))
    );
}

// Mirrors docs/compiler/goals/abstract-entity.md#fan-out-hints: a member the
// node's own document lists carries the listing section (the heading it sits
// under) beside the document it is mentioned in most; a member the node's
// document never names carries its document alone.
#[test]
fn fan_out_hints_carry_the_listing_section_of_the_nodes_document() {
    let mut s = levels_store();
    s.graph
        .entities
        .get_mut("ent:cart")
        .unwrap()
        .mentions
        .push(SourceRef {
            doc: "arch.md".into(),
            section: "/arch/data-model".into(),
            quote: "cart".into(),
        });
    let goals = AbstractEntity.derive_goals(&s, &gen_settings());
    let node = goals.iter().find(|g| g.target == "ent:backend").unwrap();
    assert!(
        node.hints
            .iter()
            .any(|h| h.starts_with("member ent:cart «component» (arch.md#/arch/data-model")),
        "{:?}",
        node.hints
    );
    assert!(
        node.hints
            .contains(&"member ent:pricing «component» (api.md)".to_string()),
        "{:?}",
        node.hints
    );
}

// Mirrors docs/compiler/goals/abstract-entity.md#fan-out-hints: a member whose
// name is the heading of the document naming it and other members is the
// level's namesake node.
#[test]
fn fan_out_hints_name_the_namesake_of_a_document() {
    let mut s = levels_store();
    let text = "# Shipment\nA shipment moves.\n";
    s.docs.insert(
        "shipping.md".into(),
        crate::model::DocRecord {
            content_hash: crate::model::hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: Default::default(),
        },
    );
    let goals = AbstractEntity.derive_goals(&s, &gen_settings());
    let node = goals.iter().find(|g| g.target == "ent:backend").unwrap();
    assert!(
        node.hints.contains(
            &"namesake ent:shipment (shipping.md is headed Shipment; 2 other member(s) of the level from it)"
                .to_string()
        ),
        "{:?}",
        node.hints
    );
    assert!(!node
        .hints
        .iter()
        .any(|h| h.starts_with("namesake ent:cart")));
}

fn grouping(id: &str, parent: Option<&str>, from: &[&str]) -> Op {
    Op::CreateEntity {
        id: id.into(),
        entity: Entity {
            name: id.trim_start_matches("ent:").into(),
            definition: Some("Holds the ordering area of the backend.".into()),
            parent: parent.map(String::from),
            provenance: Some(Provenance::Derived {
                from: from.iter().map(|f| f.to_string()).collect(),
                reasoning: "the documents treat these as one area".into(),
            }),
            ..Default::default()
        },
    }
}

fn reparent(id: &str, parent: &str) -> Op {
    Op::UpdateEntity {
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
    }
}

fn rules(v: &[Violation]) -> Vec<&str> {
    v.iter().map(|x| x.rule.as_str()).collect()
}

#[test]
fn fan_out_derivation_at_a_node_and_at_the_scope_root() {
    let s = levels_store();
    let goals = AbstractEntity.derive_goals(&s, &gen_settings());
    let ids: Vec<&str> = goals.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "g:abstract-entity:ent:backend",
            "g:abstract-entity:scope:public"
        ]
    );
    let node = &goals[0];
    assert!(!node.mandatory);
    assert_eq!(node.change["fan_out"], 12);
    assert_eq!(node.change["limit"], json!({"soft": 9, "hard": 15}));
    assert!(node.change.get("limits").is_none());
    let cands = node.change["candidates"].as_array().unwrap();
    assert_eq!(
        cands[0],
        json!(["ent:cart", "ent:checkout", "ent:discount", "ent:pricing"])
    );
    assert_eq!(
        cands[1],
        json!(["ent:carrier", "ent:shipment", "ent:tracking"])
    );
    assert_eq!(cands[2], json!(["ent:mailer", "ent:queue"]));
    assert!(node.hints.contains(&"load ent:backend".to_string()));
    assert!(node
        .hints
        .contains(&"fan-out 12 > 9 (children-per-entity, soft 9, hard 15)".to_string()));
    assert!(node.hints.contains(
        &"candidate [ent:cart, ent:checkout, ent:discount, ent:pricing] (weight 6)".to_string()
    ));
    assert!(node
        .hints
        .contains(&"member ent:cart «component» (orders.md)".to_string()));
    assert!(node
        .hints
        .contains(&"grouping ent:cart (1 children)".to_string()));
    assert!(!node.hints.iter().any(|h| h.starts_with("child ")));
    assert_eq!(
        node.hints.last().map(String::as_str),
        Some("skill abstraction; group_entities, update_entity, dissolve_entity")
    );
    let root = &goals[1];
    assert_eq!(root.target, "scope:public");
    assert_eq!(root.change["fan_out"], 10);
    assert_eq!(root.change["candidates"].as_array().unwrap().len(), 0);
    assert!(root.hints.contains(&"load scope:public".to_string()));
    assert!(root
        .hints
        .contains(&"member ent:backend «component» (arch.md)".to_string()));
    assert!(root
        .hints
        .contains(&"grouping ent:backend (12 children)".to_string()));
    let pack = AbstractEntity.pack(&s, &goals);
    assert!(pack.contains("- scope:public full\n"));
    assert!(pack.contains("- ent:backend full\n"));
    assert!(pack.contains("- ent:top-0 stub\n"));
    assert!(pack.contains("- ent:cart-item stub\n"));
    assert!(!pack.contains("arch.md#/x full"));

    // Over hard the goal is mandatory; a caps record on the same node folds in.
    let mut s = levels_store();
    fan_out_record(&mut s, "ent:backend", 16);
    threshold_record(&mut s, "ent:cart", "requirements-per-entity", 54);
    fan_out_record(&mut s, "ent:cart", 10);
    let goals = AbstractEntity.derive_goals(&s, &gen_settings());
    let backend = goals.iter().find(|g| g.target == "ent:backend").unwrap();
    assert!(backend.mandatory);
    let cart = goals.iter().find(|g| g.target == "ent:cart").unwrap();
    assert_eq!(cart.change["fan_out"], 10);
    assert_eq!(cart.change["limits"][0]["limit"], "requirements-per-entity");
    assert!(cart
        .hints
        .contains(&"54 > 50 (requirements-per-entity, soft 50, hard 80)".to_string()));
    assert!(cart
        .hints
        .contains(&"child ent:cart-item (1 requirements)".to_string()));
    assert!(AbstractEntity
        .pack(&s, &goals)
        .contains("- orders.md#/x full\n"));
}

#[test]
fn coupling_hints_deterministic_and_bounded() {
    let s = levels_store();
    let a = coupling_candidates(&s, "ent:backend", 9);
    let b = coupling_candidates(&s, "ent:backend", 9);
    assert_eq!(a, b);
    assert_eq!(a[0].weight, 6);
    assert_eq!(a[1].weight, 3);
    assert_eq!(a[2].weight, 1);
    // The stragglers nothing touches are never merged at zero weight, and a
    // singleton is no candidate.
    assert_eq!(a.len(), 3);
    assert!(a.iter().all(|c| c.ids.len() >= 2 && c.rest == 0));
    // Descendants lift: the cart item's requirement counted for cart.
    let w = coupling_weights(&s, &level_members(&s, "ent:backend"));
    assert_eq!(w[&("ent:cart".to_string(), "ent:pricing".to_string())], 3);
    assert_eq!(w[&("ent:cart".to_string(), "ent:checkout".to_string())], 1);
    // Bounded: a level of thirty peers in one chain yields at most the soft
    // threshold of clusters and twelve ids per cluster with the rest counted.
    let mut s = Store::default();
    for i in 0..30 {
        s.graph.entities.insert(
            format!("ent:n{:02}", i),
            child(&format!("n {}", i), Some("ent:hub"), "hub.md"),
        );
    }
    s.graph
        .entities
        .insert("ent:hub".into(), child("Hub", None, "hub.md"));
    for i in 0..29 {
        link(
            &mut s,
            i,
            &format!("ent:n{:02}", i),
            &format!("ent:n{:02}", i + 1),
        );
    }
    let c = coupling_candidates(&s, "ent:hub", 9);
    assert!(c.len() <= 9);
    assert!(c.iter().all(|x| x.ids.len() <= FAN_OUT_CANDIDATE_IDS));
    // A candidate must itself fit under the limit: no cluster past the soft
    // threshold, so the chain yields several candidates, never one whole.
    assert!(c.iter().all(|x| x.ids.len() + x.rest <= 4), "{:?}", c);
    assert!(c.len() >= 4, "{:?}", c);
    let total: usize = c.iter().map(|x| x.ids.len() + x.rest).sum();
    assert!((28..=30).contains(&total), "{}", total);
    assert_eq!(c, coupling_candidates(&s, "ent:hub", 9));
    // Ties break by id: two equal-weight pairs merge the one whose ids sort first.
    let mut s = Store::default();
    for n in ["a", "b", "c", "d"] {
        s.graph
            .entities
            .insert(format!("ent:{}", n), child(n, Some("ent:hub"), "hub.md"));
    }
    s.graph
        .entities
        .insert("ent:hub".into(), child("Hub", None, "hub.md"));
    link(&mut s, 1, "ent:c", "ent:d");
    link(&mut s, 2, "ent:a", "ent:b");
    let c = coupling_candidates(&s, "ent:hub", 1);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].ids, vec!["ent:a", "ent:b"]);
}

#[test]
fn fan_out_gate_accepts_a_valid_grouping() {
    let s = levels_store();
    let members = ["ent:cart", "ent:pricing", "ent:checkout", "ent:discount"];
    let mut staged = vec![grouping("ent:orders", Some("ent:backend"), &members)];
    staged.extend(members.iter().map(|m| reparent(m, "ent:orders")));
    // Twelve minus four plus one: nine, at the soft threshold.
    assert_eq!(fan_out_gates(&s, &staged), Vec::new());
    assert_eq!(AbstractEntity.gates(&s, &staged), Vec::new());
    // A top-level grouping at the scope root: ten minus two plus one.
    let staged = vec![
        grouping("ent:tops", None, &["ent:top-0", "ent:top-1"]),
        reparent("ent:top-0", "ent:tops"),
        reparent("ent:top-1", "ent:tops"),
    ];
    assert_eq!(fan_out_gates(&s, &staged), Vec::new());
    // A caps split with requirements re-pointed is not a grouping: one child
    // under it and a statement moved passes the fan-out gate untouched.
    let mut s = s;
    s.status.changes.clear();
    let staged = vec![
        grouping(
            "ent:cart-pricing",
            Some("ent:cart"),
            &["ent:cart", "req:x-1"],
        ),
        Op::UpdateRequirement {
            id: "req:x-1".into(),
            statement: None,
            entities: Some(vec!["ent:cart-pricing".into(), "ent:pricing".into()]),
            edges: None,
            transition: None,
            facets: None,
            source: None,
            provenance: None,
        },
    ];
    assert_eq!(fan_out_gates(&s, &staged), Vec::new());
}

#[test]
fn fan_out_gate_rejects_a_one_member_grouping() {
    let s = levels_store();
    let staged = vec![
        grouping("ent:orders", Some("ent:backend"), &["ent:cart"]),
        reparent("ent:cart", "ent:orders"),
    ];
    let v = fan_out_gates(&s, &staged);
    assert!(rules(&v).contains(&"grouping-too-small"), "{:?}", v);
    assert!(v[0].message.contains("ent:orders"));
    // The level stays over the limit too: twelve minus one plus one.
    assert!(rules(&v).contains(&"fan-out-over-limit"), "{:?}", v);
    assert_eq!(
        rules(&AbstractEntity.gates(&s, &staged))[0],
        "grouping-too-small"
    );
    // Provenance naming other than the members, and a missing definition.
    let mut staged = vec![grouping(
        "ent:orders",
        Some("ent:backend"),
        &["ent:cart", "ent:pricing", "ent:backend"],
    )];
    staged.extend(
        ["ent:cart", "ent:pricing", "ent:checkout", "ent:discount"]
            .iter()
            .map(|m| reparent(m, "ent:orders")),
    );
    if let Op::CreateEntity { entity, .. } = &mut staged[0] {
        entity.definition = None;
    }
    let v = fan_out_gates(&s, &staged);
    assert!(rules(&v).contains(&"provenance-mismatch"), "{:?}", v);
    assert!(rules(&v).contains(&"definition-required"), "{:?}", v);
    // Grouping only two of twelve leaves the level over soft.
    let staged = vec![
        grouping(
            "ent:mail",
            Some("ent:backend"),
            &["ent:queue", "ent:mailer"],
        ),
        reparent("ent:queue", "ent:mail"),
        reparent("ent:mailer", "ent:mail"),
    ];
    let v = fan_out_gates(&s, &staged);
    assert_eq!(rules(&v), vec!["fan-out-over-limit"]);
    assert!(v[0]
        .message
        .starts_with("ent:backend: 11 children after the changeset, over 9"));
    // Over hard the limit in force is hard: sixteen to fifteen passes.
    let mut s = levels_store();
    fan_out_record(&mut s, "ent:backend", 16);
    assert_eq!(fan_out_gates(&s, &staged), Vec::new());
}

#[test]
fn fan_out_gate_rejects_a_cross_level_grouping() {
    let s = levels_store();
    // A member from the scope root beside members of the backend's level.
    let members = ["ent:cart", "ent:pricing", "ent:top-0"];
    let mut staged = vec![grouping("ent:orders", Some("ent:backend"), &members)];
    staged.extend(members.iter().map(|m| reparent(m, "ent:orders")));
    let v = fan_out_gates(&s, &staged);
    assert_eq!(rules(&v)[0], "cross-level", "{:?}", v);
    assert_eq!(rules(&v).iter().filter(|r| **r == "cross-level").count(), 1);
    assert!(v[0]
        .message
        .contains("ent:top-0 sits under the scope root while the grouping takes ent:backend"));
    // The grouping's own parent is not the members' parent either.
    let members = ["ent:cart", "ent:pricing"];
    let mut staged = vec![grouping("ent:orders", None, &members)];
    staged.extend(members.iter().map(|m| reparent(m, "ent:orders")));
    let v = fan_out_gates(&s, &staged);
    assert_eq!(&rules(&v)[..2], &["cross-level", "cross-level"], "{:?}", v);
    // A member of another scope.
    let mut s = levels_store();
    s.graph.entities.get_mut("ent:pricing").unwrap().scope = "internal".into();
    let members = ["ent:cart", "ent:pricing"];
    let mut staged = vec![grouping("ent:orders", Some("ent:backend"), &members)];
    staged.extend(members.iter().map(|m| reparent(m, "ent:orders")));
    let v = fan_out_gates(&s, &staged);
    assert!(rules(&v).contains(&"scope-mismatch"), "{:?}", v);
    // A dissolve lands the children back on the level: the count check sees them.
    let mut s = levels_store();
    s.graph.entities.insert(
        "ent:mail".into(),
        Entity {
            name: "mail".into(),
            parent: Some("ent:backend".into()),
            provenance: Some(Provenance::Derived {
                from: vec!["ent:queue".into(), "ent:mailer".into()],
                reasoning: "one area".into(),
            }),
            ..Default::default()
        },
    );
    for m in ["ent:queue", "ent:mailer"] {
        s.graph.entities.get_mut(m).unwrap().parent = Some("ent:mail".into());
    }
    fan_out_record(&mut s, "ent:backend", 11);
    let staged = vec![Op::DissolveEntity {
        id: "ent:mail".into(),
        reason: "one member left".into(),
        parent: None,
        children: Vec::new(),
    }];
    let v = fan_out_gates(&s, &staged);
    assert_eq!(rules(&v), vec!["fan-out-over-limit"], "{:?}", v);
    assert!(v[0].message.starts_with("ent:backend: 12 children"));
}
