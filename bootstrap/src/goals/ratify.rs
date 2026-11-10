use super::*;

// ---- ratify and answer (blocked on a human) ----

pub struct Ratify;

impl GoalKind for Ratify {
    fn kind(&self) -> &'static str {
        "ratify"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "fact"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut out: BTreeMap<String, Goal> = BTreeMap::new();
        let proposal_for = |subject: &str| -> Option<(String, &Diagnostic)> {
            store
                .graph
                .diagnostics
                .iter()
                .find(|(_, d)| {
                    d.lifecycle == "open"
                        && d.rule == "ratification-pending"
                        && d.subjects.iter().any(|s| s == subject)
                })
                .map(|(id, d)| (id.clone(), d))
        };
        let provenance_of = |subject: &str| -> Option<Provenance> {
            store
                .graph
                .requirements
                .get(subject)
                .and_then(|r| r.provenance.clone())
                .or_else(|| {
                    store
                        .graph
                        .entities
                        .get(subject)
                        .and_then(|e| e.provenance.clone())
                })
                .or_else(|| {
                    store
                        .graph
                        .criteria
                        .get(subject)
                        .and_then(|c| c.provenance.clone())
                })
        };
        let hints_for =
            |subject: &str, prov: Option<&Provenance>, proposal: Option<&(String, &Diagnostic)>| {
                let mut hints = Vec::new();
                match prov {
                    Some(Provenance::Derived { from, reasoning }) => {
                        hints.push(format!(
                            "derived from {}: {}",
                            from.join(", "),
                            truncate(reasoning, 120)
                        ));
                    }
                    Some(Provenance::Decree { author, at, note }) => {
                        hints.push(format!(
                            "decree by {} at {}{}",
                            author,
                            at,
                            note.as_ref()
                                .map(|n| format!(": {}", n))
                                .unwrap_or_default()
                        ));
                    }
                    _ => {}
                }
                match proposal {
                    Some((did, d)) => {
                        if let Some(p) = &d.prompt {
                            hints.push(format!("proposal {}: {}", did, p.question));
                            for o in &p.options {
                                if let Some(e) = &o.edit {
                                    hints.push(format!(
                                        "edit {}#{}: {}",
                                        e.doc,
                                        e.section,
                                        truncate(&e.new_text, 120)
                                    ));
                                }
                            }
                        }
                        hints.push(format!("accept (Apply: the edit) or retract, on {}", did));
                    }
                    None => hints.push(format!("no proposal filed yet for {}", subject)),
                }
                hints
            };
        for rec in records_of(store, &[store::CHANGE_PROVENANCE_PENDING]) {
            let subject = &rec.subject;
            let Some(prov) = provenance_of(subject) else {
                continue;
            };
            if matches!(prov, Provenance::Quote(_)) {
                continue;
            }
            let proposal = proposal_for(subject);
            let change = json!({
                "provenance": prov.kind(),
                "from": match &prov { Provenance::Derived { from, .. } => json!(from), _ => Value::Null },
                "proposal": proposal.as_ref().map(|(id, _)| id.clone()),
            });
            let mut g = build_goal(
                self,
                subject,
                true,
                change,
                Some(rec.cause()),
                hints_for(subject, Some(&prov), proposal.as_ref()),
            );
            g.state = GoalState::Blocked {
                on: "human: ratification".into(),
            };
            out.insert(g.id.clone(), g);
        }
        // Attribute-level proposals carry no node-level record: the open diagnostic
        // alone derives the goal.
        for (did, d) in &store.graph.diagnostics {
            if d.lifecycle != "open" || d.rule != "ratification-pending" {
                continue;
            }
            let Some(subject) = d.subjects.first() else {
                continue;
            };
            let id = goal_id("ratify", subject);
            if out.contains_key(&id) {
                continue;
            }
            if !store.graph.entities.contains_key(subject)
                && !store.graph.requirements.contains_key(subject)
                && !store.graph.criteria.contains_key(subject)
            {
                continue;
            }
            let attribute = store.graph.entities.get(subject).and_then(|e| {
                e.attributes
                    .iter()
                    .find(|a| {
                        !matches!(a.provenance, Provenance::Quote(_)) && d.message.contains(&a.name)
                    })
                    .map(|a| a.name.clone())
            });
            let created = d
                .created
                .as_deref()
                .and_then(|b| b.strip_prefix('g'))
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(store.status.generation);
            let change =
                json!({"provenance": "attribute", "attribute": attribute, "proposal": did});
            let mut g = build_goal(
                self,
                subject,
                true,
                change,
                Some(Cause {
                    generation: created,
                    mutation: 0,
                    via: "provenance".into(),
                }),
                hints_for(subject, None, Some(&(did.clone(), d))),
            );
            g.state = GoalState::Blocked {
                on: "human: ratification".into(),
            };
            out.insert(g.id.clone(), g);
        }
        out.into_values().collect()
    }
    fn ready(&self, _goal: &Goal, _board: &Board) -> Ready {
        Ready::Blocked("awaiting ratification: accept the proposal or retract the fact".into())
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        batch
            .iter()
            .map(|g| format!("- {} full", g.target))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn prompt(&self) -> &'static str {
        ""
    }
}
