use super::*;

// Mirrors docs/compiler/tools.md#goal-tools: evidence handed as a JSON string (the
// text codec's shape) is parsed before the gate reads it.
#[test]
fn string_encoded_evidence_is_parsed_before_the_gate() {
    let mut s = Store::default();
    s.graph.entities.insert("ent:sort".into(), plain("Sort"));
    s.graph.requirements.insert(
        "req:a".into(),
        Requirement {
            statement: "The utility sorts its lines.".into(),
            entities: vec!["ent:sort".into()],
            ..Default::default()
        },
    );
    s.graph.criteria.insert(
        "crit:a-1".into(),
        crate::model::Criterion {
            requirement: "req:a".into(),
            statement: "Three lines come out ordered.".into(),
            kind: "positive".into(),
            provenance: Some(Provenance::Derived {
                from: vec!["req:a".into()],
                reasoning: "the rule".into(),
            }),
            ..Default::default()
        },
    );
    let goals = vec![Goal {
        id: "g:derive-criteria:ent:sort".into(),
        kind: "derive-criteria".into(),
        mandatory: true,
        target: "ent:sort".into(),
        change: json!({"none": ["req:a"], "revised": []}),
        ..Default::default()
    }];
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    let v = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": "g:derive-criteria:ent:sort", "justification": "Criteria written.",
                    "evidence": "{\"requirements\": [{\"id\": \"req:a\", \"criteria\": [\"crit:a-1\"], \"verdict\": \"complete\"}]}"}),
        )
        .unwrap();
    assert_eq!(v["marked"], "g:derive-criteria:ent:sort", "{}", v);
    let err = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": "g:derive-criteria:ent:sort", "justification": "Criteria written.", "evidence": [{"id": "req:a"}]}),
        )
        .unwrap_err();
    assert!(err.message.contains("bare array"), "{}", err.message);
}

// Mirrors docs/compiler/goals/review-entity.md#gate: with lint rules declared, a
// review names each rule by number, and filed needs a lint finding on the entity.
#[test]
fn review_names_every_lint_rule_and_backs_filed() {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:gadget".into(), plain("Gadget"));
    s.graph.requirements.insert(
        "req:gadget-1".into(),
        Requirement {
            statement: "The Gadget shall report its battery level.".into(),
            entities: vec!["ent:gadget".into()],
            ..Default::default()
        },
    );
    let goals = vec![Goal {
        id: "g:review-entity:ent:gadget".into(),
        kind: "review-entity".into(),
        mandatory: true,
        target: "ent:gadget".into(),
        ..Default::default()
    }];
    let mark = |t: &mut ToolSession, lint: Value| {
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:review-entity:ent:gadget", "justification": "Reviewed.",
                    "evidence": {"definition": "current", "lint": lint}}),
        )
    };
    // No rules declared: nothing to name.
    let mut bare = ToolSession::new(s.clone(), WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    assert!(bare
        .dispatch("mark_goal_done", &json!({"goal": "g:review-entity:ent:gadget", "justification": "Reviewed.", "evidence": {"definition": "current"}}))
        .is_ok());
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    t.lint = crate::project::Linting {
        warnings: vec!["An em dash appears in prose.".into()],
        errors: vec!["A TODO marker appears in prose.".into()],
    };
    let err = mark(&mut t, json!([{"rule": 1, "verdict": "clean"}])).unwrap_err();
    assert_eq!(err.rule, "lint-unchecked", "{}", err.message);
    assert!(err.message.contains("rule [2]"), "{}", err.message);
    let err = mark(
        &mut t,
        json!([{"rule": 1, "verdict": "filed"}, {"rule": 2, "verdict": "clean"}]),
    )
    .unwrap_err();
    assert_eq!(err.rule, "lint-unbacked", "{}", err.message);
    t.dispatch(
        "report_diagnostic",
        &json!({"rule": "lint", "severity": "warning", "subjects": ["req:gadget-1"],
                "message": "The quote carries an em dash."}),
    )
    .unwrap();
    let v = mark(
        &mut t,
        json!([{"rule": 1, "verdict": "filed"}, {"rule": "2", "verdict": "clean"}]),
    )
    .unwrap();
    assert_eq!(v["marked"], "g:review-entity:ent:gadget", "{}", v);
}

// Mirrors docs/compiler/goals/review-entity.md#gate: a word-match verdict is
// remembered on the entity, a referenced verdict needs the staged update behind it.
#[test]
fn review_unreferenced_verdicts_are_backed_or_remembered() {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:customer".into(), plain("Customer"));
    s.graph.entities.insert("ent:order".into(), plain("Order"));
    s.graph.requirements.insert(
        "req:a".into(),
        Requirement {
            statement: "An Order names the Customer who placed it.".into(),
            entities: vec!["ent:order".into()],
            ..Default::default()
        },
    );
    let goals = vec![Goal {
        id: "g:review-entity:ent:customer".into(),
        kind: "review-entity".into(),
        mandatory: true,
        target: "ent:customer".into(),
        ..Default::default()
    }];
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    let err = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": "g:review-entity:ent:customer", "justification": "Reviewed.",
                    "evidence": {"definition": "current", "unreferenced": [{"id": "req:a", "verdict": "referenced"}]}}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unreferenced-unbacked", "{}", err.message);
    let v = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": "g:review-entity:ent:customer", "justification": "Reviewed.",
                    "evidence": {"definition": "current", "unreferenced": [{"id": "req:a", "verdict": "word-match", "reason": "the Customer is the object here"}]}}),
        )
        .unwrap();
    assert_eq!(v["marked"], "g:review-entity:ent:customer", "{}", v);
    assert!(
        t.staged.iter().any(|o| matches!(o, Op::UpdateEntity { id, not_about_add, .. } if id == "ent:customer" && not_about_add == &vec!["req:a".to_string()])),
        "{:?}",
        t.staged.len()
    );
    // A repeated claim stages no second update.
    t.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:review-entity:ent:customer", "justification": "Reviewed.",
                "evidence": {"definition": "current", "unreferenced": [{"id": "req:a", "verdict": "word-match", "reason": "the Customer is the object here"}]}}),
    )
    .unwrap();
    assert_eq!(t.staged.len(), 1, "{:?}", t.staged.len());
}

