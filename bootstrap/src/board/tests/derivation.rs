// Derivation, parked and failed entries, flip parking, and the derivation indices.
use super::*;

// A failed abstract-entity goal stands while the count grows by less than a
// fifth (docs/compiler/reconciler.md#parked-and-failed).
#[test]
fn failed_abstract_entity_stands_within_the_band() {
    let mk = |count: u64, generation: u64| Goal {
        id: "g:abstract-entity:ent:ines".into(),
        kind: "abstract-entity".into(),
        target: "ent:ines".into(),
        change: json!({"limits": [{"limit": "requirements-per-entity", "count": count, "soft": 50, "hard": 80, "level": "hard"}]}),
        cause: Some(crate::model::Cause {
            generation,
            mutation: 0,
            via: "limits".into(),
        }),
        ..Default::default()
    };
    let failed = mk(82, 60);
    assert!(failure_stands(&failed, &mk(82, 61)));
    assert!(failure_stands(&failed, &mk(95, 70)));
    assert!(!failure_stands(&failed, &mk(99, 70)));
    let other = Goal {
        kind: "review-entity".into(),
        ..mk(82, 60)
    };
    assert!(!failure_stands(
        &other,
        &Goal {
            kind: "review-entity".into(),
            ..mk(83, 61)
        }
    ));
}

// A declined fan-out goal stands while its candidates move: only the child count
// growing by a fifth, or a bump of the limits, reopens it.
// Mirrors docs/compiler/reconciler.md#parked-and-failed.
#[test]
fn failed_fan_out_stands_while_coupling_moves() {
    let mk = |count: u64, soft: u64, candidates: Value, generation: u64| Goal {
        id: "g:abstract-entity:ent:shop".into(),
        kind: "abstract-entity".into(),
        target: "ent:shop".into(),
        change: json!({"fan_out": count, "limit": {"soft": soft, "hard": 15}, "candidates": candidates}),
        cause: Some(crate::model::Cause {
            generation,
            mutation: 0,
            via: "limits".into(),
        }),
        ..Default::default()
    };
    let failed = mk(10, 9, json!([["ent:a", "ent:b"]]), 60);
    assert!(failure_stands(
        &failed,
        &mk(10, 9, json!([["ent:a", "ent:c"]]), 70)
    ));
    assert!(failure_stands(&failed, &mk(11, 9, json!([]), 71)));
    assert!(!failure_stands(&failed, &mk(12, 9, json!([]), 72)));
    assert!(!failure_stands(
        &failed,
        &mk(10, 8, json!([["ent:a", "ent:b"]]), 73)
    ));
}

#[test]
fn a_clean_store_derives_zero_goals() {
    let s = settled_store();
    let b = derive(&s);
    assert!(
        b.open_goals().is_empty(),
        "{:?}",
        b.open_goals().iter().map(|g| &g.id).collect::<Vec<_>>()
    );
    let v = b.verdict();
    assert!(v.converged());
    assert_eq!(v.open, 0);
    assert!(
        v.blocked > 0,
        "the ledger goals ride as blocked on the generate release"
    );
    assert!(
        b.summary_line().starts_with("compile: 0 goals, "),
        "{}",
        b.summary_line()
    );
    assert!(b.batches.is_empty());
}

#[test]
fn goals_keep_their_identity_across_two_derivations_of_one_record() {
    let mut s = settled_store();
    record(
        &mut s,
        7,
        store::CHANGE_ENTITY,
        "ent:order",
        "entities",
        json!({"requirements": ["req:shop-5"]}),
    );
    let a = derive(&s);
    let b = derive(&s);
    let ga = a.goal("g:review-entity:ent:order").expect("derived");
    let gb = b.goal("g:review-entity:ent:order").expect("derived again");
    assert_eq!(ga.change, gb.change);
    assert_eq!(ga.cause, gb.cause);
    assert_eq!(ga.cause.as_ref().map(|c| c.generation), Some(7));
    assert_eq!(a.records_of(&ga.id), b.records_of(&gb.id));
    assert_eq!(a.records_of(&ga.id).len(), 1);
    // A parked entry with the same change keeps the goal parked and first in line.
    s.status.parked.push(ParkedGoal::new(
        ga.clone(),
        "left open after its retry in session b7-1",
    ));
    let c = derive(&s);
    assert_eq!(
        c.parked_reasons.get(&ga.id).map(String::as_str),
        Some("left open after its retry in session b7-1")
    );
    assert!(c
        .explain(&s, &ga.id)
        .unwrap()
        .contains("parked: left open after its retry"));
    assert_eq!(
        c.goal_json(c.goal(&ga.id).unwrap())["parkedReason"],
        "left open after its retry in session b7-1"
    );
    assert_eq!(c.goal(&ga.id).unwrap().state, GoalState::Parked);
    assert!(c.dropped_parked.is_empty());
    assert_eq!(c.counts().parked, 1);
    assert_eq!(c.counts().open, 1);
}

