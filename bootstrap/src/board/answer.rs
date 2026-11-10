// The answer the MCP serving and the GUI render, and the one-line-per-goal board.
// Mirrors docs/frontends/mcp.md#compilation-over-mcp.
use super::*;

impl Board {
    // ---- the answer the MCP serving and the GUI render ----

    // The goals leased to the reading serving are its own: the blocking sentence says
    // so instead of naming the serving's worker id as a stranger's claim.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    pub fn mark_own(&mut self, worker: &str) {
        let own: Vec<String> = self
            .claimed
            .iter()
            .filter(|(_, w)| w.as_str() == worker)
            .map(|(id, _)| id.clone())
            .collect();
        for id in own {
            self.readiness.insert(
                id.clone(),
                Ready::Blocked(
                    "yours: your open batch holds it; mark_goal_done or mark_goal_failed it, then done"
                        .to_string(),
                ),
            );
            self.own.insert(id);
        }
    }

    pub fn goal_json(&self, g: &Goal) -> Value {
        let ready = self.is_ready(&g.id);
        let mut v = json!({
            "id": g.id,
            "kind": g.kind,
            "target": g.target,
            "unit": g.unit,
            "class": g.class,
            "mandatory": g.mandatory,
            "state": g.state,
            "ready": ready,
            "gated": self.gated.contains(&g.id),
            "hints": g.hints,
            "cause": g.cause,
            "change": g.change,
        });
        if let Some(r) = self.readiness.get(&g.id).and_then(|r| r.reason()) {
            v["blockedBy"] = json!(r);
        }
        if let Some(r) = self.parked_reasons.get(&g.id) {
            v["parkedReason"] = json!(r);
        }
        if let Some(w) = self.claimed.get(&g.id) {
            v["claimedBy"] = json!(if self.own.contains(&g.id) {
                "you"
            } else {
                w.as_str()
            });
        }
        if let Some(b) = self.batches.iter().find(|b| b.goals.contains(&g.id)) {
            v["batch"] = json!(b.id);
        }
        v
    }

    pub fn batch_json(&self, b: &Batch) -> Value {
        json!({
            "id": b.id,
            "class": b.class.name(),
            "tier": b.tier,
            "executor": b.executor,
            "locality": b.locality,
            "goals": b.goals.iter().filter_map(|id| self.goal(id)).map(|g| json!({
                "id": g.id, "kind": g.kind, "target": g.target, "mandatory": g.mandatory,
            })).collect::<Vec<_>>(),
        })
    }

    // The compact `goals` reply: the counts, the batches, the ready goals outside a
    // batch, and the rest summarized by kind and blocking reason with their ids.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    pub fn answer_compact(&self) -> Value {
        let mut full = self.answer();
        let batched: BTreeSet<&str> = self
            .batches
            .iter()
            .flat_map(|b| b.goals.iter().map(String::as_str))
            .collect();
        let mut ready_loose: Vec<Value> = Vec::new();
        let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
        let mut waiting: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in &self.goals {
            if matches!(g.state, GoalState::Failed { .. }) {
                continue;
            }
            if is_open(g) && self.is_ready(&g.id) {
                if !batched.contains(g.id.as_str()) {
                    ready_loose.push(json!({
                        "id": g.id, "kind": g.kind, "target": g.target, "mandatory": g.mandatory,
                    }));
                }
                continue;
            }
            // Open goals that are not ready and blocked goals (a release, a human
            // answer) both land here, so the counts and the summary agree.
            *by_kind.entry(g.kind.clone()).or_insert(0) += 1;
            let reason = self
                .readiness
                .get(&g.id)
                .and_then(|r| r.reason())
                .map(|r| r.to_string())
                .unwrap_or_else(|| match &g.state {
                    GoalState::Blocked { on } => format!("blocked on {}", on),
                    _ => "not ready".to_string(),
                });
            waiting.entry(reason).or_default().push(g.id.clone());
        }
        full["goals"] = json!(ready_loose);
        // Past the first few batches only the shape travels, and a reason lists a
        // handful of ids: a 41-batch board once answered with ninety thousand
        // characters. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
        const COMPACT_BATCHES: usize = 6;
        const COMPACT_IDS: usize = 8;
        if let Some(arr) = full["batches"].as_array_mut() {
            if arr.len() > COMPACT_BATCHES {
                let rest: Vec<Value> = arr
                    .split_off(COMPACT_BATCHES)
                    .iter()
                    .map(|b| {
                        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
                        for g in b["goals"].as_array().into_iter().flatten() {
                            let k = g["kind"].as_str().unwrap_or("?").to_string();
                            *kinds.entry(k).or_insert(0) += 1;
                        }
                        json!({"id": b["id"], "locality": b["locality"], "goals": kinds})
                    })
                    .collect();
                full["moreBatches"] = json!(rest);
                full["batchesNote"] = json!(format!(
                    "the first {} batches in full; moreBatches lists the rest as counts by kind (begin_goals takes them in order)",
                    COMPACT_BATCHES
                ));
            }
        }
        let waiting: BTreeMap<String, Value> = waiting
            .into_iter()
            .map(|(reason, ids)| {
                let v = if ids.len() > COMPACT_IDS {
                    let mut shown: Vec<Value> =
                        ids[..COMPACT_IDS].iter().map(|i| json!(i)).collect();
                    shown.push(json!(format!("and {} more", ids.len() - COMPACT_IDS)));
                    json!(shown)
                } else {
                    json!(ids)
                };
                (reason, v)
            })
            .collect();
        full["notReady"] = json!({"byKind": by_kind, "byReason": waiting});
        full["countsNote"] = json!("open, ready, parked, and failed count mandatory goals; optional counts the rest; by_class and by_kind include both; blocked are the goals waiting on a release or a human answer, gated the subset a release opens");
        full["note"] = json!(match full.get("note").and_then(Value::as_str) {
            Some(n) => format!(
                "{}; goals {{full: true}} lists every goal with its hints",
                n
            ),
            None => "compact board; goals {full: true} lists every goal with its hints".to_string(),
        });
        full
    }

    // The `goals` reply: every goal with readiness, the batches the scheduler would
    // form, the counts, and the verdict with the diagnostic counts when nothing is
    // open. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    pub fn answer(&self) -> Value {
        let counts = self.counts();
        let mut v = json!({
            "generation": self.generation,
            "goals": self.goals.iter().map(|g| self.goal_json(g)).collect::<Vec<_>>(),
            "batches": self.batches.iter().map(|b| self.batch_json(b)).collect::<Vec<_>>(),
            "counts": counts,
        });
        // No mandatory goal open: the verdict stands, and any batch still listed holds
        // optional goals only. Say so, since a reader once weighed "converged" against
        // a next line inviting a claim (docs/frontends/mcp.md#compilation-over-mcp).
        if self.open_mandatory() == 0 {
            v["verdict"] = json!(self.verdict.to_string());
            if let Some(b) = self.batches.first() {
                let optional: usize = self.batches.iter().map(|b| b.goals.len()).sum();
                v["note"] = json!(format!(
                    "nothing mandatory to compile; the graph reflects the docs. {} optional goal(s) in {} batch(es) stay claimable (begin_goals takes {}); the verdict does not wait for them",
                    optional,
                    self.batches.len(),
                    b.id
                ));
                v["next"] = json!(format!(
                    "optional: begin_goals claims {} or stop here",
                    b.id
                ));
            } else {
                v["note"] = json!("nothing to compile; the graph reflects the docs");
            }
        } else if self.batches.is_empty() {
            v["note"] = json!("no goal is ready; blockedBy says why");
        } else {
            v["next"] = json!(format!(
                "begin_goals claims the next batch ({})",
                self.batches[0].id
            ));
        }
        if !self.open_diags.is_empty() {
            v["openDiagnostics"] = json!(self.open_diags);
        }
        v
    }

    // The whole board, one line per goal: ready first, then waiting, blocked, parked,
    // failed.
    pub fn render(&self) -> String {
        let mut rows: Vec<(u8, String)> = self
            .goals
            .iter()
            .map(|g| {
                let rank = match (&g.state, self.is_ready(&g.id)) {
                    (GoalState::Open, true) => 0,
                    (GoalState::Parked, _) => 3,
                    (GoalState::Open, false) => 1,
                    (GoalState::Blocked { .. }, _) => 2,
                    (GoalState::Failed { .. }, _) => 4,
                };
                let readiness = match self.readiness.get(&g.id) {
                    Some(Ready::Ready) => "ready".to_string(),
                    Some(Ready::Blocked(r)) => r.clone(),
                    None => String::new(),
                };
                (
                    rank,
                    format!(
                        "{}  {}, {}, {}, {}",
                        g.id,
                        g.class,
                        if g.mandatory { "mandatory" } else { "optional" },
                        state_word(&g.state),
                        readiness
                    ),
                )
            })
            .collect();
        rows.sort();
        rows.into_iter()
            .map(|(_, r)| r)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(super) fn state_word(s: &GoalState) -> String {
    match s {
        GoalState::Open => "open".into(),
        GoalState::Parked => "parked".into(),
        GoalState::Blocked { on } => format!("blocked ({})", on),
        GoalState::Failed { reason } => format!("failed ({})", reason),
    }
}

pub(super) fn condense_change(v: &Value) -> String {
    let s = v.to_string();
    crate::llm::truncate(&s, 200)
}
