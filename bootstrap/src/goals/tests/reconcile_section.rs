use super::*;

// Mirrors docs/compiler/goals/reconcile-section.md#created-when: a bare heading's
// dirt derives no goal, the sections with a body derive theirs.
#[test]
fn a_dirty_bare_heading_derives_no_reconcile_goal() {
    let mut s = Store::default();
    let text = "# Backend\n\n## API Endpoints\n\n## Database\n\nThe backend uses PostgreSQL.\n";
    s.docs.insert(
        "backend.md".into(),
        crate::model::DocRecord {
            content_hash: crate::model::hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: Default::default(),
        },
    );
    let mut batch = store::RecordBatch::new(1);
    for sec in ["/backend", "/backend/api-endpoints", "/backend/database"] {
        batch.push(
            0,
            store::CHANGE_SECTION_DIRTY,
            &format!("backend.md#{}", sec),
            "edit",
            json!({"added": 1}),
        );
    }
    for r in batch.records() {
        s.status.record_change(r.clone());
    }
    s.status.generation = 1;
    let goals = ReconcileSection.derive_goals(&s, &gen_settings());
    let targets: Vec<&str> = goals.iter().map(|g| g.target.as_str()).collect();
    assert_eq!(
        targets,
        vec!["backend.md#/backend/database"],
        "{:?}",
        targets
    );
}

// Mirrors docs/compiler/compilation.md#coverage: a code block follows its parent's
// coverage; a diagram owes its own.
#[test]
fn a_code_block_section_owes_no_coverage_of_its_own() {
    let text = "# Doc\n\nprose\n\n```rust\nfn main() {}\n```\n\n```plantuml\n@startuml\nA -> B\n@enduml\n```\n";
    let sections = crate::md::parse_sections(text);
    let block = sections
        .values()
        .find(|s| s.kind == "code-block")
        .expect("code-block");
    assert!(!section_has_body(block));
    let diagram = sections
        .values()
        .find(|s| s.kind == "diagram")
        .expect("diagram");
    assert!(section_has_body(diagram));
    // A preamble's whole text is its body: one line of prose before the first
    // heading owes its mark; a heading alone owes none.
    let sections = crate::md::parse_sections("The system shall respond.\n\n# Doc\n");
    assert_eq!(sections["/"].kind, "preamble");
    assert!(section_has_body(&sections["/"]));
    assert_eq!(sections["/doc"].kind, "root");
    assert!(!section_has_body(&sections["/doc"]));
}

// The one-pass linked subjects list what the per-document scan lists, for every
// document, in its order: one entry per entity through its first linking mention,
// the requirement quotes after, twelve at most.
#[test]
fn linked_subjects_match_the_per_document_scan() {
    let mut s = Store::default();
    let ent = |name: &str, mentions: &[(&str, &str)]| Entity {
        name: name.into(),
        mentions: mentions
            .iter()
            .map(|(d, q)| SourceRef {
                doc: d.to_string(),
                section: "/x".into(),
                quote: q.to_string(),
            })
            .collect(),
        ..Default::default()
    };
    // Order links to orders.md twice (the first mention counts), Cart links to
    // both, Stock names nothing it links, Ledger is linked only through a quote.
    s.graph.entities.insert(
        "ent:order".into(),
        ent(
            "Order",
            &[
                ("a.md", "An [Order](orders.md) is placed."),
                ("b.md", "The [Order](orders.md) again."),
            ],
        ),
    );
    s.graph.entities.insert(
        "ent:cart".into(),
        ent(
            "Cart",
            &[(
                "a.md",
                "The [Cart](cart.md) feeds the [order flow](orders.md).",
            )],
        ),
    );
    s.graph.entities.insert(
        "ent:stock".into(),
        ent("Stock", &[("a.md", "See [inventory](stock.md).")]),
    );
    s.graph.entities.insert(
        "ent:ledger".into(),
        ent("Ledger", &[("orders.md", "Ledger")]),
    );
    s.graph.requirements.insert(
        "req:a-1".into(),
        Requirement {
            statement: "The ledger records every order.".into(),
            entities: vec!["ent:ledger".into(), "ent:order".into()],
            source: Some(SourceRef {
                doc: "a.md".into(),
                section: "/a".into(),
                quote: "The Ledger records every [Order](orders.md).".into(),
            }),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:o-1".into(),
        Requirement {
            statement: "Stock is reserved.".into(),
            entities: vec!["ent:stock".into()],
            source: Some(SourceRef {
                doc: "orders.md".into(),
                section: "/o".into(),
                quote: "[Stock](stock.md) is reserved.".into(),
            }),
            ..Default::default()
        },
    );
    for st in [
        s,
        crate::board::tests::settled_store(),
        crate::derive::tests::synthetic_store(120, 300),
    ] {
        let all = linked_subjects_all(&st);
        let mut docs: BTreeSet<String> = st.docs.keys().cloned().collect();
        docs.extend(all.keys().cloned());
        docs.extend([
            "orders.md".to_string(),
            "cart.md".to_string(),
            "stock.md".to_string(),
            "nope.md".to_string(),
        ]);
        for d in docs {
            assert_eq!(
                linked_subjects(&st, &d),
                linked_subjects_scan(&st, &d),
                "{}",
                d
            );
        }
    }
    let st = Store::default();
    assert!(linked_subjects_all(&st).is_empty());
}
