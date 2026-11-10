use super::*;

// Mirrors docs/compiler/goals/review-entity.md#hints: a review names the level
// views around its entity (its own when it has children, its parent's level),
// and leaves the harness-owned bookkeeping out of the open diagnostics.
#[test]
fn review_hints_name_the_level_views_and_skip_harness_owned_diagnostics() {
    let mut s = levels_store();
    let n = s.status.changes.len();
    s.status.changes.push(ChangeRecord::new(
        7,
        n,
        0,
        store::CHANGE_ENTITY,
        "ent:backend",
        "fields",
    ));
    s.graph.diagnostics.insert(
        "diag:incomplete-build-1".into(),
        crate::model::Diagnostic {
            rule: "incomplete-build".into(),
            severity: "warning".into(),
            subjects: vec!["ent:backend".into()],
            message: "parked".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    // A criterion-missing finding is the build's too: a review can neither add a
    // criterion nor clear the check that refiles it
    // (docs/compiler/goals/review-entity.md#hints).
    s.graph.requirements.insert(
        "req:backend-1".into(),
        Requirement {
            statement: "The Backend serves the REST API.".into(),
            entities: vec!["ent:backend".into()],
            ..Default::default()
        },
    );
    s.graph.diagnostics.insert(
        "diag:criterion-missing-1".into(),
        crate::model::Diagnostic {
            rule: "criterion-missing".into(),
            severity: "warning".into(),
            subjects: vec![s
                .requirements_referencing("ent:backend")
                .first()
                .cloned()
                .expect("the fixture's backend carries a requirement")],
            message: "no acceptance criterion".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    let rid = s.requirements_referencing("ent:backend")[0].clone();
    s.graph.diagnostics.insert(
        "diag:ambiguity-1".into(),
        crate::model::Diagnostic {
            rule: "ambiguity".into(),
            severity: "warning".into(),
            subjects: vec![rid],
            message: "two readings".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    let goals = ReviewEntity.derive_goals(&s, &gen_settings());
    let g = goals.iter().find(|g| g.target == "ent:backend").unwrap();
    // The control: a finding a review can act on is listed on the same requirement.
    assert!(
        g.hints.iter().any(|h| h.contains("diag:ambiguity-1")),
        "{:?}",
        g.hints
    );
    assert!(
        !g.hints.iter().any(|h| h.contains("criterion-missing")),
        "{:?}",
        g.hints
    );
    let own = crate::derive::level_view_id(&s, "ent:backend").unwrap();
    let parent = crate::derive::level_view_id(&s, "scope:public").unwrap();
    assert!(
        g.hints.contains(&format!(
            "level views: {} (this entity), {} (the level above)",
            own, parent
        )),
        "{:?}",
        g.hints
    );
    assert!(
        !g.hints.iter().any(|h| h.contains("incomplete-build")),
        "{:?}",
        g.hints
    );
}

// Mirrors docs/compiler/model/entity.md#fields: a word match a review judged stays
// off the unreferenced list.
#[test]
fn judged_word_matches_leave_the_unreferenced_list() {
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:customer".into(),
        Entity {
            name: "Customer".into(),
            not_about: vec!["req:a".into()],
            ..Default::default()
        },
    );
    for (id, st) in [
        ("req:a", "An Order names the Customer who placed it."),
        ("req:b", "A Customer receives a receipt."),
    ] {
        s.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: st.into(),
                entities: vec!["ent:order".into()],
                ..Default::default()
            },
        );
    }
    let ids: Vec<String> = unreferenced_statements(&s, "ent:customer")
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ids, vec!["req:b".to_string()]);
}
