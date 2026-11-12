use super::*;

// ---- rejudge-pair ----

pub struct RejudgePair;

fn shared_entity(store: &Store, a: &Requirement, b: &Requirement) -> Option<String> {
    let bs: BTreeSet<&str> = b.entities.iter().map(|e| store.resolve_id(e)).collect();
    a.entities
        .iter()
        .map(|e| store.resolve_id(e))
        .find(|e| bs.contains(e))
        .map(String::from)
}

// A pair the cap holds back: it derives no goal until a slot frees on the full side.
#[derive(Clone, Debug, PartialEq)]
pub struct WaitingPair {
    pub target: String,
    pub score: usize,
    // The side or sides holding `pairs-per-requirement` open pairs.
    pub full: Vec<String>,
    pub cause: Cause,
}

// The pair board: the open pair goals and the pairs waiting under the cap.
#[derive(Debug, Default)]
pub struct PairPlan {
    pub open: Vec<Goal>,
    pub waiting: BTreeMap<String, WaitingPair>,
}

impl PairPlan {
    // The waiting pair for a goal id or a pair target.
    pub fn waiting_for(&self, id_or_target: &str) -> Option<&WaitingPair> {
        let id = if id_or_target.starts_with("g:rejudge-pair:") {
            id_or_target.to_string()
        } else {
            goal_id("rejudge-pair", id_or_target)
        };
        self.waiting.get(&id)
    }
}

