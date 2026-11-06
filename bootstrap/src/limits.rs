// The limits registry: built into the binary, never a project setting. Crossing a soft
// threshold opens an optional goal; crossing the hard one makes it mandatory. Session
// and build budgets, document quality thresholds, and alignment thresholds live here
// too. Mirrors docs/compiler/graph.md#limits.

pub struct Limit {
    pub name: &'static str,
    pub soft: u64,
    pub hard: u64,
    // The goal kind that resolves a crossing.
    pub goal: &'static str,
}

// One node's direct children, its level; the scope root counts its parentless entities
// under the same row. Crossing derives the fan-out variant of `abstract-entity`.
// Mirrors docs/compiler/concepts/levels.md#levels.
pub const CHILDREN_PER_ENTITY: &str = "children-per-entity";
pub const CHILDREN_PER_ENTITY_SOFT: u64 = 9;
pub const CHILDREN_PER_ENTITY_HARD: u64 = 15;

pub const LIMITS: [Limit; 8] = [
    Limit {
        name: "requirements-per-entity",
        soft: 50,
        hard: 80,
        goal: "abstract-entity",
    },
    Limit {
        name: CHILDREN_PER_ENTITY,
        soft: CHILDREN_PER_ENTITY_SOFT,
        hard: CHILDREN_PER_ENTITY_HARD,
        goal: "abstract-entity",
    },
    Limit {
        name: "members-per-structural-view",
        soft: 20,
        hard: 30,
        goal: "split-view",
    },
    Limit {
        name: "edges-per-view",
        soft: 40,
        hard: 60,
        goal: "split-view",
    },
    Limit {
        name: "members-per-flow-view",
        soft: 12,
        hard: 20,
        goal: "split-view",
    },
    Limit {
        name: "participants-per-sequence-view",
        soft: 8,
        hard: 12,
        goal: "split-view",
    },
    Limit {
        name: "instances-per-object-view",
        soft: 15,
        hard: 25,
        goal: "split-view",
    },
    Limit {
        name: "states-per-state-machine",
        soft: 12,
        hard: 20,
        goal: "abstract-entity",
    },
];

// The registry row for a limit name; None for a name outside the registry.
pub fn limit(name: &str) -> Option<&'static Limit> {
    LIMITS.iter().find(|l| l.name == name)
}

// The (soft, hard) thresholds for a node: the per-node bump is the soft threshold and
// the hard threshold is the bump plus the registry's distance between soft and hard,
// so escalation keeps its shape. Mirrors docs/compiler/graph.md#per-node-bumps.
pub fn threshold(name: &str, node_bump: Option<u64>) -> Option<(u64, u64)> {
    let l = limit(name)?;
    Some(match node_bump {
        Some(n) => (n, n.saturating_add(l.hard - l.soft)),
        None => (l.soft, l.hard),
    })
}

// The limits that apply to an entity, and the ones that apply to a view.
pub const ENTITY_LIMITS: [&str; 3] = [
    "requirements-per-entity",
    CHILDREN_PER_ENTITY,
    "states-per-state-machine",
];
pub const VIEW_LIMITS: [&str; 5] = [
    "members-per-structural-view",
    "edges-per-view",
    "members-per-flow-view",
    "participants-per-sequence-view",
    "instances-per-object-view",
];

// The pair degree cap: the most open rejudge-pair goals one requirement takes part
// in at once, the count a pair session settles. Pinned pairs never wait; the rest
// open by score as slots free. No soft and hard, no resolving goal, so it is a
// constant beside the budgets and not a row of the table.
// Mirrors docs/compiler/reconciler.md#pairs and graph.md#budgets-and-thresholds.
pub const PAIRS_PER_REQUIREMENT: &str = "pairs-per-requirement";
pub const PAIRS_PER_REQUIREMENT_CAP: usize = 8;

