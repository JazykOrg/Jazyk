// Batching: estimates, localities, packing, and walls.
use super::*;

// The pair estimate follows the statements it will load, never a flat guess.
#[test]
fn rejudge_pair_estimate_follows_the_statements() {
    let mut s = Store::default();
    let mut req = |id: &str, statement: &str, n: usize| {
        s.graph.requirements.insert(
            id.into(),
            crate::model::Requirement {
                statement: statement.into(),
                entities: (0..n).map(|i| format!("ent:e{}", i)).collect(),
                ..Default::default()
            },
        );
    };
    req("req:a", "Short.", 1);
    req("req:b", "Also short.", 1);
    req(
        "req:c",
        &"A long statement about approvals and thresholds. ".repeat(8),
        4,
    );
    req(
        "req:d",
        &"Another long statement naming several entities and bands. ".repeat(8),
        4,
    );
    let g = |target: &str| Goal {
        id: format!("g:rejudge-pair:{}", target),
        kind: "rejudge-pair".into(),
        class: "compile".into(),
        mandatory: true,
        target: target.into(),
        unit: "pair".into(),
        change: serde_json::json!({}),
        cause: None,
        state: GoalState::Open,
        hints: Vec::new(),
    };
    let short = estimate(&s, &g("req:a~req:b"));
    let long = estimate(&s, &g("req:c~req:d"));
    assert!(long > short, "{} vs {}", long, short);
    assert!(long > 1_400, "{}", long);
}

// Ledger goals join through the component group root: the bind, generate, and
// verify goals of one component subtree share one locality, so they batch into
// one session. Mirrors docs/consumers/gen.md#grouping-by-component.
#[test]
fn ledger_goals_join_their_component_group() {
    let mut s = Store::default();
    for (id, parent) in [
        ("ent:sys", None),
        ("ent:svc", Some("ent:sys")),
        ("ent:svc-part", Some("ent:svc")),
    ] {
        s.graph.entities.insert(
            id.into(),
            Entity {
                name: id.into(),
                parent: parent.map(String::from),
                ..Default::default()
            },
        );
    }
    let goal = |kind: &str, target: &str, change: Value| Goal {
        id: format!("g:{}:{}", kind, target),
        kind: kind.into(),
        class: "compile".into(),
        mandatory: true,
        target: target.into(),
        unit: "entity".into(),
        change,
        cause: None,
        state: GoalState::Open,
        hints: Vec::new(),
    };
    let gen_part = goal("generate", "ent:svc-part", json!({"goal": "generate"}));
    let bind_part = goal("bind", "req:shop-9", json!({"entity": "ent:svc-part"}));
    let verify_svc = goal("verify", "req:shop-9", json!({"entity": "ent:svc"}));
    assert_eq!(locality_keys(&s, &gen_part)[0], "ent ent:svc");
    assert_eq!(locality_keys(&s, &bind_part)[0], "ent ent:svc");
    assert_eq!(locality_keys(&s, &verify_svc)[0], "ent ent:svc");
    // The system generates alone.
    let gen_sys = goal("generate", "ent:sys", json!({"goal": "generate"}));
    assert_eq!(locality_keys(&s, &gen_sys)[0], "ent ent:sys");
}

