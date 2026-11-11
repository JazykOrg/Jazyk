use super::*;

// Mirrors docs/compiler/goals/derive-criteria.md#created-when: one goal per leading
// entity whose requirements owe criteria, only under a configured deliverable, and
// bind waits on the criterion (reconciler.md#tiers).
#[test]
fn derive_criteria_goals_follow_the_leading_entity_and_the_deliverable() {
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:sort".into(),
        Entity {
            name: "Sort".into(),
            ..Default::default()
        },
    );
    s.graph.entities.insert(
        "ent:line".into(),
        Entity {
            name: "Line".into(),
            ..Default::default()
        },
    );
    for (id, st, ents) in [
        (
            "req:a-1",
            "The utility sorts its lines.",
            vec!["ent:sort", "ent:line"],
        ),
        ("req:a-2", "A line is trimmed.", vec!["ent:line"]),
        ("req:a-3", "The utility reads STDIN.", vec!["ent:sort"]),
    ] {
        s.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: st.into(),
                entities: ents.iter().map(|e| e.to_string()).collect(),
                ..Default::default()
            },
        );
    }
    s.graph.criteria.insert(
        "crit:a-1".into(),
        crate::model::Criterion {
            requirement: "req:a-3".into(),
            statement: "Two lines on STDIN are both read.".into(),
            kind: "positive".into(),
            provenance: Some(Provenance::Derived {
                from: vec!["req:a-3".into()],
                reasoning: "the input".into(),
            }),
            stale: true,
            ..Default::default()
        },
    );
    let mut gen = gen_settings();
    gen.configured = false;
    assert!(DeriveCriteria.derive_goals(&s, &gen).is_empty());
    gen.configured = true;
    let goals = DeriveCriteria.derive_goals(&s, &gen);
    let mut targets: Vec<&str> = goals.iter().map(|g| g.target.as_str()).collect();
    targets.sort();
    assert_eq!(targets, vec!["ent:line", "ent:sort"], "{:?}", targets);

    // A requirement waiting on an open question is not listed until it closes.
    let mut waiting = s.clone();
    waiting.graph.diagnostics.insert(
        "diag:invented-choice-1".into(),
        Diagnostic {
            goal: None,
            rule: "invented-choice".into(),
            severity: "warning".into(),
            subjects: vec!["req:a-2".into()],
            message: "How long may a line be?".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    let held: Vec<String> = DeriveCriteria
        .derive_goals(&waiting, &gen)
        .iter()
        .map(|g| g.target.clone())
        .collect();
    assert_eq!(
        held,
        vec!["ent:sort".to_string()],
        "ent:line waits on its question"
    );
    waiting
        .graph
        .diagnostics
        .get_mut("diag:invented-choice-1")
        .unwrap()
        .lifecycle = "resolved".into();
    assert_eq!(
        DeriveCriteria.derive_goals(&waiting, &gen).len(),
        2,
        "the answer reopens it"
    );
    let sort = goals.iter().find(|g| g.target == "ent:sort").unwrap();
    assert_eq!(sort.change["none"], json!(["req:a-1"]));
    assert_eq!(sort.change["revised"], json!(["req:a-3"]));
    assert!(sort.hints.iter().any(|h| h == "skill criteria"));
    assert_eq!(
        skills_for("derive-criteria", &s, "ent:sort"),
        vec!["criteria"]
    );
    assert_eq!(tier("derive-criteria"), Some(3));
}