// A read-only derivation never commits: Board::compute on a store whose documents
// changed on disk leaves the journal, the section trees, and the generation
// untouched while the board shows the dirty section's goal.
// Mirrors docs/compiler/reconciler.md#goal-derivation.
#[test]
fn compute_derives_from_changed_documents_without_writing() {
    let root = std::env::temp_dir().join(format!(
        "jazyk-board-compute-{}-{}",
        crate::sys::process_id(),
        crate::verify::now_iso().replace([':', '.'], "-")
    ));
    let docs = root.join("docs");
    crate::vfs::create_dir_all(&docs).unwrap();
    crate::vfs::write(docs.join("a.md"), "# A\n\nThe first body.\n").unwrap();
    let out = root.join(".jazyk");
    let proj = Project {
        root: root.clone(),
        out: out.clone(),
        docs_glob: vec!["docs/**/*.md".into()],
        ..Default::default()
    };
    // A build's alignment writes the trees once.
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    let (parsed, _) = crate::reconcile::parse_all(&proj);
    assert!(parsed.contains_key("docs/a.md"), "{:?}", parsed.keys());
    s.sync_docs(&parsed);
    let generation = s.status.generation;
    let journal = |out: &Path| {
        crate::vfs::read_dir(crate::layout::journal_dir(out))
            .map(|d| d.count())
            .unwrap_or(0)
    };
    let entries = journal(&out);
    assert!(entries >= 1);
    // The document changes on disk; a read-only derivation sees it and writes nothing.
    crate::vfs::write(docs.join("a.md"), "# A\n\nThe second body.\n").unwrap();
    let b = Board::compute(&proj, &out);
    assert!(
        b.goals
            .iter()
            .any(|g| g.kind == "reconcile-section" && g.target == "docs/a.md#/a"),
        "{:?}",
        b.goals.iter().map(|g| &g.id).collect::<Vec<_>>()
    );
    assert_eq!(journal(&out), entries);
    let on_disk = Store::load(&out);
    assert_eq!(on_disk.status.generation, generation);
    assert!(on_disk.docs["docs/a.md"].sections["/a"]
        .raw
        .contains("The first body."));
    crate::vfs::remove_dir_all(&root).ok();
}

// A failed place-anchors goal leaves its document undecided: the document's
// sections wait, and nothing else does. Mirrors
// docs/compiler/goals/place-anchors.md#readiness.
#[test]
fn a_failed_place_anchors_goal_blocks_its_own_sections_only() {
    let mut s = settled_store();
    record(
        &mut s,
        10,
        store::CHANGE_ALIGNMENT_PENDING,
        "shop.md",
        "alignment",
        json!({"proposals": 1}),
    );
    record(
        &mut s,
        10,
        store::CHANGE_SECTION_DIRTY,
        "shop.md#/shop/orders",
        "section",
        json!({"added": 1}),
    );
    record(
        &mut s,
        10,
        store::CHANGE_ENTITY,
        "ent:customer",
        "entities",
        json!({"requirement": "req:shop-1"}),
    );
    let b = derive(&s);
    assert!(
        !b.is_ready("g:review-entity:ent:customer"),
        "tier 0 and 1 run first"
    );
    let anchors = Goal {
        id: "g:place-anchors:shop.md".into(),
        kind: "place-anchors".into(),
        class: "compile".into(),
        mandatory: true,
        target: "shop.md".into(),
        unit: "document".into(),
        change: json!({"proposals": 1}),
        ..Default::default()
    };
    s.status.failed.push(crate::model::FailedGoal {
        goal: anchors,
        reason: "no candidate fits".into(),
    });
    let b = derive(&s);
    assert!(matches!(
        b.goal("g:place-anchors:shop.md").map(|g| &g.state),
        Some(GoalState::Failed { .. })
    ));
    assert!(
        !b.is_ready("g:reconcile-section:shop.md#/shop/orders"),
        "{:?}",
        b.readiness.get("g:reconcile-section:shop.md#/shop/orders")
    );
    assert!(
        b.is_ready("g:review-entity:ent:customer"),
        "{:?}",
        b.readiness.get("g:review-entity:ent:customer")
    );
    assert!(!b.verdict().converged());
}

