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
        hosts[&agent.name].new_session(
            &self.project.root,
            mcp,
            super::policy::PermissionPolicy::Auto,
        )
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
        let agent = match self.executor_for(batch) {
            Ok(a) => a,
            Err(e) => return failed_report(format!("executor: {}", e), 0, 0),
        };
        trace.event(TraceEvent::SessionStart {
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
        });
        let gen_before = crate::store::read_generation(&self.out);
        let session = match self.session_on(&agent, vec![self.mcp_spec(batch, &agent)]) {
            Ok(s) => s,
            Err(e) => return failed_report(format!("session: {}", e), 0, 0),
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
            Err(e) => return failed_report(e, rounds, tokens),
        };

        let gen_after = crate::store::read_generation(&self.out);
        let (applied, resolved) = journal_diff(&self.out, gen_before, gen_after, &goal_ids);

        // Success is the store's word: the batch's goals must be resolved, failed,
        // or parked; a batch whose goals all still stand open did not land. Ledger
        // batches are judged by their callers against the ledger.
        let failed = if batch.is_ledger() {
            None
        } else {
            let board = crate::board::Board::compute(&self.project, &self.out);
            let still_open: Vec<&String> = goal_ids.iter().filter(|id| board.open(id)).collect();
            if resolved.is_empty() && !still_open.is_empty() {
                Some(format!(
                    "the batch did not land (session stopped: {}{})",
                    stop.stop,
                    if stop.idled {
                        ", idle watchdog fired"
                    } else {
                        ""
                    }
                ))
            } else {
                None
            }
        };
        if failed.is_none() && !batch.is_ledger() {
            trace.event(TraceEvent::SessionDone {
                label,
                goals: goal_ids,
                staged: applied,
                rounds,
                mode: "done".into(),
                summary: String::new(),
            });
        }
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
