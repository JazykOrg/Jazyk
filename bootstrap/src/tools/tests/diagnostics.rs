use super::*;

// The severity gate is the model's own list: `none` (a considered judgment
// recorded but not surfaced) passes, an invented level is refused naming the
// list. Mirrors docs/compiler/model/diagnostic.md#fields.
#[test]
fn severity_gate_is_the_diagnostic_models_list() {
    let mut t = session();
    let v = t
        .dispatch(
            "report_diagnostic",
            &json!({"rule": "ambiguity", "severity": "none",
                    "subjects": ["ent:customer"], "message": "Considered, not surfaced."}),
        )
        .unwrap();
    assert_eq!(v["created"], true);
    let err = t
        .dispatch(
            "report_diagnostic",
            &json!({"rule": "ambiguity", "severity": "critical",
                    "subjects": ["ent:customer"], "message": "Nope."}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-severity");
    for s in SEVERITIES {
        assert!(err.message.contains(s), "{}", err.message);
    }
    let err = t
        .dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "unprocessed"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-state");
}

#[test]
fn report_diagnostic_answers_with_the_id_and_reads_see_it() {
    // The model's own feedback: a bare {"reported": true} left a just-filed
    // finding unaddressable. The reply carries the id, a re-report answers
    // with the id it updates, and the session's reads see staged findings.
    let mut t = session();
    let v = t
        .dispatch(
            "report_diagnostic",
            &json!({"rule": "ambiguity", "severity": "warning",
                    "subjects": ["ent:customer"], "message": "Which customer?"}),
        )
        .unwrap();
    let id = v["id"].as_str().expect("an id").to_string();
    assert!(id.starts_with("diag:ambiguity-"), "{}", id);
    assert_eq!(v["created"], true);
    let again = t
        .dispatch(
            "report_diagnostic",
            &json!({"rule": "ambiguity", "severity": "info",
                    "subjects": ["ent:customer"], "message": "Which customer, really?"}),
        )
        .unwrap();
    assert_eq!(again["id"], id.as_str());
    assert_eq!(again["created"], false);
    let list = t.dispatch("diagnostics", &json!({})).unwrap();
    assert!(
        list["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == id.as_str()),
        "{}",
        list
    );
    // The build's own bookkeeping reads as harness-owned and never resolves
    // through a session. Mirrors docs/compiler/model/diagnostic.md#lifecycle-and-triage.
    let mut parked = t.staged_diags.values().next().unwrap().clone();
    parked.rule = "incomplete-build".into();
    parked.message = "goal parked".into();
    t.snapshot
        .graph
        .diagnostics
        .insert("diag:incomplete-build-1".into(), parked);
    let list = t
        .dispatch("diagnostics", &json!({"rule": "incomplete-build"}))
        .unwrap();
    let owned = &list["diagnostics"].as_array().unwrap()[0];
    assert_eq!(owned["owner"], "harness", "{}", list);
    let err = t
        .dispatch(
            "resolve_diagnostic",
            &json!({"id": "diag:incomplete-build-1", "reason": "resumed"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "not-resolvable");
    assert!(
        err.message.contains("owned by the harness"),
        "{}",
        err.message
    );
    let r = t
        .dispatch(
            "resolve_diagnostic",
            &json!({"id": id, "reason": "resolved in review"}),
        )
        .unwrap();
    assert_eq!(r["resolved"], true);
}

// edit_fact stages a dual write when the fact is quoted and a sentence rewrite was
// accepted, and a decree with its ratification proposal otherwise.
// Mirrors docs/compiler/tools.md#chat-tools.
#[test]
fn edit_fact_dual_writes_a_quoted_fact_and_decrees_otherwise() {
    let mut t = session();
    let old = "The Shopping Cart holds items a Customer intends to buy.";
    t.snapshot
        .graph
        .requirements
        .insert("req:shop-1".into(), quoted_req(old, &["ent:customer"], old));
    let new = "The Shopping Cart holds items a Customer selected.";
    let v = t
        .dispatch(
            "edit_fact",
            &json!({"id": "req:shop-1", "field": "statement", "value": new, "note": new}),
        )
        .unwrap();
    assert_eq!(v["path"], "dual-write");
    assert_eq!(v["prose"]["old_text"], old);
    assert_eq!(v["prose"]["new_text"], new);
    assert!(matches!(
        t.staged.last(),
        Some(Op::UpdateRequirement { statement: Some(s), source: Some(src), provenance: None, .. })
            if s == new && src.quote == new
    ));
    let v = t
        .dispatch("edit_fact", &json!({"id": "req:shop-1", "field": "facets", "value": [{"facet": "behavior", "reasoning": "a step"}]}))
        .unwrap();
    assert_eq!(v["path"], "decree");
    let n = t.staged.len();
    assert!(matches!(
        &t.staged[n - 2],
        Op::UpdateRequirement { facets: Some(f), source: None, provenance: Some(Provenance::Decree { .. }), .. }
            if f.len() == 1
    ));
    match &t.staged[n - 1] {
        Op::ReportDiagnostic { diagnostic, .. } => {
            assert_eq!(diagnostic.rule, "ratification-pending");
            assert_eq!(diagnostic.subjects, vec!["req:shop-1".to_string()]);
            let p = diagnostic.prompt.as_ref().unwrap();
            let e = p.options[0].edit.as_ref().unwrap();
            assert_eq!(
                (e.doc.as_str(), e.section.as_str()),
                ("shop.md", "/shop/cart")
            );
            assert_eq!(e.old_text, old);
            assert_eq!(e.new_text, old, "the statement is the proposal");
            assert_eq!(p.options[1].answer.as_deref(), Some("retract"));
            assert!(p.freeform);
        }
        other => panic!("unexpected {:?}", other),
    }
    let err = t
        .dispatch(
            "edit_fact",
            &json!({"id": "req:shop-1", "field": "created", "value": "g9"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-field");
    let err = t
        .dispatch(
            "edit_fact",
            &json!({"id": "req:nope", "field": "statement", "value": "x"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "unknown-id");
    // An entity's parent by decree, and a view's members.
    t.snapshot
        .graph
        .entities
        .insert("ent:cart".into(), plain("Cart"));
    let v = t
        .dispatch(
            "edit_fact",
            &json!({"id": "ent:customer", "field": "parent", "value": "ent:cart"}),
        )
        .unwrap();
    assert_eq!(v["path"], "decree");
    let n = t.staged.len();
    assert!(matches!(
        &t.staged[n - 2],
        Op::UpdateEntity { parent: Some(p), provenance: Some(Provenance::Decree { .. }), .. } if p == "ent:cart"
    ));
    t.snapshot.graph.views.insert(
        "view:class/public".into(),
        View {
            kind: "class".into(),
            title: "Public".into(),
            members: vec!["ent:customer".into()],
            default: true,
            ..Default::default()
        },
    );
    let v = t
        .dispatch("edit_fact", &json!({"id": "view:class/public", "field": "members", "value": ["ent:customer", "ent:cart"]}))
        .unwrap();
    assert_eq!(v["path"], "decree");
    assert!(matches!(
        t.staged.last(),
        Some(Op::UpdateView { members: Some(m), .. }) if m.len() == 2
    ));
    // A ratification proposal never closes through resolve_diagnostic.
    t.snapshot.graph.diagnostics.insert(
        "diag:ratification-pending-1".into(),
        Diagnostic {
            goal: None,
            rule: "ratification-pending".into(),
            severity: "warning".into(),
            subjects: vec!["req:shop-1".into()],
            message: "x".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    let err = t
        .dispatch(
            "resolve_diagnostic",
            &json!({"id": "diag:ratification-pending-1", "reason": "done"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "not-resolvable");
}

// An edit option's facts are gated at report time: a requirement quoted from the
// edit's section, an editable field, a value the update_requirement gates accept.
// Mirrors docs/compiler/model/diagnostic.md#prompts.
#[test]
fn a_prompt_edits_facts_pass_the_requirement_gates() {
    let mut t = session();
    let old = "The Shopping Cart holds items a Customer intends to buy.";
    t.snapshot
        .graph
        .requirements
        .insert("req:shop-1".into(), quoted_req(old, &["ent:customer"], old));
    let prompt = |section: &str, facts: Value| {
        let old_text = if section == "/shop" {
            "intro text"
        } else {
            "holds items"
        };
        normalize_args(&json!({"prompt": {"question": "Which?", "options": [
            {"label": "A", "edit": {"doc": "shop.md", "section": section,
             "old_text": old_text, "new_text": "keeps items", "facts": facts}}]}}))
    };
    let ok = prompt(
        "/shop/cart",
        json!([{"id": "shop-1", "field": "transition",
        "value": {"subject": "ent:customer", "from": "browsing", "to": "buying"}}]),
    );
    let p = t.parse_prompt(&ok["prompt"]).unwrap().unwrap();
    let facts = &p.options[0].edit.as_ref().unwrap().facts;
    assert_eq!(facts.len(), 1);
    let ops = t.fact_ops("shop.md", "/shop/cart", facts).unwrap();
    assert!(
        matches!(&ops[0], Op::UpdateRequirement { id, transition: Some(tr), .. }
        if id == "req:shop-1" && tr.to == "buying")
    );
    // An entity list first: the transition after it is judged against the new list.
    t.snapshot
        .graph
        .entities
        .insert("ent:cart".into(), plain("Cart"));
    let reordered = prompt(
        "/shop/cart",
        json!([
        {"id": "req:shop-1", "field": "entities", "value": ["ent:cart"]},
        {"id": "req:shop-1", "field": "transition", "value": {"subject": "ent:customer", "from": "a", "to": "b"}}]),
    );
    let err = t.parse_prompt(&reordered["prompt"]).unwrap_err();
    assert!(
        err.message.contains("fact 1") && err.message.contains("not among"),
        "{}",
        err.message
    );
    let wrong_field = prompt(
        "/shop/cart",
        json!([{"id": "req:shop-1", "field": "definition", "value": "x"}]),
    );
    let err = t.parse_prompt(&wrong_field["prompt"]).unwrap_err();
    assert_eq!(err.rule, "bad-fact");
    let elsewhere = prompt(
        "/shop",
        json!([{"id": "req:shop-1", "field": "facets",
        "value": [{"facet": "behavior", "reasoning": "a step"}]}]),
    );
    let err = t.parse_prompt(&elsewhere["prompt"]);
    let err = match err {
        Err(e) => e,
        Ok(_) => panic!("a requirement quoted elsewhere is refused"),
    };
    assert!(err.message.contains("not quoted from"), "{}", err.message);
}
