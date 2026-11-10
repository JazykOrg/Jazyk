use super::*;

// ---- GC: curate-view ----

pub struct CurateView;

impl GoalKind for CurateView {
    fn kind(&self) -> &'static str {
        "curate-view"
    }
    fn class(&self) -> Class {
        Class::Gc
    }
    fn unit(&self) -> &'static str {
        "view"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut by_view: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_QUERY_MATCH, CHANGE_FLOW_UNPLACED]) {
            if store.graph.views.contains_key(&rec.subject) {
                by_view.entry(rec.subject.clone()).or_default().push(rec);
            }
        }
        let mut out = Vec::new();
        for (vid, recs) in by_view {
            let v = &store.graph.views[&vid];
            let mut matched: Vec<Value> = Vec::new();
            let mut hints = vec![format!("load {}", vid)];
            for r in &recs {
                if r.kind == store::CHANGE_QUERY_MATCH {
                    for id in str_list(&r.detail["added"]) {
                        matched.push(json!({"id": id, "why": "query-match"}));
                        hints.push(format!("load {} (query match)", id));
                    }
                } else {
                    let rid = r.detail["requirement"].as_str().unwrap_or("").to_string();
                    let facet = r.detail["facet"].as_str().unwrap_or("").to_string();
                    let rec_shared = str_list(&r.detail["shared"]);
                    let why = if facet == "failure-mode" {
                        "unrepresented failure mode"
                    } else {
                        "unplaced behavior"
                    };
                    matched.push(json!({"id": rid, "why": why, "facet": facet, "diagnostic": r.detail["diagnostic"]}));
                    hints.push(format!("load {} ({})", rid, why));
                    for alt in str_list(&r.detail["alternatives"]).iter().take(3) {
                        let shared = store
                            .graph
                            .views
                            .get(alt)
                            .map(|av| {
                                let ents: BTreeSet<String> = av
                                    .members
                                    .iter()
                                    .filter_map(|m| store.graph.requirements.get(m))
                                    .flat_map(|q| q.entities.iter().cloned())
                                    .collect();
                                rec_shared.iter().filter(|e| ents.contains(*e)).count()
                            })
                            .unwrap_or(0);
                        hints.push(format!("alternative {} (shares {} entities)", alt, shared));
                    }
                    if facet == "failure-mode" {
                        if let Some(req) = store.graph.requirements.get(&rid) {
                            if let Some(t) = req.transition.as_ref().and_then(|t| t.trigger.clone())
                            {
                                let after = v
                                    .members
                                    .iter()
                                    .find(|m| {
                                        store.graph.requirements.get(*m).is_some_and(|q| {
                                            q.transition.as_ref().and_then(|x| x.trigger.as_ref())
                                                == Some(&t)
                                        })
                                    })
                                    .cloned();
                                if let Some(a) = after {
                                    hints.push(format!("after {} ({})", a, t));
                                }
                            }
                        }
                    }
                }
            }
            let cause = earliest_cause(&recs);
            let change = json!({"matched": matched});
            for s in skills_for("curate-view", store, &vid) {
                hints.push(format!("skill {}", s));
            }
            hints.push("update_view add_members or exclude".into());
            out.push(build_goal(self, &vid, false, change, cause, hints));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        gc_ready(goal, board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            if let Some(m) = g.change["matched"].as_array() {
                for x in m {
                    if let Some(id) = x["id"].as_str() {
                        lines.push(format!("- {} full", id));
                    }
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &["upsert_view", "update_view", "delete_view"]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        view_gates(store, staged)
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../../docs/compiler/goals/prompts/curate-view.md")
    }
}

pub(super) fn view_gates(store: &Store, staged: &[Op]) -> Vec<Violation> {
    let mut out = Vec::new();
    const PLACEHOLDERS: [&str; 5] = ["none", "n/a", "na", "-", "null"];
    for op in staged {
        match op {
            Op::DeleteView { id, reason } => {
                if store.graph.views.get(id).is_some_and(|v| v.default) {
                    out.push(Violation::new(
                        "default-view",
                        format!("{} is a default view; exclude members or curate it instead of deleting", id),
                    ));
                }
                if reason.trim().is_empty() {
                    out.push(Violation::new(
                        "reason-required",
                        format!("delete_view {} carries no reason", id),
                    ));
                }
            }
            Op::UpdateView { id, exclude, .. } => {
                for x in exclude {
                    let n = x.note.trim().to_lowercase();
                    if n.is_empty() || PLACEHOLDERS.contains(&n.as_str()) {
                        out.push(Violation::new(
                            "note-required",
                            format!(
                                "{}: excluding {} needs a note naming the sentence or the rule",
                                id, x.id
                            ),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    out
}