// Node localities pack on into one batch until the budget is spent: two pair
// judgments over disjoint entity neighborhoods ride one session, since a
// locality is where a batch starts, not where it must end.
// Mirrors docs/compiler/reconciler.md#batching.
#[test]
fn pair_batches_fill_across_node_localities() {
    let mut s = Store::default();
    let text = "# T\n\nThe X serves the Y nightly. The X also names the Y nightly. The P serves the Q nightly. The P also names the Q nightly.\n";
    let mut coverage = BTreeMap::new();
    {
        let r = "/t";
        coverage.insert(
            r.to_string(),
            Coverage {
                state: "covered".into(),
                note: None,
                claimed_by: Some("g1".into()),
            },
        );
    }
    s.docs.insert(
        "t.md".into(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage,
        },
    );
    let mention = |q: &str| crate::model::SourceRef {
        doc: "t.md".into(),
        section: "/t".into(),
        quote: q.into(),
    };
    for (id, name) in [
        ("ent:x", "X"),
        ("ent:y", "Y"),
        ("ent:p", "P"),
        ("ent:q", "Q"),
    ] {
        s.graph.entities.insert(
            id.into(),
            Entity {
                name: name.into(),
                mentions: vec![mention(name)],
                ..Default::default()
            },
        );
    }
    let req = |statement: &str, ents: &[&str], quote: &str| Requirement {
        statement: statement.into(),
        entities: ents.iter().map(|e| e.to_string()).collect(),
        source: Some(mention(quote)),
        ..Default::default()
    };
    s.graph.requirements.insert(
        "req:t-1".into(),
        req(
            "The X serves the Y nightly.",
            &["ent:x", "ent:y"],
            "The X serves the Y nightly.",
        ),
    );
    s.graph.requirements.insert(
        "req:t-2".into(),
        req(
            "The X also names the Y nightly.",
            &["ent:x", "ent:y"],
            "The X also names the Y nightly.",
        ),
    );
    s.graph.requirements.insert(
        "req:t-3".into(),
        req(
            "The P serves the Q nightly.",
            &["ent:p", "ent:q"],
            "The P serves the Q nightly.",
        ),
    );
    s.graph.requirements.insert(
        "req:t-4".into(),
        req(
            "The P also names the Q nightly.",
            &["ent:p", "ent:q"],
            "The P also names the Q nightly.",
        ),
    );
    s.status.generation = 5;
    for rid in ["req:t-1", "req:t-3"] {
        record(
            &mut s,
            5,
            store::CHANGE_REQ_REVISED,
            rid,
            "fields",
            json!({}),
        );
    }
    let b = derive(&s);
    let pairs: Vec<&Goal> = b
        .goals
        .iter()
        .filter(|g| g.kind == "rejudge-pair")
        .collect();
    assert_eq!(
        pairs.len(),
        2,
        "{:?}",
        pairs.iter().map(|g| &g.id).collect::<Vec<_>>()
    );
    for g in &pairs {
        assert!(b.is_ready(&g.id), "{:?}", b.readiness.get(&g.id));
    }
    // Two neighborhoods (x,y and p,q), one batch.
    let localities = b.localities(&s, &pairs);
    assert_eq!(localities.len(), 2, "{:?}", localities);
    assert_eq!(b.batches.len(), 1, "{:?}", b.batches);
    assert_eq!(b.batches[0].goals.len(), 2);
}

