use super::*;

// ---- the pair degree cap ----

// One broad statement picked by twelve created requirements. Every partner
// shares the Gateway entity and two content tokens with the hub (`reject` and
// one word of its own from the hub's list) and only `reject` with the other
// partners, so the hub is each partner's sole neighbor: twelve pairs on one
// requirement, eleven at score 3 and `req:p12` at score 4.
// Two words each, since the verb every partner would otherwise repeat is the
// gateway's boilerplate and counts for nothing (reconciler.md#pairs).
pub(crate) const HUB_WORDS: [&str; 12] = [
    "drops expired tokens",
    "logs refusals",
    "alerts operators",
    "counts failures",
    "throttles clients",
    "audits sessions",
    "blocks replays",
    "signs receipts",
    "rotates keys",
    "caches verdicts",
    "purges entries",
    "reports",
];

pub(crate) fn fanned_store() -> Store {
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:gw".into(),
        Entity {
            name: "Gateway".into(),
            ..Default::default()
        },
    );
    let req = |statement: &str| Requirement {
        statement: statement.into(),
        entities: vec!["ent:gw".into()],
        ..Default::default()
    };
    s.graph.requirements.insert(
        "req:hub".into(),
        req("The gateway rejects expired tokens, logs each refusal, alerts operators, counts failures, throttles clients, audits sessions, blocks replays, signs receipts, rotates keys, caches verdicts, purges stale entries, and reports metrics."),
    );
    for (i, w) in HUB_WORDS.iter().enumerate() {
        let id = format!("req:p{:02}", i + 1);
        let statement = if i + 1 == 12 {
            "The gateway rejects reports and metrics.".to_string()
        } else {
            format!("The gateway {}.", w)
        };
        s.graph.requirements.insert(id.clone(), req(&statement));
        let mut batch = store::RecordBatch::new(5);
        batch.push(0, store::CHANGE_REQ_CREATED, &id, "section", Value::Null);
        for r in batch.records() {
            s.status.record_change(r.clone());
        }
    }
    s.status.generation = 5;
    s
}

fn pair_id(p: &str) -> String {
    goal_id("rejudge-pair", &pair_target("req:hub", p))
}

// (a) Twelve partners open eight and wait four: the best score first, then by
// target; the hub's open degree is the cap; the hints carry the score and the
// waiting count per side. Mirrors docs/compiler/reconciler.md#pairs.
#[test]
fn a_requirement_picked_by_twelve_opens_eight_and_waits_four() {
    let s = fanned_store();
    let plan = pair_plan(&s);
    let open: Vec<&str> = plan.open.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(open.len(), limits::PAIRS_PER_REQUIREMENT_CAP, "{:?}", open);
    assert_eq!(
        plan.waiting.len(),
        4,
        "{:?}",
        plan.waiting.keys().collect::<Vec<_>>()
    );
    // The score-4 pair opens ahead of the score-3 ones whatever its id says.
    assert!(open.contains(&pair_id("req:p12").as_str()), "{:?}", open);
    for p in ["req:p01", "req:p07"] {
        assert!(open.contains(&pair_id(p).as_str()), "{:?}", open);
    }
    for p in ["req:p08", "req:p09", "req:p10", "req:p11"] {
        let w = plan
            .waiting
            .get(&pair_id(p))
            .unwrap_or_else(|| panic!("{} waits", p));
        assert_eq!(w.score, 3);
        assert_eq!(w.full, vec!["req:hub".to_string()]);
        assert_eq!(w.cause.generation, 5);
    }
    let degree = plan
        .open
        .iter()
        .filter(|g| pair_members(&g.target).is_some_and(|(a, b)| a == "req:hub" || b == "req:hub"))
        .count();
    assert_eq!(degree, limits::PAIRS_PER_REQUIREMENT_CAP);
    let g = plan
        .open
        .iter()
        .find(|g| g.id == pair_id("req:p01"))
        .unwrap();
    assert!(
        g.hints
            .iter()
            .any(|h| h == "score 3; req:hub: 4 pair(s) waiting, req:p01: none waiting"),
        "{:?}",
        g.hints
    );
    let g12 = plan
        .open
        .iter()
        .find(|g| g.id == pair_id("req:p12"))
        .unwrap();
    assert!(
        g12.hints.iter().any(|h| h.starts_with("score 4;")),
        "{:?}",
        g12.hints
    );
    // The board lists the waiting pairs and explains one; none of them is a goal.
    let b = crate::board::Board::derive(
        &s,
        &crate::board::tests::project_for(&s),
        &crate::board::tests::control_auto(),
    );
    assert_eq!(b.waiting.len(), 4);
    assert!(b.goal(&pair_id("req:p08")).is_none());
    assert!(b.waiting_pair("req:hub~req:p08").is_some());
    let text = b
        .explain(&s, &pair_id("req:p08"))
        .expect("explains a waiting pair");
    assert!(text.contains("waiting under the cap"), "{}", text);
    assert!(text.contains("req:hub hold(s) 8 open pairs"), "{}", text);
    assert!(b.summary_line().contains("4 pair(s) waiting under the cap"));
}

