use super::*;

// ---- the per-goal gate helpers, over the store plus the staged work ----

// Proposals a place-anchors goal leaves undecided: neither placed nor orphaned.
pub fn undecided_proposals(staged: &[Op], proposals: &[String]) -> Vec<String> {
    proposals
        .iter()
        .filter(|a| {
            !staged.iter().any(|o| {
                matches!(o, Op::PlaceAnchor { id, .. } | Op::OrphanAnchor { id, .. } if id == *a)
            })
        })
        .cloned()
        .collect()
}

// Stale anchors are a contract. Each must be re-anchored (its quote locates
// again), re-recorded under its natural key, revised, or deleted. An anchor
// flagged for re-evaluation owes a decision even though its quote locates.
pub fn untouched_stale(store: &Store, staged: &[Op], anchors: &[String]) -> Vec<String> {
    let mut untouched: Vec<String> = Vec::new();
    for a in anchors {
        let Some(r) = store.graph.requirements.get(a) else {
            continue;
        };
        let Some(src) = r.source.as_ref() else {
            continue;
        };
        if store.quote_locates(&src.doc, &src.section, &src.quote)
            && !store.status.reevaluate.contains(a)
        {
            continue;
        }
        let addressed = staged.iter().any(|o| match o {
            Op::UpdateRequirement { id, .. } | Op::DeleteRequirement { id, .. } => id == a,
            // A staged create that resolved to the anchor carries its id; the
            // statement-equality fallback covers pre-resolution stages.
            Op::CreateRequirement { id, requirement } => {
                id == a
                    || (requirement.anchored_at(&src.doc, &src.section)
                        && store::normalize_statement(&requirement.statement)
                            == store::normalize_statement(&r.statement))
            }
            _ => false,
        });
        if !addressed {
            untouched.push(a.clone());
        }
    }
    untouched
}

// ---- place-anchors ----

pub struct PlaceAnchors;

impl GoalKind for PlaceAnchors {
    fn kind(&self) -> &'static str {
        "place-anchors"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "document"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut out = Vec::new();
        for rec in records_of(store, &[store::CHANGE_ALIGNMENT_PENDING]) {
            let Some(block) = store.status.alignment.iter().find(|b| b.doc == rec.subject) else {
                continue;
            };
            if block.proposals.is_empty() {
                continue;
            }
            let mut ops: BTreeMap<String, usize> = BTreeMap::new();
            for c in &block.changes {
                *ops.entry(c.op.clone()).or_insert(0) += 1;
            }
            let anchors: BTreeSet<String> =
                block.proposals.iter().map(|p| p.anchor.clone()).collect();
            let change = json!({
                "proposals": block.proposals.len(),
                "ops": ops,
                "anchors": anchors,
            });
            let ops_line: Vec<String> = ops.iter().map(|(k, n)| format!("{} {}", n, k)).collect();
            let mut hints = vec![format!(
                "{} proposal(s){}; aligned in g{}",
                block.proposals.len(),
                if ops_line.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", ops_line.join(", "))
                },
                rec.generation
            )];
            let mut loads: Vec<String> = Vec::new();
            for p in &block.proposals {
                let kind = if store.graph.requirements.contains_key(&p.anchor) {
                    "requirement"
                } else {
                    "mention"
                };
                let best = p.candidates.first();
                hints.push(format!(
                    "{} ({}): {} candidate(s), quote locates in the best: {}",
                    p.anchor,
                    kind,
                    p.candidates.len(),
                    match best {
                        Some(c) if c.quote_locates => "yes",
                        Some(_) => "no",
                        None => "no candidate (homeless)",
                    }
                ));
                if let Some(c) = best {
                    if !loads.contains(&c.section) && loads.len() < 6 {
                        loads.push(c.section.clone());
                    }
                }
            }
            hints.extend(loads.iter().map(|s| format!("load {}", s)));
            hints.push("place_anchor with reevaluate; orphan_anchor".into());
            out.push(build_goal(
                self,
                &rec.subject,
                true,
                change,
                Some(rec.cause()),
                hints,
            ));
        }
        out
    }
    fn ready(&self, _goal: &Goal, _board: &Board) -> Ready {
        Ready::Ready
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- alignment:{} full", g.target));
            if let Some(block) = store.status.alignment.iter().find(|b| b.doc == g.target) {
                let mut seen = BTreeSet::new();
                for p in &block.proposals {
                    for c in &p.candidates {
                        if seen.insert(c.section.clone()) {
                            lines.push(format!("- {} stub", c.section));
                        }
                    }
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &["place_anchor", "orphan_anchor"]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        // Every proposal of a document the changeset touches is decided.
        let decided: BTreeSet<(String, String)> = staged
            .iter()
            .filter_map(|o| match o {
                Op::PlaceAnchor { id, from, .. } | Op::OrphanAnchor { id, from } => {
                    Some((id.clone(), format!("{}#{}", from.doc, from.section)))
                }
                _ => None,
            })
            .collect();
        let docs: BTreeSet<&str> = staged
            .iter()
            .filter_map(|o| match o {
                Op::PlaceAnchor { from, .. } | Op::OrphanAnchor { from, .. } => {
                    Some(from.doc.as_str())
                }
                _ => None,
            })
            .collect();
        let mut out = Vec::new();
        for b in store
            .status
            .alignment
            .iter()
            .filter(|b| docs.contains(b.doc.as_str()))
        {
            let undecided: Vec<String> = b
                .proposals
                .iter()
                .filter(|p| !decided.contains(&(p.anchor.clone(), p.from.clone())))
                .map(|p| p.anchor.clone())
                .collect();
            if !undecided.is_empty() {
                out.push(Violation::new(
                    "undecided-proposal",
                    format!(
                        "{} still has undecided proposal(s): {}; place_anchor or orphan_anchor each",
                        b.doc,
                        undecided.join(", ")
                    ),
                ));
            }
        }
        out
    }
    // Every proposal of the goal is placed or orphaned.
    // Mirrors docs/compiler/goals/place-anchors.md#gate.
    fn goal_gate(&self, cx: &GateContext) -> Option<Violation> {
        let undecided = undecided_proposals(cx.staged, &cx.goal.proposals);
        (!undecided.is_empty()).then(|| {
            Violation::new(
                "undecided-proposal",
                format!(
                    "{}: proposals left undecided: {}; place_anchor or orphan_anchor each",
                    cx.goal.goal,
                    undecided.join(", ")
                ),
            )
        })
    }
    fn prompt(&self) -> &'static str {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/goals/place-anchors.md"
        ))
    }
}