// A goal parked mid-build takes no batch, and its batch neighbors still run.
// Mirrors docs/compiler/reconciler.md#parked-and-failed.
#[test]
fn withholding_a_parked_goal_keeps_its_batch_neighbors() {
    let mut s = settled_store();
    record(
        &mut s,
        10,
        store::CHANGE_ENTITY,
        "ent:customer",
        "entities",
        json!({"requirement": "req:shop-1"}),
    );
    record(
        &mut s,
        10,
        store::CHANGE_ENTITY,
        "ent:order",
        "entities",
        json!({"requirement": "req:shop-2"}),
    );
    let mut b = derive(&s);
    let (a, o) = (
        "g:review-entity:ent:customer".to_string(),
        "g:review-entity:ent:order".to_string(),
    );
    assert!(
        b.batches.iter().flat_map(|bt| &bt.goals).any(|g| *g == a),
        "{:?}",
        b.batches
    );
    assert!(
        b.batches.iter().flat_map(|bt| &bt.goals).any(|g| *g == o),
        "{:?}",
        b.batches
    );
    b.withhold(&BTreeSet::from([a.clone()]), &s, &project_for(&s));
    let batched: Vec<&String> = b.batches.iter().flat_map(|bt| &bt.goals).collect();
    assert!(!batched.contains(&&a), "{:?}", b.batches);
    assert!(batched.contains(&&o), "{:?}", b.batches);
    assert!(!b.is_ready(&a));
}

// A pending flip ruling holds its pair blocked on a human: the verdict converges
// with the pair in the blocked count. Mirrors docs/compiler/compilation.md#convergence.
#[test]
fn a_pending_flip_ruling_rides_as_blocked_in_the_verdict() {
    let mut s = settled_store();
    let before = derive(&s).verdict();
    assert!(before.converged(), "{:?}", before);
    let flip = Goal {
        id: "g:review-entity:ent:customer".into(),
        kind: "review-entity".into(),
        class: "compile".into(),
        mandatory: true,
        target: "ent:customer".into(),
        unit: "entity".into(),
        change: json!({"unstable": "customer", "subject": "ent:customer"}),
        cause: Some(crate::model::Cause {
            generation: 2,
            mutation: 0,
            via: "flip-detection".into(),
        }),
        ..Default::default()
    };
    s.status.parked.push(flip.into());
    s.graph.diagnostics.insert(
        "diag:unstable-derivation-1".into(),
        unstable_ruling("ent:customer"),
    );
    let v = derive(&s).verdict();
    assert!(v.converged(), "{:?}", v);
    assert_eq!(v.blocked, before.blocked + 1, "{:?}", v);
}

// A goal parked by flip detection stays parked when it derives again: its parked
// change names the flip, never the goal's own records, so a change comparison
// would drop it at once. Mirrors docs/compiler/reconciler.md#flip-detection.
#[test]
fn a_flip_parked_goal_stays_parked_when_it_derives_again() {
    let mut s = settled_store();
    record(
        &mut s,
        10,
        store::CHANGE_ENTITY,
        "ent:customer",
        "entities",
        json!({"requirement": "req:shop-1"}),
    );
    let derived = derive(&s)
        .goal("g:review-entity:ent:customer")
        .cloned()
        .expect("the entity change derives a review");
    let mut flip = derived.clone();
    flip.change = json!({"unstable": "customer", "subject": "ent:customer"});
    flip.cause = Some(crate::model::Cause {
        generation: 40,
        mutation: 0,
        via: "flip-detection".into(),
    });
    s.status.parked.push(flip.clone().into());
    s.graph.diagnostics.insert(
        "diag:unstable-derivation-1".into(),
        unstable_ruling("ent:customer"),
    );
    let b = derive(&s);
    let g = b.goal(&flip.id).expect("the flip entry stands");
    assert_eq!(
        g.change, derived.change,
        "the goal keeps its derived change"
    );
    assert!(
        matches!(&g.state, GoalState::Blocked { on } if on.contains("ent:customer")),
        "{:?}",
        g.state
    );
    assert!(!b.dropped_parked.contains(&flip.id));
    assert!(b.batches.iter().all(|bt| !bt.goals.contains(&flip.id)));
}

