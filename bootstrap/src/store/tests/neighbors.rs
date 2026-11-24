// Text normalization, search, and requirement neighbors.
use super::*;

#[test]
fn recorded_pair_survives_frontier_churn_and_is_reused_on_return() {
    let mut store = Store {
        out: own_dir("pair-retention"),
        ..Default::default()
    };
    let left = "req:reconciler-batching-s1d1";
    let right = "req:reconciler-goal-derivation-yra3";
    let left_text =
        "A session gets one batch of goals in one class, tier and kind with the same executor.";
    let right_text = "Goals identify a kind and class, and mandatory goals block convergence.";
    let requirement = |statement: &str| Requirement {
        statement: statement.into(),
        entities: vec!["ent:reconciler".into()],
        ..Default::default()
    };
    store
        .graph
        .entities
        .insert("ent:reconciler".into(), entity("Reconciler"));
    store
        .graph
        .requirements
        .insert(left.into(), requirement(left_text));
    store
        .graph
        .requirements
        .insert(right.into(), requirement(right_text));
    assert!(store
        .requirement_neighbors(left)
        .contains(&right.to_string()));
    let key = crate::goals::pair_target(left, right);
    let decision = PairDecision {
        verdict: "distinct".into(),
        statements: BTreeMap::from([
            (left.into(), super::super::v5::statement_hash(left_text)),
            (right.into(), super::super::v5::statement_hash(right_text)),
        ]),
    };
    store.decisions.pairs.insert(key.clone(), decision.clone());
    for index in 0..6 {
        for (tag, wording) in [("batch", left_text), ("goals", right_text)] {
            store.graph.requirements.insert(
                format!("req:frontier-{tag}-{index}"),
                requirement(&format!("{wording} Priority {index}.")),
            );
        }
    }
    store.graph.requirements.insert(
        "req:frontier-filler-a".into(),
        requirement("Captures audio."),
    );
    store.graph.requirements.insert(
        "req:frontier-filler-b".into(),
        requirement("Publishes diagrams."),
    );
    assert!(!store
        .pair_review_neighbors(left)
        .contains(&right.to_string()));
    assert!(!store
        .pair_review_neighbors(right)
        .contains(&left.to_string()));
    for id in [
        "req:reconciler-bubbling-3d89",
        "req:reconciler-batching-y9gw",
    ] {
        store.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: "The preview joins the next board.".into(),
                ..Default::default()
            },
        );
    }
    store.save();
    let report = store.apply(
        vec![Op::MergeRequirements {
            keep: "req:reconciler-bubbling-3d89".into(),
            absorb: "req:reconciler-batching-y9gw".into(),
            reason: "Both describe preview joining.".into(),
        }],
        &session(),
    );
    assert_eq!(report.applied, 1, "{:?}", report.skipped);
    let mut store = Store::load(&store.out);
    assert_eq!(store.decisions.pairs.get(&key), Some(&decision));
    store
        .graph
        .requirements
        .retain(|id, _| !id.starts_with("req:frontier-"));
    assert!(store
        .requirement_neighbors(left)
        .contains(&right.to_string()));
    let mut rebuilt = ChangeRecord::new(0, 0, 0, CHANGE_REQ_REVISED, left, "decision");
    assert!(!store
        .pair_review_candidates(&rebuilt)
        .iter()
        .any(|(id, _)| id == right));
    rebuilt.via = "fields".into();
    assert!(store
        .pair_review_candidates(&rebuilt)
        .iter()
        .any(|(id, _)| id == right));
    crate::vfs::remove_dir_all(&store.out).unwrap();
}

