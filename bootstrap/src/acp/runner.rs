// The session runner: executes one goal batch as an ACP worker session against the
// batch's resolved executor. Jazyk stays the client and sole initiator; the session
// gets one injected `jazyk mcp` serving scoped to the batch, the prompt is the
// assembled session prompt, and success is read from the store, never from the
// agent's word. Mirrors docs/frontends/acp.md#worker-sessions.
use super::config::{self, ResolvedAgent, EMBEDDED};
use super::host::{AcpHost, McpSpec};
use super::translate::UpdateTranslator;
use crate::llm::Llm;
use crate::model::Goal;
use crate::project::Project;
use crate::session::{Trace, TraceEvent};
use crate::tools::WorkScope;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// What one session left behind: the mutations its commits applied, the goals they
// resolved (from the journal), and whether the batch landed.
pub struct SessionReport {
    pub applied: usize,
    pub rounds: u32,
    pub tokens: u64,
    pub failed: Option<String>,
    // Goal ids the batch's journal entries resolved.
    pub resolved: Vec<String>,
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

    fn goal_ids(&self) -> Vec<String> {
        self.goals.iter().map(|g| g.id.clone()).collect()
    }

    fn kind(&self) -> String {
        self.goals
            .first()
            .map(|g| g.kind.clone())
            .unwrap_or_default()
    }

    fn class(&self) -> String {
        self.goals
            .first()
            .map(|g| g.class.clone())
            .unwrap_or_else(|| "compile".to_string())
    }

    // The serving the batch's goal kinds need: bind and generate work the ledger and
    // the deliverable, verify judges rows, everything else works the graph.
    fn serving_mode(&self) -> &'static str {
        let kinds: BTreeSet<&str> = self.goals.iter().map(|g| g.kind.as_str()).collect();
        if !kinds.is_empty() && kinds.iter().all(|k| matches!(*k, "bind" | "generate")) {
            "generate"
        } else if !kinds.is_empty() && kinds.iter().all(|k| *k == "verify") {
            "verify"
        } else {
            "compile"
        }
    }

    fn is_ledger(&self) -> bool {
        self.serving_mode() != "compile"
    }
}

pub struct AcpRunner {
    // One host per executor profile, spawned on the first session against it, so a
    // run with no work never pays for an agent (a no-op rebuild stays free).
    hosts: Mutex<BTreeMap<String, AcpHost>>,
    // The [acp] agent: chat, answer, and follow sessions run on it, and it is the
    // executor for every kind without an override.
    default_agent: ResolvedAgent,
    llm: Llm,
    project: Project,
    out: PathBuf,
    // The build's worker id when this runner is part of one internal build; its
    // servings skip the build-lease refusal and the release gate for their batches.
    build_token: Mutex<Option<String>>,
    // Attempts spent per batch, keyed by its first goal, so `sessionFailed` names the
    // attempt number the scheduler will count. A resolved goal clears its count.
    attempts: Mutex<BTreeMap<String, u32>>,
}

// One session attempt's terminal line, owed from `sessionStart` until exactly one of
// `sessionDone`, `sessionFailed`, or `sessionEnded` lands. Dropping it still open (a
// panic, an exit path nobody closed) emits `sessionEnded` naming that, so no attempt
// vanishes from the trace. Mirrors docs/compiler/sessions.md#trace-events.
pub(crate) struct Attempt {
    trace: Trace,
    label: String,
    goals: Vec<String>,
    attempt: u32,
    open: bool,
}

impl Attempt {
    pub(crate) fn open(trace: &Trace, start: TraceEvent, attempt: u32) -> Attempt {
        let (label, goals) = match &start {
            TraceEvent::SessionStart { label, goals, .. } => (label.clone(), goals.clone()),
            _ => unreachable!("an attempt opens on sessionStart"),
        };
        trace.event(start);
        Attempt {
            trace: trace.clone(),
            label,
            goals,
            attempt,
            open: true,
        }
    }