// The lookalike score (docs/compiler/goals/dedupe-candidates.md#the-lookalike-score):
// the two term weights, the threshold a candidate pair's score reaches, and the holder
// count under which a name token keeps its full weight (a token held by more entities
// weighs its inverse document frequency over the graph's entity names and aliases).
pub const LOOKALIKE_TOKEN_WEIGHT: f64 = 0.75;
pub const LOOKALIKE_DOCUMENT_WEIGHT: f64 = 0.25;
pub const LOOKALIKE_THRESHOLD: f64 = 0.5;
pub const LOOKALIKE_DISTINCTIVE_HOLDERS: usize = 3;

// Session budgets. Mirrors docs/compiler/sessions.md#budgets.
pub const SESSION_ROUNDS: u32 = 24;
// The embedded agent's flat runaway stop, in model round-trips (one may carry
// several tool calls). `JAZYK_AGENT_MAX_ROUNDS` overrides.
pub const AGENT_MAX_ROUNDS: u32 = 512;
pub fn agent_max_rounds() -> u32 {
    agent_rounds_from(crate::sys::env_var("JAZYK_AGENT_MAX_ROUNDS").ok().as_deref())
}

fn agent_rounds_from(value: Option<&str>) -> u32 {
    value.and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(AGENT_MAX_ROUNDS)
        .max(1)
}

// Also bounds a single reply containing many invalid calls outside a graph batch.
pub const AGENT_TOOL_ERROR_LIMIT: u32 = 20;
pub const SESSION_MUTATIONS: usize = 1024;
pub const CONTEXT_BUDGET: usize = 96_000;
// Provider context is tokens, not loaded graph characters.
pub const MODEL_CONTEXT_TOKENS: usize = 131_072;
pub const DEFAULT_MAX_COMPLETION_TOKENS: u64 = 65_536;
pub const GOAL_ATTEMPTS: u32 = 4;
pub const ROUNDS_PER_SECTION: u32 = 8;
pub const SKILLS_PER_SESSION: usize = 8;
// The most derive-criteria goals one batch packs: each lists several requirements
// with their source sections and grows the changeset's mutation budget.
// Mirrors docs/compiler/reconciler.md#batching.
pub const CRITERIA_GOALS_PER_BATCH: usize = 4;
// The fraction of the context budget past which `load` refuses.
pub const LOADED_HIGH_WATER: f64 = 0.9;

// The build cap: BUILD_SESSION_FACTOR times the derived goals, plus the floor.
pub const BUILD_SESSION_FACTOR: usize = 8;
pub const BUILD_SESSION_FLOOR: usize = 64;
// Consecutive zero-token session failures that stop a build early: an endpoint
// answering only errors must not grind the session cap into futile attempts.
pub const ENDPOINT_BREAKER: usize = 5;
// The times one goal may be resolved and re-opened with the same change in one build
// before it parks: a record the commit recomputes while its condition holds would
// otherwise re-open the identical goal after every session until the build cap.
// Mirrors docs/compiler/reconciler.md#parked-and-failed.
pub const REOPEN_CYCLES: u32 = 2;

// Document quality thresholds.
pub const MAX_SECTION_CHARS: usize = 6_000;
pub const MAX_DOC_SECTIONS: usize = 40;

// Alignment thresholds. Mirrors docs/compiler/alignment.md.
pub const ALIGN_MOVE_SIMILARITY: f64 = 0.5;
pub const ALIGN_SPLIT_COVERAGE: f64 = 0.6;
// The candidates kept per relocated anchor, by descending similarity: what the
// place-anchors session sees (`alignment-candidates`).
pub const ALIGN_CANDIDATES: usize = 3;

// The most one shell command runs (a recorded build or test, a session's
// `run_command`) before the harness kills it and records exit 124.
// `JAZYK_COMMAND_TIMEOUT` overrides, in seconds.
// Mirrors docs/consumers/gen.md#a-test-that-could-not-run-says-nothing.
pub const COMMAND_TIMEOUT_SECS: u64 = 1800;