// Criteria goals share batches, at most CRITERIA_GOALS_PER_BATCH to one, instead
// of one session per entity. Mirrors docs/compiler/reconciler.md#batching.
#[test]
fn criteria_goals_pack_up_to_the_cap() {
    let mut s = Store::default();
    for i in 0..6 {
        let eid = format!("ent:e{}", i);
        s.graph.entities.insert(
            eid.clone(),
            Entity {
                name: format!("E{}", i),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            format!("req:r-{}", i),
            Requirement {
                statement: format!("The E{} answers within {} seconds.", i, i + 1),
                entities: vec![eid],
                ..Default::default()
            },
        );
    }
    let proj = Project {
        out: s.out.clone(),
        gen_deliverable: Some("product".into()),
        ..Default::default()
    };
    let b = Board::derive(&s, &proj, &control_auto());
    let criteria: Vec<&Goal> = b
        .goals
        .iter()
        .filter(|g| g.kind == "derive-criteria")
        .collect();
    assert_eq!(
        criteria.len(),
        6,
        "{:?}",
        b.goals.iter().map(|g| &g.id).collect::<Vec<_>>()
    );
    let per_batch: Vec<usize> = b
        .batches
        .iter()
        .map(|bt| {
            bt.goals
                .iter()
                .filter(|id| id.starts_with("g:derive-criteria:"))
                .count()
        })
        .filter(|n| *n > 0)
        .collect();
    assert_eq!(per_batch.iter().sum::<usize>(), 6, "{:?}", b.batches);
    assert!(
        per_batch
            .iter()
            .all(|n| *n <= limits::CRITERIA_GOALS_PER_BATCH),
        "{:?}",
        per_batch
    );
    assert!(
        per_batch.iter().any(|n| *n > 1),
        "criteria goals never shared a batch: {:?}",
        per_batch
    );
}

// A pair batch charges each requirement once: one long statement paired with six
// others loads once, so its six pairs ride one session where charging both sides
// per pair would cut them in two. Mirrors docs/compiler/reconciler.md#batching.
#[test]
fn a_shared_side_is_charged_once_per_pair_batch() {
    let mut s = Store::default();
    let pad =
        " It holds under the nightly settlement rules for gold keys and silver locks.".repeat(6);
    s.graph.entities.insert(
        "ent:x".into(),
        Entity {
            name: "X".into(),
            ..Default::default()
        },
    );
    let req = |statement: String| Requirement {
        statement,
        entities: vec!["ent:x".into()],
        ..Default::default()
    };
    // Each pair shares two words no other statement carries, so no filter drops it.
    let words = [
        ("amber", "basil"),
        ("cedar", "dahlia"),
        ("elder", "fennel"),
        ("ginger", "hazel"),
        ("iris", "juniper"),
        ("kelp", "lotus"),
    ];
    let hub: Vec<String> = words.iter().map(|(a, b)| format!("{} {}", a, b)).collect();
    s.graph.requirements.insert(
        "req:t-0".into(),
        req(format!("The ledger weighs {}.{}", hub.join(", "), pad)),
    );
    for (i, (a, b)) in words.iter().enumerate() {
        s.graph.requirements.insert(
            format!("req:t-{}", i + 1),
            req(format!("The ledger weighs {} {} apart.{}", a, b, pad)),
        );
    }
    s.status.generation = 5;
    record(
        &mut s,
        5,
        store::CHANGE_REQ_REVISED,
        "req:t-0",
        "fields",
        json!({}),
    );
    let b = derive(&s);
    let pairs: Vec<&Goal> = b
        .goals
        .iter()
        .filter(|g| g.kind == "rejudge-pair")
        .collect();
    assert_eq!(
        pairs.len(),
        6,
        "{:?}",
        pairs.iter().map(|g| &g.id).collect::<Vec<_>>()
    );
    let with_pairs: Vec<usize> = b
        .batches
        .iter()
        .map(|bt| {
            bt.goals
                .iter()
                .filter(|id| id.starts_with("g:rejudge-pair:"))
                .count()
        })
        .filter(|n| *n > 0)
        .collect();
    // Per pair both sides would be about 2.9k each, six pairs about 17k, past the
    // budget the judgment skill leaves; charged once, they fit one batch.
    assert_eq!(with_pairs, vec![6], "{:?}", with_pairs);
}

// Six entities owing criteria, each its own locality, pack on into one batch
// until the cap and spill the rest into the next: four, then two.
// Mirrors docs/compiler/reconciler.md#batching.
#[test]
fn six_criteria_goals_pack_as_four_and_two() {
    let mut s = Store::default();
    for i in 0..6 {
        let eid = format!("ent:e{}", i);
        s.graph.entities.insert(
            eid.clone(),
            Entity {
                name: format!("E{}", i),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            format!("req:r-{}", i),
            Requirement {
                statement: format!("The E{} answers within {} seconds.", i, i + 1),
                entities: vec![eid],
                ..Default::default()
            },
        );
    }
    let proj = Project {
        out: s.out.clone(),
        gen_deliverable: Some("product".into()),
        ..Default::default()
    };
    let b = Board::derive(&s, &proj, &control_auto());
    let per_batch: Vec<usize> = b
        .batches
        .iter()
        .map(|bt| {
            bt.goals
                .iter()
                .filter(|id| id.starts_with("g:derive-criteria:"))
                .count()
        })
        .filter(|n| *n > 0)
        .collect();
    assert_eq!(limits::CRITERIA_GOALS_PER_BATCH, 4);
    assert_eq!(per_batch, vec![4, 2], "{:?}", b.batches);
    // A batch of criteria goals holds nothing else.
    for bt in b.batches.iter().filter(|bt| {
        bt.goals
            .iter()
            .any(|id| id.starts_with("g:derive-criteria:"))
    }) {
        assert!(
            bt.goals
                .iter()
                .all(|id| id.starts_with("g:derive-criteria:")),
            "{:?}",
            bt
        );
    }
}

// Generation batches follow the leaf-first order over the relationships: the part
// a whole composes generates before the whole, whatever their ids.
// Mirrors docs/compiler/goals/generate.md#readiness.
#[test]
fn generate_batches_run_leaf_first() {
    let mut s = Store::default();
    for (eid, rid, crit) in [
        ("ent:a-order", "req:r-a", "crit:a-1"),
        ("ent:z-line", "req:r-z", "crit:z-1"),
    ] {
        s.graph.entities.insert(
            eid.into(),
            Entity {
                name: eid.into(),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            rid.into(),
            Requirement {
                statement: format!("The {} answers.", eid),
                entities: vec![eid.into()],
                ..Default::default()
            },
        );
        s.graph.criteria.insert(
            crit.into(),
            Criterion {
                requirement: rid.into(),
                statement: format!("Asked once, the {} answers.", eid),
                kind: "positive".into(),
                ..Default::default()
            },
        );
    }
    s.graph.relationships.insert(
        "rel:a-order~z-line".into(),
        crate::model::Relationship {
            members: vec!["ent:a-order".into(), "ent:z-line".into()],
            contributions: vec![crate::model::Contribution {
                a: "ent:a-order".into(),
                b: "ent:z-line".into(),
                r#type: "composition".into(),
                cardinality: None,
                requirements: vec!["req:r-a".into()],
            }],
        },
    );
    let proj = Project {
        out: s.out.clone(),
        gen_deliverable: Some("product".into()),
        ..Default::default()
    };
    let control = Control {
        generate: "auto".into(),
        ..control_auto()
    };
    let b = Board::derive(&s, &proj, &control);
    let pos = |id: &str| {
        b.batches
            .iter()
            .position(|bt| bt.goals.iter().any(|g| g == id))
            .unwrap_or_else(|| panic!("{} unbatched: {:?}", id, b.batches))
    };
    assert!(
        pos("g:generate:ent:z-line") < pos("g:generate:ent:a-order"),
        "{:?}",
        b.batches
    );
}

// A ledger locality is a wall: the batch flushes before and after it, and its
// counters (size, sections, criteria, charged sides) start over. A pair never
// meets a wall (pairs are tier 2, the walled kinds tiers 0, 1, and 3), so the
// reset of charged sides has no pair to show it; what a wall does show is a
// component's Generate and Bind goals together, without the criteria goals of
// other localities. Those goals pack among themselves, where without the wall
// they would share the ledger batch. Mirrors docs/compiler/reconciler.md#batching.
#[test]
fn a_walled_locality_never_shares_a_batch() {
    let mut s = Store::default();
    // One requirement already carrying a criterion owes development and a bind,
    // but no criteria. Development starts first within its component locality.
    s.graph.entities.insert(
        "ent:g".into(),
        Entity {
            name: "G".into(),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:r-g".into(),
        Requirement {
            statement: "The G answers within 9 seconds.".into(),
            entities: vec!["ent:g".into()],
            ..Default::default()
        },
    );
    s.graph.criteria.insert(
        "crit:g-1".into(),
        Criterion {
            requirement: "req:r-g".into(),
            statement: "A G asked once answers in 8 seconds.".into(),
            kind: "positive".into(),
            ..Default::default()
        },
    );
    for i in 0..6 {
        let eid = format!("ent:e{}", i);
        s.graph.entities.insert(
            eid.clone(),
            Entity {
                name: format!("E{}", i),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            format!("req:r-{}", i),
            Requirement {
                statement: format!("The E{} answers within {} seconds.", i, i + 1),
                entities: vec![eid],
                ..Default::default()
            },
        );
    }
    let proj = Project {
        out: s.out.clone(),
        gen_deliverable: Some("product".into()),
        ..Default::default()
    };
    let control = Control {
        generate: "auto".into(),
        ..control_auto()
    };
    let b = Board::derive(&s, &proj, &control);
    assert!(
        b.is_ready("g:generate:ent:g"),
        "{:?}",
        b.readiness.get("g:generate:ent:g")
    );
    assert!(
        b.is_ready("g:bind:req:r-g"),
        "{:?}",
        b.readiness.get("g:bind:req:r-g")
    );
    let bind = b
        .batches
        .iter()
        .find(|bt| bt.goals.iter().any(|id| id == "g:bind:req:r-g"))
        .unwrap_or_else(|| panic!("{:?}", b.batches));
    assert_eq!(
        bind.goals,
        vec!["g:generate:ent:g".to_string(), "g:bind:req:r-g".to_string()],
        "{:?}",
        bind
    );
    for batch in b.batches.iter().filter(|batch| {
        batch
            .goals
            .iter()
            .any(|id| id.starts_with("g:derive-criteria:"))
    }) {
        assert!(
            batch
                .goals
                .iter()
                .all(|id| id.starts_with("g:derive-criteria:")),
            "{:?}",
            batch
        );
    }
    let per_batch: Vec<usize> = b
        .batches
        .iter()
        .map(|bt| {
            bt.goals
                .iter()
                .filter(|id| id.starts_with("g:derive-criteria:"))
                .count()
        })
        .filter(|n| *n > 0)
        .collect();
    assert_eq!(per_batch, vec![4, 2], "{:?}", b.batches);
}

// A pair that overflows its batch opens the next one charged in full: the side
// the spent batch held is loaded again by the fresh one. A hub paired with six
// spokes, sized so two pairs fill a batch with the hub and four would without
// it, goes three batches of two, never two and then four.
// Mirrors docs/compiler/reconciler.md#batching.
#[test]
fn an_overflowing_pair_recharges_in_full() {
    let judgment = crate::session::SKILLS
        .iter()
        .find(|(k, _)| *k == "judgment")
        .map(|(_, p)| p.len())
        .unwrap();
    let budget = limits::CONTEXT_BUDGET.saturating_sub(judgment).max(6_000);
    // A side costs 260 + statement + 90 per entity; a pair 700 plus its uncharged
    // sides. A spoke side is 1800 (a pair step of 2500); the hub side sits so
    // that two steps fit beside it and three do not, while four fit without it.
    let spoke_len = 1800 - 350;
    let hub_len = (budget - 6250) - 350;
    assert!(4 * 2500 <= budget, "{}", budget);
    let pad = " It holds under the nightly settlement rules for gold keys and silver locks.";
    let fill = |base: String, len: usize| -> String {
        let mut s = base;
        while s.len() < len {
            s.push_str(pad);
        }
        s.truncate(len);
        s
    };
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:x".into(),
        Entity {
            name: "X".into(),
            ..Default::default()
        },
    );
    let req = |statement: String| Requirement {
        statement,
        entities: vec!["ent:x".into()],
        ..Default::default()
    };
    // Each pair shares two words no other statement carries, so no filter drops it.
    let words = [
        ("amber", "basil"),
        ("cedar", "dahlia"),
        ("elder", "fennel"),
        ("ginger", "hazel"),
        ("iris", "juniper"),
        ("kelp", "lotus"),
    ];
    let hub: Vec<String> = words.iter().map(|(a, b)| format!("{} {}", a, b)).collect();
    s.graph.requirements.insert(
        "req:t-0".into(),
        req(fill(
            format!("The ledger weighs {}.", hub.join(", ")),
            hub_len,
        )),
    );
    for (i, (a, b)) in words.iter().enumerate() {
        s.graph.requirements.insert(
            format!("req:t-{}", i + 1),
            req(fill(
                format!("The ledger weighs {} {} apart.", a, b),
                spoke_len,
            )),
        );
    }
    s.status.generation = 5;
    record(
        &mut s,
        5,
        store::CHANGE_REQ_REVISED,
        "req:t-0",
        "fields",
        json!({}),
    );
    let b = derive(&s);
    let pairs: Vec<&Goal> = b
        .goals
        .iter()
        .filter(|g| g.kind == "rejudge-pair")
        .collect();
    assert_eq!(
        pairs.len(),
        6,
        "{:?}",
        pairs.iter().map(|g| &g.id).collect::<Vec<_>>()
    );
    for g in &pairs {
        assert_eq!(estimate(&s, g), 700 + (budget - 6250) + 1800, "{}", g.id);
    }
    let with_pairs: Vec<usize> = b
        .batches
        .iter()
        .map(|bt| {
            bt.goals
                .iter()
                .filter(|id| id.starts_with("g:rejudge-pair:"))
                .count()
        })
        .filter(|n| *n > 0)
        .collect();
    assert_eq!(with_pairs, vec![2, 2, 2], "{:?}", with_pairs);
}

#[test]
fn two_documents_never_share_a_batch() {
    let mut s = crate::derive::tests::showcase_store();
    s.status.generation = 3;
    let text = "# Other\n\nother body\n\n## Part\n\npart body\n";
    s.docs.insert(
        "other.md".into(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: BTreeMap::new(),
        },
    );
    let b = derive(&s);
    let sections: Vec<&Goal> = b
        .goals
        .iter()
        .filter(|g| g.kind == "reconcile-section")
        .collect();
    assert!(sections.len() >= 5, "{}", sections.len());
    // No roots: documents are levels in path order, other.md before shop.md.
    assert!(b.is_ready("g:reconcile-section:other.md#/other"));
    assert!(!b.is_ready("g:reconcile-section:shop.md#/shop"));
    for batch in &b.batches {
        let docs: BTreeSet<String> = batch
            .goals
            .iter()
            .filter_map(|id| b.goal(id))
            .filter_map(|g| target_doc(&g.target))
            .collect();
        assert_eq!(docs.len(), 1, "{:?}", batch);
        assert!(batch.goals.len() <= 3, "{:?}", batch);
    }
}