#[test]
fn merging_requirements_preserves_survivor_verdict_and_reopens_absorbed_pair() {
    let mut store = Store {
        out: tmp(),
        ..Default::default()
    };
    let keep = "req:a-survivor";
    let absorb = "req:z-absorb";
    let peer = "req:m-peer";
    let incoming = "req:n-incoming";
    let settled = "req:o-settled";
    let durable = "ent:durable";
    let evidence = "ent:evidence";
    let reporting = "ent:reporting";
    let keep_statement = "Save the event snapshot after validating its checksum.";
    let absorb_statement = "Persist the event snapshot once checksum validation succeeds.";
    let peer_statement = "Expose a saved event snapshot and its checksum in history.";
    let incoming_statement = "Display the event snapshot checksum when viewing it.";
    for (id, name) in [
        (durable, "Durability"),
        (evidence, "Evidence"),
        (reporting, "Reporting"),
    ] {
        store.graph.entities.insert(id.into(), entity(name));
    }
    for (id, statement, entities) in [
        (keep, keep_statement, vec![durable, evidence, reporting]),
        (absorb, absorb_statement, vec![durable, evidence, reporting]),
        (peer, peer_statement, vec![evidence]),
        (incoming, incoming_statement, vec![durable]),
        (
            settled,
            "Print the event snapshot checksum for inspections.",
            vec![reporting],
        ),
    ] {
        store.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: statement.into(),
                entities: entities.into_iter().map(String::from).collect(),
                ..Default::default()
            },
        );
    }
    for index in 0..6 {
        let entity_id = format!("ent:intake-{index}");
        store
            .graph
            .entities
            .insert(entity_id.clone(), entity("Intake"));
        store
            .graph
            .requirements
            .get_mut(keep)
            .unwrap()
            .entities
            .push(entity_id.clone());
        store.graph.requirements.insert(
            format!("req:intake-{index}"),
            Requirement {
                statement: format!("{keep_statement} Priority {index}."),
                entities: vec![entity_id],
                ..Default::default()
            },
        );
    }
    let decision = |left: &str, right: &str, verdict: &str| PairDecision {
        verdict: verdict.into(),
        statements: [left, right]
            .into_iter()
            .map(|id| {
                let statement = &store.graph.requirements[id].statement;
                (id.into(), super::super::v5::statement_hash(statement))
            })
            .collect(),
    };
    let survivor_decision = decision(keep, peer, "distinct");
    let absorbed_collision = decision(absorb, peer, "consistent");
    let absorbed_only = decision(absorb, incoming, "consistent");
    let absorbed_settled = decision(absorb, settled, "diagnosed");
    let accepted = decision(keep, settled, "consistent");
    let survivor_key = crate::goals::pair_target(keep, peer);
    let reopened_key = crate::goals::pair_target(keep, incoming);
    store.decisions.pairs.extend([
        (survivor_key.clone(), survivor_decision.clone()),
        (crate::goals::pair_target(absorb, peer), absorbed_collision),
        (crate::goals::pair_target(absorb, incoming), absorbed_only),
        (crate::goals::pair_target(absorb, settled), absorbed_settled),
    ]);
    assert!(store.pair_review_neighbors(peer).contains(&keep.into()));
    assert!(store.pair_review_neighbors(incoming).contains(&keep.into()));
    assert!(!store.pair_review_neighbors(keep).contains(&incoming.into()));
    store.save();
    let accepted_key = crate::goals::pair_target(keep, settled);
    let mut commit = session();
    commit.resolved.push(Resolved {
        goal: format!("g:rejudge-pair:{accepted_key}"),
        justification: "The saved snapshot can be printed for inspection.".into(),
        evidence: serde_json::json!({"verdict": "consistent"}),
    });
    let report = store.apply(
        vec![Op::MergeRequirements {
            keep: keep.into(),
            absorb: absorb.into(),
            reason: "The two statements express the same obligation.".into(),
        }],
        &commit,
    );
    assert_eq!(report.applied, 1, "{:?}", report.skipped);
    store.project_decisions();
    assert!(!store.decisions.pairs.contains_key(&reopened_key));
    assert_eq!(store.decisions.pairs.get(&accepted_key), Some(&accepted));
    let store = Store::load(&store.out);
    assert_eq!(store.resolve_id(absorb), keep);
    assert_eq!(
        store.decisions.pairs.get(&survivor_key),
        Some(&survivor_decision)
    );
    assert!(!store.decisions.pairs.contains_key(&reopened_key));
    assert!(!store
        .decisions
        .pairs
        .values()
        .any(|decision| decision.statements.contains_key(absorb)));
    let review = |left: &str, right: &str| {
        store.status.changes.iter().any(|record| {
            record.subject == left
                && record.kind == CHANGE_REQ_REVISED
                && store
                    .pair_review_candidates(record)
                    .iter()
                    .any(|(partner, _)| partner == right)
        })
    };
    assert!(review(incoming, keep));
    assert!(!review(peer, keep) && !review(keep, peer));
    crate::vfs::remove_dir_all(&store.out).unwrap();
}