// Mirrors docs/compiler/goals/review-entity.md#gate: every lookalike, open
// diagnostic, and unparented part the hints list carries a verdict, each backed by
// the staged work, and the definition claim agrees with it.
#[test]
fn review_holds_a_verdict_per_listed_lookalike_diagnostic_and_part() {
    let mut s = Store::default();
    s.graph.entities.insert("ent:cart".into(), plain("Cart"));
    s.graph
        .entities
        .insert("ent:cart-page".into(), plain("Cart Page"));
    s.graph.entities.insert("ent:item".into(), plain("Item"));
    s.graph.requirements.insert(
        "req:a".into(),
        Requirement {
            statement: "The Cart contains Items.".into(),
            entities: vec!["ent:cart".into(), "ent:item".into()],
            edges: vec![ReqEdge {
                a: "ent:cart".into(),
                b: "ent:item".into(),
                rel_type: Some("composition".into()),
                cardinality: None,
            }],
            ..Default::default()
        },
    );
    s.graph.diagnostics.insert(
        "diag:ambiguity-1".into(),
        open_diag("ambiguity", &["req:a"]),
    );
    let goals = one_goal("review-entity", "ent:cart", Value::Null);
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    let mark = |t: &mut ToolSession, evidence: Value| {
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:review-entity:ent:cart", "justification": "Reviewed.", "evidence": evidence}),
        )
    };
    let rule = |r: Result<Value, ToolError>| r.map(|v| v.to_string()).unwrap_err().rule;
    assert_eq!(rule(mark(&mut t, json!({}))), "bad-evidence");
    assert_eq!(
        rule(mark(&mut t, json!({"definition": "current"}))),
        "lookalike-unjudged"
    );
    let separate = json!([{"id": "ent:cart-page", "verdict": "separate", "reason": "a page showing the Cart, not the Cart"}]);
    assert_eq!(
        rule(mark(
            &mut t,
            json!({"definition": "current", "lookalikes": [{"id": "ent:cart-page", "verdict": "merged"}]})
        )),
        "merge-unbacked"
    );
    assert_eq!(
        rule(mark(
            &mut t,
            json!({"definition": "current", "lookalikes": [{"id": "ent:cart-page", "verdict": "separate", "reason": "n/a"}]})
        )),
        "reason-required"
    );
    assert_eq!(
        rule(mark(
            &mut t,
            json!({"definition": "current", "lookalikes": separate})
        )),
        "diagnostic-unjudged"
    );
    let stands = json!([{"id": "diag:ambiguity-1", "verdict": "stands"}]);
    assert_eq!(
        rule(mark(
            &mut t,
            json!({"definition": "current", "lookalikes": separate, "diagnostics": [{"id": "diag:ambiguity-1", "verdict": "resolved"}]})
        )),
        "diagnostic-unbacked"
    );
    assert_eq!(
        rule(mark(
            &mut t,
            json!({"definition": "current", "lookalikes": separate, "diagnostics": stands})
        )),
        "part-unjudged"
    );
    let parented = json!([{"id": "ent:item", "verdict": "parented"}]);
    assert_eq!(
        rule(mark(
            &mut t,
            json!({"definition": "current", "lookalikes": separate, "diagnostics": stands, "parts": parented})
        )),
        "part-unbacked"
    );
    assert_eq!(
        rule(mark(
            &mut t,
            json!({"definition": "current", "lookalikes": separate, "diagnostics": stands, "parts": [{"id": "ent:item", "verdict": "kept"}]})
        )),
        "reason-required"
    );
    t.dispatch(
        "update_entity",
        &json!({"id": "ent:item", "parent": "ent:cart"}),
    )
    .unwrap();
    assert_eq!(
        rule(mark(
            &mut t,
            json!({"definition": "refreshed", "lookalikes": separate, "diagnostics": stands, "parts": parented})
        )),
        "definition-unbacked"
    );
    let v = mark(&mut t, json!({"definition": "current", "lookalikes": separate, "diagnostics": stands, "parts": parented})).unwrap();
    assert_eq!(v["marked"], "g:review-entity:ent:cart", "{}", v);
}

// Mirrors docs/compiler/goals/review-entity.md#gate: a statement the hints list as
// naming the entity without referencing it takes an unreferenced verdict.
#[test]
fn review_judges_every_listed_unreferenced_statement() {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:customer".into(), plain("Customer"));
    s.graph.entities.insert("ent:order".into(), plain("Order"));
    s.graph.requirements.insert(
        "req:a".into(),
        Requirement {
            statement: "An Order names the Customer who placed it.".into(),
            entities: vec!["ent:order".into()],
            ..Default::default()
        },
    );
    let goals = one_goal("review-entity", "ent:customer", Value::Null);
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    let err = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": "g:review-entity:ent:customer", "justification": "Reviewed.", "evidence": {"definition": "current"}}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unreferenced-unjudged", "{}", err.message);
    assert!(t.staged.is_empty());
}

