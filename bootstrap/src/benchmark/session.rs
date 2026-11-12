use crate::llm::Llm;
use crate::store::Store;

use super::*;

// One case as a turn on the generic loop: the same GDK runtime the embedded agent runs,
// dispatching into a ToolSession over the sandbox. What run_turn used to be, scoped
// to grading. Mirrors docs/benchmark/benchmark.md#runs.
// How a case turn fell short: the session aborted, or the model marked the batch's
// goal failed (docs/benchmark/benchmark.md#runs).
pub(super) enum TurnFail {
    Abort(String),
    GoalFailed(String),
}

// A case's session before any call: the tool session over the sandbox snapshot,
// the prompt a build would assemble, and the goal's toolset. A benchmark turn drives
// it with a model; a replay drives it with scripted steps.
// Mirrors docs/benchmark/replays.md#replay-format.
pub struct CaseSession {
    pub session: crate::tools::ToolSession,
    pub prompt: String,
    pub tools: Vec<crate::harness::GenericTool>,
}

pub fn case_session(
    snapshot: Store,
    item: &crate::model::WorkItem,
    lint: &crate::project::Linting,
    gs: &crate::gen::GenSettings,
    derived: Option<&crate::model::Goal>,
) -> CaseSession {
    case_session_batch(snapshot, item, lint, gs, derived, &[])
}