// Every candidate pair from the created, revised, and node-deleted records, placed
// in one deterministic order and opened while both sides have a slot: pinned pairs
// first (sticky, a dead diagnostic subject, parked, failed; a failed pair holds no
// slot), then by score, highest first, ties by target. The rest wait and derive no
// goal. Mirrors docs/compiler/reconciler.md#pairs.
pub fn pair_plan(store: &Store) -> PairPlan {
    struct PairWork {
        change: Value,
        cause: Cause,
        hints: Vec<String>,
        score: usize,
        pinned: bool,
    }
    let mut pairs: BTreeMap<String, PairWork> = BTreeMap::new();
    // The scorer's inputs once for the pass (docs/compiler/reconciler.md#pairs).
    let scoring = store.pair_scoring();
    for rec in records_of(
        store,
        &[store::CHANGE_REQ_CREATED, store::CHANGE_REQ_REVISED],
    ) {
        let rid = &rec.subject;
        let Some(req) = store.graph.requirements.get(rid) else {
            continue;
        };
        for (nbr, score) in store.pair_review_candidates_with(rec, &scoring) {
            let Some(other) = store.graph.requirements.get(&nbr) else {
                continue;
            };
            let target = pair_target(rid, &nbr);
            let shared_ent = shared_entity(store, req, other);
            // The words both statements share, minus the shared entities' boilerplate
            // the scorer dropped (docs/compiler/reconciler.md#pairs).
            let boilerplate = store.pair_boilerplate_with(req, other, &scoring);
            let other_stems = content_tokens(store, &other.statement, &other.entities);
            let mut tokens: Vec<String> = Vec::new();
            for (stemmed, word) in content_token_words(store, &req.statement, &req.entities) {
                if other_stems.contains(&stemmed)
                    && !boilerplate.contains(&store::content_stem(&word))
                    && !tokens.contains(&stemmed)
                {
                    tokens.push(stemmed);
                }
            }
            let sticky: Vec<String> = open_diags_naming(store, &[rid, &nbr])
                .into_iter()
                .filter(|(_, d)| {
                    (d.rule == "contradiction" || d.rule == "duplicate-requirement")
                        && d.subjects.iter().any(|s| s == rid)
                        && d.subjects.iter().any(|s| s == &nbr)
                })
                .map(|(id, _)| id.clone())
                .collect();
            let verb = if rec.kind == store::CHANGE_REQ_CREATED {
                "created"
            } else {
                "revised"
            };
            let change = json!({
                verb: rid,
                "fields": rec.detail["fields"],
                "shared": {"entity": shared_ent, "tokens": tokens},
                "sticky": sticky,
            });
            let cause = Cause {
                generation: rec.generation,
                mutation: rec.mutation,
                via: "entities".into(),
            };
            let same_doc = match (req.source.as_ref(), other.source.as_ref()) {
                (Some(a), Some(b)) => a.doc == b.doc,
                _ => false,
            };
            let mut hints = vec![format!(
                "{} {} in g{}{}{}",
                verb,
                rid,
                rec.generation,
                shared_ent
                    .as_ref()
                    .map(|e| format!("; shares {}", e))
                    .unwrap_or_default(),
                if tokens.is_empty() {
                    String::new()
                } else {
                    format!(" and tokens {}", tokens.join(", "))
                }
            )];
            if !sticky.is_empty() {
                hints.push(format!("sticky: {}", sticky.join(", ")));
            }
            hints.push(format!(
                "same document: {}",
                if same_doc {
                    "yes (a duplicate verdict would delete the worse-sourced side)"
                } else {
                    include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/goals-rejudge_pair/no-a-duplicate-verdict-would-keep-both-2fd37.md"
                    ))
                }
            ));
            let diags = open_diags_naming(store, &[rid, &nbr]);
            if diags.is_empty() {
                hints.push("no open diagnostic on either side".into());
            } else {
                // A diagnostic naming both sides is on the pair; one naming one side
                // is context, and the hint says which.
                for (id, d) in diags {
                    let on_pair =
                        d.subjects.iter().any(|s| s == rid) && d.subjects.iter().any(|s| s == &nbr);
                    let where_ = if on_pair {
                        "on the pair".to_string()
                    } else if d.subjects.iter().any(|s| s == rid) {
                        format!("on {} only", rid)
                    } else {
                        format!("on {} only", nbr)
                    };
                    hints.push(format!(
                        "open {} {} ({}, {}): {}",
                        id,
                        where_,
                        d.rule,
                        d.severity,
                        truncate(&d.message, 100)
                    ));
                }
            }
            hints.push(format!("load {}; load {}", rid, nbr));
            if sticky.is_empty() {
                hints.push("skill judgment; duplicate: delete_requirement or report_diagnostic duplicate-requirement; contradiction: report_diagnostic with a prompt; consistent: mark_goal_done alone".into());
            } else {
                // The standing finding is the verdict's carrier: no second filing.
                // Mirrors docs/compiler/goals/rejudge-pair.md#tools.
                hints.push(format!(
                    "skill judgment; {} already stands on the pair: settle_pairs verdict contradiction (or duplicate) carries it and files nothing, resolved closes it when the conflict is gone, consistent is wrong while it stands",
                    sticky.join(", ")
                ));
            }
            let pinned = !sticky.is_empty();
            match pairs.get_mut(&target) {
                // The same pair from the other side: the earliest cause names the
                // goal, the score is the same from either side, a sticky tie pins.
                Some(p) => {
                    p.score = p.score.max(score);
                    p.pinned |= pinned;
                    if (cause.generation, cause.mutation) < (p.cause.generation, p.cause.mutation) {
                        p.change = change;
                        p.cause = cause;
                        p.hints = hints;
                    }
                }
                None => {
                    pairs.insert(
                        target,
                        PairWork {
                            change,
                            cause,
                            hints,
                            score,
                            pinned,
                        },
                    );
                }
            }
        }
    }
    // A surviving subject of an open pair diagnostic whose partner died.
    for rec in records_of(store, &[store::CHANGE_NODE_DELETED]) {
        if rec.via != "subjects" || !rec.subject.starts_with("req:") {
            continue;
        }
        if !store.graph.requirements.contains_key(&rec.subject) {
            continue;
        }
        let deleted = str_list(&rec.detail["deleted"]);
        let Some(dead) = deleted.first() else {
            continue;
        };
        let did = rec.detail["diagnostic"].as_str().unwrap_or("").to_string();
        if !store
            .graph
            .diagnostics
            .get(&did)
            .is_some_and(|d| d.lifecycle == "open")
        {
            continue;
        }
        let target = pair_target(&rec.subject, dead);
        if let Some(p) = pairs.get_mut(&target) {
            p.pinned = true;
            continue;
        }
        let hints = vec![
            format!(
                "{} (deleted in g{}); {} names it",
                dead, rec.generation, did
            ),
            format!("load {}", rec.subject),
            "skill judgment; resolve_diagnostic when the finding died with the requirement, report_diagnostic to refile it".into(),
        ];
        pairs.insert(
            target,
            PairWork {
                change: json!({"deleted": deleted, "diagnostic": did, "survivor": rec.subject}),
                cause: rec.cause(),
                hints,
                score: 0,
                pinned: true,
            },
        );
    }
    // A parked or failed pair is pinned: it stays on the board with its state. A
    // failed pair holds no slot. Mirrors docs/compiler/reconciler.md#parked-and-failed.
    let parked: BTreeSet<&str> = store
        .status
        .parked
        .iter()
        .filter(|g| g.goal.kind == "rejudge-pair")
        .map(|g| g.goal.target.as_str())
        .collect();
    let failed: BTreeSet<&str> = store
        .status
        .failed
        .iter()
        .filter(|f| f.goal.kind == "rejudge-pair")
        .map(|f| f.goal.target.as_str())
        .collect();
    for (target, p) in pairs.iter_mut() {
        if parked.contains(target.as_str()) || failed.contains(target.as_str()) {
            p.pinned = true;
        }
    }

    // The order: pinned by target, then score descending, ties by target.
    let mut order: Vec<(&String, &PairWork)> = pairs.iter().collect();
    order.sort_by(|(ta, a), (tb, b)| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| {
                if a.pinned {
                    std::cmp::Ordering::Equal
                } else {
                    b.score.cmp(&a.score)
                }
            })
            .then_with(|| ta.cmp(tb))
    });
    let cap = limits::PAIRS_PER_REQUIREMENT_CAP;
    let mut degree: BTreeMap<&str, usize> = BTreeMap::new();
    let mut opened: Vec<&String> = Vec::new();
    let mut waiting: BTreeMap<String, WaitingPair> = BTreeMap::new();
    let mut waiting_by_side: BTreeMap<String, usize> = BTreeMap::new();
    for (target, p) in order {
        let Some((a, b)) = pair_members(target) else {
            continue;
        };
        let full: Vec<String> = [a, b]
            .iter()
            .filter(|s| degree.get(**s).copied().unwrap_or(0) >= cap)
            .map(|s| s.to_string())
            .collect();
        if p.pinned || full.is_empty() {
            opened.push(target);
            if !failed.contains(target.as_str()) {
                *degree.entry(a).or_insert(0) += 1;
                *degree.entry(b).or_insert(0) += 1;
            }
        } else {
            for s in [a, b] {
                *waiting_by_side.entry(s.to_string()).or_insert(0) += 1;
            }
            waiting.insert(
                goal_id("rejudge-pair", target),
                WaitingPair {
                    target: target.clone(),
                    score: p.score,
                    full,
                    cause: p.cause.clone(),
                },
            );
        }
    }
    let kind = &RejudgePair;
    let open = opened
        .into_iter()
        .map(|target| {
            let p = &pairs[target];
            let mut hints = p.hints.clone();
            if let Some((a, b)) = pair_members(target) {
                let side = |s: &str| match waiting_by_side.get(s).copied().unwrap_or(0) {
                    0 => format!("{}: none waiting", s),
                    n => format!("{}: {} pair(s) waiting", s, n),
                };
                hints.push(format!("score {}; {}, {}", p.score, side(a), side(b)));
            }
            build_goal(
                kind,
                target,
                true,
                p.change.clone(),
                Some(p.cause.clone()),
                hints,
            )
        })
        .collect();
    PairPlan { open, waiting }
}

