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
        for rec in records_of(store, &[store::CHANGE_QUERY_MATCH]) {
            if store.graph.views.contains_key(&rec.subject) {
                by_view.entry(rec.subject.clone()).or_default().push(rec);
            }
        }
        by_view
            .into_iter()
            .map(|(vid, recs)| {
                let mut matched = Vec::new();
                let mut hints = vec![format!("load {}", vid)];
                for rec in &recs {
                    for id in str_list(&rec.detail["added"]) {
                        matched.push(json!({"id":id,"why":"query-match"}));
                        hints.push(format!("load {} (query match)", id));
                    }
                }
                for skill in skills_for("curate-view", store, &vid) {
                    hints.push(format!("skill {}", skill));
                }
                build_goal(
                    self,
                    &vid,
                    false,
                    json!({"matched":matched}),
                    earliest_cause(&recs),
                    hints,
                )
            })
            .collect()
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
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/goals/curate-view.md"
        ))
    }
}

pub(super) fn view_gates(_store: &Store, staged: &[Op]) -> Vec<Violation> {
    let mut out = Vec::new();
    const PLACEHOLDERS: [&str; 5] = ["none", "n/a", "na", "-", "null"];
    for op in staged {
        match op {
            Op::DeleteView { id, reason } => {
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