// Mirrors docs/compiler/goals/rejudge-pair.md#gate: the verdict is named, and
// the staged work or a standing finding carries it.
#[test]
fn rejudge_pair_verdict_is_backed_by_the_staged_work() {
    let mut t = pair_session();
    let mark = |t: &mut ToolSession, goal: &str, evidence: Value| {
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": goal, "justification": "Judged.", "evidence": evidence}),
        )
    };
    let ab = "g:rejudge-pair:req:a~req:b";
    assert_eq!(
        mark(&mut t, ab, Value::Null).unwrap_err().rule,
        "bad-evidence"
    );
    assert_eq!(
        mark(&mut t, ab, json!({"verdict": "duplicate"}))
            .unwrap_err()
            .rule,
        "duplicate-unbacked"
    );
    assert_eq!(
        mark(&mut t, ab, json!({"verdict": "contradiction"}))
            .unwrap_err()
            .rule,
        "contradiction-unbacked"
    );
    assert!(mark(&mut t, ab, json!({"verdict": "consistent"})).is_ok());
    t.snapshot.graph.diagnostics.insert(
        "diag:contradiction-1".into(),
        open_diag("contradiction", &["req:a", "req:c"]),
    );
    let ac = "g:rejudge-pair:req:a~req:c";
    assert_eq!(
        mark(&mut t, ac, json!({"verdict": "consistent"}))
            .unwrap_err()
            .rule,
        "pair-diagnostic-open"
    );
    assert!(mark(
        &mut t,
        ac,
        json!({"verdict": "contradiction", "carried_by": "diag:contradiction-1"})
    )
    .is_ok());
    t.snapshot.graph.diagnostics.insert(
        "diag:duplicate-requirement-2".into(),
        open_diag("duplicate-requirement", &["req:b", "req:c", "req:gone"]),
    );
    let bc = "g:rejudge-pair:req:b~req:c";
    assert_eq!(
        mark(&mut t, bc, json!({"verdict": "duplicate"}))
            .unwrap_err()
            .rule,
        "dead-subject"
    );
    t.dispatch(
        "resolve_diagnostic",
        &json!({"id": "diag:duplicate-requirement-2", "reason": "req:gone is deleted"}),
    )
    .unwrap();
    assert!(mark(&mut t, bc, json!("consistent")).is_ok());
}

// Mirrors docs/compiler/goals/declare-edges.md#gate and dedupe-candidates.md#gate:
// the single verdict names what the staged work did.
#[test]
fn declare_edges_and_dedupe_verdicts_match_the_staged_work() {
    let mut s = Store::default();
    s.graph.entities.insert("ent:cart".into(), plain("Cart"));
    s.graph.entities.insert("ent:item".into(), plain("Item"));
    s.graph.requirements.insert(
        "req:a".into(),
        Requirement {
            statement: "The Cart holds Items.".into(),
            entities: vec!["ent:cart".into(), "ent:item".into()],
            ..Default::default()
        },
    );
    let goals = one_goal("declare-edges", "req:a", Value::Null);
    let mut t = ToolSession::new(s.clone(), WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    let mark = |t: &mut ToolSession, goal: &str, evidence: Value| {
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": goal, "justification": "Judged.", "evidence": evidence}),
        )
    };
    let g = "g:declare-edges:req:a";
    assert_eq!(
        mark(&mut t, g, Value::Null).unwrap_err().rule,
        "bad-evidence"
    );
    assert_eq!(
        mark(&mut t, g, json!("declared")).unwrap_err().rule,
        "edges-unbacked"
    );
    t.dispatch(
        "update_requirement",
        &json!({"id": "req:a", "edges": [{"a": "ent:cart", "b": "ent:item", "type": "aggregation"}]}),
    )
    .unwrap();
    assert_eq!(
        mark(&mut t, g, json!("not-structural")).unwrap_err().rule,
        "edges-staged"
    );
    assert!(mark(&mut t, g, json!({"verdict": "declared"})).is_ok());

    let goals = one_goal("dedupe-candidates", "ent:cart~ent:item", Value::Null);
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    let g = "g:dedupe-candidates:ent:cart~ent:item";
    assert_eq!(
        mark(&mut t, g, json!("merged")).unwrap_err().rule,
        "merge-unbacked"
    );
    t.dispatch(
        "merge_entities",
        &json!({"keep": "ent:cart", "absorb": "ent:item", "reason": "one concept named twice"}),
    )
    .unwrap();
    assert_eq!(
        mark(&mut t, g, json!("separate")).unwrap_err().rule,
        "merge-staged"
    );
    assert!(mark(&mut t, g, json!("merged")).is_ok());
}

