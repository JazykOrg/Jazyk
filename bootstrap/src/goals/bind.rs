use super::*;

// ---- ledger kinds ----

// The persisted ledger-stale record on the subject, else the latest record that
// explains the disagreement, else the ledger comparison at the current generation.
pub(super) fn ledger_cause(store: &Store, subject: &str) -> Cause {
    store
        .status
        .changes
        .iter()
        .filter(|c| c.kind == CHANGE_LEDGER_STALE && c.subject == subject)
        .max_by_key(|c| (c.generation, c.mutation))
        .map(|c| c.cause())
        .or_else(|| {
            store
                .status
                .changes
                .iter()
                .filter(|c| {
                    c.subject == subject
                        && matches!(
                            c.kind.as_str(),
                            store::CHANGE_REQ_CREATED
                                | store::CHANGE_REQ_REVISED
                                | store::CHANGE_ENTITY
                        )
                })
                .max_by_key(|c| (c.generation, c.mutation))
                .map(|c| c.cause())
        })
        .unwrap_or(Cause {
            generation: store.status.generation,
            mutation: 0,
            via: "ledger".into(),
        })
}

// A ledger kind's gate reads the landed ledger, never the changeset: the claim is
// validated against the row or record the goal page names, and the refusal carries
// the repair. Mirrors docs/compiler/goals/bind.md#gate, generate.md#gate, verify.md#gate.
pub(super) fn ledger_violation(cx: &GateContext, result: Result<(), String>) -> Option<Violation> {
    result.err().map(|message| {
        Violation::new(
            &format!("{}-gate", cx.goal.kind),
            format!("{}: {}", cx.goal.goal, message),
        )
    })
}

pub struct Bind;

impl GoalKind for Bind {
    fn kind(&self) -> &'static str {
        "bind"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "requirement"
    }
    fn derive_goals(&self, store: &Store, gen: &GenSettings) -> Vec<Goal> {
        let ledger = crate::gen::Ledger::load(&store.out);
        crate::bind::pending(store, gen)
            .into_iter()
            .filter_map(|p| {
                let rid = p["requirement"].as_str()?.to_string();
                let entity = p["entity"].as_str().unwrap_or("").to_string();
                let reason = p["reason"].as_str().unwrap_or("").to_string();
                let mut hints = vec![format!("reason: {}", reason)];
                if let Some(row) = ledger.requirements.get(&rid) {
                    hints.push(format!("previous test: {}", row.test.name));
                }
                if let Some(e) = ledger.entities.get(crate::ids::entity_slug(&entity)) {
                    if !e.files.is_empty() {
                        hints.push(format!("entity files: {}", e.files.join(", ")));
                    }
                }
                hints.push(format!(
                    "suggested test name: {}-{}",
                    crate::gen::req_slug(&rid),
                    &hash_hex(
                        &store
                            .graph
                            .requirements
                            .get(&rid)
                            .map(|r| r.statement.clone())
                            .unwrap_or_default()
                    )[..8]
                ));
                hints.push(format!(
                    "medium: {}",
                    ledger
                        .medium
                        .as_ref()
                        .map(|m| m.line())
                        .unwrap_or_else(|| "undecided; this session decides it".into())
                ));
                hints.push(format!("load {}", entity));
                hints.push("begin_binding, then record_binding".into());
                let criteria = store.criteria_of(&rid);
                if !criteria.is_empty() {
                    hints.push(format!("criteria: {}", criteria.join(", ")));
                }
                Some(build_goal(
                    self,
                    &rid,
                    true,
                    json!({"goal": "bind", "reason": reason, "entity": entity, "criteria": criteria.len(), "sourceConfigured": gen.configured}),
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
        // The evidence exists before a test encodes it
        // (docs/compiler/reconciler.md#tiers).
        if goal.change["sourceConfigured"] == json!(false) {
            return Ready::Blocked(
                "source mapping: configure the document set's source patterns with list_document_sets and update_document_set; then criteria goals can derive".into(),
            );
        }
        let entity = goal.change["entity"].as_str().unwrap_or("");
        if board.open_kind_on("derive-criteria", entity) {
            return Ready::Blocked(format!(
                "criteria: derive-criteria on {} runs first",
                entity
            ));
        }
        if goal.change["criteria"].as_u64() == Some(0) {
            return Ready::Blocked(
                "criteria: the requirement carries none; derive-criteria writes them".into(),
            );
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
            "binding_tasks",
            "begin_binding",
            "record_binding",
            "generation_tasks",
            "begin_generation",
            "record_generation",
            "record_medium",
            "run_tests",
        ]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn goal_gate(&self, cx: &GateContext) -> Option<Violation> {
        ledger_violation(cx, crate::bind::gate(cx.store, cx.gen, &cx.goal.target))
    }
    fn prompt(&self) -> &'static str {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/goals/bind.md"
        ))
    }
}