#[test]
fn accepted_unranked_pair_expires_after_an_endpoint_revision() {
    let mut store = Store {
        out: tmp(),
        ..Default::default()
    };
    let left = "req:snapshot";
    let right = "req:history";
    let parent = "ent:store";
    let source = "ent:persistence";
    let mut declared = entity("Persistence");
    declared.parent = Some(parent.into());
    declared.provenance = Some(Provenance::Decree {
        author: "owner".into(),
        at: "now".into(),
        note: None,
    });
    store.graph.entities.insert(source.into(), declared);
    store.graph.entities.insert(parent.into(), entity("Store"));
    let statement = "Save a snapshot of the journal.";
    store.graph.requirements.insert(
        left.into(),
        Requirement {
            statement: statement.into(),
            entities: vec![source.into()],
            ..Default::default()
        },
    );
    store.graph.requirements.insert(
        right.into(),
        Requirement {
            statement: "Read recent history.".into(),
            ..Default::default()
        },
    );
    assert!(store.pair_review_neighbors(left).is_empty());
    store.save();
    let key = crate::goals::pair_target(left, right);
    let mut commit = session();
    commit.resolved.push(Resolved {
        goal: format!("g:rejudge-pair:{key}"),
        justification: "Reading history does not constrain saving snapshots.".into(),
        evidence: serde_json::json!({"verdict": "consistent"}),
    });
    store.apply(Vec::new(), &commit);
    let mut store = Store::load(&store.out);
    assert_eq!(store.decisions.pairs[&key].verdict, "consistent");
    let report = store.apply(
        vec![Op::RetractDecree {
            id: source.into(),
            reason: "Use its parent for persistence obligations.".into(),
        }],
        &session(),
    );
    assert_eq!(report.applied, 1, "{:?}", report.skipped);
    let mut store = Store::load(&store.out);
    assert_eq!(store.graph.requirements[left].statement, statement);
    assert_eq!(store.graph.requirements[left].entities, [parent]);
    assert!(!store.decisions.pairs.contains_key(&key));

    commit.base = Some(Box::new(store.clone()));
    let report = store.apply(
        vec![Op::UpdateRequirement {
            id: right.into(),
            statement: Some("Keep history private until an administrator requests it.".into()),
            entities: None,
            source: None,
            provenance: None,
            edges: None,
            transition: None,
        }],
        &session(),
    );
    assert_eq!(report.applied, 1, "{:?}", report.skipped);
    store.apply(Vec::new(), &commit);
    assert!(!Store::load(&store.out).decisions.pairs.contains_key(&key));
    crate::vfs::remove_dir_all(&store.out).unwrap();
}

#[test]
fn dormant_pairs_require_original_live_endpoints_and_statement_hashes() {
    let mut store = Store::default();
    let left = "req:old";
    let right = "req:partner";
    let unrelated = "req:unreviewed";
    let left_text = "Read current sources.";
    let right_text = "Save current decisions.";
    for (id, wording) in [
        (left, left_text),
        (right, right_text),
        (unrelated, "Read current sources for the audit."),
    ] {
        store.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: wording.into(),
                ..Default::default()
            },
        );
    }
    store
        .graph
        .entities
        .insert("ent:audit".into(), entity("Audit"));
    for id in [left, unrelated] {
        store
            .graph
            .requirements
            .get_mut(id)
            .unwrap()
            .entities
            .push("ent:audit".into());
    }
    assert!(store
        .pair_review_neighbors(left)
        .contains(&unrelated.into()));
    let key = crate::goals::pair_target(left, right);
    let decision = PairDecision {
        verdict: "consistent".into(),
        statements: BTreeMap::from([
            (left.into(), super::super::v5::statement_hash(left_text)),
            (right.into(), super::super::v5::statement_hash(right_text)),
        ]),
    };
    store.decisions.pairs.insert(key.clone(), decision.clone());
    store.project_decisions();
    assert_eq!(store.decisions.pairs.len(), 1);
    assert_eq!(store.decisions.pairs.get(&key), Some(&decision));
    store.graph.requirements.get_mut(left).unwrap().statement = "Discard current sources.".into();
    store.project_decisions();
    assert!(store.decisions.pairs.is_empty());
    store.graph.requirements.remove(left);
    store.graph.redirects.insert(left.into(), unrelated.into());
    store.decisions.pairs.insert(key, decision);
    store.project_decisions();
    assert!(store.decisions.pairs.is_empty());
}

