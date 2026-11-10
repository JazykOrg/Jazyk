use super::*;

// The conform-instance type rule keys on the goal's own type and yields to the
// batch goal that owns the edit. Mirrors docs/compiler/goals/conform-instance.md#gate.
#[test]
fn conform_instance_refuses_type_edits_its_batch_does_not_own() {
    let mut s = Store::default();
    for id in ["ent:ana", "ent:customer", "ent:order"] {
        s.graph.entities.insert(
            id.into(),
            Entity {
                name: id.into(),
                ..Default::default()
            },
        );
    }
    s.graph.requirements.insert(
        "req:customer-1".into(),
        Requirement {
            statement: "A customer has a tier.".into(),
            entities: vec!["ent:customer".into()],
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:ana-1".into(),
        Requirement {
            statement: "Ana is gold tier.".into(),
            entities: vec!["ent:ana".into()],
            ..Default::default()
        },
    );
    let g = scope(
        "conform-instance",
        "ent:ana",
        json!({"type": "ent:customer"}),
    );
    let alone = vec![g.clone()];
    let type_edit = [op(
        json!({"op": "update_entity", "id": "ent:customer", "definition": "A buyer."}),
    )];
    let type_statement = [op(
        json!({"op": "update_requirement", "id": "req:customer-1", "statement": "A customer has a tier and a region."}),
    )];
    let own_statement = [op(
        json!({"op": "update_requirement", "id": "req:ana-1", "statement": "Ana is gold tier in the EU."}),
    )];
    let annotation = [op(
        json!({"op": "update_entity", "id": "ent:customer", "not_about_add": ["req:ana-1"]}),
    )];
    let other_entity = [op(
        json!({"op": "update_entity", "id": "ent:order", "definition": "A purchase."}),
    )];
    assert_eq!(
        goal_gate_of(&s, &type_edit, &g, &alone).map(|v| v.rule),
        Some("type-edited".into())
    );
    assert_eq!(
        goal_gate_of(&s, &type_statement, &g, &alone).map(|v| v.rule),
        Some("type-edited".into())
    );
    assert_eq!(goal_gate_of(&s, &own_statement, &g, &alone), None);
    assert_eq!(goal_gate_of(&s, &annotation, &g, &alone), None);
    assert_eq!(
        goal_gate_of(&s, &other_entity, &g, &alone),
        None,
        "another entity is not this goal's type"
    );
    // A review of the type in the same batch owns the type's edits.
    let mixed = vec![g.clone(), scope("review-entity", "ent:customer", json!({}))];
    assert_eq!(goal_gate_of(&s, &type_edit, &g, &mixed), None);
    assert_eq!(goal_gate_of(&s, &type_statement, &g, &mixed), None);
}
