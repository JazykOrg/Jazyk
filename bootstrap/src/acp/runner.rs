// The turn runner: executes one work item as an ACP worker session against the
// configured agent. Jazyk stays the client and sole initiator; the session gets one
// injected `jazyk mcp` serving scoped to the task, the prompt is fixed and
// agent-neutral, and success is read from the store, never from the agent's word.
// Mirrors docs/frontends/acp.md#worker-sessions.
use super::config::{self, ResolvedAgent, EMBEDDED};
use super::host::{AcpHost, McpSpec};
use super::translate::UpdateTranslator;
use crate::llm::Llm;
use crate::model::WorkItem;
use crate::project::Project;
use crate::turn::{Trace, TraceEvent};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct TurnReport {
    pub applied: usize,
    // Entity ids the item's commits touched, requirement ids they changed: what the
    // reconciler schedules reviews from, derived from the journal entries the item
    // landed.
    pub touched: BTreeSet<String>,
    pub changed: BTreeSet<String>,
    pub rounds: u32,
    pub tokens: u64,
    pub failed: Option<String>,
}

pub struct AcpRunner {
    // The agent spawns on the first session, so a run with no work never pays for
    // one (a no-op rebuild stays free).
    host: Mutex<Option<AcpHost>>,
    agent: ResolvedAgent,
    extra_env: Vec<(String, String)>,
    project: Project,
    out: PathBuf,
    // The build's worker id when this runner is part of one internal build; its
    // servings skip the build-lease refusal and the release gate for their targets.
    build_token: Mutex<Option<String>>,
}

impl AcpRunner {
    // Resolve the agent (config ladder; JAZYK_ACP_AGENT carries the --agent flag).
    // The embedded profile gets the resolved LLM settings as environment. The agent
    // process itself spawns lazily on the first session.
    pub fn start(project: &Project, llm: &Llm, out: &Path) -> Result<AcpRunner, String> {
        let agent = config::resolve_acp(
            None,
            &project.acp,
            &crate::project::load_global_acp(),
            |name| std::env::var(name).ok(),
        )?;
        let extra_env = if agent.name == EMBEDDED {
            let mut v = vec![
                ("JAZYK_LLM_BASE_URL".to_string(), llm.base_url.clone()),
                ("JAZYK_MODEL".to_string(), llm.model.clone()),
            ];
            if !llm.api_key.is_empty() {
                v.push(("JAZYK_API_KEY".to_string(), llm.api_key.clone()));
            }
            if let Some(t) = llm.temperature {
                v.push(("JAZYK_TEMPERATURE".to_string(), t.to_string()));
            }
            v
        } else {
            Vec::new()
        };
        Ok(AcpRunner {
            host: Mutex::new(None),
            agent,
            extra_env,
            project: project.clone(),
            out: out.to_path_buf(),
            build_token: Mutex::new(None),
        })
    }

    pub fn agent(&self) -> &ResolvedAgent {
        &self.agent
    }

    // One session, spawning the agent on first use. The lock guards the spawn, not
    // the session: concurrent items share the one host.
    fn session(&self, mcp: Vec<McpSpec>) -> Result<super::host::SessionHandle, String> {
        let mut h = self.host.lock().unwrap();
        if h.is_none() {
            *h = Some(AcpHost::start(
                self.agent.clone(),
                self.project.root.clone(),
                self.extra_env.clone(),
            )?);
        }
        h.as_ref().unwrap().new_session(
            &self.project.root,
            mcp,
            super::policy::PermissionPolicy::Auto,
        )
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

    // The serving injected into one work item's session.
    fn mcp_spec(&self, item: &WorkItem) -> McpSpec {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "jazyk".to_string());
        let modes = match item.task.as_str() {
            "bind-requirement" | "generate-entity" => "generate",
            _ => "compile",
        };
        let mut args = vec![
            "mcp".to_string(),
            modes.to_string(),
            "--ephemeral".to_string(),
            "--out".to_string(),
            self.out.to_string_lossy().into_owned(),
        ];
        if modes == "compile" {
            args.push("--only".to_string());
            args.push(item.target.clone());
            // The contract travels as the session prompt; begin answers with an ack.
            args.push("--packaged".to_string());
        }
        if let Some(t) = self.build_token.lock().unwrap().as_ref() {
            args.push("--build-token".to_string());
            args.push(t.clone());
        }
        if modes == "generate" && self.agent.serve_files {
            args.push("--serve-files".to_string());
        }
        McpSpec {
            name: "jazyk".to_string(),
            command: exe,
            args,
            env: Vec::new(),
        }
    }