#[test]
fn normalize_statement_keeps_operators_signs_and_decimals() {
    assert_eq!(
        normalize_statement("The reply arrives, under 30 s."),
        "the reply arrives under 30 s"
    );
    assert_ne!(
        normalize_statement("latency > 30 s"),
        normalize_statement("latency < 30 s")
    );
    assert_ne!(
        normalize_statement("under 30 s"),
        normalize_statement("under 3.0 s")
    );
    assert_ne!(
        normalize_statement("an offset of -1"),
        normalize_statement("an offset of 1")
    );
    assert_eq!(
        normalize_statement("Read-only mode."),
        normalize_statement("read only mode")
    );
    assert_eq!(normalize_statement("on 2024-09-27"), "on 2024 09 27");
    assert_eq!(normalize_statement("x >= 5"), "x > = 5");
}

#[test]
fn search_tiers() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    s.graph.entities.insert(
        "ent:shopping-cart".into(),
        Entity {
            name: "Shopping Cart".into(),
            aliases: vec!["cart".into()],
            ..Default::default()
        },
    );
    s.graph.entities.insert(
        "ent:card".into(),
        Entity {
            name: "Credit Card".into(),
            ..Default::default()
        },
    );
    let hits = s.search("cart");
    assert_eq!(hits[0].0, "ent:shopping-cart");
    let hits2 = s.search("credit card");
    assert_eq!(hits2[0].0, "ent:card");
}

#[test]
fn requirement_neighbors_find_the_topic_cluster() {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:util".into(), entity("Sorting Algorithm CLI Utility"));
    let mk = |statement: &str| Requirement {
        statement: statement.into(),
        entities: vec!["ent:util".into()],
        sources: vec![mention("m.md", "/m", "q")],
        ..Default::default()
    };
    // The example-sort failure: three statements about reverse-order sorting were
    // never put side by side. Stemmed content-token overlap pairs them.
    s.graph.requirements.insert(
        "req:m-2".into(),
        mk("The system allows the -r argument, which reverses sorting order to descending."),
    );
    s.graph.requirements.insert(
        "req:m-3".into(),
        mk("The Sorting Algorithm CLI Utility keeps track of reverse order with `-r`."),
    );
    s.graph.requirements.insert("req:m-5".into(), mk("The Sorting Algorithm CLI Utility strips out whitespace before and after the current line."));
    s.graph.requirements.insert("req:m-8".into(), mk("The Sorting Algorithm CLI Utility sorts lines descending, or ascending if reverse order is set."));
    let n = s.requirement_neighbors("req:m-8");
    assert!(n.contains(&"req:m-2".to_string()), "{:?}", n);
    assert!(n.contains(&"req:m-3".to_string()), "{:?}", n);
    assert!(!n.contains(&"req:m-5".to_string()), "{:?}", n);
}

