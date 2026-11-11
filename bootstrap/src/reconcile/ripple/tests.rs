use super::*;
use crate::reconcile::test_support::*;
use crate::store::Store;
use serde_json::Value;

#[test]
fn ripple_walks_opened_goals_to_their_resolvers() {
    let dir = journal_dir();
    let s = Store {
        out: dir.clone(),
        ..Default::default()
    };
    let edit = JournalEntry {
        build: "g87".into(),
        generation: 87,
        kind: "edit".into(),
        dirtied: vec!["docs/orders.md#/orders/holds".into()],
        opened_goals: vec![OpenedGoal {
            goal: "g:reconcile-section:docs/orders.md#/orders/holds".into(),
            cause: Cause {
                generation: 87,
                mutation: 1,
                via: "section".into(),
            },
        }],
        ..Default::default()
    };
    crate::vfs::write(
        crate::layout::journal_file(&dir, 87),
        serde_norway::to_string(&edit).unwrap(),
    )
    .unwrap();
    let mut session = JournalEntry {
        build: "g88".into(),
        generation: 88,
        kind: "session".into(),
        batch: vec!["g:reconcile-section:docs/orders.md#/orders/holds".into()],
        mutations: vec![
            json!({"UpdateRequirement": {"id": "req:orders-6", "statement": "30 days"}}),
        ],
        resolved_goals: vec![Resolved {
            goal: "g:reconcile-section:docs/orders.md#/orders/holds".into(),
            justification: "req:orders-6 revised".into(),
            evidence: Value::Null,
        }],
        opened_goals: vec![OpenedGoal {
            goal: "g:rejudge-pair:req:orders-6~req:payment-9".into(),
            cause: Cause {
                generation: 88,
                mutation: 1,
                via: "entities".into(),
            },
        }],
        tokens: Some(9_800),
        ..Default::default()
    };
    crate::vfs::write(
        crate::layout::journal_file(&dir, 88),
        serde_norway::to_string(&session).unwrap(),
    )
    .unwrap();
    session.generation = 89;
    session.build = "g89".into();
    session.batch = vec!["g:rejudge-pair:req:orders-6~req:payment-9".into()];
    session.mutations = Vec::new();
    session.resolved_goals = vec![Resolved {
        goal: "g:rejudge-pair:req:orders-6~req:payment-9".into(),
        justification: "consistent".into(),
        evidence: Value::Null,
    }];
    session.opened_goals = Vec::new();
    crate::vfs::write(
        crate::layout::journal_file(&dir, 89),
        serde_norway::to_string(&session).unwrap(),
    )
    .unwrap();
    let tree = ripple(&s, "g87", false).expect("root generation exists");
    assert_eq!(tree.root.generation, 87);
    assert_eq!(tree.root.children.len(), 1);
    assert_eq!(tree.root.children[0].generation, 88);
    assert_eq!(tree.root.children[0].children[0].generation, 89);
    assert_eq!(tree.sessions, 2);
    assert_eq!(tree.tokens, 19_600);
    let text = render_ripple(&tree);
    assert!(
        text.contains("edit docs/orders.md#/orders/holds (human) g87"),
        "{}",
        text
    );
    assert!(
        text.contains(
            "└─ reconcile-section docs/orders.md#/orders/holds g88: req:orders-6 revised"
        ),
        "{}",
        text
    );
    assert!(
        text.contains("└─ rejudge-pair req:orders-6~req:payment-9 g89: consistent"),
        "{}",
        text
    );
    assert!(text.contains("19k tokens"), "{}", text);
    // A batch that resolved several goals prints a header and one line per goal:
    // joining them ran past any terminal's width
    // (docs/frontends/cli.md#jazyk-ripple).
    let mut many = tree.root.children[0].children[0].clone();
    many.resolved = vec![
        Resolved {
            goal: "g:rejudge-pair:req:orders-6~req:payment-9".into(),
            justification: "consistent: one states the deadline, the other the handoff".into(),
            evidence: serde_json::Value::Null,
        },
        Resolved {
            goal: "g:rejudge-pair:req:orders-6~req:orders-7".into(),
            justification: "duplicate: both state the 30 day hold".into(),
            evidence: serde_json::Value::Null,
        },
    ];
    many.batch = vec![
        "g:rejudge-pair:req:orders-6~req:payment-9".into(),
        "g:rejudge-pair:req:orders-6~req:orders-7".into(),
    ];
    let mut wide = tree.clone();
    wide.root.children[0].children[0] = many;
    let text = render_ripple(&wide);
    assert!(text.contains("rejudge-pair x2 g89"), "{}", text);
    assert!(
        text.contains("req:orders-6~req:orders-7: duplicate: both state the 30 day hold"),
        "{}",
        text
    );
    // The one-goal shape is unchanged: kind, target, generation, justification.
    assert!(
        text.contains("reconcile-section docs/orders.md#/orders/holds g88: req:orders-6 revised"),
        "{}",
        text
    );
    // A node root finds the last cascade that touched it and starts at its edit,
    // skipping a later check run that names the node but carries no goals
    // (docs/frontends/cli.md#jazyk-ripple).
    let checks = JournalEntry {
        build: "g95".into(),
        generation: 95,
        kind: "checks".into(),
        mutations: vec![serde_json::json!({
            "op": "diagnostic_filed",
            "subject": "req:orders-6",
        })],
        ..Default::default()
    };
    crate::vfs::write(
        crate::layout::journal_file(&dir, 95),
        serde_norway::to_string(&checks).unwrap(),
    )
    .unwrap();
    let by_node = ripple(&s, "req:orders-6", false).unwrap();
    assert_eq!(by_node.root.generation, 87);
    assert_eq!(by_node.root.kind, "edit");
    let back = ripple(&s, "89", true).unwrap();
    assert_eq!(back.root.generation, 87);
    assert_eq!(back.root.children[0].generation, 88);
    assert!(ripple(&s, "docs/orders.md", false).is_some());
    assert!(ripple(&s, "g4000", false).is_none());
    crate::vfs::remove_dir_all(&dir).ok();
}
