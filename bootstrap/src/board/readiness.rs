// The readiness queries the kinds ask, the counts, and the verdict the board implies.
// Mirrors docs/compiler/reconciler.md#readiness.
use super::*;

impl Board {
    // ---- readiness queries the kinds ask ----

    pub fn tier_open(&self, tier: u8) -> usize {
        self.tier_open[tier as usize]
    }

    // The earlier level a document waits for, when one still has open tier 1 goals.
    pub fn level_waiting(&self, doc: &str) -> Option<usize> {
        let mine = *self.doc_levels.get(doc)?;
        self.tier1_open_docs
            .iter()
            .filter_map(|d| self.doc_levels.get(d))
            .filter(|l| **l < mine)
            .min()
            .copied()
    }

    pub fn bind_open_for_entity(&self, entity: &str) -> usize {
        self.bind_open_by_entity.get(entity).copied().unwrap_or(0)
    }

    pub fn open(&self, goal_id: &str) -> bool {
        self.goal(goal_id).is_some_and(is_open)
    }

    // Whether an open goal of the kind stands on the target.
    pub fn open_kind_on(&self, kind: &str, target: &str) -> bool {
        self.open_kinds
            .contains(&(kind.to_string(), target.to_string()))
    }

    // The open compile goals in a GC goal's cone.
    pub fn cone_blockers(&self, goal_id: &str) -> Vec<String> {
        self.cone_blockers.get(goal_id).cloned().unwrap_or_default()
    }

    // The level a split-view target belongs to, when it is a level view or a lifted
    // flow view of one. Mirrors docs/compiler/reconciler.md#gc-gating.
    pub fn view_level(&self, view_id: &str) -> Option<&str> {
        self.view_levels.get(view_id).map(String::as_str)
    }

    pub fn goal(&self, id: &str) -> Option<&Goal> {
        // The index is built when the goals are final; a scan stands in for a board
        // whose goals moved under it.
        match self.by_id.get(id).and_then(|i| self.goals.get(*i)) {
            Some(g) if g.id == id => Some(g),
            _ => self.goals.iter().find(|g| g.id == id),
        }
    }

    // The change record ids a goal stands on.
    pub fn records_of(&self, goal_id: &str) -> Vec<String> {
        self.records.get(goal_id).cloned().unwrap_or_default()
    }

    // The pair waiting under the degree cap for a goal id or a pair target.
    pub fn waiting_pair(&self, id_or_target: &str) -> Option<&goals::WaitingPair> {
        let id = if id_or_target.starts_with("g:rejudge-pair:") {
            id_or_target.to_string()
        } else {
            goals::goal_id("rejudge-pair", id_or_target)
        };
        self.waiting.get(&id)
    }

    // One line for a waiting pair: that it waits, its score, the full side.
    pub(super) fn explain_waiting(&self, store: &Store, w: &goals::WaitingPair) -> String {
        let mut s = format!(
            "g:rejudge-pair:{}  rejudge-pair, compile, mandatory, waiting under the cap\n",
            w.target
        );
        s.push_str(&format!(
            "  waits:  score {}; {} hold(s) {} open pairs ({})\n",
            w.score,
            w.full.join(" and "),
            crate::limits::PAIRS_PER_REQUIREMENT_CAP,
            crate::limits::PAIRS_PER_REQUIREMENT
        ));
        s.push_str(&format!(
            "  cause:  g{} mutation {} via {}\n",
            w.cause.generation, w.cause.mutation, w.cause.via
        ));
        if let Some((a, b)) = goals::pair_members(&w.target) {
            for c in store.status.changes.iter().filter(|c| {
                goals::record_kinds("rejudge-pair").contains(&c.kind.as_str())
                    && (c.subject == a || c.subject == b)
            }) {
                s.push_str(&format!("  record: {}\n", c.id));
            }
        }
        s
    }

    pub fn is_ready(&self, goal_id: &str) -> bool {
        self.readiness.get(goal_id).is_some_and(|r| r.is_ready())
            && !self.gated.contains(goal_id)
            && !self.claimed.contains_key(goal_id)
    }

