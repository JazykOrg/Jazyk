use super::*;

// The retrace gate: nothing on the target points at a dead node over the store
// plus the staged work, and a target deleted in the session passes.
// Mirrors docs/compiler/goals/retrace.md#gate.
#[test]
fn retrace_refuses_a_dangling_reference_until_it_is_repaired() {
    let mut s = Store::default();
    s.graph.requirements.insert(
        "req:holds-1".into(),
        Requirement {
            statement: "Holds land.".into(),
            ..Default::default()
        },
    );
    s.graph.views.insert(
        "view:usecase/holds".into(),
        View {
            kind: "usecase".into(),
            title: "Holds".into(),
            members: vec!["req:holds-1".into(), "req:orders-6".into()],
            ..Default::default()
        },
    );
    let g = scope(
        "retrace",
        "view:usecase/holds",
        json!({"deleted": ["req:orders-6"]}),
    );
    let batch = vec![g.clone()];
    let v = goal_gate_of(&s, &[], &g, &batch).expect("the dead member dangles");
    assert_eq!(v.rule, "dangling-reference");
    assert!(v.message.contains("req:orders-6"), "{}", v.message);
    let repaired = [op(
        json!({"op": "update_view", "id": "view:usecase/holds", "remove_members": ["req:orders-6"]}),
    )];
    assert_eq!(goal_gate_of(&s, &repaired, &g, &batch), None);
    let deleted = [op(
        json!({"op": "delete_view", "id": "view:usecase/holds", "reason": "nothing left to show"}),
    )];
    assert_eq!(goal_gate_of(&s, &deleted, &g, &batch), None);
}
