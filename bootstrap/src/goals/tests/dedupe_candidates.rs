use super::*;

#[test]
fn lookalike_scores_pair_cross_document_name_variants_only() {
    let mut s = Store::default();
    let ent = |name: &str, docs: &[&str]| Entity {
        name: name.into(),
        mentions: docs
            .iter()
            .map(|d| SourceRef {
                doc: d.to_string(),
                section: "/x".into(),
                quote: name.into(),
            })
            .collect(),
        ..Default::default()
    };
    s.graph
        .entities
        .insert("ent:backend".into(), ent("backend", &["api.md"]));
    s.graph.entities.insert(
        "ent:backend-system".into(),
        ent("backend system", &["deploy.md"]),
    );
    s.graph
        .entities
        .insert("ent:order".into(), ent("Order", &["api.md"]));
    s.graph.entities.insert(
        "ent:reorder-point".into(),
        ent("Reorder point", &["deploy.md"]),
    );
    s.graph
        .entities
        .insert("ent:lonely".into(), ent("backend host", &["api.md"]));
    let scores = lookalike_scores(&s);
    let pair = scores
        .iter()
        .find(|l| l.a == "ent:backend" && l.b == "ent:backend-system")
        .expect("backend and backend system are candidates");
    assert!((pair.tokens - 0.67).abs() < 0.01, "{}", pair.tokens);
    assert_eq!(pair.documents, 0.0);
    assert!((pair.score - 0.5).abs() < 0.01);
    assert!(!scores
        .iter()
        .any(|l| l.a == "ent:order" || l.b == "ent:reorder-point"));
    // Same document only: review-entity's to judge, never a candidate.
    assert!(!scores
        .iter()
        .any(|l| l.a == "ent:backend" && l.b == "ent:lonely"));
}

// A token many names share carries less signal: sixteen `X Service` entities
// mentioned in the same two documents pair with none of each other, while two names
// sharing one distinctive token still pair, and the weights follow the documented
// formula. Mirrors docs/compiler/goals/dedupe-candidates.md#the-lookalike-score.
#[test]
fn a_token_many_names_share_carries_less_signal() {
    let mut s = Store::default();
    let ent = |name: &str, docs: &[&str]| Entity {
        name: name.into(),
        mentions: docs
            .iter()
            .map(|d| SourceRef {
                doc: d.to_string(),
                section: "/x".into(),
                quote: name.into(),
            })
            .collect(),
        ..Default::default()
    };
    let services = [
        "Auth",
        "Catalog",
        "Search",
        "Cart",
        "Checkout",
        "Pricing",
        "Promotion",
        "Review",
        "Billing",
        "Inventory",
        "Shipping",
        "Notification",
        "Analytics",
        "Reporting",
        "Support",
        "Fraud",
    ];
    for name in services {
        let id = format!("ent:{}-service", name.to_lowercase());
        s.graph.entities.insert(
            id,
            ent(&format!("{} Service", name), &["services.md", "storage.md"]),
        );
    }
    for name in [
        "Platform",
        "Data Layer",
        "User Store",
        "Product Store",
        "Order Store",
        "Payment Store",
        "Session Cache",
    ] {
        s.graph.entities.insert(
            format!("ent:{}", crate::md::slug(name)),
            ent(name, &["storage.md"]),
        );
    }
    s.graph.entities.insert(
        "ent:audit-log".into(),
        ent("Audit Log", &["services.md", "storage.md"]),
    );
    s.graph.entities.insert(
        "ent:audit-trail".into(),
        ent("Audit Trail", &["services.md"]),
    );
    assert_eq!(s.graph.entities.len(), 25);
    // The weights: full up to three holders, falling from there.
    assert_eq!(token_weight(25, 1), 1.0);
    assert_eq!(token_weight(25, 2), 1.0);
    assert_eq!(token_weight(25, 3), 1.0);
    assert!(
        (token_weight(25, 16) - 0.42).abs() < 0.01,
        "{}",
        token_weight(25, 16)
    );
    assert_eq!(token_weight(2, 2), 1.0);
    let scores = lookalike_scores(&s);
    let ids: Vec<(&str, &str)> = scores
        .iter()
        .map(|l| (l.a.as_str(), l.b.as_str()))
        .collect();
    assert_eq!(ids, vec![("ent:audit-log", "ent:audit-trail")], "{:?}", ids);
    assert_eq!(
        (scores[0].tokens, scores[0].documents, scores[0].score),
        (0.5, 0.5, 0.5)
    );
    // The same two services alone in a graph: `service` is distinctive and they pair.
    let mut two = Store::default();
    for name in ["Auth", "Billing"] {
        two.graph.entities.insert(
            format!("ent:{}-service", name.to_lowercase()),
            ent(&format!("{} Service", name), &["services.md", "storage.md"]),
        );
    }
    let scores = lookalike_scores(&two);
    assert_eq!(scores.len(), 1);
    assert_eq!(
        (scores[0].tokens, scores[0].documents, scores[0].score),
        (0.5, 1.0, 0.63)
    );
    assert_eq!(lookalike_scores(&s), lookalike_scores_scan(&s));
    assert_eq!(lookalike_scores(&two), lookalike_scores_scan(&two));
}