    // The batch landed: the store's word, never the agent's.
    pub(crate) fn done(mut self, staged: usize, rounds: u32) {
        self.open = false;
        self.trace.event(TraceEvent::SessionDone {
            label: self.label.clone(),
            goals: self.goals.clone(),
            staged,
            rounds,
            mode: "done".into(),
            summary: String::new(),
        });
    }

    // The attempt failed: the session errored, or the batch did not land.
    pub(crate) fn failed(mut self, error: &str) {
        self.open = false;
        self.trace.event(TraceEvent::SessionFailed {
            label: self.label.clone(),
            goals: self.goals.clone(),
            attempt: self.attempt,
            error: error.to_string(),
        });
    }

    // Nothing landed and the session is not to blame.
    pub(crate) fn ended(mut self, reason: &str) {
        self.open = false;
        self.trace.event(TraceEvent::SessionEnded {
            label: self.label.clone(),
            goals: self.goals.clone(),
            reason: reason.to_string(),
        });
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        if self.open {
            self.open = false;
            self.trace.event(TraceEvent::SessionEnded {
                label: self.label.clone(),
                goals: self.goals.clone(),
                reason: if std::thread::panicking() {
                    "the runner panicked mid-attempt".into()
                } else {
                    "the runner left the attempt without a verdict".into()
                },
            });
        }
    }
}

impl AcpRunner {
    // Resolve the default agent (config ladder; JAZYK_ACP_AGENT carries the --agent
    // flag). Hosts spawn lazily per profile on the first session.
    pub fn start(project: &Project, llm: &Llm, out: &Path) -> Result<AcpRunner, String> {
        let default_agent = config::resolve_acp(
            None,
            &project.acp,
            &crate::project::load_global_acp(),
            |name| std::env::var(name).ok(),
        )?;
        Ok(AcpRunner {
            hosts: Mutex::new(BTreeMap::new()),
            default_agent,
            llm: llm.clone(),
            project: project.clone(),
            out: out.to_path_buf(),
            build_token: Mutex::new(None),
            attempts: Mutex::new(BTreeMap::new()),
        })
    }

    pub fn agent(&self) -> &ResolvedAgent {
        &self.default_agent
    }

    // The executor for one batch: the kind override, then the class override, then
    // the [acp] agent. Mirrors docs/compiler/control-plane.md#executors.
    fn executor_for(&self, batch: &BatchRun) -> Result<ResolvedAgent, String> {
        config::resolve_executor(
            None,
            &batch.kind(),
            &batch.class(),
            &self.project.acp,
            &self.project.executors,
            &crate::project::load_global_acp(),
            &crate::project::load_global_executors(),
            |name| std::env::var(name).ok(),
        )
    }

    // The embedded profile prompts the resolved endpoint; external agents bring
    // their own model.
    fn extra_env_for(&self, agent: &ResolvedAgent) -> Vec<(String, String)> {
        if agent.name != EMBEDDED {
            return Vec::new();
        }
        let mut v = vec![
            ("JAZYK_LLM_BASE_URL".to_string(), self.llm.base_url.clone()),
            ("JAZYK_MODEL".to_string(), self.llm.model.clone()),
        ];
        if !self.llm.api_key.is_empty() {
            v.push(("JAZYK_API_KEY".to_string(), self.llm.api_key.clone()));
        }
        if let Some(t) = self.llm.temperature {
            v.push(("JAZYK_TEMPERATURE".to_string(), t.to_string()));
        }
        v
    }

