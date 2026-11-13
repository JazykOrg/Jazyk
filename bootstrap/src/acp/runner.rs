use crate::runner::{journal_diff, BatchRun, SessionReport, SessionRunner};
// The session runner: executes one goal batch as an ACP worker session against the
// batch's resolved executor. Jazyk stays the client and sole initiator; the session
// gets one injected `jazyk mcp` serving scoped to the batch, the prompt is the
// assembled session prompt, and success is read from the store, never from the
// agent's word. Mirrors docs/frontends/acp.md#worker-sessions.
use super::config::{self, McpTransport, ResolvedAgent, EMBEDDED};
use super::host::{AcpHost, McpEntry, McpSpec, SessionHandle};
use super::http::HttpServing;
use super::translate::UpdateTranslator;
use crate::llm::Llm;
use crate::mcp::BridgeFlags;
use crate::project::Project;
use crate::session::{Trace, TraceEvent};
use crate::tools::WorkScope;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// The serving one session gets, before its transport is known: the toolsets and the
// bridge flags. Rendered as the `jazyk mcp` command line for a stdio entry, or
// started in process for an HTTP entry (docs/frontends/mcp.md#mcp-over-http).
struct ServingPlan {
    modes: Vec<String>,
    flags: BridgeFlags,
}

impl ServingPlan {
    fn stdio_spec(&self, out: &Path) -> McpSpec {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "jazyk".to_string());
        let mut args = vec![
            "mcp".to_string(),
            self.modes.join(","),
            "--out".to_string(),
            out.to_string_lossy().into_owned(),
        ];
        if let Some(id) = out
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|id| crate::document_sets::valid_id(id))
        {
            args.extend(["--set".into(), id.to_string()]);
        }
        if self.flags.ephemeral {
            args.push("--ephemeral".to_string());
        }
        if let Some(only) = &self.flags.only {
            args.push("--only".to_string());
            args.push(only.clone());
        }
        if self.flags.packaged {
            args.push("--packaged".to_string());
        }
        if let Some(t) = &self.flags.build_token {
            args.push("--build-token".to_string());
            args.push(t.clone());
        }
        if let Some(path) = &self.flags.cancel_file {
            args.extend(["--cancel-file".into(), path.to_string_lossy().into_owned()]);
        }
        if self.flags.serve_files {
            args.push("--serve-files".to_string());
        }
        if let Some(sink) = &self.flags.edit_sink {
            args.push("--edit-sink".to_string());
            args.push(sink.clone());
        }
        McpSpec {
            name: "jazyk".to_string(),
            command: exe,
            args,
            env: Vec::new(),
        }
    }

    fn http_serving(&self, project: &Project, out: &Path) -> Result<HttpServing, String> {
        HttpServing::start(
            project.clone(),
            out.to_path_buf(),
            self.modes.clone(),
            self.flags.clone(),
        )
    }
}

// A session with its serving: the HTTP serving lives exactly as long as the session
// and is stopped after it closes; a stdio serving is the agent's child and needs
// nothing from us.
struct Opened {
    session: SessionHandle,
    serving: Option<HttpServing>,
}

impl Opened {
    // Close the session (the agent tears down; a stdio serving runs its implicit
    // finish on the EOF), then stop the HTTP serving (its implicit finish).
    fn close(self) {
        self.session.close();
        if let Some(h) = self.serving {
            h.stop();
        }
    }
}