// Mirrors docs/compiler/goals/curate-view.md#gate: every matched node is added or
// excluded with a note, and a note naming another flow names one that lists it.
#[test]
fn curate_view_decides_every_matched_node() {
    let mut s = Store::default();
    s.graph.entities.insert("ent:cart".into(), plain("Cart"));
    for id in ["req:a", "req:b"] {
        s.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: format!("The Cart acts ({}).", id),
                entities: vec!["ent:cart".into()],
                ..Default::default()
            },
        );
    }
    for (id, title, members) in [
        (
            "view:use-case/checkout",
            "Checkout",
            vec!["req:a".to_string()],
        ),
        ("view:use-case/returns", "Returns", Vec::new()),
    ] {
        s.graph.views.insert(
            id.into(),
            View {
                kind: "use-case".into(),
                title: title.into(),
                members,
                ..Default::default()
            },
        );
    }
    let goals = one_goal(
        "curate-view",
        "view:use-case/checkout",
        json!({"matched": [{"id": "req:b", "why": "unplaced behavior"}]}),
    );
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    let mark = |t: &mut ToolSession, evidence: Value| {
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:curate-view:view:use-case/checkout", "justification": "Decided.", "evidence": evidence}),
        )
    };
    assert_eq!(
        mark(&mut t, Value::Null).unwrap_err().rule,
        "member-unjudged"
    );
    assert_eq!(
        mark(&mut t, json!([{"id": "req:b", "verdict": "added"}]))
            .unwrap_err()
            .rule,
        "added-unbacked"
    );
    let excluded = json!([{"id": "req:b", "verdict": "excluded"}]);
    assert_eq!(
        mark(&mut t, excluded.clone()).unwrap_err().rule,
        "excluded-unbacked"
    );
    t.dispatch(
        "update_view",
        &json!({"id": "view:use-case/checkout", "exclude": [{"id": "req:b", "note": "belongs to view:use-case/returns."}]}),
    )
    .unwrap();
    assert_eq!(
        mark(&mut t, excluded.clone()).unwrap_err().rule,
        "exclusion-points-nowhere"
    );
    t.dispatch(
        "update_view",
        &json!({"id": "view:use-case/returns", "add_members": ["req:b"]}),
    )
    .unwrap();
    assert!(mark(&mut t, excluded).is_ok());
}

// Mirrors docs/compiler/goals/conform-instance.md#gate: one verdict per declared
// attribute and per link, nonconformant backed, a standing finding answered.
#[test]
fn conform_instance_holds_a_verdict_per_attribute_and_link() {
    let mut s = Store::default();
    let decree = Provenance::Decree {
        author: "t".into(),
        at: "t".into(),
        note: None,
    };
    let mut customer = plain("Customer");
    customer.attributes.push(Attribute {
        name: "tier".into(),
        r#type: Some("string".into()),
        value: None,
        provenance: decree,
    });
    s.graph.entities.insert("ent:customer".into(), customer);
    s.graph.entities.insert("ent:ana".into(), plain("Ana"));
    s.graph.entities.insert("ent:cart".into(), plain("Cart"));
    let contribution = |b: &str, ty: &str| Contribution {
        a: "ent:ana".into(),
        b: b.into(),
        r#type: ty.into(),
        cardinality: None,
        requirements: Vec::new(),
    };
    s.graph.relationships.insert(
        "rel:1".into(),
        Relationship {
            members: vec!["ent:ana".into(), "ent:customer".into()],
            contributions: vec![contribution("ent:customer", "instantiation")],
        },
    );
    s.graph.relationships.insert(
        "rel:2".into(),
        Relationship {
            members: vec!["ent:ana".into(), "ent:cart".into()],
            contributions: vec![contribution("ent:cart", "association")],
        },
    );
    let goals = one_goal("conform-instance", "ent:ana", Value::Null);
    let mut t = ToolSession::new(s.clone(), WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    let mark = |t: &mut ToolSession, evidence: Value| {
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:conform-instance:ent:ana", "justification": "Compared.", "evidence": evidence}),
        )
    };
    assert_eq!(
        mark(&mut t, Value::Null).unwrap_err().rule,
        "conformance-unjudged"
    );
    let attrs = json!([{"name": "Tier", "verdict": "missing"}]);
    let err = mark(&mut t, json!({"attributes": attrs})).unwrap_err();
    assert_eq!(err.rule, "conformance-unjudged");
    assert!(err.message.contains("ent:cart"), "{}", err.message);
    let bad_link =
        json!({"attributes": attrs, "links": [{"to": "ent:cart", "verdict": "nonconformant"}]});
    assert_eq!(
        mark(&mut t, bad_link.clone()).unwrap_err().rule,
        "nonconformant-unbacked"
    );
    t.dispatch(
        "report_diagnostic",
        &json!({"rule": "nonconformant-instance", "severity": "warning", "subjects": ["ent:ana", "ent:customer"],
                "message": "Ana links a Cart; Customer has no Cart relationship."}),
    )
    .unwrap();
    assert!(mark(&mut t, bad_link).is_ok());

    s.graph.diagnostics.insert(
        "diag:nonconformant-instance-1".into(),
        open_diag("nonconformant-instance", &["ent:ana", "ent:customer"]),
    );
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    let conforms =
        json!({"attributes": attrs, "links": [{"to": "ent:cart", "verdict": "conforms"}]});
    assert_eq!(
        mark(&mut t, conforms).unwrap_err().rule,
        "nonconformant-open"
    );
}

// Mirrors docs/compiler/goals/derive-criteria.md#gate: one entry per requirement
// the goal lists.
#[test]
fn derive_criteria_judges_every_listed_requirement() {
    let mut s = Store::default();
    s.graph.entities.insert("ent:sort".into(), plain("Sort"));
    for id in ["req:a", "req:b"] {
        s.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: format!("The utility sorts ({}).", id),
                entities: vec!["ent:sort".into()],
                ..Default::default()
            },
        );
        s.graph.criteria.insert(
            format!("crit:{}-1", id),
            crate::model::Criterion {
                requirement: id.into(),
                statement: format!("Three lines come out ordered ({}).", id),
                kind: "positive".into(),
                ..Default::default()
            },
        );
    }
    let goals = one_goal(
        "derive-criteria",
        "ent:sort",
        json!({"none": ["req:a", "req:b"], "revised": []}),
    );
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    let err = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": "g:derive-criteria:ent:sort", "justification": "Criteria written.",
                    "evidence": {"requirements": [{"id": "req:a", "criteria": ["crit:req:a-1"], "verdict": "stated-only"}]}}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "criteria-unjudged", "{}", err.message);
    assert!(err.message.contains("req:b"), "{}", err.message);
}