    // The prompt for one work item. Compilation tasks carry their full contract (the
    // task instructions and the work package) directly: a prompt is what a model
    // reads best, and the serving's begin call answers with a short ack instead of
    // repeating it. Binding and generation packages still ride the begin reply.
    // Mirrors docs/frontends/acp.md#worker-sessions.
    fn prompt_for(&self, item: &WorkItem) -> String {
        if !matches!(item.task.as_str(), "bind-requirement" | "generate-entity") {
            let mut store = crate::store::Store::load(&self.out);
            let (parsed, _) = crate::reconcile::parse_all(&self.project);
            store.sync_docs(&parsed);
            let gs = crate::gen::GenSettings::resolve(&self.project);
            let (system, pack) = crate::turn::task_prompt(&store, item, &self.project.linting, &gs);
            return format!(
                "{}\n\n{}\n\n{}",
                crate::turn::with_feedback_note(system),
                pack,
                include_str!("../../../docs/compiler/goals/prompts/worker-protocol.md")
                    .replace("{target}", &item.target)
            );
        }
        match item.task.as_str() {
            "bind-requirement" => {
                include_str!("../../../docs/compiler/goals/prompts/bind-pointer.md")
                    .replace("{target}", &item.target)
            }
            "generate-entity" => {
                include_str!("../../../docs/compiler/goals/prompts/generate-pointer.md")
                    .replace("{target}", &item.target)
            }
            _ => unreachable!("compilation prompts are packaged above"),
        }
    }