pub struct AcpRunner {
    // One host per executor profile, spawned on the first session against it, so a
    // run with no work never pays for an agent (a no-op rebuild stays free).
    hosts: Mutex<BTreeMap<String, AcpHost>>,
    // The [acp] agent: chat, answer, and follow sessions run on it, and it is the
    // executor for every kind without an override.
    default_agent: ResolvedAgent,
    selected: bool,
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
        Self::start_selected(project, llm, out, None)
    }

    pub fn start_selected(
        project: &Project,
        llm: &Llm,
        out: &Path,
        selected: Option<&str>,
    ) -> Result<AcpRunner, String> {
        if crate::vfs::is_scoped() {
            return Err("hosted workspaces use the in-process runner".into());
        }
        let default_agent = config::resolve_acp(
            selected,
            &project.acp,
            &crate::project::load_global_acp(),
            |name| std::env::var(name).ok(),
        )?;
        Ok(AcpRunner {
            hosts: Mutex::new(BTreeMap::new()),
            default_agent,
            selected: selected.is_some(),
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
            |name| {
                if self.selected {
                    None
                } else {
                    std::env::var(name).ok()
                }
            },
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
            ("JAZYK_API_KEY".to_string(), self.llm.api_key.clone()),
            ("JAZYK_LLM_API".to_string(), self.llm.api.clone()),
        ];
        if let Some(key_env) = self
            .project
            .llm
            .api_key_env
            .clone()
            .or_else(|| crate::project::load_global_llm().api_key_env)
        {
            v.push((key_env, self.llm.api_key.clone()));
        }
        if let Some(summary) = &self.llm.reasoning_summary {
            v.push(("JAZYK_REASONING_SUMMARY".into(), summary.clone()));
        }
        if let Some(t) = self.llm.temperature {
            v.push(("JAZYK_TEMPERATURE".to_string(), t.to_string()));
        }
        v
    }

    // One session on the named agent, spawning its host on first use. The lock
    // guards the spawn, not the session. The serving's transport follows what the
    // host's initialize reply advertised (docs/frontends/acp.md#worker-sessions).
    fn session_on(&self, agent: &ResolvedAgent, plan: &ServingPlan) -> Result<Opened, String> {
        let mut hosts = self.hosts.lock().unwrap();
        if !hosts.contains_key(&agent.name) {
            let host = AcpHost::start(
                agent.clone(),
                self.project.root.clone(),
                self.extra_env_for(agent),
            )?;
            hosts.insert(agent.name.clone(), host);
        }
        match self.open_on(&hosts[&agent.name], plan) {
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
                let s = self.open_on(&host, plan);
                hosts.insert(agent.name.clone(), host);
                s
            }
            r => r,
        }
    }

    // One session on a live host: the serving over the transport the host's agent
    // takes, started before the session and stopped after it.
    fn open_on(&self, host: &AcpHost, plan: &ServingPlan) -> Result<Opened, String> {
        let transport =
            config::choose_transport(host.info().mcp_http, config::mcp_transport_pin().as_deref());
        let (entry, serving) = match transport {
            McpTransport::Stdio => (McpEntry::Stdio(plan.stdio_spec(&self.out)), None),
            McpTransport::Http => {
                let serving = plan.http_serving(&self.project, &self.out)?;
                let entry = McpEntry::Http {
                    name: "jazyk".to_string(),
                    url: serving.url.clone(),
                    headers: vec![serving.header()],
                };
                (entry, Some(serving))
            }
        };
        let session = host.new_session_with(
            &self.project.root,
            vec![entry],
            super::policy::PermissionPolicy::Auto,
        )?;
        Ok(Opened { session, serving })
    }

    fn session(&self, plan: &ServingPlan) -> Result<Opened, String> {
        let agent = self.default_agent.clone();
        self.session_on(&agent, plan)
    }

    // A session with no serving at all, for one-shot prose completions on the
    // default agent. No transport to choose.
    fn bare_session(&self) -> Result<SessionHandle, String> {
        let agent = self.default_agent.clone();
        let mut hosts = self.hosts.lock().unwrap();
        if !hosts.contains_key(&agent.name) {
            let host = AcpHost::start(
                agent.clone(),
                self.project.root.clone(),
                self.extra_env_for(&agent),
            )?;
            hosts.insert(agent.name.clone(), host);
        }
        let open = |host: &AcpHost| {
            host.new_session(
                &self.project.root,
                Vec::new(),
                super::policy::PermissionPolicy::Auto,
            )
        };
        match open(&hosts[&agent.name]) {
            Err(e) if e.contains("acp host") => {
                hosts.remove(&agent.name);
                let host = AcpHost::start(
                    agent.clone(),
                    self.project.root.clone(),
                    self.extra_env_for(&agent),
                )?;
                let s = open(&host);
                hosts.insert(agent.name.clone(), host);
                s
            }
            r => r,
        }
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
        let plan = ServingPlan {
            modes: vec!["chat".to_string()],
            flags: BridgeFlags {
                ephemeral: true,
                ..Default::default()
            },
        };
        let opened = self.session(&plan)?;
        let outcome = opened.session.prompt(prompt, Arc::new(|_| {}));
        opened.close();
        outcome.map(|_| ())
    }

    // The serving injected into one batch's session: the mode the goal class needs,
    // scoped to the batch. Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
    fn serving_plan(&self, batch: &BatchRun, agent: &ResolvedAgent) -> ServingPlan {
        let mode = batch.serving_mode();
        ServingPlan {
            modes: vec![mode.to_string()],
            flags: BridgeFlags {
                ephemeral: true,
                only: Some(batch.id.clone()),
                // The contract travels as the session prompt; begin_goals answers
                // with an ack.
                packaged: true,
                build_token: self.build_token.lock().unwrap().clone(),
                cancel_file: None,
                serve_files: matches!(mode, "generate" | "verify") && agent.serve_files,
                edit_sink: None,
                progressive: false,
                reader: false,
            },
        }
    }

    // The prompt for one batch: the assembled session prompt, protocol line naming
    // the batch. Binding and generation packages ride the begin_* replies; the
    // prompt is the same assembly for every kind.
    // Mirrors docs/compiler/sessions.md#the-prompt.
    fn prompt_for(&self, batch: &BatchRun) -> String {
        crate::runner::batch_prompt(&self.project, &self.out, batch)
    }

    // Run one goal batch as one session. The commit happens inside the injected
    // serving; the report is derived from the journal and the board afterwards.
    pub fn run_item(&self, batch: &BatchRun, trace: &Trace) -> SessionReport {
        let label = batch.id.clone();
        let goal_ids = batch.goal_ids();
        let scope = WorkScope::for_batch(&batch.id, &batch.goals);
        let failed_report = |failed: String, rounds: u32, tokens: u64, cached: u64| SessionReport {
            applied: 0,
            rounds,
            tokens,
            cached,
            failed: Some(failed),
            resolved: Vec::new(),
            stopped: None,
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
        let _preview_guard = crate::live_preview::SidecarGuard::start(&self.out, &label);
        let report = self.run_attempt(batch, trace, attempt, &failed_report);
        let prior = crate::live_preview::read(&self.out, &label);
        let store = crate::store::Store::load(&self.out);
        trace.event(TraceEvent::GraphPreview {
            preview: crate::live_preview::Preview {
                label: label.clone(),
                attempt: prior
                    .as_ref()
                    .map(|p| p.attempt.clone())
                    .unwrap_or_default(),
                revision: prior.map_or(1, |p| p.revision + 1),
                phase: "committed".into(),
                generation: store.status.generation,
                graph: Some(crate::live_preview::authored(&store.graph)),
                reason: None,
            }
            .bounded(),
        });
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
        failed_report: &dyn Fn(String, u32, u64, u64) -> SessionReport,
    ) -> SessionReport {
        let label = batch.id.clone();
        let goal_ids = batch.goal_ids();
        let scope = WorkScope::for_batch(&batch.id, &batch.goals);
        let agent = match self.executor_for(batch) {
            Ok(a) => a,
            Err(e) => {
                let e = format!("executor: {}", e);
                attempt.failed(&e);
                return failed_report(e, 0, 0, 0);
            }
        };
        let gen_before = crate::store::read_generation(&self.out);
        let mut plan = self.serving_plan(batch, &agent);
        let mut nonce = [0u8; 16];
        if let Err(e) = getrandom::fill(&mut nonce) {
            return failed_report(e.to_string(), 0, 0, 0);
        }
        let name: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
        // A cancel signal is machine state, kept in the set's local/ folder.
        let cancel_file = crate::layout::local_dir(&self.out).join(format!("cancel-{name}"));
        plan.flags.cancel_file = Some(cancel_file.clone());
        let opened = match self.session_on(&agent, &plan) {
            Ok(s) => s,
            Err(e) => {
                let e = format!("session: {}", e);
                attempt.failed(&e);
                return failed_report(e, 0, 0, 0);
            }
        };
        // The transport is news only when it is not the default.
        if let Some(h) = &opened.serving {
            trace.event(TraceEvent::Note {
                label: label.clone(),
                text: format!("mcp serving: http on 127.0.0.1:{}", h.port),
                verbose: false,
            });
        }
        let session = opened.session.clone();
        let translator = Arc::new(Mutex::new(
            UpdateTranslator::new(&label).with_doc(scope.doc()),
        ));
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        // The agent's own error, when its loop failed: it arrives as a message chunk
        // and becomes the session's stop reason (docs/frontends/acp.md#the-embedded-agent).
        let agent_error: Arc<Mutex<Option<String>>> = Default::default();
        let cb_translator = translator.clone();
        let cb_trace = trace.clone();
        let cb_calls = calls.clone();
        let cb_error = agent_error.clone();
        let preview_out = self.out.clone();
        let preview_label = label.clone();
        let preview_revision = Mutex::new(0u64);
        let on_update: super::host::OnUpdate = Arc::new(move |ev| {
            if let super::host::HostEvent::Update(u) = ev {
                if matches!(
                    u,
                    agent_client_protocol::schema::v1::SessionUpdate::ToolCall(_)
                ) {
                    cb_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                if let agent_client_protocol::schema::v1::SessionUpdate::AgentMessageChunk(c) = u {
                    if let agent_client_protocol::schema::v1::ContentBlock::Text(t) = &c.content {
                        if let Some(e) = t.text.strip_prefix("agent error:") {
                            *cb_error.lock().unwrap() = Some(e.trim().to_string());
                        }
                    }
                }
                cb_translator.lock().unwrap().on_update(u, &cb_trace);
                if let Some(preview) = crate::live_preview::read(&preview_out, &preview_label) {
                    let mut revision = preview_revision.lock().unwrap();
                    if preview.revision > *revision {
                        *revision = preview.revision;
                        cb_trace.event(TraceEvent::GraphPreview { preview });
                    }
                }
            }
        });
        let cancelled = || {
            if !trace.is_cancelled() {
                return false;
            }
            crate::vfs::create_dir_all(cancel_file.parent().unwrap())
                .expect("cancellation fence directory");
            crate::vfs::write(&cancel_file, b"stopped").expect("cancellation fence");
            true
        };
        let mut outcome =
            session.prompt_cancellable(&self.prompt_for(batch), on_update.clone(), &cancelled);
        // One reminder when the session ended in prose with the batch uncommitted:
        // the generic agent ends its turn on a plain answer by design, so the client
        // owns the "you are mid-batch" reminder. A second prose ending fails the
        // session through the board check below.
        // Mirrors docs/frontends/acp.md#worker-sessions.
        if !batch.is_ledger() && !cancelled() {
            if let Ok(o) = &outcome {
                if o.stop == "end_turn" && !o.idled {
                    let board = crate::board::Board::compute(&self.project, &self.out);
                    if goal_ids.iter().any(|id| board.open(id)) {
                        outcome = session.prompt_cancellable(
                            &format!(
                                "The goals are not resolved: {}. Continue with the tool calls the instructions name, mark each goal with mark_goal_done or mark_goal_failed, then finish with done.",
                                goal_ids.join(", ")
                            ),
                            on_update,
                            &cancelled,
                        );
                    }
                }
            }
        }
        opened.close();
        let mut t = translator.lock().unwrap();
        t.finish(trace);
        let tokens = t.tokens;
        let cached = t.cached;
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
                return failed_report(e, rounds, tokens, cached);
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
        // The reason a caller judging its own ledger names when nothing landed.
        let stop_reason: Option<String> = match stop.stop.as_str() {
            "end_turn" if !stop.idled => None,
            "end_turn" => Some("the idle watchdog fired".to_string()),
            "refusal" => Some(
                agent_error
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| "the agent refused the prompt".to_string()),
            ),
            "max_turn_requests" => Some(crate::runner::unlanded_reason(
                &crate::harness::Stop::MaxRounds,
                rounds,
            )),
            "cancelled" => Some("the session was cancelled".to_string()),
            other => Some(format!("the session stopped: {}", other)),
        };

        // A ledger batch is judged by its caller against the ledger: the attempt ends
        // with the verdict left to it.
        if batch.is_ledger() {
            attempt.ended(&format!("the ledger judges what landed ({})", stopped));
            return SessionReport {
                applied,
                rounds,
                tokens,
                cached,
                failed: (!trace.is_cancelled() && stop.stop == "refusal")
                    .then(|| stop_reason.clone())
                    .flatten(),
                resolved,
                stopped: stop_reason,
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
            cached,
            failed,
            resolved,
            stopped: stop_reason,
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
        let session = self.bare_session().map_err(|e| format!("session: {}", e))?;
        let text: Arc<Mutex<String>> = Default::default();
        let sink = text.clone();
        let prompt = format!("{}\n\n{}", system, user);
        let outcome = session.prompt_cancellable(
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
            &|| trace.is_some_and(|t| t.is_cancelled()),
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
                summary: false,
                label: label.to_string(),
                text: crate::llm::truncate(&text, 2_000),
                reply: true,
            });
        }
        Ok(text)
    }
}

// Attribute the journal entries between two generations to the batch, and read what
// they resolved. The exact semantics live in the store's commit; this reads what it
// wrote. Mirrors docs/frontends/acp.md#worker-sessions.

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

impl SessionRunner for AcpRunner {
    fn agent_name(&self) -> String {
        self.agent().name.clone()
    }
    fn set_build_token(&self, token: Option<String>) {
        AcpRunner::set_build_token(self, token)
    }
    fn run_item(&self, batch: &BatchRun, trace: &Trace) -> SessionReport {
        AcpRunner::run_item(self, batch, trace)
    }
    fn ask(&self, system: &str, user: &str, label: &str, step: &str) -> Result<String, String> {
        AcpRunner::ask(self, system, user, label, step)
    }
    fn ask_traced(
        &self,
        system: &str,
        user: &str,
        label: &str,
        step: &str,
        trace: Option<&Trace>,
    ) -> Result<String, String> {
        AcpRunner::ask_traced(self, system, user, label, step, trace)
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