#[test]
fn requirement_neighbors_pair_on_shared_entities_alone() {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:reorder-point".into(), entity("Reorder point"));
    s.graph
        .entities
        .insert("ent:restock-task".into(), entity("Restock task"));
    let mk = |statement: &str, entities: &[&str]| Requirement {
        statement: statement.into(),
        entities: entities.iter().map(|e| e.to_string()).collect(),
        sources: vec![mention("m.md", "/m", "q")],
        ..Default::default()
    };
    // A glossary definition restating an inventory rule shares both entities
    // and, with their names out of the token pool, one other word ("triggers"
    // against "triggered"): two shared entities plus one token qualify the pair.
    s.graph.requirements.insert(
        "req:inv-3".into(),
        mk(
            "A Restock task is triggered when Stock falls below the Reorder point.",
            &["ent:reorder-point", "ent:restock-task"],
        ),
    );
    s.graph.requirements.insert(
        "req:glos-3".into(),
        mk(
            "The Reorder point is whatever level triggers a Restock task.",
            &["ent:reorder-point", "ent:restock-task"],
        ),
    );
    s.graph.requirements.insert(
        "req:other".into(),
        mk(
            "The Restock task queue drains nightly.",
            &["ent:restock-task"],
        ),
    );
    // Two shared entities and no word in common are what every statement of one
    // page shares: no pair (review-entity reads the entity's statements whole).
    s.graph.requirements.insert(
        "req:unrelated".into(),
        mk(
            "The Reorder point of a Restock task is set by hand.",
            &["ent:reorder-point", "ent:restock-task"],
        ),
    );
    let n = s.requirement_neighbors("req:glos-3");
    assert!(n.contains(&"req:inv-3".to_string()), "{:?}", n);
    assert!(!n.contains(&"req:other".to_string()), "{:?}", n);
    assert!(!n.contains(&"req:unrelated".to_string()), "{:?}", n);
}

// The example-org expenses shapes: an opening-paragraph sentence against a step
// below it sharing one entity and the verb stem pairs (expenses-5 against
// expenses-6), and so does a restated transition (expenses-1 against expenses-15).
// Mirrors docs/compiler/reconciler.md#pairs.
#[test]
fn requirement_neighbors_pair_intro_restatements_and_same_transitions() {
    let mut s = Store::default();
    seed_doc(
        &mut s,
        "expenses.md",
        "# Expenses\n\nAn employee files an expense claim.\n\n## The claim\n\nClaims are filed on form EX-12.\n\n## Steps\n\nFinance reimburses.\n",
    );
    let opening = s.opening_section("expenses.md").unwrap().to_string();
    let mut later: Vec<String> = s.docs["expenses.md"]
        .sections
        .iter()
        .filter(|(r, _)| **r != opening)
        .map(|(r, _)| r.clone())
        .collect();
    later.sort();
    assert_eq!(later.len(), 2, "{:?}", later);
    s.graph
        .entities
        .insert("ent:expense-claim".into(), entity("Expense claim"));
    s.graph
        .entities
        .insert("ent:employees".into(), entity("Employees"));
    s.graph
        .entities
        .insert("ent:expense-tool".into(), entity("Expense tool"));
    s.graph
        .entities
        .insert("ent:finance".into(), entity("Finance"));
    s.graph
        .entities
        .insert("ent:payroll".into(), entity("Payroll"));
    let mk = |statement: &str, entities: &[&str], section: &str| Requirement {
        statement: statement.into(),
        entities: entities.iter().map(|e| e.to_string()).collect(),
        sources: vec![mention("expenses.md", section, "q")],
        ..Default::default()
    };
    // expenses-5 ~ expenses-6: one shared entity, one shared token ("file").
    s.graph.requirements.insert(
        "req:expenses-5".into(),
        mk(
            "An employee who spends their own money on company business files an expense claim to be reimbursed.",
            &["ent:employees", "ent:expense-claim"],
            &opening,
        ),
    );
    s.graph.requirements.insert(
        "req:expenses-6".into(),
        mk(
            "Claims are filed on form EX-12 in the expense tool.",
            &["ent:expense-claim", "ent:expense-tool"],
            &later[0],
        ),
    );
    // The same shape between two later sections: one token is not enough.
    s.graph.requirements.insert(
        "req:expenses-9".into(),
        mk(
            "A claim is filed with a receipt for every line.",
            &["ent:expense-claim"],
            &later[1],
        ),
    );
    let n = s.requirement_neighbors("req:expenses-5");
    assert!(n.contains(&"req:expenses-6".to_string()), "{:?}", n);
    let n = s.requirement_neighbors("req:expenses-6");
    assert!(n.contains(&"req:expenses-5".to_string()), "{:?}", n);
    assert!(!n.contains(&"req:expenses-9".to_string()), "{:?}", n);
    // expenses-1 ~ expenses-15 as a transition pair alone: same subject, same from
    // and to, no shared token, one shared entity, different documents.
    let tr = |from: &str, to: &str| {
        Some(crate::model::Transition {
            subject: "ent:expense-claim".into(),
            from: from.into(),
            to: to.into(),
            trigger: None,
            guard: None,
        })
    };
    s.graph.requirements.insert(
        "req:expenses-1".into(),
        Requirement {
            transition: tr("approved", "reimbursed"),
            ..mk(
                "Finance pays it back.",
                &["ent:finance", "ent:expense-claim"],
                &later[1],
            )
        },
    );
    s.graph.requirements.insert(
        "req:policies-4".into(),
        Requirement {
            transition: tr("Approved", "Reimbursed"),
            sources: vec![mention("policies.md", "/policies", "q")],
            ..mk(
                "The next run settles the money owed.",
                &["ent:expense-claim", "ent:payroll"],
                &later[1],
            )
        },
    );
    s.graph.requirements.insert(
        "req:policies-5".into(),
        Requirement {
            transition: tr("approved", "rejected"),
            sources: vec![mention("policies.md", "/policies", "q")],
            ..mk(
                "A wrong total sends it back.",
                &["ent:expense-claim"],
                &later[1],
            )
        },
    );
    let n = s.requirement_neighbors("req:expenses-1");
    assert!(n.contains(&"req:policies-4".to_string()), "{:?}", n);
    assert!(!n.contains(&"req:policies-5".to_string()), "{:?}", n);
}

