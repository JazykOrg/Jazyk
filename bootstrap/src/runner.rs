// What runs one goal batch as a session. The reconciler and the ledger workers speak
// only this: a name for the trace, the build token, and one call that turns a batch
// into a report. Natively it is an ACP worker session against the configured agent
// (`crate::acp::runner`); in a browser tab it is the loop running in the same wasm
// module (`crate::wasm::runner`). See docs/frontends/wasm.md#sessions.
use crate::model::Goal;
use crate::session::Trace;
use std::collections::BTreeSet;

// What one session left behind: the mutations its commits applied, the goals they
// resolved (from the journal), and whether the batch landed.
pub struct SessionReport {
    pub applied: usize,
    pub rounds: u32,
    // What the turn billed, and the input among it the endpoint served from its prompt
    // cache (docs/compiler/sessions.md#budgets).
    pub tokens: u64,
    pub cached: u64,
    pub failed: Option<String>,
    // Goal ids the batch's journal entries resolved.
    pub resolved: Vec<String>,
    // Why the session stopped short of a plain end of turn (the agent's error text on
    // a refusal, the round budget, a cancel), for a caller that judges its own ledger.
    // Mirrors docs/frontends/acp.md#the-embedded-agent.
    pub stopped: Option<String>,
}

// One session's worth of work: the batch id, its goals, and the scheduler's executor
// hint. The reconciler builds one per board batch; the ledger workers build
// single-goal batches per row.
pub struct BatchRun {
    pub id: String,
    pub goals: Vec<Goal>,
    pub executor: Option<String>,
}

impl BatchRun {
    pub fn single(goal: Goal) -> BatchRun {
        BatchRun {
            id: goal.id.clone(),
            goals: vec![goal],
            executor: None,
        }
    }

    pub(crate) fn goal_ids(&self) -> Vec<String> {
        self.goals.iter().map(|g| g.id.clone()).collect()
    }

    pub(crate) fn kind(&self) -> String {
        self.goals
            .first()
            .map(|g| g.kind.clone())
            .unwrap_or_default()
    }

    pub(crate) fn class(&self) -> String {
        self.goals
            .first()
            .map(|g| g.class.clone())
            .unwrap_or_else(|| "compile".to_string())
    }

    // The serving the batch's goal kinds need: bind and generate work the ledger and
    // the deliverable, verify judges rows, everything else works the graph.
    pub(crate) fn serving_mode(&self) -> &'static str {
        let kinds: BTreeSet<&str> = self.goals.iter().map(|g| g.kind.as_str()).collect();
        if !kinds.is_empty() && kinds.iter().all(|k| matches!(*k, "bind" | "generate")) {
            "generate"
        } else if !kinds.is_empty() && kinds.iter().all(|k| *k == "verify") {
            "verify"
        } else {
            "compile"
        }
    }

    pub(crate) fn is_ledger(&self) -> bool {
        self.serving_mode() != "compile"
    }
}

// The contract every runner satisfies.
pub trait SessionRunner {
    // Named in the trace line that opens a build.
    fn agent_name(&self) -> String;

    // The control plane's token for the run, carried into every session's serving.
    fn set_build_token(&self, token: Option<String>);

    // One batch, one session. A runner reports what landed; it never decides
    // whether the goals are resolved, which is read from the store.
    fn run_item(&self, batch: &BatchRun, trace: &Trace) -> SessionReport;

    // One prompt, one reply, no tools and no session: the ledger's one-shots.
    fn ask(&self, system: &str, user: &str, label: &str, step: &str) -> Result<String, String>;

    fn ask_traced(
        &self,
        system: &str,
        user: &str,
        label: &str,
        step: &str,
        trace: Option<&Trace>,
    ) -> Result<String, String>;
}

// What the journal says one session landed between two generations: the mutations
// its entries applied, and the goals they resolved. A runner never reports its own
// success; it reads it here (docs/compiler/graph.md#journal).
pub fn journal_diff(out: &std::path::Path, from: u64, to: u64, goal_ids: &[String]) -> (usize, Vec<String>) {

    let mut applied = 0usize;
    let mut resolved: Vec<String> = Vec::new();
    for g in (from + 1)..=to {
        let path = out.join("journal").join(format!("g{}.yaml", g));
        let Ok(text) = crate::vfs::read_to_string(&path) else {
            continue;
        };
        let Ok(entry) = serde_norway::from_str::<crate::model::JournalEntry>(&text) else {
            continue;
        };
        if entry.kind != "session" || !entry.batch.iter().any(|g| goal_ids.contains(g)) {
            continue;
        }
        applied += entry.mutations.len();
        for r in &entry.resolved_goals {
            if !resolved.contains(&r.goal) {
                resolved.push(r.goal.clone());
            }
        }
    }
    (applied, resolved)
}