pub fn command_timeout() -> std::time::Duration {
    let secs = crate::sys::env_var("JAZYK_COMMAND_TIMEOUT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(COMMAND_TIMEOUT_SECS);
    std::time::Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_request_override_preserves_the_default_and_minimum() {
        assert_eq!(agent_rounds_from(None), 512);
        assert_eq!(agent_rounds_from(Some("invalid")), 512);
        assert_eq!(agent_rounds_from(Some("4294967296")), 512);
        assert_eq!(agent_rounds_from(Some("0")), 1);
        assert_eq!(agent_rounds_from(Some("2")), 2);
        assert_eq!(agent_rounds_from(Some("72")), 72);
        let _fs = crate::vfs::Scope::enter(&Default::default());
        assert_eq!(agent_max_rounds(), AGENT_MAX_ROUNDS);
        assert_eq!(SESSION_ROUNDS, 24, "graph batch sizing is independent");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_low_agent_override_stops_at_its_model_admission_limit() {
        use crate::harness::{self, GenericTool, LoopArgs, ModelHost, ModelRequest, ModelResponse, Stop};
        use serde_json::json;
        struct Repeating(usize);
        impl ModelHost for Repeating {
            fn infer<'a>(&'a mut self, _: ModelRequest) -> harness::LocalFuture<'a, Result<ModelResponse, String>> {
                self.0 += 1;
                let id = format!("call-{}", self.0);
                Box::pin(async move { Ok(ModelResponse { body: json!({"choices":[{"message":{
                    "role":"assistant", "tool_calls":[{"id":id,"type":"function",
                    "function":{"name":"edit","arguments":"{}"}}]}}]}) }) })
            }
        }
        let llm = crate::llm::Llm { api: "chat_completions".into(), reasoning_summary: None,  base_url:"unused".into(), model:"fixture".into(), api_key:String::new(),
            temperature:None, trace:None };
        let mut model = Repeating(0);
        let mut history = vec![json!({"role":"user","content":"Continue until the work is complete."})];
        let mut edits = 0;
        let stop = harness::run_with_host(LoopArgs {
            llm:&llm, history:&mut history,
            tools:&[GenericTool { name:"edit".into(), description:"Advance unfinished work".into(),
                parameters:json!({"type":"object","properties":{}}) }],
            dispatch:&mut |_,_| { edits += 1; Ok("More work remains.".into()) },
            emit:&mut |_|{}, cancelled:&||false, max_rounds:agent_rounds_from(Some("2")),
            label:"explicit low override".into(), conversation:false, inputs:None, input_close:None,
        }, "", &mut model, &mut harness::MemoryRecovery);
        assert_eq!(stop, Stop::MaxRounds);
        assert_eq!((model.0, edits), (2, 2));
        assert_eq!(crate::runner::unlanded_reason(&stop, model.0 as u32),
            "Agent reached the session limit after 2 model requests; unfinished work remains open");
    }

    #[test]
    fn registry_has_eight_rows_with_soft_below_hard() {
        assert_eq!(LIMITS.len(), 8);
        for l in &LIMITS {
            assert!(l.soft < l.hard, "{}", l.name);
            assert!(
                l.goal == "abstract-entity" || l.goal == "split-view",
                "{}",
                l.name
            );
            assert_eq!(limit(l.name).map(|x| x.hard), Some(l.hard));
        }
        assert!(limit("no-such-limit").is_none());
        assert_eq!(threshold("edges-per-view", None), Some((40, 60)));
        assert_eq!(threshold("edges-per-view", Some(70)), Some((70, 90)));
        assert_eq!(
            threshold("requirements-per-entity", Some(70)),
            Some((70, 100))
        );
        assert_eq!(
            threshold(CHILDREN_PER_ENTITY, None),
            Some((CHILDREN_PER_ENTITY_SOFT, CHILDREN_PER_ENTITY_HARD))
        );
        assert_eq!(threshold(CHILDREN_PER_ENTITY, None), Some((9, 15)));
        assert_eq!(threshold(CHILDREN_PER_ENTITY, Some(12)), Some((12, 18)));
        assert_eq!(
            threshold("edges-per-view", Some(u64::MAX)),
            Some((u64::MAX, u64::MAX))
        );
        assert_eq!(ENTITY_LIMITS.len() + VIEW_LIMITS.len(), LIMITS.len());
        for l in &LIMITS {
            assert!(ENTITY_LIMITS.contains(&l.name) || VIEW_LIMITS.contains(&l.name));
        }
    }
}
