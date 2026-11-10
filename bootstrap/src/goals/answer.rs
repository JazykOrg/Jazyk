use super::*;

pub struct Answer;

impl GoalKind for Answer {
    fn kind(&self) -> &'static str {
        "answer"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "diagnostic"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut out = Vec::new();
        for rec in records_of(store, &[store::CHANGE_PROMPT_UNANSWERED]) {
            let Some(d) = store.graph.diagnostics.get(&rec.subject) else {
                continue;
            };
            let Some(p) = d.prompt.as_ref() else { continue };
            if d.lifecycle != "open"
                || d.answer.is_some()
                || d.rule == "ratification-pending"
                || matches!(d.triage.as_deref(), Some("suppressed") | Some("wontfix"))
            {
                continue;
            }
            // An observation's prompt is advice, not a standing question: an
            // info-severity diagnostic (other than a decision, which exists to ask)
            // derives no answer goal. Mirrors docs/compiler/goals/answer.md#created-when.
            if d.severity == "info" && d.rule != "decision" {
                continue;
            }
            let change = json!({
                "rule": d.rule,
                "subjects": d.subjects,
                "options": p.options.len(),
                "freeform": p.freeform,
            });
            let mut hints = vec![format!("question: {}", p.question)];
            for (i, o) in p.options.iter().enumerate() {
                hints.push(match &o.edit {
                    Some(e) => format!(
                        "option {}: {} (edit {}#{})",
                        i + 1,
                        o.label,
                        e.doc,
                        e.section
                    ),
                    None => format!("option {}: {}", i + 1, o.label),
                });
            }
            if p.freeform {
                hints.push("a freeform reply is accepted".into());
            }
            for s in &d.subjects {
                hints.push(format!("subject {}", node_label(store, s)));
            }
            if let Some(r) = &d.reasoning {
                hints.push(format!("reasoning: {}", truncate(r, 160)));
            }
            hints.push(format!(
                "filed by {} in g{}; answer through the LSP code action, the GUI questions panel, or chat",
                rec.via, rec.generation
            ));
            let mut g = build_goal(self, &rec.subject, true, change, Some(rec.cause()), hints);
            g.state = GoalState::Blocked {
                on: "human: answer".into(),
            };
            out.push(g);
        }
        out
    }
    fn ready(&self, _goal: &Goal, _board: &Board) -> Ready {
        Ready::Blocked("awaiting a human answer".into())
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