    pub fn ready_goals(&self) -> Vec<&Goal> {
        self.goals
            .iter()
            .filter(|g| is_open(g) && self.is_ready(&g.id))
            .collect()
    }

    // Open or parked goals, both classes.
    pub fn open_goals(&self) -> Vec<&Goal> {
        self.goals.iter().filter(|g| is_open(g)).collect()
    }

    // The open goals a selected build runs: every open goal without a selection.
    pub fn selected_open_goals(&self) -> Vec<&Goal> {
        self.goals
            .iter()
            .filter(|g| is_open(g) && !self.outside.contains(&g.id))
            .collect()
    }

    // Mandatory goals still open or parked: what keeps a build from converging.
    pub fn open_mandatory(&self) -> usize {
        self.goals
            .iter()
            .filter(|g| is_open(g) && g.mandatory)
            .count()
    }

    // Open goals of the named kinds; the graph kinds are everything but the ledger's.
    pub fn open_of(&self, kinds: &[&str]) -> usize {
        self.goals
            .iter()
            .filter(|g| is_open(g) && kinds.contains(&g.kind.as_str()))
            .count()
    }

    pub fn ready_of(&self, kinds: &[&str]) -> usize {
        self.goals
            .iter()
            .filter(|g| is_open(g) && kinds.contains(&g.kind.as_str()) && self.is_ready(&g.id))
            .count()
    }

    pub fn gated_of(&self, kinds: &[&str]) -> usize {
        self.goals
            .iter()
            .filter(|g| kinds.contains(&g.kind.as_str()) && self.gated.contains(&g.id))
            .count()
    }

    pub fn graph_kinds() -> Vec<&'static str> {
        REGISTRY
            .iter()
            .map(|k| k.kind())
            .filter(|k| !LEDGER_KINDS.contains(k))
            .collect()
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for g in &self.goals {
            match &g.state {
                GoalState::Open | GoalState::Parked => {
                    if g.mandatory {
                        c.open += 1;
                    } else {
                        c.optional += 1;
                    }
                    if matches!(g.state, GoalState::Parked) {
                        c.parked += 1;
                    }
                    *c.by_class.entry(g.class.clone()).or_insert(0) += 1;
                    *c.by_kind.entry(g.kind.clone()).or_insert(0) += 1;
                    // ready counts mandatory goals like open does; an optional ready
                    // batch stays under optional (docs/frontends/mcp.md#compilation-over-mcp).
                    if g.mandatory && self.is_ready(&g.id) {
                        c.ready += 1;
                    }
                }
                GoalState::Blocked { .. } => c.blocked += 1,
                GoalState::Failed { .. } => {
                    if g.mandatory {
                        c.failed += 1;
                    } else {
                        c.optional += 1;
                    }
                }
            }
        }
        c.gated = self.gated.len();
        c.claimed = self.claimed.len();
        c
    }

    // The verdict the board implies; the checks decide the rest.
    pub fn verdict(&self) -> Verdict {
        let c = self.counts();
        Verdict {
            state: if c.open == 0 && c.failed == 0 {
                "converged".into()
            } else {
                "incomplete".into()
            },
            open: c.open as u64,
            failed: c.failed as u64,
            blocked: c.blocked as u64,
            optional: c.optional as u64,
        }
    }

    // `compile: N goals (k kind, ...), b blocked`. Mirrors docs/frontends/cli.md#jazyk-compile.
    pub fn summary_line(&self) -> String {
        let c = self.counts();
        let total: usize = c.by_kind.values().sum();
        let mut kinds: Vec<(&String, &usize)> = c.by_kind.iter().collect();
        kinds.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let per_kind: Vec<String> = kinds.iter().map(|(k, n)| format!("{} {}", n, k)).collect();
        let mut s = format!("compile: {} goals", total);
        if !per_kind.is_empty() {
            s.push_str(&format!(" ({})", per_kind.join(", ")));
        }
        if c.blocked > 0 {
            s.push_str(&format!(", {} blocked", c.blocked));
        }
        if !self.waiting.is_empty() {
            s.push_str(&format!(
                ", {} pair(s) waiting under the cap",
                self.waiting.len()
            ));
        }
        s
    }
}