// A claim reads the store after the staged work, at the mark and again at done:
// a criterion deleted in the session backs no verdict.
// Mirrors docs/compiler/sessions.md#resolving-failing-parking.
#[test]
fn derive_criteria_evidence_reads_the_staged_work_at_mark_and_done() {
    let mut s = Store::default();
    s.graph.entities.insert("ent:sort".into(), plain("Sort"));
    s.graph.requirements.insert(
        "req:a".into(),
        Requirement {
            statement: "The utility sorts.".into(),
            entities: vec!["ent:sort".into()],
            ..Default::default()
        },
    );
    s.graph.criteria.insert(
        "crit:a-1".into(),
        crate::model::Criterion {
            requirement: "req:a".into(),
            statement: "Three lines come out ordered.".into(),
            kind: "positive".into(),
            ..Default::default()
        },
    );
    let goals = one_goal(
        "derive-criteria",
        "ent:sort",
        json!({"none": ["req:a"], "revised": []}),
    );
    let claim = json!({"goal": "g:derive-criteria:ent:sort", "justification": "Criteria written.",
            "evidence": {"requirements": [{"id": "req:a", "criteria": ["crit:a-1"], "verdict": "stated-only"}]}});
    let delete = json!({"id": "crit:a-1", "reason": "wrong"});
    // A delete staged before the claim.
    let mut t = ToolSession::new(s.clone(), WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    t.dispatch("delete_criterion", &delete).unwrap();
    let err = t.dispatch("mark_goal_done", &claim).unwrap_err();
    assert_eq!(err.rule, "criterion-missing", "{}", err.message);
    // A delete staged after the claim is caught at done.
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    t.dispatch("mark_goal_done", &claim).unwrap();
    t.dispatch("delete_criterion", &delete).unwrap();
    let err = t.dispatch("done", &json!({"summary": "x"})).unwrap_err();
    assert_eq!(err.rule, "criterion-missing", "{}", err.message);
}

#[test]
fn derive_criteria_requires_refreshing_every_stale_criterion() {
    let _fs = crate::vfs::Scope::enter(&BTreeMap::new());
    for verdict in ["complete", "stated-only", "open-question"] {
        for repair in ["update", "upsert", "delete"] {
            let mut s = Store::default();
            s.graph.entities.insert("ent:sort".into(), plain("Sort"));
            s.graph.requirements.insert(
                "req:a".into(),
                Requirement {
                    statement: "The utility sorts its lines.".into(),
                    entities: vec!["ent:sort".into()],
                    ..Default::default()
                },
            );
            for (id, stale) in [("crit:a-1", true), ("crit:a-2", false)] {
                s.graph.criteria.insert(
                    id.into(),
                    Criterion {
                        requirement: "req:a".into(),
                        statement: format!("Three lines come out ordered ({id})."),
                        kind: "positive".into(),
                        stale,
                        ..Default::default()
                    },
                );
            }
            s.graph.diagnostics.insert(
                "diag:choice".into(),
                open_diag("invented-choice", &["req:a"]),
            );
            let goals = one_goal(
                "derive-criteria",
                "ent:sort",
                json!({"none": [], "revised": ["req:a"]}),
            );
            let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
            let claim = json!({"goal": "g:derive-criteria:ent:sort", "justification": "The existing criteria cover the revised requirement.",
                "evidence": {"requirements": [{"id": "req:a", "criteria": ["crit:a-2"], "verdict": verdict}]}});
            let err = t.dispatch("mark_goal_done", &claim).unwrap_err();
            assert_eq!(err.rule, "criterion-stale", "{verdict}: {}", err.message);
            assert!(err.message.contains("crit:a-1"), "{}", err.message);
            assert!(err.message.contains("update_criterion"), "{}", err.message);
            assert!(t.outcomes.is_empty());
            // Refresh unchanged wording or remove the obsolete stale criterion.
            match repair {
                "update" => {
                    t.dispatch("update_criterion", &json!({"id": "crit:a-1", "statement": "Three lines come out ordered (crit:a-1)."})).unwrap();
                }
                "upsert" => {
                    t.dispatch("upsert_criterion", &json!({"requirement": "req:a", "statement": "Three lines come out ordered (crit:a-1).", "kind": "positive", "provenance": {"derived": {"from": ["req:a"], "reasoning": "The revised rule still requires this ordering."}}})).unwrap();
                }
                "delete" => {
                    t.dispatch("delete_criterion", &json!({"id": "crit:a-1", "reason": "The other current criterion covers this outcome."})).unwrap();
                }
                _ => unreachable!(),
            }
            t.dispatch("mark_goal_done", &claim).unwrap();
            t.dispatch(
                "done",
                &json!({"summary": "Reviewed the existing criteria."}),
            )
            .unwrap();
            let mut store = t.snapshot.clone();
            store.out = format!("/project/.jazyk/{verdict}-{repair}").into();
            let report = store.apply(t.staged.clone(), &t.commit(1, None));
            assert!(report.skipped.is_empty(), "{:?}", report.skipped);
            let loaded = Store::load(&store.out);
            assert!(!loaded.graph.criteria.is_empty());
            assert!(loaded.graph.criteria.values().all(|c| !c.stale));
            assert!(!crate::goals::criteria_owed(&loaded).contains_key("ent:sort"));
        }
    }
}

#[test]
fn implicit_done_drops_dishonest_coverage_and_commits_the_rest() {
    let mut t = session();
    t.dispatch(
        "upsert_entity",
        &json!({"name": "Shopping Cart", "definition": "holds items", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}),
    )
    .unwrap();
    // A covered claim with no requirement sourced from the section is dishonest;
    // the explicit done bounces it.
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered"}),
    )
    .unwrap();
    assert!(t.dispatch("done", &json!({"summary": "covered"})).is_err());
    // The implicit done drops the offending mark and commits the rest.
    assert!(t.finish_implicit("(implicit: test)"));
    assert!(t.done.is_some());
    assert!(t
        .staged
        .iter()
        .any(|op| matches!(op, Op::CreateEntity { .. })));
    assert!(!t
        .staged
        .iter()
        .any(|op| matches!(op, Op::SetCoverage { state, .. } if state == "covered")));
}