// Mirrors docs/compiler/reconciler.md#pairs: two transitions of one subject with
// different ends are siblings of one machine and never pair; the same ends pair.
#[test]
fn sibling_transitions_of_one_machine_never_pair() {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:application".into(), entity("Application"));
    let mk = |st: &str, from: &str, to: &str| Requirement {
        statement: st.into(),
        entities: vec!["ent:application".into()],
        transition: Some(Transition {
            subject: "ent:application".into(),
            from: from.into(),
            to: to.into(),
            trigger: None,
            guard: None,
        }),
        sources: vec![mention("hiring.md", "/moving", "q")],
        ..Default::default()
    };
    s.graph.requirements.insert(
        "req:h-1".into(),
        mk(
            "An Application moves from applied to screened when the recruiter reviews it.",
            "applied",
            "screened",
        ),
    );
    s.graph.requirements.insert(
        "req:h-2".into(),
        mk(
            "An Application moves from applied to rejected when the recruiter declines it.",
            "applied",
            "rejected",
        ),
    );
    s.graph.requirements.insert(
        "req:h-3".into(),
        mk(
            "The recruiter moves an Application from applied to screened after reviewing it.",
            "applied",
            "screened",
        ),
    );
    let n = s.requirement_neighbors("req:h-1");
    assert!(!n.contains(&"req:h-2".to_string()), "{:?}", n);
    assert!(n.contains(&"req:h-3".to_string()), "{:?}", n);
}