// (c) A pinned pair opens even when its side is saturated: an open contradiction
// ties the hub to req:p11, which would otherwise wait, and the slot it takes
// sends req:p07 to waiting instead. Mirrors docs/compiler/reconciler.md#pairs.
#[test]
fn a_pinned_pair_opens_on_a_saturated_side() {
    let mut s = fanned_store();
    s.graph.diagnostics.insert(
        "diag:contradiction-1".into(),
        Diagnostic {
            rule: "contradiction".into(),
            severity: "error".into(),
            subjects: vec!["req:hub".into(), "req:p11".into()],
            message: "cannot both hold".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    let plan = pair_plan(&s);
    let open: Vec<&str> = plan.open.iter().map(|g| g.id.as_str()).collect();
    assert!(open.contains(&pair_id("req:p11").as_str()), "{:?}", open);
    assert!(open.contains(&pair_id("req:p12").as_str()), "{:?}", open);
    assert_eq!(open.len(), limits::PAIRS_PER_REQUIREMENT_CAP, "{:?}", open);
    assert!(plan.waiting.contains_key(&pair_id("req:p07")));
    assert!(!plan.waiting.contains_key(&pair_id("req:p11")));
    // A failed pair is pinned too and holds no slot: failing req:p08's pair
    // keeps it on the board without costing req:p07 its slot back.
    s.status.failed.push(FailedGoal {
        goal: plan
            .open
            .iter()
            .find(|g| g.id == pair_id("req:p01"))
            .cloned()
            .map(|mut g| {
                g.id = pair_id("req:p08");
                g.target = pair_target("req:hub", "req:p08");
                g
            })
            .unwrap(),
        reason: "undecidable".into(),
    });
    let plan = pair_plan(&s);
    let open: Vec<&str> = plan.open.iter().map(|g| g.id.as_str()).collect();
    assert!(open.contains(&pair_id("req:p08").as_str()), "{:?}", open);
    assert_eq!(
        open.len(),
        limits::PAIRS_PER_REQUIREMENT_CAP + 1,
        "{:?}",
        open
    );
    assert!(
        plan.waiting.contains_key(&pair_id("req:p07")),
        "{:?}",
        plan.waiting.keys()
    );
}

// (e) A judged partner derives no pair from the record that lists it; a new
// record on the same requirement starts with an empty `judged`, so the pair
// derives again with the new cause. Mirrors docs/compiler/graph.md#change-records.
#[test]
fn a_re_revised_requirement_starts_with_an_empty_judged_list() {
    let mut s = fanned_store();
    for c in s
        .status
        .changes
        .iter_mut()
        .filter(|c| c.subject == "req:p01")
    {
        c.add_judged("req:hub");
        c.add_judged("req:hub");
        assert_eq!(c.judged(), vec!["req:hub".to_string()]);
    }
    let plan = pair_plan(&s);
    assert!(plan.open.iter().all(|g| g.id != pair_id("req:p01")));
    assert!(!plan.waiting.contains_key(&pair_id("req:p01")));
    // The slot freed opens the next pair by score and target.
    assert!(plan.open.iter().any(|g| g.id == pair_id("req:p08")));
    // Revised again: the new record supersedes nothing of the created kind but
    // carries an empty list, and the pair derives from it with the new cause.
    let mut batch = store::RecordBatch::new(6);
    batch.push(
        0,
        store::CHANGE_REQ_REVISED,
        "req:p01",
        "fields",
        Value::Null,
    );
    for r in batch.records() {
        s.status.record_change(r.clone());
    }
    let revised = s
        .status
        .changes
        .iter()
        .find(|c| c.kind == store::CHANGE_REQ_REVISED && c.subject == "req:p01")
        .unwrap();
    assert!(revised.judged().is_empty());
    let plan = pair_plan(&s);
    let g = plan
        .open
        .iter()
        .find(|g| g.id == pair_id("req:p01"))
        .expect("the pair derives again");
    assert_eq!(g.cause.as_ref().map(|c| c.generation), Some(6));
    assert_eq!(g.change["revised"], "req:p01");
    // A second revision supersedes the first record; its list is empty again.
    for c in s
        .status
        .changes
        .iter_mut()
        .filter(|c| c.subject == "req:p01")
    {
        c.add_judged("req:hub");
    }
    let mut batch = store::RecordBatch::new(7);
    batch.push(
        0,
        store::CHANGE_REQ_REVISED,
        "req:p01",
        "fields",
        Value::Null,
    );
    for r in batch.records() {
        s.status.record_change(r.clone());
    }
    let revised: Vec<&ChangeRecord> = s
        .status
        .changes
        .iter()
        .filter(|c| c.kind == store::CHANGE_REQ_REVISED && c.subject == "req:p01")
        .collect();
    assert_eq!(revised.len(), 1);
    assert_eq!(revised[0].generation, 7);
    assert!(revised[0].judged().is_empty());
}