#[test]
fn full_flow_stages_ops_and_done() {
    let mut t = session();
    let e = t
        .dispatch("upsert_entity", &json!({"name": "Shopping Cart", "definition": "holds items", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}))
        .unwrap();
    let id = e["id"].as_str().unwrap().to_string();
    assert_eq!(id, "ent:shopping-cart");
    t.dispatch(
        "upsert_requirement",
        &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The Shopping Cart shall hold items a Customer intends to buy.", "entities": [id, "ent:customer"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy", "edges": [{"a": "ent:shopping-cart", "b": "ent:customer", "type": "association"}]}),
    )
    .unwrap();
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered"}),
    )
    .unwrap();
    let err = t
        .dispatch(
            "set_coverage",
            &json!({"section": "/nope", "state": "covered"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unknown-section");
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop", "state": "non-normative", "note": "intro text only"}),
    )
    .unwrap();
    t.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "Extracted the cart statement and marked the section."}),
    )
    .unwrap();
    t.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:reconcile-section:shop.md#/shop", "justification": "Intro text only."}),
    )
    .unwrap();
    t.dispatch("done", &json!({"summary": "reconciled cart"}))
        .unwrap();
    assert!(t.done.is_some());
    assert_eq!(t.staged.len(), 4);
}

// done refuses while a mandatory goal is unaddressed, and mark_goal_done rejects
// a false claim: the reconcile-section goal whose section carries no mark.
#[test]
fn explicit_done_requires_a_mark_per_dirty_section() {
    let mut t = session();
    t.dispatch(
        "upsert_entity",
        &json!({"name": "Shopping Cart", "definition": "holds items", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}),
    )
    .unwrap();
    t.dispatch(
        "upsert_requirement",
        &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The Shopping Cart shall hold items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy"}),
    )
    .unwrap();
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered"}),
    )
    .unwrap();
    t.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "Extracted and marked."}),
    )
    .unwrap();
    // The other section's goal owes its mark: the claim is rejected naming the gate.
    let err = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": "g:reconcile-section:shop.md#/shop", "justification": "Nothing here."}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unmarked-section");
    assert!(err.message.contains("/shop"));
    // done with a mandatory goal neither done nor failed is rejected naming it.
    let err = t
        .dispatch("done", &json!({"summary": "cart only"}))
        .unwrap_err();
    assert_eq!(err.rule, "open-goal");
    assert!(err.message.contains("g:reconcile-section:shop.md#/shop"));
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop", "state": "non-normative", "note": "intro text only"}),
    )
    .unwrap();
    t.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:reconcile-section:shop.md#/shop", "justification": "Intro text only."}),
    )
    .unwrap();
    t.dispatch("done", &json!({"summary": "all sections marked"}))
        .unwrap();
    assert!(t.done.is_some());
}

#[test]
fn inline_done_validates_final_claims_and_preserves_earlier_work_for_repair() {
    let mut t = session();
    t.dispatch("set_coverage", &json!({"section":"/shop", "state":"non-normative", "note":"The root contains introductory text."})).unwrap();
    let earlier = t.staged.len();
    let root = json!({"goal":"g:reconcile-section:shop.md#/shop", "justification":"The root's introductory text is marked non-normative."});
    let cart = json!({"goal":"g:reconcile-section:shop.md#/shop/cart", "justification":"The cart section's coverage is recorded."});

    // The first inline claim passes, but the second still owes its coverage.
    let error = t
        .dispatch(
            "done",
            &json!({"summary":"Recorded both sections.", "goals":[root, cart]}),
        )
        .unwrap_err();
    assert_eq!(error.rule, "unmarked-section");
    assert_eq!(t.staged.len(), earlier);
    assert!(t.outcomes.is_empty());
    assert!(t.done.is_none());

    // An otherwise valid inline claim cannot skip the remaining batch goal.
    let error = t
        .dispatch("done", &json!({"summary":"Only the root.", "goals":[root]}))
        .unwrap_err();
    assert_eq!(error.rule, "open-goal");
    assert!(t.outcomes.is_empty());
    assert_eq!(t.staged.len(), earlier);

    t.dispatch("set_coverage", &json!({"section":"/shop/cart", "state":"non-normative", "note":"The fixture records this section's classification."})).unwrap();
    t.dispatch(
        "done",
        &json!({"summary":"Recorded both sections.", "goals":[root, cart]}),
    )
    .unwrap();
    assert!(t.done.is_some());
    let commit = t.commit(1, None);
    assert_eq!(commit.resolved.len(), 2);
    assert!(commit.resolved.iter().all(|r| !r.justification.is_empty()));
}

