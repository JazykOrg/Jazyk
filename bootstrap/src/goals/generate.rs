use super::*;

pub struct Generate;

impl GoalKind for Generate {
    fn kind(&self) -> &'static str {
        "generate"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "entity"
    }
    fn derive_goals(&self, store: &Store, gen: &GenSettings) -> Vec<Goal> {
        let ledger = crate::gen::Ledger::load(&store.out);
        let pending = crate::gen::pending(store, gen);
        if pending.is_empty() {
            return Vec::new();
        }
        let by_entity = crate::gen::ReqsByEntity::build(store);
        pending
            .into_iter()
            .filter_map(|p| {
                let id = p["entity"].as_str()?.to_string();
                let reason = p["reason"].as_str().unwrap_or("").to_string();
                let changed = str_list(&p["changed"]);
                let mut hints = vec![format!("reason: {}", reason)];
                if !changed.is_empty() {
                    hints.push(format!("changed: {}", changed.join(", ")));
                }
                let rids = by_entity.of(&id);
                let bound: Vec<String> = rids
                    .iter()
                    .filter_map(|r| ledger.requirements.get(r).map(|row| row.test.name.clone()))
                    .collect();
                if !bound.is_empty() {
                    hints.push(format!("bound tests to make pass: {}", bound.join(", ")));
                }
                if let Some(b) = &ledger.build {
                    hints.push(format!("build: {} (cwd {})", b.run, b.cwd));
                }
                let n = rids.len();
                if n > 20 {
                    hints.push(format!("{} parts of 20 requirements", n.div_ceil(20)));
                }
                if let Some(p) = store.graph.entities.get(&id).and_then(|e| e.parent.clone()) {
                    hints.push(format!("load {}", p));
                }
                hints.push("begin_generation, then record_generation, then run_tests".into());
                Some(build_goal(
                    self,
                    &id,
                    true,
                    json!({"goal": "generate", "reason": reason, "changed": changed}),
                    Some(ledger_cause(store, &id)),
                    hints,
                ))
            })
            .collect()
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        if let Ready::Blocked(r) = tier3_ready(board) {
            return Ready::Blocked(r);
        }
        if board.open_kind_on("derive-criteria", &goal.target) {
            return Ready::Blocked(format!(
                "criteria: derive-criteria on {} runs first",
                goal.target
            ));
        }
        Ready::Ready
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        batch
            .iter()
            .flat_map(|g| {
                let mut v = vec![format!("- {} full", g.target)];
                if let Some(e) = store.graph.entities.get(&g.target) {
                    if let Some(p) = &e.parent {
                        v.push(format!("- {} stub", p));
                    }
                }
                for (id, e) in &store.graph.entities {
                    if e.parent.as_deref() == Some(g.target.as_str()) {
                        v.push(format!("- {} stub", id));
                    }
                }
                v
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "link_asset",
            "inspect_asset_link",
            "remove_asset_link",
            "generation_tasks",
            "begin_generation",
            "record_generation",
            "record_medium",
            "binding_tasks",
            "begin_binding",
            "record_binding",
            "run_tests",
        ]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn goal_gate(&self, cx: &GateContext) -> Option<Violation> {
        ledger_violation(
            cx,
            crate::gen::gate(cx.store, cx.gen, &cx.goal.target, true).map(|_| ()),
        )
    }
    fn prompt(&self) -> &'static str {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/goals/generate.md"
        ))
    }
}
