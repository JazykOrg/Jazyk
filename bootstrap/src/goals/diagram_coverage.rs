use super::*;

pub struct DiagramCoverage;
impl GoalKind for DiagramCoverage {
    fn kind(&self) -> &'static str {
        "diagram-coverage"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "area"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        crate::diagrams::findings(store)
            .into_iter()
            .filter(|f| !f.covered(store) && !f.suppressed(store))
            .map(|f| {
                let mut data = f.value();
                data["areaHash"] = json!(f.hash());
                data["entities"] = json!(crate::diagrams::descendants(store, &f.target));
                let mut hints = vec![format!("load {}", f.target), "skill diagrams".into()];
                hints.extend(
                    f.candidates
                        .keys()
                        .filter(|id| {
                            store.graph.entities.contains_key(*id)
                                || store.graph.requirements.contains_key(*id)
                                || store.graph.criteria.contains_key(*id)
                                || crate::model::split_section_ref(id).is_some()
                        })
                        .map(|id| format!("load {}", id)),
                );
                build_goal(self, &f.id, f.count() > f.hard, data, None, hints)
            })
            .collect()
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        let upstream = tier3_ready(board);
        if !upstream.is_ready() {
            return upstream;
        }
        if let Some(target) = goal.change["target"].as_str() {
            if board.open_kind_on("abstract-entity", target) {
                return Ready::Blocked("Structural review runs before diagram review".into());
            }
        }
        Ready::Ready
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        batch
            .iter()
            .flat_map(|g| g.hints.iter())
            .filter_map(|h| h.strip_prefix("load "))
            .map(|t| format!("- {} stub", t))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "upsert_view",
            "update_view",
            "delete_view",
            "suppress_diagram_coverage",
            "unsuppress_diagram_coverage",
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        view_gates(store, staged)
    }
    fn goal_gate(&self, cx: &GateContext) -> Option<Violation> {
        let finding = crate::diagrams::findings(cx.store)
            .into_iter()
            .find(|f| f.id == cx.goal.target)?;
        // Validate the staged definition against the snapshot without persisting or rendering.
        let mut simulated = cx.store.clone();
        for op in cx.staged {
            match op {
                Op::CreateView { id, view } => {
                    simulated.graph.views.insert(id.clone(), view.clone());
                }
                Op::UpdateView {
                    id,
                    authored,
                    title,
                    clear_query,
                    clear_excluded,
                    query,
                    collapse,
                    exclude,
                    members,
                    add_members,
                    remove_members,
                    ..
                } => {
                    if let Some(v) = simulated.graph.views.get_mut(id) {
                        for (key, value) in authored {
                            if value.is_null() {
                                v.authored.remove(key);
                            } else {
                                v.authored.insert(key.clone(), value.clone());
                            }
                        }
                        if let Some(title) = title {
                            v.title = title.clone();
                        }
                        if *clear_query {
                            v.query = None;
                        } else if let Some(query) = query {
                            v.query = Some(query.clone());
                        }
                        if let Some(collapse) = collapse {
                            v.collapse = collapse.clone();
                        }
                        if *clear_excluded {
                            v.excluded.clear();
                        }
                        for exclusion in exclude {
                            if let Some(old) = v.excluded.iter_mut().find(|x| x.id == exclusion.id)
                            {
                                *old = exclusion.clone();
                            } else {
                                v.excluded.push(exclusion.clone());
                            }
                        }
                        if let Some(ms) = members {
                            v.members = ms.clone();
                        }
                        for m in add_members {
                            if !v.members.contains(m) {
                                v.members.push(m.clone());
                            }
                        }
                        v.members.retain(|m| !remove_members.contains(m));
                    }
                }
                Op::SetViewProvenance { id, provenance } => {
                    if let Some(v) = simulated.graph.views.get_mut(id) {
                        v.provenance = Some(provenance.clone());
                    }
                }
                Op::RatifyProvenance { id, source } => {
                    if let Some(v) = simulated.graph.views.get_mut(id) {
                        v.provenance = Some(Provenance::Quote(source.clone()));
                    }
                }
                Op::DeleteView { id, .. } => {
                    simulated.graph.views.remove(id);
                }
                Op::SetDiagramDecision {
                    id,
                    decision: Some(d),
                } => {
                    simulated
                        .decisions
                        .diagram_coverage
                        .insert(id.clone(), d.clone());
                }
                Op::SetDiagramDecision { id, decision: None } => {
                    simulated.decisions.diagram_coverage.remove(id);
                }
                _ => {}
            }
        }
        if finding.covered(&simulated) || finding.suppressed(&simulated) {
            None
        } else {
            Some(Violation::new(
                "diagram-coverage",
                format!(
                    "{} still needs an authored explanation or current suppression",
                    finding.id
                ),
            ))
        }
    }
    fn prompt(&self) -> &'static str {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/goals/diagram-coverage.md"
        ))
    }
}
