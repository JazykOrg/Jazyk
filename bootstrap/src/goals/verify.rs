use super::*;

pub struct Verify;

impl GoalKind for Verify {
    fn kind(&self) -> &'static str {
        "verify"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "ledger row"
    }
    fn derive_goals(&self, store: &Store, gen: &GenSettings) -> Vec<Goal> {
        let bind_reqs: BTreeSet<String> = crate::bind::pending(store, gen)
            .iter()
            .filter_map(|p| p["requirement"].as_str().map(String::from))
            .collect();
        crate::verify::pending(store, gen, Some("stale"), None)
            .into_iter()
            .filter(|p| {
                p["status"] != "unimplemented"
                    && p["status"] != "partial"
                    && p["reason"] != "not-generated"
            })
            .filter(|p| !bind_reqs.contains(p["requirement"].as_str().unwrap_or_default()))
            .filter_map(|p| {
                let rid = p["requirement"].as_str()?.to_string();
                let entity = p["entity"].as_str().unwrap_or("").to_string();
                let reason = p["reason"].as_str().unwrap_or("").to_string();
                let kind = p["test"]["kind"].as_str().unwrap_or("").to_string();
                let mut hints = vec![format!(
                    "reason: {}; previous verdict: {}",
                    reason,
                    p["lastVerdict"].as_str().unwrap_or("none")
                )];
                if kind == "programmatic" {
                    hints.push(format!(
                        "run: {} (cwd {})",
                        p["test"]["run"].as_str().unwrap_or(""),
                        p["test"]["cwd"].as_str().unwrap_or(".")
                    ));
                } else {
                    hints.push(format!(
                        "criteria: {}",
                        p["test"]["artifact"].as_str().unwrap_or("")
                    ));
                }
                hints.push("begin_verification, then record_verdict".into());
                Some(build_goal(
                    self,
                    &rid,
                    true,
                    json!({"goal": "verify", "reason": reason, "kind": kind, "entity": entity}),
                    Some(ledger_cause(store, &rid)),
                    hints,
                ))
            })
            .collect()
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        if let Ready::Blocked(r) = tier3_ready(board) {
            return Ready::Blocked(r);
        }
        let entity = goal.change["entity"].as_str().unwrap_or("");
        if board.bind_open_for_entity(entity) > 0 || board.open(&goal_id("bind", &goal.target)) {
            return Ready::Blocked(
                "binding first: the row's requirement or entity owes a bind".into(),
            );
        }
        if board.open(&goal_id("generate", entity)) {
            return Ready::Blocked(format!("generation first: {} owes a generate", entity));
        }
        Ready::Ready
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        batch
            .iter()
            .flat_map(|g| {
                let mut v = vec![format!("- {} full", g.target)];
                if let Some(e) = g.change["entity"].as_str() {
                    v.push(format!("- {} stub", e));
                }
                v
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "verification_tasks",
            "begin_verification",
            "run_tests",
            "record_verdict",
        ]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn goal_gate(&self, cx: &GateContext) -> Option<Violation> {
        ledger_violation(cx, crate::verify::gate(cx.store, cx.gen, &cx.goal.target))
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../../docs/compiler/goals/prompts/verify.md")
    }
}
