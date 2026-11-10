// What a build reports: the build report with its verdict line, and the per-kind
// and GC burst counts the build trace prints.
use super::*;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildReport {
    pub verdict: String,
    // Goals the board derived when the build opened.
    pub goals: usize,
    pub sessions: u32,
    pub applied: usize,
    pub parked: usize,
    pub failed: usize,
    pub blocked: usize,
    pub optional: usize,
    pub errors: usize,
    pub warnings: usize,
    pub coverage_pct: u32,
    pub tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspend_reason: Option<String>,
    // Session-owned tasks the build's sweep removed.
    // Mirrors docs/consumers/gen.md#session-tasks-are-swept-when-unused.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired_tasks: Vec<String>,
    // Why the verdict write was refused: the verdict and the parked goals never reached
    // disk. Mirrors docs/compiler/graph.md#concurrency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsaved: Option<String>,
}

impl BuildReport {
    // A build whose verdict was not saved never counts as converged.
    // Mirrors docs/compiler/compilation.md#convergence.
    pub fn converged(&self) -> bool {
        self.unsaved.is_none() && self.verdict.starts_with("converged")
    }

    // The printed verdict, naming a refused verdict write.
    pub fn verdict_line(&self) -> String {
        match &self.unsaved {
            Some(why) => format!("{} (not saved: {})", self.verdict, why),
            None => self.verdict.clone(),
        }
    }
}

// The count per kind of the selected open goals, largest first.
pub(super) fn selected_kinds_line(board: &Board) -> Vec<(String, usize)> {
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for g in board.selected_open_goals() {
        *by_kind.entry(g.kind.clone()).or_insert(0) += 1;
    }
    let mut kinds: Vec<(String, usize)> = by_kind.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    kinds
}

pub(super) fn kinds_line(board: &Board) -> Vec<(String, usize)> {
    let c = board.counts();
    let mut kinds: Vec<(String, usize)> = c.by_kind.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    kinds
}

// The `gc burst:` count against the limit for one GC goal.
pub(super) fn burst_measure(g: &Goal) -> (u64, u64, String) {
    if let Some(l) = g.change["limits"].as_array().and_then(|a| a.first()) {
        return (
            l["count"].as_u64().unwrap_or(0),
            l["soft"].as_u64().unwrap_or(0),
            l["limit"].as_str().unwrap_or("").to_string(),
        );
    }
    // The fan-out variant: `{fan_out, limit: {soft, hard}}`
    // (docs/compiler/goals/abstract-entity.md#the-change).
    if let Some(n) = g.change["fan_out"].as_u64() {
        return (
            n,
            g.change["limit"]["soft"].as_u64().unwrap_or(0),
            crate::limits::CHILDREN_PER_ENTITY.to_string(),
        );
    }
    match g.kind.as_str() {
        "declare-edges" => (
            g.change["entities"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0) as u64,
            1,
            "entities without edges".into(),
        ),
        "dedupe-candidates" => (
            (g.change["score"].as_f64().unwrap_or(0.0) * 100.0) as u64,
            (crate::limits::LOOKALIKE_THRESHOLD * 100.0) as u64,
            "lookalike score, percent".into(),
        ),
        _ => (
            g.change["matched"].as_array().map(|a| a.len()).unwrap_or(0) as u64,
            0,
            "matched nodes".into(),
        ),
    }
}