// The sections a reconcile-doc case's batch owns: those that owe a coverage mark, as
// a build derives them (a fenced block follows its holding section, a bare heading owes
// none). Mirrors docs/benchmark/cases.md#case-format.
pub fn case_dirty_sections(store: &Store, case: &Case) -> Vec<String> {
    if case.task_type != "reconcile-doc" {
        return Vec::new();
    }
    store
        .docs
        .get(&case.target)
        .map(|r| {
            r.sections
                .iter()
                .filter(|(_, s)| crate::goals::section_has_body(s))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default()
}

// The same session over a batch: the derived goal plus the batch's other derived
// goals, as a replay of a recorded batch holds them
// (docs/benchmark/replays.md#replay-format).
pub fn case_session_batch(
    snapshot: Store,
    item: &crate::model::WorkItem,
    lint: &crate::project::Linting,
    gs: &crate::gen::GenSettings,
    derived: Option<&crate::model::Goal>,
    extra: &[crate::model::Goal],
) -> CaseSession {
    use crate::harness::GenericTool;
    use crate::tools::{catalog, toolset, ToolSession, WorkScope};
    let goal = match derived {
        Some(g) => g.clone(),
        None => item.to_goal(crate::model::GoalState::Open),
    };
    let mut goals: Vec<crate::model::Goal> = vec![goal.clone()];
    goals.extend(extra.iter().cloned());
    let scope = match derived {
        Some(g) => WorkScope::for_batch(&format!("snippet-{}", g.id), &goals),
        // An assembled primary with batch mates scopes the whole batch, as a
        // recorded session did (docs/benchmark/replays.md#replay-format).
        None if !extra.is_empty() => WorkScope::for_batch(&format!("replay-{}", goal.id), &goals),
        None => WorkScope::from_item(item),
    };
    // The case's own lint rules ride on the project block (docs/benchmark/cases.md).
    // The batch is open and the toolset holds no begin_goals: the open-batch protocol
    // line (docs/compiler/sessions.md#the-prompt).
    let prompt = crate::session::open_batch_prompt(&snapshot, &goals, lint);
    // The session holds what its prompt lists, as a build session does
    // (docs/benchmark/benchmark.md#runs): the same seeding `begin_goals` applies.
    let (loaded, skills) = crate::session::initial_loaded(&snapshot, &goals);
    let names = toolset(&item.task);
    let tools: Vec<GenericTool> = catalog()
        .iter()
        .filter(|t| names.contains(&t.name))
        .map(|t| GenericTool {
            name: t.name.to_string(),
            description: t.description.to_string(),
            parameters: t.parameters.clone(),
        })
        .collect();
    let mut s = ToolSession::new(
        snapshot,
        scope,
        crate::limits::SESSION_MUTATIONS,
        crate::limits::CONTEXT_BUDGET,
    );
    s.gen = gs.clone();
    s.lint = lint.clone();
    s.loaded = loaded;
    s.skills = skills;
    // The prompt opened with the deliverable contracts a build session opens with
    // (docs/benchmark/benchmark.md#runs).
    s.opening_contracts = true;
    s.caller = crate::feedback::Caller {
        source: "benchmark".into(),
        target: item.target.clone(),
        ..Default::default()
    };
    CaseSession {
        session: s,
        prompt,
        tools,
    }
}

pub(super) fn run_case_turn(
    llm: &Llm,
    snapshot: Store,
    item: &crate::model::WorkItem,
    lint: &crate::project::Linting,
    gs: &crate::gen::GenSettings,
    transcript: Option<&std::path::Path>,
    // A snippet runs a goal the board derived, change and hints intact; a case
    // runs the goal its item describes. Mirrors docs/benchmark/benchmark.md#snippets-from-a-real-project.
    derived: Option<&crate::model::Goal>,
) -> (
    Vec<crate::store::Op>,
    u32,
    Option<TurnFail>,
    crate::store::Commit,
) {
    use crate::harness::{self, AgentEvent, LoopArgs, Stop};
    let CaseSession {
        session,
        prompt,
        tools,
    } = case_session(snapshot, item, lint, gs, derived);
    let session = std::cell::RefCell::new(session);
    let mut history = vec![serde_json::json!({"role": "user", "content": prompt})];
    let rounds = std::cell::Cell::new(0u32);
    let mut dispatch = |name: &str, args: &serde_json::Value| -> Result<String, String> {
        match session.borrow_mut().dispatch(name, args) {
            Ok(v) => Ok(v.to_string()),
            Err(e) => Err(e.to_value().to_string()),
        }
    };
    let mut emit = |ev: AgentEvent| {
        if let AgentEvent::Usage { .. } = ev {
            rounds.set(rounds.get() + 1);
        }
    };
    let round_budget = crate::limits::SESSION_ROUNDS
        .max(item.dirty_sections.len() as u32 * crate::limits::ROUNDS_PER_SECTION);
    let mut stop = harness::run_loop(LoopArgs {
        llm,
        history: &mut history,
        tools: &tools,
        dispatch: &mut dispatch,
        emit: &mut emit,
        // `done` ends the turn: the cancel check is how the loop learns it.
        cancelled: &|| session.borrow().done.is_some(),
        max_rounds: round_budget,
        label: format!("bench {}", item.target),
        conversation: false,
        inputs: None,
        input_close: None,
    });
    // The production client sends one mid-task reminder when a turn ends in prose
    // without finishing (docs/frontends/acp.md#worker-sessions); the benchmark
    // client mirrors it so a graded turn faces the same harness a real one does.
    if matches!(stop, Stop::EndTurn) && session.borrow().done.is_none() {
        history.push(serde_json::json!({"role": "user", "content":
            include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/benchmark-session/the-task-is-not-finished-nothing-has-cdcf1.md"))}));
        stop = harness::run_loop(LoopArgs {
            llm,
            history: &mut history,
            tools: &tools,
            dispatch: &mut dispatch,
            emit: &mut emit,
            cancelled: &|| session.borrow().done.is_some(),
            max_rounds: round_budget,
            label: format!("bench {} (reminded)", item.target),
            conversation: false,
            inputs: None,
            input_close: None,
        });
    }
    let mut s = session.into_inner();
    // A goal the model marked failed fails its case, done or not: every fixture is
    // resolvable by design, so the claim measures the model, not the fixture.
    // Mirrors docs/benchmark/benchmark.md#runs.
    let failed = if let Some((_, reason)) = s.failed_goals().into_iter().next() {
        Some(TurnFail::GoalFailed(reason))
    } else if s.done.is_some() {
        None
    } else {
        match stop {
            Stop::Error(e) => Some(TurnFail::Abort(e)),
            _ => {
                // Graph turns land staged work or fail; generation and binding land
                // in the ledger and the deliverable, so their checks are the judge.
                if s.finish_implicit("(implicit: the turn ended without done)")
                    || matches!(item.task.as_str(), "generate-entity" | "bind-requirement")
                {
                    None
                } else {
                    Some(TurnFail::Abort(
                        "the turn ended without done and nothing valid was staged".to_string(),
                    ))
                }
            }
        }
    };
    // The full exchange, kept beside the scores: a failed check is diagnosed from
    // what the model saw and said. Mirrors docs/benchmark/benchmark.md#running-a-subset.
    if let Some(path) = transcript {
        if let Some(dir) = path.parent() {
            crate::vfs::create_dir_all(dir).ok();
        }
        crate::vfs::write(
            path,
            serde_json::to_string_pretty(&history).unwrap_or_default(),
        )
        .ok();
    }
    let commit = s.commit(rounds.get(), Some(0));
    (std::mem::take(&mut s.staged), rounds.get(), failed, commit)
}
