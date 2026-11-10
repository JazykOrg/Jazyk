use super::*;

// ---- conform-instance ----

pub struct ConformInstance;

// Attribute names a type declares, its generalizations included.
pub fn declared_attributes(store: &Store, ty: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut frontier = vec![ty.to_string()];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    while let Some(t) = frontier.pop() {
        if !seen.insert(t.clone()) {
            continue;
        }
        if let Some(e) = store.graph.entities.get(&t) {
            out.extend(e.attributes.iter().map(|a| a.name.to_lowercase()));
        }
        for rel in store.graph.relationships.values() {
            for c in &rel.contributions {
                if c.r#type == "generalization" && c.a == t {
                    frontier.push(c.b.clone());
                }
            }
        }
    }
    out
}

// The links an instance carries: its outgoing contributions other than the
// instantiation edge, (to, type), as the links hint lists them and the gate holds
// one verdict per `to`. Mirrors docs/compiler/goals/conform-instance.md#gate.
pub fn instance_links(store: &Store, id: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for c in store
        .graph
        .relationships
        .values()
        .flat_map(|r| r.contributions.iter())
    {
        if c.a == id
            && c.r#type != INSTANTIATION
            && !out.iter().any(|(b, t)| b == &c.b && t == &c.r#type)
        {
            out.push((c.b.clone(), c.r#type.clone()));
        }
    }
    out
}

impl GoalKind for ConformInstance {
    fn kind(&self) -> &'static str {
        "conform-instance"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "instance"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let types = crate::derive::instance_types(store);
        let mut by_subject: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_INSTANCE]) {
            if store.graph.entities.contains_key(&rec.subject) && types.contains_key(&rec.subject) {
                by_subject.entry(rec.subject.clone()).or_default().push(rec);
            }
        }
        let mut out = Vec::new();
        for (id, recs) in by_subject {
            let ty = types[&id].clone();
            let vias: BTreeSet<String> = recs.iter().map(|r| r.via.clone()).collect();
            let cause = earliest_cause(&recs);
            let change = json!({"type": ty, "via": vias});
            let inst = &store.graph.entities[&id];
            let mut hints = vec![format!(
                "type {} changed in g{} via {}",
                ty,
                cause.as_ref().map(|c| c.generation).unwrap_or(0),
                vias.iter().cloned().collect::<Vec<_>>().join(", ")
            )];
            let declared: Vec<String> = store
                .graph
                .entities
                .get(&ty)
                .map(|t| {
                    t.attributes
                        .iter()
                        .map(|a| {
                            format!("{}: {}", a.name, a.r#type.as_deref().unwrap_or("untyped"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            // Attributes a generalization of the type declares take a verdict too.
            let own: BTreeSet<String> = store
                .graph
                .entities
                .get(&ty)
                .map(|t| t.attributes.iter().map(|a| a.name.to_lowercase()).collect())
                .unwrap_or_default();
            let inherited: Vec<String> = declared_attributes(store, &ty)
                .into_iter()
                .filter(|n| !own.contains(n))
                .collect();
            hints.push(format!(
                "type declares {}{}",
                if declared.is_empty() {
                    "no attributes".to_string()
                } else {
                    declared.join(", ")
                },
                if inherited.is_empty() {
                    String::new()
                } else {
                    format!("; inherited {}", inherited.join(", "))
                }
            ));
            let values: Vec<String> = inst
                .attributes
                .iter()
                .map(|a| format!("{} = {}", a.name, a.value.as_deref().unwrap_or("?")))
                .collect();
            if !values.is_empty() {
                hints.push(format!("instance values {}", values.join(", ")));
            }
            let links: Vec<String> = instance_links(store, &id)
                .into_iter()
                .map(|(to, ty)| format!("{} ({})", to, ty))
                .collect();
            if !links.is_empty() {
                hints.push(format!("links {}", links.join(", ")));
            }
            let diags = open_diags_naming(store, &[&id]);
            if diags.is_empty() {
                hints.push("no open diagnostic".into());
            } else {
                hints.push(format!(
                    "open: {}",
                    diags
                        .iter()
                        .map(|(did, d)| format!("{} ({})", did, d.rule))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            hints.push(format!("load {}; load {}", id, ty));
            hints.push("skill conformance; update_entity on the instance, update_requirement on the example's requirement, report_diagnostic nonconformant-instance".into());
            out.push(build_goal(self, &id, true, change, cause, hints));
        }
        out
    }
    fn ready(&self, _goal: &Goal, board: &Board) -> Ready {
        tier2_ready(board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            if let Some(t) = g.change["type"].as_str() {
                lines.push(format!("- {} full", t));
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "update_entity",
            "update_requirement",
            "report_diagnostic",
            "resolve_diagnostic",
        ]
    }
    // The type rule needs the goal and its batch: it lives in goal_gate.
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    // The type is never edited from this goal: an update on the instance's type, or on
    // a requirement not naming the instance, is refused unless another goal of the
    // batch owns it (a review of the type, a sibling instance's goal). A not_about-only
    // update is a review annotation and passes.
    // Mirrors docs/compiler/goals/conform-instance.md#gate.
    fn goal_gate(&self, cx: &GateContext) -> Option<Violation> {
        let instance = cx.store.resolve_id(&cx.goal.target).to_string();
        let ty = cx.goal.change["type"]
            .as_str()
            .map(String::from)
            .or_else(|| {
                crate::derive::instance_types(cx.store)
                    .get(&instance)
                    .cloned()
            });
        for op in cx.staged {
            match op {
                Op::UpdateEntity {
                    id,
                    name,
                    definition,
                    add_aliases,
                    remove_aliases,
                    add_mention,
                    stereotype,
                    parent,
                    clear_parent,
                    set_attributes,
                    add_attributes,
                    provenance,
                    ..
                } => {
                    let annotation_only = name.is_none()
                        && definition.is_none()
                        && add_aliases.is_empty()
                        && remove_aliases.is_empty()
                        && add_mention.is_none()
                        && stereotype.is_none()
                        && parent.is_none()
                        && !clear_parent
                        && set_attributes.is_none()
                        && add_attributes.is_empty()
                        && provenance.is_none();
                    let on_type = ty
                        .as_deref()
                        .is_some_and(|t| cx.store.resolve_id(id) == cx.store.resolve_id(t));
                    if on_type && !annotation_only && !cx.owned_elsewhere(id) {
                        return Some(Violation::new(
                            "type-edited",
                            format!(
                                "{}: {} is the type; conform the instance, never the type",
                                cx.goal.goal, id
                            ),
                        ));
                    }
                }
                Op::UpdateRequirement { id, entities, .. } => {
                    let names_instance =
                        |list: &[String]| list.iter().any(|e| cx.store.resolve_id(e) == instance);
                    let stored = cx.store.graph.requirements.get(cx.store.resolve_id(id));
                    let named = stored.is_some_and(|r| names_instance(&r.entities))
                        || entities.as_deref().is_some_and(names_instance);
                    let owned = cx.owned_elsewhere(id)
                        || stored.is_some_and(|r| r.entities.iter().any(|e| cx.owned_elsewhere(e)));
                    if !named && !owned {
                        return Some(Violation::new(
                            "type-edited",
                            format!(
                                "{}: {} does not name {}; conform the instance through its own requirements, never the type's",
                                cx.goal.goal, id, instance
                            ),
                        ));
                    }
                }
                _ => {}
            }
        }
        None
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../../docs/compiler/goals/prompts/conform-instance.md")
    }
}