#[test]
fn inline_done_refuses_foreign_duplicate_and_false_goal_claims() {
    let mut t = session();
    let claim =
        json!({"goal":"g:reconcile-section:shop.md#/shop", "justification":"The root is covered."});
    let error = t
        .dispatch(
            "done",
            &json!({"summary":"Duplicate.", "goals":[claim, claim]}),
        )
        .unwrap_err();
    assert_eq!(error.rule, "duplicate-goal");
    let error = t.dispatch("done", &json!({"summary":"Foreign.", "goals":[{"goal":"g:reconcile-section:elsewhere.md#/elsewhere", "justification":"The section is covered."}]})).unwrap_err();
    assert_eq!(error.rule, "unknown-goal");
    let error = t
        .dispatch("done", &json!({"summary":"False claim.", "goals":[claim]}))
        .unwrap_err();
    assert_eq!(error.rule, "unmarked-section");
    assert!(t.outcomes.is_empty());
    assert!(t.staged.is_empty());
    assert!(t.done.is_none());
}

#[test]
fn inline_done_rolls_back_review_mutations_and_retains_earlier_claims() {
    let mut store = Store::default();
    store
        .graph
        .entities
        .insert("ent:customer".into(), plain("Customer"));
    store
        .graph
        .entities
        .insert("ent:order".into(), plain("Order"));
    store.graph.requirements.insert(
        "req:a".into(),
        Requirement {
            statement: "An Order names the Customer who placed it.".into(),
            entities: vec!["ent:order".into()],
            ..Default::default()
        },
    );
    let goals = ["customer", "order"].map(|name| Goal {
        id: format!("g:review-entity:ent:{name}"),
        kind: "review-entity".into(),
        mandatory: true,
        target: format!("ent:{name}"),
        ..Default::default()
    });
    let mut t = ToolSession::new(store, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    t.dispatch(
        "update_entity",
        &json!({"id":"ent:order", "definition":"A purchase record naming its customer."}),
    )
    .unwrap();
    t.dispatch(
        "mark_goal_done",
        &json!({
            "goal":"g:review-entity:ent:order",
            "justification":"The definition now identifies the purchase record.",
            "evidence":{"definition":"refreshed"}
        }),
    )
    .unwrap();
    let staged_before = serde_json::to_value(&t.staged).unwrap();
    let outcomes_before = serde_json::to_value(&t.outcomes).unwrap();
    let customer = json!({
        "goal":"g:review-entity:ent:customer",
        "justification":"The requirement names the order's customer without defining customer behavior.",
        "evidence":{"definition":"current", "unreferenced":[{
            "id":"req:a", "verdict":"word-match", "reason":"Customer is the object of the order's statement."
        }]}
    });
    // The first claim stages not_about. The second contradicts the definition
    // update that was already staged and claimed before this call.
    let error = t
        .dispatch(
            "done",
            &json!({"summary":"Reviewed both entities.", "goals":[customer, {
                "goal":"g:review-entity:ent:order", "justification":"The definition is unchanged.",
                "evidence":{"definition":"current"}
            }]}),
        )
        .unwrap_err();
    assert_eq!(error.rule, "definition-unbacked");
    assert_eq!(serde_json::to_value(&t.staged).unwrap(), staged_before);
    assert_eq!(serde_json::to_value(&t.outcomes).unwrap(), outcomes_before);
    assert!(t.review_staged_note.is_none());
    assert!(t.done.is_none());

    t.dispatch(
        "done",
        &json!({"summary":"Reviewed both entities.", "goals":[customer]}),
    )
    .unwrap();
    assert!(t.done.is_some());
    assert_eq!(
        t.staged
            .iter()
            .filter(|op| matches!(op,
                Op::UpdateEntity { id, not_about_add, .. }
                    if id == "ent:customer" && not_about_add == &vec!["req:a".to_string()]
            ))
            .count(),
        1
    );
    assert_eq!(t.commit(1, None).resolved.len(), 2);
}

#[test]
fn placeholder_note_counts_as_absent() {
    let mut t = session();
    let err = t
        .dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "non-normative", "note": "<nil>"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "note-required");
}

#[test]
fn skill_free_fixed_goal_load_does_not_autoactivate_reference_skill() {
    let snapshot = session().snapshot;
    let goals = vec![Goal {
        id: "g:place-anchors:shop.md".into(),
        kind: "place-anchors".into(),
        target: "shop.md".into(),
        mandatory: true,
        ..Default::default()
    }];
    let mut fixed = ToolSession::new(
        snapshot.clone(),
        WorkScope::for_batch("fixed", &goals),
        64,
        96_000,
    );
    assert!(
        !fixed.skills.has_pinned(),
        "this fixed goal intentionally has no skill"
    );
    let loaded = fixed
        .dispatch("load", &json!({"target":"shop.md#/shop/cart"}))
        .unwrap();
    assert!(loaded.get("skill").is_none());
    assert!(!fixed.skills.is_rendered("extraction"));
    let mut serving = ToolSession::new(snapshot, WorkScope::serving("mcp-read"), 64, 96_000);
    let loaded = serving
        .dispatch("load", &json!({"target":"shop.md#/shop/cart"}))
        .unwrap();
    assert!(loaded["skill"]
        .as_str()
        .unwrap()
        .contains("[skill: extraction (active)]"));
}