// ---- verdicts are remembered ----

// A store on disk holding "backend" (api.md) and "backend system" (deploy.md): a
// lookalike pair at the threshold.
fn lookalike_store(label: &str) -> Store {
    let out = std::env::temp_dir().join(format!(
        "jazyk-lookalikes-{}-{}",
        label,
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(&out).unwrap();
    let mut s = Store {
        out,
        ..Default::default()
    };
    // Each mention is backed by its document, so the sweep keeps the entities.
    for (doc, text) in [
        (
            "api.md",
            "# X\n\nThe backend serves requests. The backend system is deployed nightly.\n",
        ),
        (
            "deploy.md",
            "# X\n\nThe backend system is deployed nightly.\n",
        ),
    ] {
        s.docs.insert(
            doc.to_string(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
    }
    let ent = |name: &str, docs: &[&str]| Entity {
        name: name.into(),
        mentions: docs
            .iter()
            .map(|d| SourceRef {
                doc: d.to_string(),
                section: "/x".into(),
                quote: name.into(),
            })
            .collect(),
        ..Default::default()
    };
    s.graph
        .entities
        .insert("ent:backend".into(), ent("backend", &["api.md"]));
    s.graph.entities.insert(
        "ent:backend-system".into(),
        ent("backend system", &["deploy.md"]),
    );
    s
}

fn session_resolving(goal: &str, evidence: Value) -> store::Commit {
    store::Commit {
        kind: "session".into(),
        batch: vec![goal.to_string()],
        resolved: vec![Resolved {
            goal: goal.to_string(),
            justification: "judged".into(),
            evidence,
        }],
        ..Default::default()
    }
}

const PAIR: &str = "ent:backend~ent:backend-system";
const DEDUPE: &str = "g:dedupe-candidates:ent:backend~ent:backend-system";

// The name index writes the pair's record at commit; the goal derives with that
// record as its cause. Resolving it `separate` writes the entry at the pair's
// score, clears the record, and the pair derives nothing more: a no-op rebuild
// derives zero goals, from the store in memory and from disk.
// Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
#[test]
fn a_separate_dedupe_verdict_is_remembered_and_the_pair_is_not_asked_again() {
    let mut s = lookalike_store("dedupe");
    s.apply(vec![], &store::Commit::store("edit"));
    let record = s
        .status
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_LOOKALIKE)
        .expect("a lookalike record");
    assert_eq!((record.subject.as_str(), record.generation), (PAIR, 1));
    assert_eq!(record.detail["score"], 0.5);
    let goals = DedupeCandidates.derive_goals(&s, &gen_settings());
    assert_eq!(goals.len(), 1);
    assert_eq!(goals[0].id, DEDUPE);
    assert_eq!(goals[0].cause.as_ref().map(|c| c.generation), Some(1));

    s.apply(vec![], &session_resolving(DEDUPE, json!("separate")));
    assert_eq!(
        s.status.lookalikes,
        vec![LookalikeVerdict {
            pair: PAIR.into(),
            verdict: "separate".into(),
            score: 0.5,
            generation: 2
        }]
    );
    // The generation is local and never written: a load reads the verdict without it.
    let unstamped = |v: &[LookalikeVerdict]| -> Vec<(String, String)> {
        v.iter()
            .map(|l| (l.pair.clone(), l.verdict.clone()))
            .collect()
    };
    assert!(!s.status.has_change(CHANGE_LOOKALIKE, PAIR));
    assert!(DedupeCandidates
        .derive_goals(&s, &gen_settings())
        .is_empty());
    // A later commit writes no record and derives nothing; the list persists.
    s.apply(vec![], &store::Commit::store("edit"));
    assert!(!s.status.has_change(CHANGE_LOOKALIKE, PAIR));
    let loaded = Store::load(&s.out);
    assert_eq!(
        unstamped(&loaded.status.lookalikes),
        unstamped(&s.status.lookalikes)
    );
    assert!(DedupeCandidates
        .derive_goals(&loaded, &gen_settings())
        .is_empty());
    // The `{verdict}` shape counts too.
    let mut t = lookalike_store("dedupe-shape");
    t.apply(
        vec![],
        &session_resolving(DEDUPE, json!({"verdict": "separate"})),
    );
    assert_eq!(t.status.lookalikes.len(), 1);
    crate::vfs::remove_dir_all(&s.out).ok();
    crate::vfs::remove_dir_all(&t.out).ok();
}

// A review-entity session that kept a lookalike apart in its evidence writes the
// same entry, so dedupe-candidates never asks about the pair. A merged verdict
// writes nothing: the merge itself takes the pair away.
// Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
#[test]
fn a_separate_review_verdict_suppresses_the_dedupe_goal() {
    let mut s = lookalike_store("review");
    let evidence = json!({
        "definition": "current",
        "lookalikes": [
            {"id": "ent:backend-system", "verdict": "separate", "reason": "a deployment unit"},
        ],
    });
    s.apply(
        vec![],
        &session_resolving("g:review-entity:ent:backend", evidence),
    );
    assert_eq!(s.status.lookalikes.len(), 1, "{:?}", s.status.lookalikes);
    assert_eq!(
        (
            s.status.lookalikes[0].pair.as_str(),
            s.status.lookalikes[0].score
        ),
        (PAIR, 0.5)
    );
    assert!(!s.status.has_change(CHANGE_LOOKALIKE, PAIR));
    assert!(DedupeCandidates
        .derive_goals(&s, &gen_settings())
        .is_empty());
    let mut t = lookalike_store("review-merged");
    let evidence = json!({"definition": "current", "lookalikes": [{"id": "ent:backend-system", "verdict": "merged"}]});
    t.apply(
        vec![],
        &session_resolving("g:review-entity:ent:backend", evidence),
    );
    assert!(t.status.lookalikes.is_empty());
    crate::vfs::remove_dir_all(&s.out).ok();
    crate::vfs::remove_dir_all(&t.out).ok();
}

// The score moves (a new mention document here): the entry drops at that commit,
// the name index writes a fresh record, and the pair is judged again with the new
// commit as its cause. Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
#[test]
fn a_moved_score_drops_the_entry_and_reopens_the_pair() {
    let mut s = lookalike_store("moved");
    s.apply(vec![], &session_resolving(DEDUPE, json!("separate")));
    assert_eq!(s.status.lookalikes.len(), 1);
    // The same evidence again: nothing moves.
    s.apply(vec![], &store::Commit::store("edit"));
    assert_eq!(s.status.lookalikes.len(), 1);
    assert!(DedupeCandidates
        .derive_goals(&s, &gen_settings())
        .is_empty());
    s.graph
        .entities
        .get_mut("ent:backend-system")
        .unwrap()
        .mentions
        .push(SourceRef {
            doc: "api.md".into(),
            section: "/x".into(),
            quote: "backend system".into(),
        });
    s.apply(vec![], &store::Commit::store("edit"));
    assert!(s.status.lookalikes.is_empty(), "{:?}", s.status.lookalikes);
    let record = s
        .status
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_LOOKALIKE)
        .expect("a fresh record");
    assert_eq!((record.subject.as_str(), record.generation), (PAIR, 3));
    assert_eq!(record.detail["documents"], 0.5);
    let goals = DedupeCandidates.derive_goals(&s, &gen_settings());
    assert_eq!(goals.len(), 1);
    assert_eq!(goals[0].change["score"], 0.63);
    assert_eq!(goals[0].cause.as_ref().map(|c| c.generation), Some(3));
    crate::vfs::remove_dir_all(&s.out).ok();
}

// A merge takes one id with it: the entry and any record drop at the same commit.
// Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
#[test]
fn a_merge_drops_the_entry_and_the_record() {
    let absorb = || Op::MergeEntities {
        keep: "ent:backend".into(),
        absorb: "ent:backend-system".into(),
        reason: "one concept".into(),
    };
    let mut s = lookalike_store("merge");
    s.apply(vec![], &session_resolving(DEDUPE, json!("separate")));
    assert_eq!(s.status.lookalikes.len(), 1);
    let report = s.apply(vec![absorb()], &store::Commit::store("decree"));
    assert_eq!(report.applied, 1, "{:?}", report.skipped);
    assert!(s.status.lookalikes.is_empty(), "{:?}", s.status.lookalikes);
    assert!(!s.status.changes.iter().any(|c| c.kind == CHANGE_LOOKALIKE));
    assert!(DedupeCandidates
        .derive_goals(&s, &gen_settings())
        .is_empty());
    // A record with no verdict drops the same way.
    let mut t = lookalike_store("merge-record");
    t.apply(vec![], &store::Commit::store("edit"));
    assert!(t.status.has_change(CHANGE_LOOKALIKE, PAIR));
    t.apply(vec![absorb()], &store::Commit::store("decree"));
    assert!(!t.status.has_change(CHANGE_LOOKALIKE, PAIR));
    crate::vfs::remove_dir_all(&s.out).ok();
    crate::vfs::remove_dir_all(&t.out).ok();
}

// The token index names every pair the pairwise scan scores, in the scan's order,
// with the scan's scores. Mirrors docs/compiler/graph.md#derived-data.
#[test]
fn the_lookalike_token_index_matches_the_pairwise_scan() {
    let mut synthetic = crate::derive::tests::synthetic_store(300, 700);
    let mut batch = store::RecordBatch::new(1);
    crate::derive::recompute(&mut synthetic, "g1", &mut batch);
    let mut s = Store::default();
    let ent = |name: &str, docs: &[&str]| Entity {
        name: name.into(),
        mentions: docs
            .iter()
            .map(|d| SourceRef {
                doc: d.to_string(),
                section: "/x".into(),
                quote: name.into(),
            })
            .collect(),
        ..Default::default()
    };
    s.graph
        .entities
        .insert("ent:backend".into(), ent("backend", &["api.md"]));
    s.graph.entities.insert(
        "ent:backend-system".into(),
        ent("backend system", &["deploy.md"]),
    );
    s.graph.entities.insert(
        "ent:backend-host".into(),
        ent("backend host", &["api.md", "deploy.md"]),
    );
    s.graph
        .entities
        .insert("ent:order".into(), ent("Order", &["api.md"]));
    s.graph.entities.insert(
        "ent:reorder-point".into(),
        ent("Reorder point", &["deploy.md"]),
    );
    for s in [crate::board::tests::settled_store(), synthetic, s] {
        let indexed = lookalike_scores(&s);
        assert_eq!(indexed, lookalike_scores_scan(&s));
    }
}
