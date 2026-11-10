use super::*;

// ---- GC: declare-edges ----

pub struct DeclareEdges;

pub(super) fn gc_ready(goal: &Goal, board: &Board) -> Ready {
    match board.cone_blockers(&goal.id) {
        v if v.is_empty() => Ready::Ready,
        v => Ready::Blocked(format!(
            "compile goal(s) open in the cone: {}",
            v.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
        )),
    }
}

impl GoalKind for DeclareEdges {
    fn kind(&self) -> &'static str {
        "declare-edges"
    }
    fn class(&self) -> Class {
        Class::Gc
    }
    fn unit(&self) -> &'static str {
        "requirement"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut out = Vec::new();
        for rec in records_of(store, &[store::CHANGE_EDGES_MISSING]) {
            let Some(r) = store.graph.requirements.get(&rec.subject) else {
                continue;
            };
            if r.entities.len() < 2 || !r.edges.is_empty() {
                continue;
            }
            let ents: Vec<String> = r
                .entities
                .iter()
                .map(|e| store.resolve_id(e).to_string())
                .collect();
            let change = json!({"entities": ents, "edges": 0});
            let mut hints = vec![
                format!("load {}", rec.subject),
                format!("{} entities, no edges (g{})", ents.len(), rec.generation),
            ];
            for e in &ents {
                hints.push(format!("load {}", e));
            }
            for rel in store.graph.relationships.values() {
                if rel.members.len() == 2 && rel.members.iter().all(|m| ents.contains(m)) {
                    hints.push(format!(
                        "related {}~{}: {} ({})",
                        rel.members[0],
                        rel.members[1],
                        rel.strongest(),
                        rel.requirements().len()
                    ));
                }
            }
            hints.push("skill edges; update_requirement passing only id and edges".into());
            out.push(build_goal(
                self,
                &rec.subject,
                false,
                change,
                Some(rec.cause()),
                hints,
            ));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        gc_ready(goal, board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        let mut seen = BTreeSet::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            for e in str_list(&g.change["entities"]) {
                if seen.insert(e.clone()) {
                    lines.push(format!("- {} stub", e));
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &["update_requirement"]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = Vec::new();
        // The edges each requirement ends with: a later update on the same id replaces
        // the edges an earlier one staged, so a repaired call is judged and the
        // superseded one is not. Mirrors docs/compiler/goals/declare-edges.md#gate.
        let mut effective: BTreeMap<&str, &Vec<ReqEdge>> = BTreeMap::new();
        for op in staged {
            if let Op::UpdateRequirement {
                id,
                edges: Some(edges),
                ..
            } = op
            {
                effective.insert(id.as_str(), edges);
            }
        }
        for op in staged {
            if let Op::UpdateRequirement {
                id,
                statement,
                entities,
                transition,
                facets,
                source,
                ..
            } = op
            {
                for (field, present) in [
                    ("statement", statement.is_some()),
                    ("entities", entities.is_some()),
                    ("transition", transition.is_some()),
                    ("facets", facets.is_some()),
                    ("section and quote", source.is_some()),
                ] {
                    if present {
                        out.push(Violation::new(
                            "field-not-this-goals",
                            format!(
                                "{}: {} is not this goal's to change; pass only id and edges",
                                id, field
                            ),
                        ));
                    }
                }
            }
        }
        for (id, edges) in effective {
            {
                {
                    let listed: Vec<String> = store
                        .graph
                        .requirements
                        .get(id)
                        .map(|r| {
                            r.entities
                                .iter()
                                .map(|e| store.resolve_id(e).to_string())
                                .collect()
                        })
                        .unwrap_or_default();
                    for e in edges {
                        if e.rel_type.is_none() {
                            out.push(Violation::new(
                                "untyped-edge",
                                format!(
                                    "{}: edge {}~{} has no type; judging the type is the goal",
                                    id, e.a, e.b
                                ),
                            ));
                        }
                        if e.a == e.b {
                            out.push(Violation::new(
                                "bad-edge",
                                format!("{}: edge {}~{} ties an entity to itself", id, e.a, e.b),
                            ));
                        }
                        for end in [&e.a, &e.b] {
                            if !listed.contains(&store.resolve_id(end).to_string()) {
                                out.push(Violation::new(
                                    "bad-edge",
                                    format!(
                                        "{}: edge end {} is not among the requirement's entities",
                                        id, end
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../../docs/compiler/goals/prompts/declare-edges.md")
    }
}