    // One session on the named agent, spawning its host on first use. The lock
    // guards the spawn, not the session.
    fn session_on(
        &self,
        agent: &ResolvedAgent,
        mcp: Vec<McpSpec>,
    ) -> Result<super::host::SessionHandle, String> {
        let mut hosts = self.hosts.lock().unwrap();
        if !hosts.contains_key(&agent.name) {
            let host = AcpHost::start(
                agent.clone(),
                self.project.root.clone(),
                self.extra_env_for(agent),
            )?;
            hosts.insert(agent.name.clone(), host);
        }
        match hosts[&agent.name].new_session(
            &self.project.root,
            mcp.clone(),
            super::policy::PermissionPolicy::Auto,
        ) {
            // The cached host died since its spawn. Its death is not the batch's:
            // replace it once and only a spawn that fails again fails the caller.
            // Mirrors docs/frontends/acp.md#worker-sessions.
            Err(e) if e.contains("acp host") => {
                hosts.remove(&agent.name);
                let host = AcpHost::start(
                    agent.clone(),
                    self.project.root.clone(),
                    self.extra_env_for(agent),
                )?;
                let s = host.new_session(
                    &self.project.root,
                    mcp,
                    super::policy::PermissionPolicy::Auto,
                );
                hosts.insert(agent.name.clone(), host);
                s
            }
            r => r,
        }
    }

    fn session(&self, mcp: Vec<McpSpec>) -> Result<super::host::SessionHandle, String> {
        let agent = self.default_agent.clone();
        self.session_on(&agent, mcp)
    }

    // Mark this runner as part of a running internal build: its servings carry the
    // build's token. Cleared when the guard drops.
    pub fn set_build_token(&self, token: Option<String>) {
        *self.build_token.lock().unwrap() = token;
    }

