use super::*;

// ---- criteria ----

// The requirements of one entity that still owe criteria: none recorded, or a
// derived one gone stale under a statement change. Keyed by the requirement's
// leading entity. Mirrors docs/compiler/goals/derive-criteria.md#created-when.
// A diagnostic that asks a person for the value a requirement's evidence waits on: an
// invented choice, or a contradiction, decision, or ambiguity carrying its prompt.
// Mirrors docs/compiler/goals/derive-criteria.md#gate.
pub fn asks_for_answer(rule: &str, prompt: &Option<DiagnosticPrompt>) -> bool {
    rule == "invented-choice"
        || (prompt.is_some() && matches!(rule, "contradiction" | "decision" | "ambiguity"))
}

// Whether an open question names the requirement: the condition the open-question
// verdict stands on, and what keeps the goal from deriving for it again.
// Mirrors docs/compiler/goals/derive-criteria.md#created-when.
pub fn awaits_answer(store: &Store, rid: &str) -> bool {
    store.graph.diagnostics.values().any(|d| {
        d.lifecycle == "open"
            && asks_for_answer(&d.rule, &d.prompt)
            && d.subjects.iter().any(|s| store.resolve_id(s) == rid)
    })
}

pub fn criteria_owed(store: &Store) -> BTreeMap<String, Vec<(String, &'static str)>> {
    let mut out: BTreeMap<String, Vec<(String, &'static str)>> = BTreeMap::new();
    for (rid, r) in &store.graph.requirements {
        let Some(first) = r.entities.first() else {
            continue;
        };
        let entity = store.resolve_id(first).to_string();
        if !store.graph.entities.contains_key(&entity) {
            continue;
        }
        let crits = store.criteria_of(rid);
        let reason = if crits.is_empty() {
            if awaits_answer(store, rid) {
                continue;
            }
            "none"
        } else if crits
            .iter()
            .any(|c| store.graph.criteria.get(c).is_some_and(|c| c.stale))
        {
            "revised"
        } else {
            continue;
        };
        out.entry(entity).or_default().push((rid.clone(), reason));
    }
    out
}

pub struct DeriveCriteria;

impl GoalKind for DeriveCriteria {
    fn kind(&self) -> &'static str {
        "derive-criteria"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "entity"
    }
    fn derive_goals(&self, store: &Store, gen: &GenSettings) -> Vec<Goal> {
        // No deliverable, no generation scope: stated criteria still enter through
        // extraction and the criterion-missing check reports the gaps.
        if !gen.configured {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (entity, reqs) in criteria_owed(store) {
            let none = reqs.iter().filter(|(_, why)| *why == "none").count();
            let revised = reqs.len() - none;
            let none_ids: Vec<&str> = reqs
                .iter()
                .filter(|(_, w)| *w == "none")
                .map(|(r, _)| r.as_str())
                .collect();
            let revised_ids: Vec<&str> = reqs
                .iter()
                .filter(|(_, w)| *w == "revised")
                .map(|(r, _)| r.as_str())
                .collect();
            let mut hints = vec![format!(
                "requirements: {} ({} without criteria, {} revised)",
                reqs.len(),
                none,
                revised
            )];
            for (rid, why) in &reqs {
                if let Some(r) = store.graph.requirements.get(rid) {
                    let crits = store.criteria_of(rid);
                    hints.push(format!(
                        "{} ({}): {}{}",
                        rid,
                        why,
                        truncate(&r.statement, 120),
                        if crits.is_empty() {
                            String::new()
                        } else {
                            format!("; criteria {}", crits.join(", "))
                        }
                    ));
                }
            }
            // Sections of the requirements' documents that read as examples or cases.
            let docs: BTreeSet<String> = reqs
                .iter()
                .filter_map(|(rid, _)| store.graph.requirements.get(rid))
                .filter_map(|r| r.source.as_ref().map(|s| s.doc.clone()))
                .collect();
            let mut nearby: Vec<String> = Vec::new();
            for doc in &docs {
                if let Some(rec) = store.docs.get(doc) {
                    for (r, sec) in &rec.sections {
                        let t = sec.title.to_lowercase();
                        if t.contains("example") || t.contains("test") || t.contains("case") {
                            nearby.push(format!("{}#{}", doc, r));
                        }
                    }
                }
            }
            if !nearby.is_empty() {
                nearby.sort();
                nearby.truncate(8);
                hints.push(format!("examples nearby: {}", nearby.join(", ")));
            }
            hints.push("skill criteria".into());
            hints.push(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/goals-derive_criteria/upsert-criterion-per-requirement-report-diagnostic-invented-85133.md"
                ))
                .into(),
            );
            let cause = store
                .status
                .changes
                .iter()
                .filter(|c| reqs.iter().any(|(rid, _)| c.subject == *rid))
                .max_by_key(|c| (c.generation, c.mutation))
                .map(|c| c.cause());
            out.push(build_goal(
                self,
                &entity,
                true,
                json!({"none": none_ids, "revised": revised_ids}),
                cause,
                hints,
            ));
        }
        out
    }
    fn ready(&self, _goal: &Goal, board: &Board) -> Ready {
        tier3_ready(board)
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        // The source section of each listed requirement, once: the criterion's values
        // live there (docs/compiler/goals/derive-criteria.md#what-the-model-sees).
        let mut sections: Vec<String> = Vec::new();
        for g in batch {
            lines.push(format!("- {} stub", g.target));
            for key in ["none", "revised"] {
                for r in g.change[key].as_array().into_iter().flatten() {
                    if let Some(id) = r.as_str() {
                        lines.push(format!("- {} full", id));
                        if let Some(src) = store
                            .graph
                            .requirements
                            .get(id)
                            .and_then(|q| q.source.as_ref())
                        {
                            let sec = format!("{}#{}", src.doc, src.section);
                            if !sections.contains(&sec) {
                                sections.push(sec);
                            }
                        }
                    }
                }
            }
        }
        for sec in sections {
            lines.push(format!("- {} full", sec));
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "upsert_criterion",
            "update_criterion",
            "delete_criterion",
            "report_diagnostic",
        ]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn prompt(&self) -> &'static str {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/goals/derive-criteria.md"
        ))
    }
}