// Auto-load once, the per-session cap, and the inactive marking when the last
// node of a kind unloads. Mirrors docs/compiler/sessions.md#skills.
#[test]
fn skills_auto_load_once_cap_and_go_inactive() {
    let mut t = session();
    t.skills.cap = 4; // Keep this cap-behavior fixture independent of the production allowance.
    assert!(
        t.skills.is_active("extraction"),
        "the goal kind's skill is active from the first round"
    );
    let v = t
        .dispatch("load", &json!({"target": "shop.md#/shop/cart"}))
        .unwrap();
    assert!(v.get("skill").is_none(), "a pinned skill never re-renders");
    // A batch reads a view as reference: no structural-views skill rides along
    // with the load (docs/compiler/sessions.md#skills).
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
    let v = t
        .dispatch("load", &json!({"target": "view:class/public"}))
        .unwrap();
    assert!(v.get("skill").is_none(), "a batch never auto-loads a skill");
    assert!(!t.skills.is_rendered("structural-views"));
    t.dispatch("load_skill", &json!({"name": "judgment"}))
        .unwrap();
    t.dispatch("load_skill", &json!({"name": "flow-views"}))
        .unwrap();
    t.dispatch("load_skill", &json!({"name": "structural-views"}))
        .unwrap();
    let err = t
        .dispatch("load_skill", &json!({"name": "abstraction"}))
        .unwrap_err();
    assert_eq!(err.rule, "skill-cap");
    let err = t
        .dispatch("load_skill", &json!({"name": "no-such"}))
        .unwrap_err();
    assert_eq!(err.rule, "unknown-skill");
    // Outside a batch nothing pins: the first section auto-loads extraction once,
    // and unloading the last section marks it inactive without dropping its slot.
    let snap = session().snapshot;
    let mut t2 = ToolSession::new(snap, WorkScope::serving("mcp-read"), 64, 24_000);
    let v = t2
        .dispatch("load", &json!({"target": "shop.md#/shop/cart"}))
        .unwrap();
    assert!(v["skill"]
        .as_str()
        .unwrap()
        .contains("[skill: extraction (active)]"));
    t2.dispatch("unload", &json!({"target": "shop.md#/shop/cart"}))
        .unwrap();
    assert!(!t2.skills.is_active("extraction"));
    assert!(
        t2.skills.is_rendered("extraction"),
        "the rendered text keeps its cap slot"
    );
}

// The claim tools: an essay justification, a false claim, and a goal outside the
// batch are rejected; a failure is always accepted.
// Mirrors docs/compiler/tools.md#goal-tools: a placeholder reason fails no goal.
#[test]
fn mark_goal_failed_refuses_a_placeholder_reason() {
    let mut t = session();
    let err = t
        .dispatch(
            "mark_goal_failed",
            &json!({"goal": "g:reconcile-section:shop.md#/shop", "reason": "placeholder"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-reason");
    assert!(
        err.message.contains("blocks convergence"),
        "{}",
        err.message
    );
    let v = t
        .dispatch(
            "mark_goal_failed",
            &json!({"goal": "g:reconcile-section:shop.md#/shop", "reason": "The section contradicts itself in every sentence."}),
        )
        .unwrap();
    assert_eq!(v["failed"], "g:reconcile-section:shop.md#/shop");
}

#[test]
fn mark_goal_done_rejects_an_essay_and_a_false_claim() {
    let mut t = session();
    let goal = "g:reconcile-section:shop.md#/shop/cart";
    let essay = "First I read the section. Then I extracted everything. Then I checked the anchors. Finally I marked coverage.";
    let err = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": goal, "justification": essay}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-justification");
    // A false claim: the section carries no coverage mark.
    let err = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": goal, "justification": "Extracted and marked."}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unmarked-section");
    let err = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": "g:reconcile-section:other.md#/x", "justification": "Done."}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unknown-goal");
    t.dispatch(
        "mark_goal_failed",
        &json!({"goal": goal, "reason": "the section contradicts itself"}),
    )
    .unwrap();
}

// A claim on a ledger goal is validated against the ledger the goal page names:
// with no row or record landed, bind, generate, and verify claims are refused
// naming the call that lands one. Mirrors docs/compiler/tools.md#goal-tools.
#[test]
fn mark_goal_done_on_ledger_goals_reads_the_ledger() {
    let dir = std::env::temp_dir().join(format!(
        "jazyk-ledger-gate-{}-{}",
        crate::sys::process_id(),
        crate::verify::now_iso().replace([':', '.'], "-")
    ));
    crate::vfs::create_dir_all(&dir).unwrap();
    let mut s = Store {
        out: dir.clone(),
        ..Default::default()
    };
    s.graph.entities.insert("ent:cart".into(), plain("Cart"));
    s.graph.requirements.insert(
        "req:t-1".into(),
        Requirement {
            statement: "The Cart holds items.".into(),
            entities: vec!["ent:cart".into()],
            ..Default::default()
        },
    );
    let goal = |kind: &str, target: &str| Goal {
        id: format!("g:{}:{}", kind, target),
        kind: kind.into(),
        mandatory: true,
        target: target.into(),
        ..Default::default()
    };
    let goals = vec![
        goal("bind", "req:t-1"),
        goal("generate", "ent:cart"),
        goal("verify", "req:t-1"),
    ];
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    for (id, rule, repair) in [
        ("g:bind:req:t-1", "bind-gate", "record_binding"),
        ("g:generate:ent:cart", "generate-gate", "record_generation"),
        ("g:verify:req:t-1", "verify-gate", "bind work"),
    ] {
        let err = t
            .dispatch(
                "mark_goal_done",
                &json!({"goal": id, "justification": "Landed."}),
            )
            .unwrap_err();
        assert_eq!(err.rule, rule);
        assert!(err.message.contains(repair), "{}", err.message);
        assert!(err.message.contains(id), "{}", err.message);
    }
    crate::vfs::remove_dir_all(&dir).ok();
}