    // One focused answer session: the chat serving injected, the handling contract
    // as the prompt. Used when a non-edit answer arrives from a frontend with no
    // live session. Mirrors docs/frontends/acp.md#answer-sessions.
    pub fn run_answer(&self, prompt: &str, _label: &str) -> Result<(), String> {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "jazyk".to_string());
        let spec = McpSpec {
            name: "jazyk".to_string(),
            command: exe,
            args: vec![
                "mcp".to_string(),
                "chat".to_string(),
                "--ephemeral".to_string(),
                "--out".to_string(),
                self.out.to_string_lossy().into_owned(),
            ],
            env: Vec::new(),
        };
        let session = self.session(vec![spec])?;
        let outcome = session.prompt(prompt, Arc::new(|_| {}));
        session.close();
        outcome.map(|_| ())
    }

    // The serving injected into one batch's session: the mode the goal class needs,
    // scoped to the batch. Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
    fn mcp_spec(&self, batch: &BatchRun, agent: &ResolvedAgent) -> McpSpec {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "jazyk".to_string());
        let mode = batch.serving_mode();
        let mut args = vec![
            "mcp".to_string(),
            mode.to_string(),
            "--ephemeral".to_string(),
            "--out".to_string(),
            self.out.to_string_lossy().into_owned(),
            "--only".to_string(),
            batch.id.clone(),
            // The contract travels as the session prompt; begin_goals answers with
            // an ack.
            "--packaged".to_string(),
        ];
        if let Some(t) = self.build_token.lock().unwrap().as_ref() {
            args.push("--build-token".to_string());
            args.push(t.clone());
        }
        if mode == "generate" && agent.serve_files {
            args.push("--serve-files".to_string());
        }
        McpSpec {
            name: "jazyk".to_string(),
            command: exe,
            args,
            env: Vec::new(),
        }
    }

    // The prompt for one batch: the assembled session prompt, protocol line naming
    // the batch. Binding and generation packages ride the begin_* replies; the
    // prompt is the same assembly for every kind.
    // Mirrors docs/compiler/sessions.md#the-prompt.
    fn prompt_for(&self, batch: &BatchRun) -> String {
        let mut store = crate::store::Store::load(&self.out);
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        store.sync_docs(&parsed);
        let control = crate::control::Control::load(&self.project, &self.out);
        let (loaded, skills) = crate::session::initial_loaded(&store, &batch.goals);
        let mut pb = crate::session::ProjectBlock::compute(&store, &batch.goals, &control.compile);
        pb.batch = batch.id.clone();
        crate::session::session_prompt(&store, &batch.goals, &loaded, &skills, &pb)
    }

    // Run one goal batch as one session. The commit happens inside the injected
    // serving; the report is derived from the journal and the board afterwards.
    pub fn run_item(&self, batch: &BatchRun, trace: &Trace) -> SessionReport {
        let label = batch.id.clone();
        let goal_ids = batch.goal_ids();
        let scope = WorkScope::for_batch(&batch.id, &batch.goals);
        let failed_report = |failed: String, rounds: u32, tokens: u64| SessionReport {
            applied: 0,
            rounds,
            tokens,
            failed: Some(failed),
            resolved: Vec::new(),
        };
        // The attempt opens before anything can fail, so every exit below closes it
        // with exactly one terminal event (sessions.md#trace-events).
        let first_goal = goal_ids.first().cloned().unwrap_or_else(|| label.clone());
        let attempt_no = self
            .attempts
            .lock()
            .unwrap()
            .get(&first_goal)
            .copied()
            .unwrap_or(0)
            + 1;
        let attempt = Attempt::open(
            trace,
            TraceEvent::SessionStart {
                label: label.clone(),
                goals: goal_ids.clone(),
                task: batch.kind(),
                target: batch
                    .goals
                    .first()
                    .map(|g| g.target.clone())
                    .unwrap_or_default(),
                doc: scope.doc(),
                sections: scope
                    .reconcile_scopes()
                    .iter()
                    .flat_map(|g| g.sections.iter().cloned())
                    .collect(),
                dirty: scope
                    .reconcile_scopes()
                    .iter()
                    .map(|g| g.sections.len())
                    .sum(),
                stale: scope.stale_anchors().len(),
                proposals: scope.proposals().len(),
            },
            attempt_no,
        );
        let report = self.run_attempt(batch, trace, attempt, &failed_report);
        // A goal that resolved starts its count over; anything else spent an attempt.
        let mut attempts = self.attempts.lock().unwrap();
        if report.resolved.contains(&first_goal) {
            attempts.remove(&first_goal);
        } else {
            attempts.insert(first_goal, attempt_no);
        }
        report
    }

    // The attempt proper. Every return closes `attempt`: `done` when the store says
    // the batch landed, `failed` when the session errored or the batch did not land,
    // `ended` when nothing landed through no fault of the session (the build was
    // cancelled, or the batch is a ledger batch its caller judges). The guard's drop
    // covers a panic. Mirrors docs/compiler/sessions.md#trace-events.
    fn run_attempt(
        &self,
        batch: &BatchRun,
        trace: &Trace,
        attempt: Attempt,
        failed_report: &dyn Fn(String, u32, u64) -> SessionReport,
    ) -> SessionReport {
        let label = batch.id.clone();
        let goal_ids = batch.goal_ids();
        let scope = WorkScope::for_batch(&batch.id, &batch.goals);
        let agent = match self.executor_for(batch) {
            Ok(a) => a,
            Err(e) => {
                let e = format!("executor: {}", e);
                attempt.failed(&e);
                return failed_report(e, 0, 0);
            }
        };
        let gen_before = crate::store::read_generation(&self.out);
        let session = match self.session_on(&agent, vec![self.mcp_spec(batch, &agent)]) {
            Ok(s) => s,
            Err(e) => {
                let e = format!("session: {}", e);
                attempt.failed(&e);
                return failed_report(e, 0, 0);
            }
        };
        let translator = Arc::new(Mutex::new(
            UpdateTranslator::new(&label).with_doc(scope.doc()),
        ));
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cb_translator = translator.clone();
        let cb_trace = trace.clone();
        let cb_calls = calls.clone();
        let on_update: super::host::OnUpdate = Arc::new(move |ev| {
            if let super::host::HostEvent::Update(u) = ev {
                if matches!(
                    u,
                    agent_client_protocol::schema::v1::SessionUpdate::ToolCall(_)
                ) {
                    cb_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                cb_translator.lock().unwrap().on_update(u, &cb_trace);
            }
        });
        let mut outcome = session.prompt(&self.prompt_for(batch), on_update.clone());
        // One reminder when the session ended in prose with the batch uncommitted:
        // the generic agent ends its turn on a plain answer by design, so the client
        // owns the "you are mid-batch" reminder. A second prose ending fails the
        // session through the board check below.
        // Mirrors docs/frontends/acp.md#worker-sessions.
        if !batch.is_ledger() {
            if let Ok(o) = &outcome {
                if o.stop == "end_turn" && !o.idled {
                    let board = crate::board::Board::compute(&self.project, &self.out);
                    if goal_ids.iter().any(|id| board.open(id)) {
                        outcome = session.prompt(
                            &format!(
                                "The goals are not resolved: {}. Continue with the tool calls the instructions name, mark each goal with mark_goal_done or mark_goal_failed, then finish with done.",
                                goal_ids.join(", ")
                            ),
                            on_update,
                        );
                    }
                }
            }
        }
        session.close();
        let mut t = translator.lock().unwrap();
        t.finish(trace);
        let tokens = t.tokens;
        drop(t);
        crate::llm::add_tokens(tokens);
        let rounds = calls.load(std::sync::atomic::Ordering::Relaxed);

        let stop = match outcome {
            Ok(o) => o,
            Err(e) => {
                // A dead host poisons every later session: drop it so the batch
                // retry spawns a fresh one.
                if e.contains("acp host") {
                    self.hosts.lock().unwrap().remove(&agent.name);
                }
                attempt.failed(&e);
                return failed_report(e, rounds, tokens);
            }
        };

        let gen_after = crate::store::read_generation(&self.out);
        let (applied, resolved) = journal_diff(&self.out, gen_before, gen_after, &goal_ids);
        let stopped = format!(
            "session stopped: {}{}",
            stop.stop,
            if stop.idled {
                ", idle watchdog fired"
            } else {
                ""
            }
        );

        // A ledger batch is judged by its caller against the ledger: the attempt ends
        // with the verdict left to it.
        if batch.is_ledger() {
            attempt.ended(&format!("the ledger judges what landed ({})", stopped));
            return SessionReport {
                applied,
                rounds,
                tokens,
                failed: None,
                resolved,
            };
        }
        // Success is the store's word: the batch's goals must be resolved, failed,
        // or parked; a batch whose goals all still stand open did not land.
        let board = crate::board::Board::compute(&self.project, &self.out);
        let still_open: Vec<&String> = goal_ids.iter().filter(|id| board.open(id)).collect();
        let landed = !(resolved.is_empty() && !still_open.is_empty());
        let failed = close_compile_attempt(
            attempt,
            landed,
            trace.is_cancelled(),
            &stopped,
            applied,
            rounds,
        );
        SessionReport {
            applied,
            rounds,
            tokens,
            failed,
            resolved,
        }
    }

    // One-shot prose completion through a bare session (no tools): the ACP form of
    // the old `llm.chat`, for the medium decision, llm-row judgment, and drafting.
    pub fn ask(&self, system: &str, user: &str, label: &str, step: &str) -> Result<String, String> {
        self.ask_traced(system, user, label, step, None)
    }

    // The traced form: the prompt size goes out as a note, the reply as model text,
    // so a transcript shows the one-shot beside the sessions.
    pub fn ask_traced(
        &self,
        system: &str,
        user: &str,
        label: &str,
        step: &str,
        trace: Option<&Trace>,
    ) -> Result<String, String> {
        if let Some(t) = trace {
            t.line(
                label,
                &format!("→ ask {} ({} chars)", step, system.len() + user.len()),
            );
        }
        let session = self
            .session(Vec::new())
            .map_err(|e| format!("session: {}", e))?;
        let text: Arc<Mutex<String>> = Default::default();
        let sink = text.clone();
        let prompt = format!("{}\n\n{}", system, user);
        let outcome = session.prompt(
            &prompt,
            Arc::new(move |ev| {
                if let super::host::HostEvent::Update(
                    agent_client_protocol::schema::v1::SessionUpdate::AgentMessageChunk(c),
                ) = ev
                {
                    if let agent_client_protocol::schema::v1::ContentBlock::Text(t) = &c.content {
                        sink.lock().unwrap().push_str(&t.text);
                    }
                }
            }),
        );
        session.close();
        let outcome = outcome?;
        let text = text.lock().unwrap().clone();
        if text.trim().is_empty() {
            return Err(format!(
                "empty reply (session stopped: {}) for {}",
                outcome.stop, label
            ));
        }
        if let Some(t) = trace {
            t.event(crate::session::TraceEvent::ModelText {
                label: label.to_string(),
                text: crate::llm::truncate(&text, 2_000),
            });
        }
        Ok(text)
    }
}