// A store of two thousand entities and five thousand requirements recomputes its
// derived data, derives its board, and runs the checks in bounded time: the
// indices keep each linear in the graph where a scan per node would not be.
// Ignored in the normal run (the ledger goals over five thousand requirements
// alone take seconds in debug); `cargo test -- --ignored a_large_store` runs it.
// Mirrors docs/compiler/graph.md#derived-data.
// The indices a derivation builds (goal by id, open kinds, the records a goal
// stands on, the compile goals in a GC goal's cone) answer what the scans over
// the goals and the change records answered.
#[test]
fn derivation_indices_match_the_scans() {
    let mut synthetic = crate::derive::tests::synthetic_store(300, 700);
    let mut batch = RecordBatch::new(1);
    crate::derive::recompute(&mut synthetic, "g1", &mut batch);
    for r in batch.records() {
        synthetic.status.record_change(r.clone());
    }
    let mut settled = settled_store();
    record(
        &mut settled,
        10,
        store::CHANGE_SECTION_DIRTY,
        "shop.md#/shop/orders",
        "section",
        json!({"added": 1}),
    );
    record(
        &mut settled,
        10,
        store::CHANGE_ENTITY,
        "ent:order",
        "entities",
        json!({}),
    );
    let mut cones = 0;
    for s in [synthetic, settled] {
        let b = derive(&s);
        cones += b.goals.iter().filter(|g| g.class == "gc").count();
        let refs = Refs::build(&s);
        for g in &b.goals {
            assert!(std::ptr::eq(b.goal(&g.id).unwrap(), g));
            assert_eq!(
                b.open_kind_on(&g.kind, &g.target),
                b.open_goals()
                    .iter()
                    .any(|o| o.kind == g.kind && o.target == g.target)
            );
            let kinds = goals::record_kinds(&g.kind);
            let subjects: Vec<&str> = match goals::pair_members(&g.target) {
                Some((x, y)) => vec![x, y],
                None => vec![g.target.as_str()],
            };
            let scanned: Vec<String> = s
                .status
                .changes
                .iter()
                .filter(|c| {
                    kinds.contains(&c.kind.as_str()) && subjects.contains(&c.subject.as_str())
                })
                .map(|c| c.id.clone())
                .collect();
            assert_eq!(b.records_of(&g.id), scanned, "{}", g.id);
            if g.class == "gc" {
                let cone = cone_with(&s, &refs, &g.target);
                let scanned: Vec<String> = b
                    .goals
                    .iter()
                    .filter(|o| o.class == "compile" && is_open(o) && cone.holds_target(&o.target))
                    .map(|o| o.id.clone())
                    .collect();
                assert_eq!(b.cone_blockers(&g.id), scanned, "{}", g.id);
            }
        }
        assert!(b.goal("g:bind:req:nope").is_none());
        assert!(!b.open_kind_on("bind", "req:nope"));
    }
    assert!(cones > 0, "a GC goal walks a cone");
}

#[test]
#[ignore]
fn a_large_store_derives_and_checks_in_bounded_time() {
    let mut s = crate::derive::tests::synthetic_store(2000, 5000);
    let mut batch = RecordBatch::new(1);
    let started = std::time::Instant::now();
    crate::derive::recompute(&mut s, "g1", &mut batch);
    for r in batch.records() {
        s.status.record_change(r.clone());
    }
    let recomputed = started.elapsed();
    let b = derive(&s);
    let derived = started.elapsed() - recomputed;
    let f = crate::reconcile::checks(&s, &project_for(&s));
    let checked = started.elapsed() - recomputed - derived;
    assert!(
        b.goals.iter().any(|g| g.class == "gc"),
        "a GC goal walks a cone"
    );
    assert!(!f.is_empty());
    eprintln!(
        "large store: recompute {:?}, derive {:?} ({} goals, {} batches), checks {:?}",
        recomputed,
        derived,
        b.goals.len(),
        b.batches.len(),
        checked
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "recompute {:?}, derive {:?}, checks {:?}",
        recomputed,
        derived,
        checked
    );
}