// The erp operation bullets: six statements of one entity sharing "supports" and
// "operation" pair on nothing, and the two about a password still pair on it.
// Mirrors docs/compiler/reconciler.md#pairs.
#[test]
fn entity_boilerplate_never_counts_as_a_shared_token() {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:um".into(), entity("User Management"));
    s.graph
        .entities
        .insert("ent:ua".into(), entity("User Account"));
    let mk = |statement: &str| Requirement {
        statement: statement.into(),
        entities: vec!["ent:um".into(), "ent:ua".into()],
        sources: vec![mention("user.md", "/ops", "q")],
        ..Default::default()
    };
    for (id, st) in [
        ("req:u-1", "The user management system supports an `authenticateUser` operation that authenticates a user with a username and password."),
        ("req:u-2", "The user management system supports a `passwordChange` operation that changes the password of the caller's own account."),
        ("req:u-3", "The user management system supports a `createUser` operation that creates a new user account."),
        ("req:u-4", "The user management system supports an `updateUser` operation that updates an existing user account."),
        ("req:u-5", "The user management system supports a `deleteUser` operation that deletes a user account."),
        ("req:u-6", "The user management system supports a `passwordReset` operation that resets the password of a user account."),
    ] {
        s.graph.requirements.insert(id.into(), mk(st));
    }
    let bp = s.pair_boilerplate(
        &s.graph.requirements["req:u-3"],
        &s.graph.requirements["req:u-5"],
    );
    assert!(
        bp.contains("support") && bp.contains("operation"),
        "{:?}",
        bp
    );
    assert!(!bp.contains("password"), "{:?}", bp);
    let n = s.requirement_neighbors("req:u-3");
    assert!(n.is_empty(), "{:?}", n);
    let n = s.requirement_neighbors("req:u-2");
    assert_eq!(
        n,
        vec!["req:u-1".to_string(), "req:u-6".to_string()],
        "{:?}",
        n
    );
}

// The qwen pair flood: requirements sharing only the hub entity need three shared
// tokens, a pair sharing any other entity still needs two, and a graph whose
// entities never meet has no hub. Mirrors docs/compiler/reconciler.md#pairs.
#[test]
fn requirement_neighbors_discount_the_hub() {
    let mut s = Store::default();
    for (id, name) in [
        ("ent:order", "Order"),
        ("ent:customer", "Customer"),
        ("ent:shipment", "Shipment"),
        ("ent:payment", "Payment"),
    ] {
        s.graph.entities.insert(id.into(), entity(name));
    }
    let mk = |statement: &str, entities: &[&str]| Requirement {
        statement: statement.into(),
        entities: entities.iter().map(|e| e.to_string()).collect(),
        sources: vec![mention("m.md", "/m", "q")],
        ..Default::default()
    };
    // ent:order meets three peers; every other entity meets one.
    s.graph.requirements.insert(
        "req:o-1".into(),
        mk(
            "The Customer places an Order; the tool records the total and the date.",
            &["ent:order", "ent:customer"],
        ),
    );
    s.graph.requirements.insert(
        "req:o-2".into(),
        mk(
            "An Order ships in one Shipment; the carrier is noted.",
            &["ent:order", "ent:shipment"],
        ),
    );
    s.graph.requirements.insert(
        "req:o-3".into(),
        mk(
            "An Order is paid by one Payment; the receipt is kept.",
            &["ent:order", "ent:payment"],
        ),
    );
    assert_eq!(s.hub_entity().as_deref(), Some("ent:order"));
    // Two generic tokens over the hub alone ("tool", "record"): not a pair.
    s.graph.requirements.insert(
        "req:o-4".into(),
        mk("The tool records every Order change.", &["ent:order"]),
    );
    // Three shared tokens over the hub alone: still a pair.
    s.graph.requirements.insert(
        "req:o-5".into(),
        mk(
            "The tool records the date of an Order total.",
            &["ent:order"],
        ),
    );
    // Two shared tokens over a non-hub entity: a pair as before.
    s.graph.requirements.insert(
        "req:c-1".into(),
        mk(
            "A Customer who places nothing records no total.",
            &["ent:customer"],
        ),
    );
    let n = s.requirement_neighbors("req:o-1");
    assert!(!n.contains(&"req:o-4".to_string()), "{:?}", n);
    assert!(n.contains(&"req:o-5".to_string()), "{:?}", n);
    assert!(n.contains(&"req:c-1".to_string()), "{:?}", n);
    // A graph of one entity, or of tied peers, names no hub.
    let mut lone = Store::default();
    lone.graph
        .entities
        .insert("ent:util".into(), entity("Util"));
    lone.graph
        .requirements
        .insert("req:u-1".into(), mk("Util sorts.", &["ent:util"]));
    assert_eq!(lone.hub_entity(), None);
}