// Attribute the journal entries between two generations to the batch, and read what
// they resolved. The exact semantics live in the store's commit; this reads what it
// wrote. Mirrors docs/frontends/acp.md#worker-sessions.
fn journal_diff(out: &Path, from: u64, to: u64, goal_ids: &[String]) -> (usize, Vec<String>) {
    let mut applied = 0usize;
    let mut resolved: Vec<String> = Vec::new();
    for g in (from + 1)..=to {
        let path = out.join("journal").join(format!("g{}.yaml", g));
        let Ok(text) = std::fs::read_to_string(&path) else {
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

// The terminal line of a compile attempt whose session returned: `sessionDone` when
// the batch landed, `sessionEnded` when it did not because the build was cancelled
// (the session is not at fault, but the report still says nothing landed), and
// `sessionFailed` otherwise. Returns the report's failure.
// Mirrors docs/compiler/sessions.md#trace-events.
fn close_compile_attempt(
    attempt: Attempt,
    landed: bool,
    cancelled: bool,
    stopped: &str,
    applied: usize,
    rounds: u32,
) -> Option<String> {
    match (landed, cancelled) {
        (true, _) => {
            attempt.done(applied, rounds);
            None
        }
        (false, true) => {
            let reason = format!("the build was cancelled ({})", stopped);
            attempt.ended(&reason);
            Some(reason)
        }
        (false, false) => {
            let e = format!("the batch did not land ({})", stopped);
            attempt.failed(&e);
            Some(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::TraceLevel;
    use std::sync::atomic::AtomicBool;

    // A trace whose events collect in a vector, with the build's cancel flag exposed.
    fn capture() -> (Trace, Arc<Mutex<Vec<TraceEvent>>>, Arc<AtomicBool>) {
        let events: Arc<Mutex<Vec<TraceEvent>>> = Default::default();
        let sink = events.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let trace = Trace::to_sink(
            TraceLevel::Normal,
            Arc::new(move |ev| sink.lock().unwrap().push(ev.clone())),
            cancel.clone(),
        );
        (trace, events, cancel)
    }

    fn start(label: &str) -> TraceEvent {
        TraceEvent::SessionStart {
            label: label.into(),
            goals: vec!["g:reconcile-section:shop.md#/shop".into()],
            task: "reconcile-section".into(),
            target: "shop.md#/shop".into(),
            doc: Some("shop.md".into()),
            sections: vec![],
            dirty: 1,
            stale: 0,
            proposals: 0,
        }
    }

    // The terminal events among what a label produced, as (kind, detail).
    fn terminals(events: &[TraceEvent], label: &str) -> Vec<(String, String)> {
        events
            .iter()
            .filter_map(|ev| match ev {
                TraceEvent::SessionDone { label: l, mode, .. } if l == label => {
                    Some(("sessionDone".to_string(), mode.clone()))
                }
                TraceEvent::SessionFailed {
                    label: l,
                    attempt,
                    error,
                    ..
                } if l == label => Some((
                    "sessionFailed".to_string(),
                    format!("{} {}", attempt, error),
                )),
                TraceEvent::SessionEnded {
                    label: l, reason, ..
                } if l == label => Some(("sessionEnded".to_string(), reason.clone())),
                _ => None,
            })
            .collect()
    }

    // A failed attempt closes with exactly one `sessionFailed` carrying the attempt
    // number; the scheduler's own `sessionFailed` for the same attempt is dropped.
    // Mirrors docs/compiler/sessions.md#trace-events.
    #[test]
    fn a_failed_attempt_leaves_exactly_one_terminal_event() {
        let (trace, events, _) = capture();
        let attempt = Attempt::open(&trace, start("b15-2"), 2);
        let failed =
            close_compile_attempt(attempt, false, false, "session stopped: end_turn", 0, 3);
        assert_eq!(
            failed.as_deref(),
            Some("the batch did not land (session stopped: end_turn)")
        );
        // The scheduler restates the failure after the runner returns.
        trace.event(TraceEvent::SessionFailed {
            label: "b15-2".into(),
            goals: vec![],
            attempt: 2,
            error: failed.clone().unwrap(),
        });
        let t = terminals(&events.lock().unwrap(), "b15-2");
        assert_eq!(t.len(), 1, "{:?}", t);
        assert_eq!(t[0].0, "sessionFailed");
        assert!(t[0].1.starts_with("2 the batch did not land"), "{}", t[0].1);
        // A host death before the prompt closes the same way.
        let attempt = Attempt::open(&trace, start("b15-2"), 3);
        attempt.failed("session: acp host is gone");
        let t = terminals(&events.lock().unwrap(), "b15-2");
        assert_eq!(t.len(), 2, "{:?}", t);
        assert_eq!(t[1].1, "3 session: acp host is gone");
    }

    // A cancelled attempt with nothing landed closes with `sessionEnded` naming the
    // cancellation, and the report still says nothing landed; a landed batch stays
    // `sessionDone` even when the cancel flag is up.
    // Mirrors docs/compiler/sessions.md#trace-events.
    #[test]
    fn a_cancelled_attempt_leaves_a_terminal_event() {
        let (trace, events, cancel) = capture();
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let attempt = Attempt::open(&trace, start("b21"), 1);
        let failed = close_compile_attempt(
            attempt,
            false,
            trace.is_cancelled(),
            "session stopped: cancelled",
            0,
            0,
        );
        assert!(failed.is_some());
        let t = terminals(&events.lock().unwrap(), "b21");
        assert_eq!(t.len(), 1, "{:?}", t);
        assert_eq!(t[0].0, "sessionEnded");
        assert_eq!(
            t[0].1,
            "the build was cancelled (session stopped: cancelled)"
        );
        let attempt = Attempt::open(&trace, start("b22"), 1);
        assert!(
            close_compile_attempt(attempt, true, true, "session stopped: end_turn", 4, 6).is_none()
        );
        let t = terminals(&events.lock().unwrap(), "b22");
        assert_eq!(t, vec![("sessionDone".to_string(), "done".to_string())]);
    }

    // An attempt the runner never closes (a panic mid-attempt) still ends with one
    // `sessionEnded` naming that, and a ledger attempt ends with the verdict left to
    // its ledger. Mirrors docs/compiler/sessions.md#trace-events.
    #[test]
    fn an_abandoned_attempt_still_leaves_a_terminal_event() {
        let (trace, events, _) = capture();
        let t2 = trace.clone();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _attempt = Attempt::open(&t2, start("b30"), 1);
            panic!("mid-attempt");
        }));
        assert!(outcome.is_err());
        let t = terminals(&events.lock().unwrap(), "b30");
        assert_eq!(t.len(), 1, "{:?}", t);
        assert_eq!(t[0].0, "sessionEnded");
        assert!(t[0].1.contains("panicked"), "{}", t[0].1);
        let attempt = Attempt::open(&trace, start("g:generate:ent:cart"), 1);
        attempt.ended("the ledger judges what landed (session stopped: end_turn)");
        let t = terminals(&events.lock().unwrap(), "g:generate:ent:cart");
        assert_eq!(t.len(), 1);
        assert!(t[0].1.starts_with("the ledger judges"));
    }
}
