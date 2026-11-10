use super::*;

// ---- GC: split-view and abstract-entity ----

pub struct SplitView;

pub(super) struct Crossed {
    pub(super) limit: String,
    pub(super) count: u64,
    pub(super) soft: u64,
    pub(super) hard: u64,
}

pub(super) fn crossings_for(recs: &[&ChangeRecord]) -> Vec<Crossed> {
    recs.iter()
        .map(|r| Crossed {
            limit: r.detail["limit"].as_str().unwrap_or("").to_string(),
            count: r.detail["count"].as_u64().unwrap_or(0),
            soft: r.detail["soft"].as_u64().unwrap_or(0),
            hard: r.detail["hard"].as_u64().unwrap_or(0),
        })
        .collect()
}

pub(super) fn threshold_change(crossed: &[Crossed]) -> Value {
    json!({
        "limits": crossed.iter().map(|c| json!({
            "limit": c.limit, "count": c.count, "soft": c.soft, "hard": c.hard,
            "level": if c.count > c.hard { "hard" } else { "soft" },
        })).collect::<Vec<_>>()
    })
}

// Parent id to its direct children, by id: one scan for every descendant walk of a
// derivation. Mirrors docs/compiler/graph.md#derived-data.
pub(super) fn children_index(store: &Store) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (cid, c) in &store.graph.entities {
        if let Some(p) = c.parent.as_ref() {
            out.entry(p.clone()).or_default().push(cid.clone());
        }
    }
    out
}

pub(super) fn descendants(children: &BTreeMap<String, Vec<String>>, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut frontier = vec![id.to_string()];
    while let Some(p) = frontier.pop() {
        for cid in children.get(&p).into_iter().flatten() {
            if !out.contains(cid) {
                out.push(cid.clone());
                frontier.push(cid.clone());
            }
        }
    }
    out
}

impl GoalKind for SplitView {
    fn kind(&self) -> &'static str {
        "split-view"
    }
    fn class(&self) -> Class {
        Class::Gc
    }
    fn unit(&self) -> &'static str {
        "view"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut by_view: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_THRESHOLD_CROSSED]) {
            if rec.detail["goal"] == "split-view" && store.graph.views.contains_key(&rec.subject) {
                by_view.entry(rec.subject.clone()).or_default().push(rec);
            }
        }
        let mut out = Vec::new();
        let children = children_index(store);
        for (vid, recs) in by_view {
            let v = &store.graph.views[&vid];
            let crossed = crossings_for(&recs);
            let mandatory = crossed.iter().any(|c| c.count > c.hard);
            let cause = earliest_cause(&recs);
            let mut hints = vec![format!("load {}", vid)];
            for c in &crossed {
                hints.push(format!(
                    "{} > {} ({}, soft {}, hard {})",
                    c.count, c.soft, c.limit, c.soft, c.hard
                ));
            }
            if is_flow_kind(&v.kind) {
                let mut last_section: Option<String> = None;
                for m in &v.members {
                    let Some(r) = store.graph.requirements.get(m) else {
                        continue;
                    };
                    let sec = r
                        .source
                        .as_ref()
                        .map(|s| format!("{}#{}", s.doc, s.section));
                    if last_section.is_some() && sec != last_section {
                        hints.push(format!("break before {} (section boundary)", m));
                    }
                    last_section = sec;
                }
                if v.kind == "sequence" || v.kind == "communication" {
                    let parts = crate::derive::flow_participants(store, &v.members);
                    hints.push(format!(
                        "participants: {} ({})",
                        parts.len(),
                        parts.iter().cloned().collect::<Vec<_>>().join(", ")
                    ));
                }
            } else {
                let mut subtree: Vec<(usize, String)> = v
                    .members
                    .iter()
                    .filter(|m| !v.collapse.contains(m))
                    .map(|m| (descendants(&children, m).len(), m.clone()))
                    .filter(|(n, _)| *n > 0)
                    .collect();
                subtree.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                for (n, m) in subtree.iter().take(3) {
                    hints.push(format!("collapse {} ({} descendants hidden)", m, n));
                }
            }
            for (oid, o) in &store.graph.views {
                let linked = o
                    .query
                    .as_ref()
                    .and_then(|q| q.parent.as_ref())
                    .is_some_and(|p| v.collapse.contains(p))
                    || v.excluded.iter().any(|x| x.note.contains(oid.as_str()));
                if linked {
                    hints.push(format!("linked {}", oid));
                }
            }
            for s in skills_for("split-view", store, &vid) {
                hints.push(format!("skill {}", s));
            }
            hints.push("update_view collapse or exclude; upsert_view for sub-views".into());
            out.push(build_goal(
                self,
                &vid,
                mandatory,
                threshold_change(&crossed),
                cause,
                hints,
            ));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        // A level's picture yields to the level's fan-out: grouping is the structural
        // answer, splitting the picture the last resort. Mirrors
        // docs/compiler/goals/split-view.md and docs/compiler/reconciler.md#gc-gating.
        if let Some(level) = board.view_level(&goal.target) {
            let fan_out = format!("g:abstract-entity:{}", level);
            if board.open(&fan_out) {
                return Ready::Blocked(format!(
                    "fan-out first: {} groups this level, and the flow lifts to the groupings",
                    fan_out
                ));
            }
        }
        gc_ready(goal, board)
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            if let Some(v) = store.graph.views.get(&g.target) {
                for m in v.members.iter().take(40) {
                    lines.push(format!("- {} stub", m));
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &["upsert_view", "update_view", "delete_view"]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = view_gates(store, staged);
        // No member lost: a member removed from a view is collapsed, excluded with a
        // note, or a member of a view staged in the same changeset.
        let staged_members: BTreeSet<String> = staged
            .iter()
            .flat_map(|o| match o {
                Op::CreateView { view, .. } => view.members.clone(),
                Op::UpdateView {
                    members: Some(m), ..
                } => m.clone(),
                Op::UpdateView { add_members, .. } => add_members.clone(),
                _ => Vec::new(),
            })
            .collect();
        for op in staged {
            if let Op::UpdateView {
                id,
                members,
                remove_members,
                collapse,
                exclude,
                ..
            } = op
            {
                let Some(v) = store.graph.views.get(id) else {
                    continue;
                };
                let after: Vec<String> = match members {
                    Some(m) => m.clone(),
                    None => v
                        .members
                        .iter()
                        .filter(|m| !remove_members.contains(m))
                        .cloned()
                        .collect(),
                };
                let collapsed: Vec<String> = collapse.clone().unwrap_or_else(|| v.collapse.clone());
                for m in &v.members {
                    let kept = after.contains(m)
                        || staged_members.contains(m)
                        || exclude.iter().any(|x| &x.id == m)
                        || v.excluded.iter().any(|x| &x.id == m)
                        || collapsed.iter().any(|c| store.is_ancestor(c, m));
                    if !kept {
                        out.push(Violation::new(
                            "member-lost",
                            format!("{}: member {} is dropped without a sub-view, a collapse, or an exclusion note", id, m),
                        ));
                    }
                }
            }
        }
        out
    }
    // The target view itself comes under its soft value.
    // Mirrors docs/compiler/goals/split-view.md#gate.
    fn goal_gate(&self, cx: &GateContext) -> Option<Violation> {
        split_view_goal_gate(cx.store, cx.staged, &cx.goal.target)
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../../docs/compiler/goals/prompts/split-view.md")
    }
}