impl GoalKind for RejudgePair {
    fn kind(&self) -> &'static str {
        "rejudge-pair"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "pair"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        pair_plan(store).open
    }
    fn ready(&self, _goal: &Goal, board: &Board) -> Ready {
        tier2_ready(board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for g in batch {
            if let Some((a, b)) = pair_members(&g.target) {
                for id in [a, b] {
                    if seen.insert(id.to_string()) {
                        lines.push(format!("- {} full", id));
                    }
                }
            }
            if let Some(e) = g.change["shared"]["entity"].as_str() {
                if seen.insert(e.to_string()) {
                    lines.push(format!("- {} stub", e));
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "update_requirement",
            "delete_requirement",
            "update_entity",
            "report_diagnostic",
            "resolve_diagnostic",
            crate::tools::PAIR_SETTLE_TOOL,
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = Vec::new();
        for op in staged {
            if let Op::ReportDiagnostic { diagnostic, .. } = op {
                for s in &diagnostic.subjects {
                    let exists = store.graph.requirements.contains_key(s)
                        || store.graph.entities.contains_key(s)
                        || staged
                            .iter()
                            .any(|o| matches!(o, Op::CreateRequirement { id, .. } if id == s));
                    if !exists {
                        out.push(Violation::new(
                            "unknown-id",
                            format!("diagnostic subject {} does not exist", s),
                        ));
                    }
                }
            }
            if let Op::DeleteRequirement { id, reason } = op {
                if reason.trim().is_empty() {
                    out.push(Violation::new(
                        "reason-required",
                        format!("delete_requirement {} carries no reason", id),
                    ));
                }
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/goals/rejudge-pair.md"
        ))
    }
}

pub(super) fn tier2_ready(board: &Board) -> Ready {
    if board.tier_open(0) > 0 {
        return Ready::Blocked("place-anchors goals run first (tier 0)".into());
    }
    if board.tier_open(1) > 0 {
        return Ready::Blocked(format!(
            "{} reconcile-section goal(s) run first (tier 1)",
            board.tier_open(1)
        ));
    }
    Ready::Ready
}

pub(super) fn tier3_ready(board: &Board) -> Ready {
    if let Ready::Blocked(r) = tier2_ready(board) {
        return Ready::Blocked(r);
    }
    if board.tier_open(2) > 0 {
        return Ready::Blocked(format!(
            "{} judgment goal(s) run first (tier 2)",
            board.tier_open(2)
        ));
    }
    Ready::Ready
}