    // Run one work item as one session. The commit happens inside the injected
    // serving; the report is derived from the journal and the queue afterwards.
    pub fn run_item(&self, item: &WorkItem, trace: &Trace) -> TurnReport {
        let label = format!("{} {}", item.task, item.target);
        trace.event(TraceEvent::SessionStart {
            label: label.clone(),
            goals: vec![item.goal_id()],
            task: item.task.clone(),
            target: item.target.clone(),
            doc: matches!(item.task.as_str(), "reconcile-doc" | "align-doc")
                .then(|| item.target.clone()),
            sections: item.dirty_sections.clone(),
            dirty: item.dirty_sections.len(),
            stale: item.stale_anchors.len(),
            proposals: item.proposals.len(),
        });
        let gen_before = crate::store::read_generation(&self.out);
        let session = match self.session(vec![self.mcp_spec(item)]) {
            Ok(s) => s,
            Err(e) => {
                return TurnReport {
                    applied: 0,
                    touched: BTreeSet::new(),
                    changed: BTreeSet::new(),
                    rounds: 0,
                    tokens: 0,
                    failed: Some(format!("session: {}", e)),
                }
            }
        };
        let translator = Arc::new(Mutex::new(UpdateTranslator::new(&label)));
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
        let mut outcome = session.prompt(&self.prompt_for(item), on_update.clone());
        // One reminder when the session ended in prose with the batch uncommitted:
        // the generic agent ends its turn on a plain answer by design, so the client
        // owns the "you are mid-task" reminder. A second prose ending fails the
        // session through the board check below. Mirrors
        // docs/frontends/acp.md#worker-sessions.
        if !matches!(item.task.as_str(), "bind-requirement" | "generate-entity") {
            if let Ok(o) = &outcome {
                if o.stop == "end_turn" && !o.idled {
                    let board = crate::board::Board::compute(&self.project, &self.out);
                    if board.item_open(item) {
                        outcome = session.prompt(
                            &format!(
                                "The task is not finished: `{} {}` has not committed. Continue with the tool calls the instructions name, then finish with done.",
                                item.task, item.target
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
                return TurnReport {
                    applied: 0,
                    touched: BTreeSet::new(),
                    changed: BTreeSet::new(),
                    rounds,
                    tokens,
                    failed: Some(e),
                }
            }
        };

        let gen_after = crate::store::read_generation(&self.out);
        let (applied, touched, changed) = journal_diff(&self.out, gen_before, gen_after, item);

        // Success is the store's word: a compilation item must have left the board,
        // a bind or generation item is judged by its caller against the ledger.
        let failed = match item.task.as_str() {
            "bind-requirement" | "generate-entity" => None,
            _ => {
                let board = crate::board::Board::compute(&self.project, &self.out);
                if board.item_open(item) {
                    Some(format!(
                        "the task did not land (session stopped: {}{})",
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
            }
        };
        if failed.is_none() && !matches!(item.task.as_str(), "bind-requirement" | "generate-entity")
        {
            trace.event(TraceEvent::SessionDone {
                label,
                goals: vec![item.goal_id()],
                staged: applied,
                rounds,
                mode: "done".into(),
                summary: String::new(),
            });
        }
        TurnReport {
            applied,
            touched,
            changed,
            rounds,
            tokens,
            failed,
        }
    }

    // One-shot prose completion through a bare session (no tools): the ACP form of
    // the old `llm.chat`, for the medium decision, llm-row judgment, and drafting.
    pub fn ask(&self, system: &str, user: &str, label: &str, step: &str) -> Result<String, String> {
        self.ask_traced(system, user, label, step, None)
    }

    // The traced form: the prompt size goes out as a note, the reply as model text,
    // so a transcript shows the one-shot beside the turns.
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
            t.event(crate::turn::TraceEvent::ModelText {
                label: label.to_string(),
                text: crate::llm::truncate(&text, 2_000),
            });
        }
        Ok(text)
    }
}

// Attribute the journal entries between two generations to the work item, and pull
// the scheduling sets out of their mutations. The exact semantics live in the store's
// commit; this reads what it wrote. Mirrors docs/frontends/acp.md#worker-sessions.
fn journal_diff(
    out: &Path,
    from: u64,
    to: u64,
    item: &WorkItem,
) -> (usize, BTreeSet<String>, BTreeSet<String>) {
    let mut applied = 0usize;
    let mut touched = BTreeSet::new();
    let mut changed = BTreeSet::new();
    for g in (from + 1)..=to {
        let path = out.join("journal").join(format!("g{}.yaml", g));
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(entry) = serde_norway::from_str::<crate::model::JournalEntry>(&text) else {
            continue;
        };
        if entry.kind != "session" || !entry.batch.iter().any(|g| *g == item.goal_id()) {
            continue;
        }
        applied += entry.mutations.len();
        for m in &entry.mutations {
            let Some(o) = m.as_object() else { continue };
            for (kind, body) in o {
                let id = |v: &Value| v.as_str().map(|s| s.to_string());
                match kind.as_str() {
                    "CreateEntity" | "UpdateEntity" => {
                        touched.extend(id(&body["id"]));
                    }
                    "MergeEntities" => {
                        touched.extend(id(&body["keep"]));
                    }
                    "CreateRequirement" => {
                        changed.extend(id(&body["id"]));
                        if let Some(ents) = body["requirement"]["entities"].as_array() {
                            touched.extend(
                                ents.iter()
                                    .filter_map(|e| e.as_str().map(|s| s.to_string())),
                            );
                        }
                    }
                    "UpdateRequirement" => {
                        if !body["statement"].is_null() {
                            changed.extend(id(&body["id"]));
                        }
                        if let Some(ents) = body["entities"].as_array() {
                            touched.extend(
                                ents.iter()
                                    .filter_map(|e| e.as_str().map(|s| s.to_string())),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    (applied, touched, changed)
}
