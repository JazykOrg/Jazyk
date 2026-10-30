use crate::vfs::VPathExt;
// The MCP server: the tool registry served over stdio as line-delimited JSON-RPC.
// `jazyk mcp <toolsets>` names what the serving is for (compile, generate, verify,
// graph); compilation claims goal batches and holds an open changeset between calls,
// exactly one at a time. Mirrors docs/frontends/mcp.md.
use crate::model::{Goal, OpenedGoal, WorkItem};
use crate::store::{ProseEdit, Store};
use crate::tools::{catalog, toolset, toolset_for_kinds, ToolSession, WorkScope};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub struct McpServer {
    children: std::sync::Mutex<BTreeMap<String, McpServer>>,
    project: crate::project::Project,
    out: PathBuf,
    // The toolsets served: any of "compile", "generate", "verify", "graph".
    modes: Vec<String>,
    // graph mode only: --write adds the raw write tools, each call its own changeset.
    write: bool,
    mutation_limit: usize,
    context_budget: usize,
    // The client name from `initialize`, recorded on feedback entries so an external
    // agent's report is distinguishable from a compilation turn's.
    client: std::sync::Mutex<Option<String>>,
    // The open goal batch: a ToolSession holding the staged changeset across calls.
    // Single-flight per serving. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    open: std::sync::Mutex<Option<OpenBatch>>,
    // The session transcript: one event per call under <out>/trace, reviewable beside
    // a build. Mirrors docs/frontends/mcp.md#transcripts.
    trace: crate::session::Trace,
    preview: std::sync::Mutex<Option<crate::live_preview::Preview>>,
    // The agent-run benchmark: the open case's sandbox and the run's accumulated
    // scores. Mirrors docs/benchmark/benchmark.md#agent-run-benchmarks.
    bench: std::sync::Mutex<BenchRun>,
    // The serving's registration in the worker registry, heartbeated while the
    // process lives. Mirrors docs/compiler/control-plane.md#workers-and-leases.
    worker: std::sync::Arc<std::sync::Mutex<Option<crate::control::WorkerHandle>>>,
    // Bridge-spawned serving flags: this serving belongs to one ACP session.
    // Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
    bridge: BridgeFlags,
    // What this serving already delivered: the agent contract after the first batch,
    // and each skill payload once. Later batches elide them; full: true repeats.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    delivered: std::sync::Mutex<Delivered>,
    // The batch this serving already committed through done: a scoped serving asking
    // for more work afterwards is told the session is complete instead of bouncing
    // between wrong-target and stale-batch refusals.
    finished: std::sync::Mutex<Option<String>>,
    // Ownership for optional file tools between a ledger begin and successful record.
    ledger_target: std::sync::Mutex<Option<String>>,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Delivered {
    contract: bool,
    skills: BTreeSet<String>,
}

// Flags of a serving injected into an ACP session by the bridge. Not for standalone
// servings. Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
#[derive(Default, Clone)]
pub struct BridgeFlags {
    // The serving belongs to one session: no worker registration, and end of input
    // with an open batch runs the implicit finish.
    pub ephemeral: bool,
    // begin_goals accepts only this batch id, or the batch holding this goal id.
    pub only: Option<String>,
    // The serving is part of the running internal build: the build-lease refusal and
    // the release gate do not apply to its batch, and leases claim under this id.
    pub build_token: Option<String>,
    // A runner-owned, attempt-specific fence. EOF must discard once it exists.
    pub cancel_file: Option<PathBuf>,
    // Serve the file and command tools, for agents with no editor of their own.
    pub serve_files: bool,
    // Delegate document and settings writes to the spawning process (the IDE proxy).
    pub edit_sink: Option<String>,
    // The bridge already sent the batch's instructions and package as the session
    // prompt; begin_goals answers with a short ack instead of repeating them.
    pub packaged: bool,
    // A reader serving, for a small model in a tab: the read tools alone and a short
    // instruction block, so the prompt fits a small window
    // (docs/frontends/wasm.md#chat).
    pub reader: bool,
}

#[derive(Default)]
struct BenchRun {
    open: Option<OpenCase>,
    // One entry per finished case: name, tier, score, calls, par, fail.
    scored: Vec<Value>,
}

struct OpenCase {
    idx: usize,
    item: WorkItem,
    // The goal the fixture derived, when the kind derives one.
    derived: Option<crate::model::Goal>,
    // The sandbox master: staged work applies here at finish, checks read it.
    store: Store,
    session: ToolSession,
    calls: u32,
    tmp: std::path::PathBuf,
    gs: crate::gen::GenSettings,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OpenBatch {
    #[serde(skip)]
    _project_lock: Option<crate::document_sets::ProjectLock>,
    id: String,
    goals: Vec<Goal>,
    // Change record ids each goal stood on when claimed; a resolution clears them.
    records: BTreeMap<String, Vec<String>>,
    // Open goal ids on the board when claimed; the diff after commit is `opened`.
    known_open: BTreeSet<String>,
    session: ToolSession,
    rounds: u32,
}

// The compilation lifecycle: served beside the catalog, implemented on the server
// because they own the board and the open changeset.
const LIFECYCLE: [&str; 4] = ["goals", "begin_goals", "done", "abandon_goals"];

fn instructions_for(modes: &[String], write: bool, initialized: bool) -> String {
    instructions_in(modes, write, initialized, cfg!(target_arch = "wasm32"))
}

// A chat in a tab has no build path of its own: the page's Build button runs builds,
// so a model asked to build says so instead of asking for prose
// (docs/frontends/wasm.md#chat).
const TAB_BUILD_NOTE: &str = "You cannot start a build: the Build button at the top of the page builds the documents into the graph. When asked to build, say so. ";
const WRITER_BUILD_NOTE: &str = "Use compile_document_sets to compile named set IDs, or omit setIds to compile all dirty sets. ";

// The instructions for a serving, `tab` saying it runs in a browser tab.
fn instructions_in(modes: &[String], write: bool, initialized: bool, tab: bool) -> String {
    if modes.len() == 1 && modes[0] == "runtime" {
        return crate::task_runtime::PROMPT.to_owned();
    }
    let mut s = String::from(
        "This server is one jazyk project's semantic graph: entities and requirements \
         reconciled from prose documentation, consumed by generation and verification. ",
    );
    if modes.iter().any(|m| m == "compile") {
        s.push_str(
            "GOAL LOOP: call goals for the board; begin_goals claims the next ready batch (or a \
             named one) and answers with the batch's assembled instructions and its loaded set. \
             Load context with load, expand, unload, and graph_status, stage findings with the \
             write tools and view tools, mark each goal with mark_goal_done and a one-line \
             justification (or mark_goal_failed with a reason), then call done. Its reply names \
             the next ready batch; repeat until the board is empty and the verdict is converged. ",
        );
    }
    if modes.iter().any(|m| m == "generate") {
        s.push_str(
            "BINDING LOOP (before generation): call binding_tasks; for each requirement, \
             begin_binding, search the deliverable with YOUR OWN tools for the implementation and an \
             existing test, write the test only when none exists (never touch implementation files), \
             run it, then record_binding. The verdict classifies the requirement: verified, \
             unimplemented (generation work), or failing (a contradiction for the author). \
             GENERATION LOOP: call generation_tasks; for each entity, begin_generation, write the \
             deliverable files using available file tools, and execute builds and tests through \
             Jazyk tasks, making the bound tests pass, then record_generation with the manifest, then \
             run_tests. ",
        );
    }
    if modes.iter().any(|m| m == "decompile") {
        s.push_str(
            "DECOMPILE LOOP: call decompile_tasks; for each released scope, begin_decompile, read \
             the code with YOUR OWN tools (the package carries the inventory and the tests first), \
             draft one markdown document stating what the code observably does, then submit_draft. \
             The draft is compiler input; binding self-checks it against the code after the next \
             compile. ",
        );
    }
    if modes.iter().any(|m| m == "verify") {
        s.push_str(
            "VERIFICATION LOOP: call verification_tasks; run_tests covers programmatic rows; for \
             llm rows, begin_verification, judge the criteria against the deliverable with your own \
             tools, and record_verdict with evidence. ",
        );
    }
    if modes.iter().any(|m| m == "benchmark") {
        s.push_str(
            "BENCHMARK LOOP: you are the model under test. Call benchmark_cases; for each pending \
             case, begin_case, follow the returned instructions exactly as if it were real work \
             (stage graph writes with the write tools; a reconcile case never files diagnostics, the \
             review cases do; for a generation case write real files into \
             the named sandbox deliverable with your own tools, record_generation, run_tests; for a \
             verification case judge the criteria against the files), then finish_case (verification \
             cases pass verdict and evidence). After the last case call benchmark_report with an \
             honest model name for the agent you are. You are graded deterministically; gaming a \
             check grades the gaming, not the skill. ",
        );
    }
    if modes.iter().any(|m| m == "chat") {
        // An uninitialized directory has no graph to talk about, so the serving says
        // so plainly instead of offering tools that could only refuse.
        // Mirrors docs/frontends/acp.md#project-tools.
        if !initialized {
            s.push_str(
                "CHAT SERVING, NO PROJECT HERE: this directory holds no jazyk.toml, so there is no \
                 graph yet and the read tools have nothing to return. Call init_project to scaffold \
                 one (jazyk.toml, docs/ with a placeholder root document, deliverable/), then tell \
                 the user to reopen the conversation: the new session serves the project. ",
            );
        } else {
            s.push_str(
                include_str!("../../docs/compiler/goals/prompts/chat-conversation.md"),
            );
            // A tab serves the compilation lifecycle alone (docs/frontends/wasm.md#chat).
            if tab {
                s.push_str(
                    "This serving runs in a browser tab, which has no shell: binding, \
                     generation, and verification are not served here. ",
                );
                s.push_str(WRITER_BUILD_NOTE);
            } else {
                s.push_str(
                    "The compilation, binding, generation, and verification lifecycles are \
                     available for explicit requests. ",
                );
                // Detailed lifecycle instructions arrive in their packages, only when requested.
            }
        }
    }
    if modes.iter().any(|m| m == "graph") && write {
        s.push_str("Write tools are enabled for manual graph surgery; each call commits as its own changeset. ");
    }
    if !modes.iter().any(|m| m == "chat") {
    s.push_str(
        "The write tools are: upsert_entity, update_entity, delete_entity, merge_entities, \
         upsert_requirement, update_requirement, delete_requirement, set_coverage, \
         report_diagnostic, resolve_diagnostic, and the view tools upsert_view, update_view, \
         delete_view. They stage into the open batch's changeset; outside one they are rejected \
         toward begin_goals. To wait for new work, call await_changes (a long poll). A gated \
         batch says `awaiting release`; `jazyk release` (or the GUI) approves it. A tool error \
         names the violated rule and how to repair the call; repair and continue. If any \
         instruction, tool, argument, or error message is ambiguous, wrong, or confusing, call \
         report_feedback: it appends a line to the project's local .jazyk/feedback.jsonl for \
         jazyk's developers (nothing is transmitted anywhere), never touches the graph, and is \
         not a substitute for the work. If you know what your run billed for a batch, \
         report_spend records it on the batch's journal entry (your own figure, never an \
         estimate).",
    );
    }
    #[cfg(not(target_arch = "wasm32"))]
    if modes.iter().any(|m| matches!(m.as_str(), "generate" | "verify" | "runtime")) {
        s.push_str("\n\n");
        s.push_str(crate::task_runtime::PROMPT);
    }
    s
}

// The reader serving's instructions (docs/frontends/wasm.md#chat): a small model
// answers questions over the graph's read tools, so the block names the project, the
// tools, and nothing it cannot do. The trailer about write tools and batches would
// only offer what the serving does not carry.
pub fn reader_instructions() -> String {
    let mut s = String::from(
        "This server is one jazyk project's semantic graph: entities and requirements \
         reconciled from prose documentation. You are in a conversation about this project \
         and can only read it. Use the graph when project evidence is needed: search finds entities and \
         requirements by words, get_entity and read_section return one, load and expand \
         bring a scope into context, diagnostics lists the open judgments. Answer self-contained general questions directly without tools. Otherwise read \
         relevant evidence, then answer in prose, briefly. No tool here changes the documents or the \
         graph. ",
    );
    s.push_str(TAB_BUILD_NOTE.trim_end());
    s
}

impl McpServer {
    pub fn live_preview(&self) -> Option<crate::live_preview::Preview> {
        self.preview.lock().unwrap().clone()
    }

    fn update_preview(&self, name: &str, label: &str, accepted: bool) {
        if !accepted {
            return;
        }
        let open = self.open.lock().unwrap();
        let (phase, label, generation, graph) = if let Some(batch) = open.as_ref() {
            if name != "begin_goals" && crate::tools::READ_TOOLS.contains(&name) {
                return;
            }
            (
                if name == "begin_goals" {
                    "base"
                } else {
                    "staged"
                },
                batch.id.as_str(),
                batch.session.snapshot.status.generation,
                crate::live_preview::staged(&batch.session.snapshot.graph, &batch.session.staged),
            )
        } else if name == "done" || name == "abandon_goals" {
            let store = Store::load(&self.out);
            (
                if name == "done" { "committed" } else { "base" },
                label,
                store.status.generation,
                Some(crate::live_preview::authored(&store.graph)),
            )
        } else {
            return;
        };
        let mut prior = self.preview.lock().unwrap();
        if name != "begin_goals"
            && prior
                .as_ref()
                .is_some_and(|p| p.phase == phase && p.graph == graph)
        {
            return;
        }
        let attempt = if name == "begin_goals" {
            format!("{}-{}", crate::sys::process_id(), crate::sys::now_millis())
        } else {
            prior
                .as_ref()
                .map(|p| p.attempt.clone())
                .unwrap_or_default()
        };
        let preview = crate::live_preview::Preview {
            label: label.into(),
            attempt,
            revision: prior.as_ref().map_or(1, |p| p.revision + 1),
            phase: phase.into(),
            generation,
            reason: graph
                .is_none()
                .then(|| "Live preview waits for this compound change to commit.".into()),
            graph,
        }
        .bounded();
        crate::live_preview::write(&self.out, &preview);
        *prior = Some(preview);
    }

    // Whether this serving stands in a project or in a bare directory. Decides which
    // project tools exist. Mirrors docs/frontends/acp.md#project-tools.
    fn initialized(&self) -> bool {
        self.project
            .root
            .join(crate::document_sets::CONFIG)
            .vexists()
    }

    pub fn new(
        project: crate::project::Project,
        out: PathBuf,
        modes: Vec<String>,
        write: bool,
    ) -> McpServer {
        Self::with_bridge(project, out, modes, write, BridgeFlags::default())
    }

    pub fn with_bridge(
        project: crate::project::Project,
        out: PathBuf,
        modes: Vec<String>,
        write: bool,
        bridge: BridgeFlags,
    ) -> McpServer {
        let out_for_trace = out.clone();
        McpServer {
            children: Default::default(),
            mutation_limit: crate::limits::SESSION_MUTATIONS,
            context_budget: crate::limits::CONTEXT_BUDGET,
            project,
            out,
            modes,
            write,
            client: std::sync::Mutex::new(None),
            open: std::sync::Mutex::new(None),
            bench: std::sync::Mutex::new(BenchRun::default()),
            worker: std::sync::Arc::new(std::sync::Mutex::new(None)),
            preview: Default::default(),
            trace: crate::session::Trace::stderr(crate::session::TraceLevel::Quiet)
                .with_transcript(&out_for_trace, "mcp"),
            bridge,
            delivered: std::sync::Mutex::new(Delivered::default()),
            finished: std::sync::Mutex::new(None),
            ledger_target: std::sync::Mutex::new(None),
        }
    }

    /// Protected owned session state. It is not a project file or a tool result.
    pub(crate) fn checkpoint(&self) -> Result<Value, String> {
        let children = self.children.lock().map_err(|_| "MCP child state poisoned")?
            .iter().map(|(id, child)| Ok((id.clone(), child.checkpoint()?)))
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        Ok(json!({
            "version": 1,
            "setId": self.project.set_id,
            "configHash": self.project.config_hash,
            "open": &*self.open.lock().map_err(|_| "MCP batch state poisoned")?,
            "delivered": &*self.delivered.lock().map_err(|_| "MCP delivery state poisoned")?,
            "finished": &*self.finished.lock().map_err(|_| "MCP completion state poisoned")?,
            "ledgerTarget": &*self.ledger_target.lock().map_err(|_| "MCP ledger state poisoned")?,
            "client": &*self.client.lock().map_err(|_| "MCP client state poisoned")?,
            "children": children
        }))
    }

    pub(crate) fn restore(&self, value: Value) -> Result<(), String> {
        if value["version"] != 1 || value["setId"] != json!(self.project.set_id)
            || value["configHash"] != json!(self.project.config_hash) {
            return Err("MCP checkpoint is incompatible with this project configuration.".into());
        }
        let mut open: Option<OpenBatch> = serde_json::from_value(value["open"].clone()).map_err(|e| e.to_string())?;
        if let Some(batch) = open.as_mut() {
            if batch.session.snapshot.out != self.out
                || self.bridge.only.as_ref().is_some_and(|id| id != &batch.id)
                || batch.session.scope.batch != batch.id {
                return Err("MCP checkpoint belongs to a different batch or workspace.".into());
            }
            let current = Store::load(&self.out);
            if current.status.generation != batch.session.snapshot.status.generation {
                return Err("Workspace generation changed while this batch was suspended.".into());
            }
            batch._project_lock = Some(crate::document_sets::ProjectLock::acquire(&self.project.root, "project.lock").map_err(|e| e.1)?);
        }
        *self.open.lock().map_err(|_| "MCP batch state poisoned")? = open;
        *self.delivered.lock().map_err(|_| "MCP delivery state poisoned")? = serde_json::from_value(value["delivered"].clone()).map_err(|e| e.to_string())?;
        *self.finished.lock().map_err(|_| "MCP completion state poisoned")? = serde_json::from_value(value["finished"].clone()).map_err(|e| e.to_string())?;
        *self.ledger_target.lock().map_err(|_| "MCP ledger state poisoned")? = serde_json::from_value(value["ledgerTarget"].clone()).map_err(|e| e.to_string())?;
        *self.client.lock().map_err(|_| "MCP client state poisoned")? = serde_json::from_value(value["client"].clone()).map_err(|e| e.to_string())?;
        if let Some(children) = value["children"].as_object() {
            let workspace = crate::document_sets::Workspace::load(&self.project.root).map_err(|e| e.1)?;
            let mut restored = self.children.lock().map_err(|_| "MCP child state poisoned")?;
            for (id, state) in children {
                let project = workspace.resolve(Some(id)).map_err(|e| e.1)?;
                let child = Self::with_bridge(project.clone(), project.out.clone(), self.modes.clone(), self.write, self.bridge.clone());
                child.restore(state.clone())?;
                restored.insert(id.clone(), child);
            }
        }
        Ok(())
    }

    // The server's own long poll: returns when the graph's generation counter moves, a
    // documentation file changes on disk, or the ledger or a watched deliverable file
    // changes, or at the timeout. timeout_seconds 0 waits indefinitely; the default
    // returns because most MCP clients bound a tool call with their own timeout.
    // Mirrors docs/frontends/mcp.md#the-work-loop.
    fn await_changes(&self, params: &Value) -> Value {
        let timeout = params["arguments"]["timeout_seconds"]
            .as_u64()
            .unwrap_or(300);
        let gs = crate::gen::GenSettings::resolve(&self.project);
        let fingerprint = |path: &std::path::Path| -> String {
            crate::vfs::entry_metadata(path)
                .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
                .unwrap_or_default()
        };
        // Watched surfaces: docs, the ledger, every file the ledger names, and the
        // control file, so a release or mode change wakes the poll.
        let watched = |gs: &crate::gen::GenSettings| -> Vec<std::path::PathBuf> {
            let mut v = self.project.doc_files();
            v.push(crate::gen::Ledger::path(&self.out));
            v.push(crate::control::Control::path(&self.out));
            let ledger = crate::gen::Ledger::load(&self.out);
            for row in ledger.requirements.values() {
                for f in &row.files {
                    v.push(gs.deliverable.join(f));
                }
                v.push(crate::gen::artifact_path(&self.out, gs, &row.test));
            }
            v.sort();
            v.dedup();
            v
        };
        let snapshot: std::collections::BTreeMap<std::path::PathBuf, String> = watched(&gs)
            .into_iter()
            .map(|f| (f.clone(), fingerprint(&f)))
            .collect();
        let start_gen = Store::load(&self.out).status.generation;
        let deadline = (timeout > 0).then(|| {
            crate::sys::Instant::now() + std::time::Duration::from_secs(timeout.clamp(1, 3600))
        });
        let mut changed_docs: Vec<String> = Vec::new();
        let mut changed = false;
        // A tab has one thread and nothing else that could move the graph while it
        // waits, so the poll answers at once with what it sees now.
        #[cfg(target_arch = "wasm32")]
        let deadline = Some(crate::sys::Instant::now());
        while deadline.is_none_or(|d| crate::sys::Instant::now() < d) {
            crate::sys::sleep(std::time::Duration::from_secs(2));
            let store = Store::load(&self.out);
            if store.status.generation != start_gen {
                changed = true;
            }
            for f in watched(&gs) {
                if snapshot.get(&f).map(|s| s.as_str()) != Some(fingerprint(&f).as_str()) {
                    let rel = f
                        .strip_prefix(&self.project.root)
                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|_| f.to_string_lossy().to_string());
                    if !changed_docs.contains(&rel) {
                        changed_docs.push(rel);
                    }
                    changed = true;
                }
            }
            if changed {
                break;
            }
        }
        let board = crate::board::Board::compute(&self.project, &self.out);
        let c = crate::control::Control::load(&self.project, &self.out);
        let graph_kinds = crate::board::Board::graph_kinds();
        let (c_act, b_act, g_act, v_act) = (
            board.ready_of(&graph_kinds),
            board.ready_of(&["bind"]),
            board.ready_of(&["generate"]),
            board.ready_of(&["verify"]),
        );
        let gated = board.gated.len();
        let counts = board.counts();
        let store = Store::load(&self.out);
        // Stale: documents changed but not yet reconciled, visible as open tier 0/1
        // goals on the derived board (docs/frontends/mcp.md#the-work-loop).
        let graph_stale = board.open_of(&["place-anchors", "reconcile-section"]) > 0;
        json!({
            "changed": changed,
            "changedDocs": changed_docs,
            "graphStale": graph_stale,
            "board": {
                "open": counts.open,
                "ready": counts.ready,
                "blocked": counts.blocked,
                "parked": counts.parked,
                "failed": counts.failed,
                "optional": counts.optional,
                "byClass": counts.by_class,
            },
            "compilationTasks": board.open_of(&graph_kinds),
            "bindingTasks": board.open_of(&["bind"]),
            "generationTasks": board.open_of(&["generate"]),
            "verificationTasks": crate::verify::pending_counts(&store, &gs),
            "workflow": {"compile": c.compile, "generate": c.generate},
            "gatedTasks": gated,
            "verdict": board.verdict.to_string(),
            "openDiagnostics": board.open_diags,
            "next": if c_act > 0 {
                "goals lists the board"
            } else if b_act > 0 {
                "binding_tasks lists the work"
            } else if g_act > 0 {
                "generation_tasks lists the work"
            } else if v_act > 0 {
                "verification_tasks lists the work"
            } else if gated > 0 {
                "work is gated; a release (`jazyk release` or the GUI) opens it, and this poll returns when it lands"
            } else {
                "nothing to do"
            },
        })
    }

    fn enabled_tools(&self) -> Vec<&'static str> {
        // The reader serving carries the read tools and nothing else, not even
        // report_feedback: every schema travels with every call of a small model.
        if self.bridge.reader {
            return crate::tools::READ_TOOLS.to_vec();
        }
        if self.modes.len() == 1 && self.modes[0] == "runtime" {
            return crate::task_runtime::TOOLS.to_vec();
        }
        let mut v: Vec<&'static str> = Vec::new();
        for m in &self.modes {
            let set = match m.as_str() {
                "runtime" => crate::task_runtime::TOOLS.to_vec(),
                "compile" => {
                    let mut t = toolset("mcp-compile");
                    t.extend(LIFECYCLE);
                    t
                }
                "generate" => {
                    let mut t = toolset("mcp-generate");
                    // Agents with no editor of their own get the sandboxed file and
                    // command tools (the embedded agent's profile sets serve_files).
                    // Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
                    if self.bridge.serve_files {
                        t.extend(crate::tools::FILE_TOOLS.into_iter().filter(|t| *t != "run_command"));
                    }
                    t
                }
                "benchmark" => {
                    let mut t = toolset("mcp-compile");
                    t.extend(crate::tools::GEN_TOOLS);
                    t.push("run_tests");
                    t.extend([
                        "benchmark_cases",
                        "begin_case",
                        "finish_case",
                        "benchmark_report",
                    ]);
                    t
                }
                "verify" => {
                    let mut t = toolset("mcp-verify");
                    // The read-only file tools for agents with no editor of their
                    // own: a verify session reads and runs, never writes.
                    // Mirrors docs/compiler/goals/verify.md#tools.
                    if self.bridge.serve_files {
                        t.extend(
                            crate::tools::FILE_TOOLS
                                .iter()
                                .copied()
                                .filter(|t| !matches!(*t, "write_text_file" | "run_command")),
                        );
                    }
                    t
                }
                // The chat serving: reads, every lifecycle, and the server-implemented
                // chat tools; compiler writes require a batch (docs/frontends/mcp.md#toolsets).
                "chat" => {
                    let mut t = toolset("mcp-compile");
                    t.extend(LIFECYCLE);
                    // A tab has no shell, so binding, generation, and verification
                    // are not served there (docs/frontends/wasm.md#chat).
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        t.extend(crate::tools::GEN_TOOLS);
                        t.extend(crate::tools::BIND_TOOLS);
                        t.extend(crate::tools::VERIFY_TOOLS);
                        t.extend(crate::task_runtime::TOOLS);
                        t.extend(crate::tools::FILE_TOOLS.into_iter().filter(|t| *t != "run_command"));
                    }
                    t
                }
                // The decompile serving: read tools only from the catalog; its own
                // lifecycle tools are server-implemented (they need the project).
                "decompile" => toolset("mcp-read"),
                _ => toolset(if self.write { "mcp-write" } else { "mcp-read" }),
            };
            for t in set {
                if !v.contains(&t) {
                    v.push(t);
                }
            }
        }
        if !v.contains(&crate::tools::FEEDBACK_TOOL) {
            v.push(crate::tools::FEEDBACK_TOOL);
        }
        v
    }

    // The stdio transport: line-delimited JSON-RPC until stdin ends.
    pub fn run(&self) {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut input = stdin.lock();
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            // Bytes, not `lines()`: one non-UTF-8 byte must not end the serving as if
            // the agent had hung up, and a line is decoded lossily instead.
            bytes.clear();
            match input.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&bytes);
            if line.trim().is_empty() {
                continue;
            }
            let Ok(req) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(resp) = self.dispatch(&req) else {
                continue; // notification, no response
            };
            let mut out = stdout.lock();
            writeln!(out, "{}", resp).ok();
            out.flush().ok();
        }
        self.finish();
    }

    // One JSON-RPC message in, its response out, whatever the transport carried it
    // (the stdio loop above, or the HTTP serving in acp/http.rs). A notification (no
    // id) gets no response. Mirrors docs/frontends/mcp.md#mcp-over-http.
    pub fn dispatch(&self, req: &Value) -> Option<Value> {
        let method = req["method"].as_str().unwrap_or_default().to_string();
        let id = req["id"].clone();
        if id.is_null() {
            return None;
        }
        Some(match self.handle(&method, &req["params"]) {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, msg)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
            }
        })
    }

    // The serving's end, on any transport: an ephemeral serving with an open batch
    // runs the implicit finish, and the transcript closes.
    pub fn finish(&self) {
        for child in self.children.lock().unwrap().values() {
            child.finish();
        }
        if self.cancelled() {
            self.cancel_owned();
        } else if self.bridge.ephemeral {
            self.eof_finish();
        }
        self.trace
            .finish_transcript("done", &json!({"modes": self.modes}));
    }

    /// Cancellation discards staged work, including children, without EOF commit.
    pub(crate) fn cancel_owned(&self) {
        for child in self.children.lock().unwrap().values() { child.cancel_owned(); }
        if let Some(batch) = self.open.lock().unwrap().take() {
            if let Some(claim) = crate::control::owned_lease(&self.out, &batch.id, &self.worker_id()) {
                let _ = crate::control::release_owned_lease(&claim);
            }
        }
        if let Some(target) = self.ledger_target.lock().unwrap().take() {
            self.release_own_ledger_claim(&target);
        }
        if let Some(worker) = self.worker.lock().unwrap().as_mut() { worker.refresh(Some("")); }
    }

    // A transport-level note in the transcript (a refused call, a bind), under the
    // serving's label rather than a batch's.
    pub fn note(&self, text: &str) {
        self.trace.event(crate::session::TraceEvent::Note {
            label: format!("mcp {}", self.modes.join(",")),
            text: text.to_string(),
            verbose: false,
        });
    }

    fn caller(&self, task: &str, target: &str) -> crate::feedback::Caller {
        crate::feedback::Caller {
            source: "mcp".into(),
            task: task.into(),
            target: target.into(),
            client: self.client.lock().unwrap().clone(),
            ..Default::default()
        }
    }

    // ---- the compilation lifecycle ----

    // The serving's identity in leases: the running build's token when this serving
    // is part of one, its registration id otherwise, or a pid-scoped fallback when
    // the client never sent initialize.
    fn worker_id(&self) -> String {
        if let Some(t) = &self.bridge.build_token {
            return t.clone();
        }
        self.worker
            .lock()
            .unwrap()
            .as_ref()
            .map(|h| h.id().to_string())
            .unwrap_or_else(|| format!("agent-{}", crate::sys::process_id()))
    }

    // The `goals` tool: the board with readiness sentences, gated and claimedBy, the
    // batches the scheduler would form, and the verdict with its counts when nothing
    // is open. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    fn goals_answer(&self, full: bool) -> Value {
        let mut board = crate::board::Board::compute(&self.project, &self.out);
        // An empty board with a stale `incomplete` verdict settles here: the tail is
        // deterministic and idempotent, and a lister that finds nothing to do may as
        // well say so truthfully. A dangling judged diagnostic settles the same way.
        if board.open_mandatory() == 0
            && (!board.verdict.converged() || board.dangling_diags)
            && self.open.lock().unwrap().is_none()
        {
            let mut s = Store::load(&self.out);
            let quiet = crate::session::Trace::stderr(crate::session::TraceLevel::Quiet);
            crate::reconcile::finalize(
                &mut s,
                &self.project,
                Vec::new(),
                &crate::model::Costs::default(),
                &quiet,
            );
            board = crate::board::Board::compute(&self.project, &self.out);
        }
        board.mark_own(&self.worker_id());
        let mut v = if full {
            board.answer()
        } else {
            board.answer_compact()
        };
        if self.bridge.only.is_some() {
            if let Some(fin) = self.finished.lock().unwrap().as_ref() {
                v["note"] = json!(format!(
                    "this session is complete (batch `{}` committed); goals listed here belong to the next session",
                    fin
                ));
            }
        }
        if let Some(o) = self.open.lock().unwrap().as_ref() {
            v["openBatch"] = json!(format!(
                "batch `{}` is already open; done or abandon_goals first",
                o.id
            ));
        }
        v
    }

    // Claim a goal batch: the named batch, the named goals when one batch holds them
    // all (one locality, one executor), the --only scope, or the next ready batch.
    // Opens the changeset and returns the assembled session prompt as `instructions`
    // and the initially loaded set as `package`.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    // A scoped serving whose batch already committed: the session's work is done, and
    // every later write, goal tool, done, or abandon says so instead of the
    // `no-open-batch` refusal that sends an agent back to begin_goals.
    // Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
    fn session_complete(&self) -> Option<Value> {
        if self.open.lock().unwrap().is_some() {
            return None;
        }
        self.session_complete_closed()
    }

    // The same answer for a caller that already holds the `open` lock and found no
    // batch: locking it again would deadlock.
    fn session_complete_closed(&self) -> Option<Value> {
        self.bridge.only.as_ref()?;
        let fin = self.finished.lock().unwrap().clone()?;
        Some(json!({"error": {"rule": "session-complete", "message": format!(
            "this session is complete: batch `{}` committed at generation {}. Nothing is left to stage, mark, or commit in this session; end your turn now",
            fin, crate::store::read_generation(&self.out))}}))
    }

    fn begin_goals(&self, params: &Value) -> Value {
        if let Some(o) = self.open.lock().unwrap().as_ref() {
            return json!({"error": {"rule": "batch-open", "message": format!(
                "batch `{}` is already open with {} staged mutation(s); done or abandon_goals first",
                o.id, o.session.staged.len())}});
        }
        // A scoped serving serves exactly one batch. Once it has committed, the goals
        // now open on the board belong to the next session; say so instead of the
        // wrong-target/stale refusal pair a confused agent bounces between.
        if self.bridge.only.is_some() {
            if let Some(fin) = self.finished.lock().unwrap().as_ref() {
                return json!({"error": {"rule": "session-complete", "message": format!(
                    "this session is complete: batch `{}` committed. Goals reopened since belong to the next session; finish without claiming more", fin)}});
            }
        }
        let project_lock = if self.bridge.build_token.is_none() && self.project.set_id.is_some() {
            match crate::document_sets::ProjectLock::acquire(&self.project.root, "project.lock") {
                Ok(lock) => Some(lock),
                Err((_, message)) => {
                    return json!({"error":{"rule":"project-busy","message":message}})
                }
            }
        } else {
            None
        };
        // The snapshot and the board derive from the same synced store.
        let mut store = Store::load(&self.out);
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        store.sync_docs(&parsed);
        let control = crate::control::Control::load(&self.project, &self.out);
        let board = crate::board::Board::derive(&store, &self.project, &control);
        let args = &params["arguments"];
        let wanted_batch = args["batch"].as_str();
        let wanted_goals: Vec<String> = args["goals"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let selected = if let Some(bid) = wanted_batch {
            board.batches.iter().find(|b| b.id == bid)
        } else if !wanted_goals.is_empty() {
            // Named goals form one batch only when one scheduled batch already holds
            // them all: same locality, same tier, executors agreeing.
            board
                .batches
                .iter()
                .find(|b| wanted_goals.iter().all(|g| b.goals.contains(g)))
        } else if let Some(only) = &self.bridge.only {
            board
                .batches
                .iter()
                .find(|b| b.id == *only || b.goals.iter().any(|g| g == only))
        } else {
            board.batches.first()
        };
        let Some(batch) = selected else {
            let mut v = board.answer();
            v["error"] = json!({"rule": "no-ready-batch", "message": match (wanted_batch, wanted_goals.is_empty()) {
                (Some(b), _) => format!("`{}` is not a batch on the current board; pick from the batches above", b),
                (None, false) => "the named goals do not share one ready batch (one locality, one executor); pick from the batches above".to_string(),
                _ => "no batch is ready; the board above says why".to_string(),
            }});
            return v;
        };
        let batch = batch.clone();
        // A bridge serving scoped to one batch (or one of its goals) refuses
        // everything else, so a confused agent cannot claim the rest of the board.
        if let Some(only) = &self.bridge.only {
            if batch.id != *only && !batch.goals.iter().any(|g| g == only) {
                return json!({"error": {"rule": "wrong-target", "message": format!(
                    "this serving is scoped to `{}`; batch `{}` belongs to another session", only, batch.id)}});
            }
        }
        // The control plane's claims, in order: a gated batch awaits its release, a
        // running internal build owns the board, a leased batch belongs to its
        // holder. A serving carrying the build's own token skips the first two.
        // Mirrors docs/frontends/mcp.md#the-control-plane-over-mcp.
        if self.bridge.build_token.is_none() {
            if batch.goals.iter().any(|id| board.gated.contains(id)) {
                return json!({"error": {"rule": "awaiting-release", "message": format!(
                    "batch `{}` is awaiting release: `jazyk release compile` (or the GUI) approves it", batch.id)}});
            }
            if let Some(l) = crate::control::build_lease(&self.out) {
                return json!({"error": {"rule": "build-running", "message": format!(
                    "an internal build is running (lease `{}`, heartbeated every 30s, expires {}s after the last heartbeat). \
                     await_changes returns when it clears. If you were spawned BY that build, its serving carries --build-token and never sees this error; \
                     seeing it means this serving is a bystander and the board is not yours yet", l.worker, crate::control::LEASE_TTL_SECS)}});
            }
        }
        if let Err(holder) =
            crate::control::claim_goals(&self.out, &batch.id, &self.worker_id(), &batch.goals)
        {
            return json!({"error": {"rule": "claimed", "message": format!(
                "batch `{}` is claimed by worker `{}`; pick another batch or wait for the lease to lapse ({}s after its last activity; a killed worker's lease lapses on its own)", batch.id, holder, crate::control::LEASE_TTL_SECS)}});
        }
        // The graph as this batch sees it, for replays (docs/compiler/sessions.md#snapshots).
        crate::store::snapshot_graph(&self.out, &batch.id);
        if let Some(h) = self.worker.lock().unwrap().as_mut() {
            h.refresh(Some(&batch.id));
        }
        let goals: Vec<Goal> = batch
            .goals
            .iter()
            .filter_map(|id| board.goal(id))
            .cloned()
            .collect();
        let records: BTreeMap<String, Vec<String>> = goals
            .iter()
            .map(|g| (g.id.clone(), board.records_of(&g.id)))
            .collect();
        let known_open: BTreeSet<String> =
            board.open_goals().iter().map(|g| g.id.clone()).collect();
        // The assembled prompt and the loaded set, before the store moves into the
        // session. The first batch of a serving ships the agent contract and the
        // skills in full; later batches elide what the agent already saw.
        let (loaded, mut skills) = crate::session::initial_loaded(&store, &goals);
        let mut pb = crate::session::ProjectBlock::compute(&store, &goals, &control.compile)
            .with_lint(&self.project.linting);
        pb.batch = batch.id.clone();
        let full = args["full"].as_bool() == Some(true);
        let (include_contract, skip_skills) = {
            let mut d = self.delivered.lock().unwrap();
            let include = full || !d.contract;
            let skip: BTreeSet<String> = if full {
                BTreeSet::new()
            } else {
                skills
                    .rendered_names()
                    .into_iter()
                    .filter(|n| d.skills.contains(n))
                    .collect()
            };
            d.contract = true;
            d.skills.extend(skills.rendered_names());
            (include, skip)
        };
        let mut instructions = crate::session::session_prompt_elided(
            &store,
            &goals,
            &loaded,
            &skills,
            &pb,
            include_contract,
            &skip_skills,
            crate::session::Protocol::Serving,
        );
        // A first reply over the client's inline limit ships the skills as pointers
        // (docs/frontends/mcp.md#compilation-over-mcp).
        const INLINE_LIMIT: usize = 45_000;
        let inline_skills: Vec<String> = skills
            .rendered_names()
            .into_iter()
            .filter(|n| !skip_skills.contains(n))
            .collect();
        if instructions.len() > INLINE_LIMIT && !inline_skills.is_empty() {
            let all: BTreeSet<String> = skills.rendered_names().into_iter().collect();
            let body = crate::session::session_prompt_elided(
                &store,
                &goals,
                &loaded,
                &skills,
                &pb,
                include_contract,
                &all,
                crate::session::Protocol::Serving,
            );
            instructions = format!(
                "Skills not inlined (the reply would pass the inline limit): {}. Call load_skill for each before the work; they are the method.\n\n{}",
                inline_skills.join(", "),
                body
            );
            // Pointers are not renderings: load_skill must hand the text over.
            for n in &inline_skills {
                skills.forget(n);
            }
        }
        let pinned: BTreeSet<String> = goals.iter().map(|g| g.target.clone()).collect();
        let package = loaded.render_status(&skills.index_line(), skills.rendered_chars(), &pinned);
        let scope = WorkScope::for_batch(&batch.id, &goals);
        let kinds = scope.kinds();
        let kind_refs: Vec<&str> = kinds.iter().map(String::as_str).collect();
        let write_tools: Vec<&str> = toolset_for_kinds(&kind_refs)
            .into_iter()
            .filter(|t| {
                !crate::tools::READ_TOOLS.contains(t)
                    && !crate::tools::GOAL_TOOLS.contains(t)
                    && *t != crate::tools::FEEDBACK_TOOL
                    && *t != crate::tools::SPEND_TOOL
            })
            .collect();
        let mut session = ToolSession::new(store, scope, self.mutation_limit, self.context_budget);
        session.loaded = loaded;
        session.skills = skills;
        session.gen = crate::gen::GenSettings::resolve(&self.project);
        session.lint = self.project.linting.clone();
        session.caller = self.caller(&kinds.join("+"), &batch.id);
        let goal_rows: Vec<Value> = goals
            .iter()
            .map(|g| {
                json!({"id": g.id, "kind": g.kind, "target": g.target, "mandatory": g.mandatory})
            })
            .collect();
        let reply = if self.bridge.packaged {
            // The bridge already delivered the contract as the session prompt.
            json!({
                "batch": batch.id,
                "goals": goals.iter().map(|g| g.id.clone()).collect::<Vec<_>>(),
                "note": "changeset open; stage findings with the write tools, mark each goal, then finish with done",
            })
        } else {
            json!({
                "batch": batch.id,
                "goals": goal_rows,
                "instructions": instructions,
                "package": package,
                "writeTools": write_tools,
                "readTools": crate::tools::READ_TOOLS.to_vec(),
                "goalTools": crate::tools::GOAL_TOOLS.to_vec(),
                "finishTool": "done",
                "next": "load what the goals name, stage findings, mark each goal with mark_goal_done, then done with a one-line summary",
            })
        };
        *self.open.lock().unwrap() = Some(OpenBatch {
            _project_lock: project_lock,
            id: batch.id,
            goals,
            records,
            known_open,
            session,
            rounds: 0,
        });
        reply
    }

    // The `done` tool: run the batch gates, commit atomically, re-derive the board,
    // and run the deterministic tail when the board empties.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    fn done_batch(&self, params: &Value) -> Value {
        if let Some(v) = self.session_complete() {
            return v;
        }
        let mut open = self.open.lock().unwrap();
        let Some(mut o) = open.take() else {
            return json!({"error": {"rule": "no-open-batch", "message": "no batch is open; begin_goals first"}});
        };
        let summary = params["arguments"]["summary"]
            .as_str()
            .unwrap_or("")
            .to_string();
        // The same batch gates an in-process session faces: every goal resolved or
        // failed, coverage contract, stale anchors, undecided proposals.
        if let Err(e) = o.session.dispatch("done", &json!({"summary": summary})) {
            let v = e.to_value();
            crate::control::refresh_lease(&self.out, &o.id);
            *open = Some(o); // the changeset stays open; repair and finish again
            return v;
        }
        let batch_id = o.id.clone();
        let mut reply = self.commit_open(&mut o);
        *self.finished.lock().unwrap() = Some(batch_id);
        drop(open);
        // The consumer that empties the board of mandatory work runs the deterministic
        // tail; optional goals stand aside, as blocked ones do.
        // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
        let board = crate::board::Board::compute(&self.project, &self.out);
        let graph_kinds = crate::board::Board::graph_kinds();
        let mandatory_ready = |b: &crate::board::Board| {
            b.ready_goals()
                .iter()
                .filter(|g| g.mandatory && graph_kinds.contains(&g.kind.as_str()))
                .count()
        };
        if mandatory_ready(&board) == 0 {
            let mut s2 = Store::load(&self.out);
            let quiet = crate::session::Trace::stderr(crate::session::TraceLevel::Quiet);
            let report = crate::reconcile::finalize(
                &mut s2,
                &self.project,
                Vec::new(),
                &crate::model::Costs::default(),
                &quiet,
            );
            reply["verdict"] = json!(report.verdict);
            if report.verdict.to_string().contains("blocked") {
                // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
                reply["verdictNote"] = json!(
                    "blocked goals wait on a release (jazyk release generate for bind and generate rows) or a human answer (jazyk answer); they do not hold the verdict"
                );
            }
            reply["coveragePct"] = json!(report.coverage_pct);
            // The verdict never travels alone (docs/compiler/compilation.md#convergence).
            let counts = s2.open_diag_counts();
            if !counts.is_empty() {
                reply["openDiagnostics"] = json!(counts);
                reply["diagnosticsNote"] = json!(
                    "open diagnostics stand in the graph; the diagnostics read tool lists them"
                );
            }
            let board2 = crate::board::Board::compute(&self.project, &self.out);
            if mandatory_ready(&board2) == 0 {
                // The generation rows that became ready ride under `ready`
                // (docs/frontends/mcp.md#compilation-over-mcp).
                let generate: Vec<String> = board2
                    .ready_goals()
                    .iter()
                    .filter(|g| g.kind == "generate")
                    .map(|g| g.target.clone())
                    .collect();
                // Optional goals stand aside for the verdict but stay claimable:
                // say so, or "nothing pending" contradicts the next goals reply.
                let optional_ready = board2
                    .ready_goals()
                    .iter()
                    .filter(|g| !g.mandatory && graph_kinds.contains(&g.kind.as_str()))
                    .count();
                // Failed goals stand between the board and convergence: say so
                // instead of "converged" (docs/frontends/mcp.md#compilation-over-mcp).
                let failed = s2.status.failed.len();
                let mut parts: Vec<String> = vec![if failed > 0 {
                    format!(
                        "compilation incomplete: {} failed goal(s) stand; a change on a goal's subject or a human retry from the board reopens it, and jazyk explain <goal> shows the reason",
                        failed
                    )
                } else {
                    "compilation converged".to_string()
                }];
                if optional_ready > 0 {
                    parts.push(format!(
                        "{} optional goal(s) stay claimable; beginNext stops at convergence, so begin_goals takes the next batch when you want it, and stopping here is right when your instruction was to converge (the verdict does not wait for them)",
                        optional_ready
                    ));
                }
                if !generate.is_empty() {
                    parts.push(format!(
                        "{} generation goal(s) ready (generation_tasks lists them)",
                        generate.len()
                    ));
                }
                if parts.len() == 1 {
                    parts.push("nothing pending".to_string());
                }
                reply["next"] = json!(parts.join("; "));
                reply["ready"] = json!({"generate": generate});
                // beginNext stops at convergence: an agent told to converge once held
                // an optional batch it had not asked for, and the note above says how
                // to take it (docs/frontends/mcp.md#compilation-over-mcp).
                return reply;
            }
            // The checks can surface new work (rare); name its first batch, never the
            // whole board (a novel's board of 205 bind goals once came back as a
            // 508k-character reply the agent could not read).
            reply["next"] = match board2.batches.first() {
                Some(b) => {
                    let mut v = board2.batch_json(b);
                    v["note"] = json!(format!("begin_goals claims the next batch ({})", b.id));
                    v
                }
                None => {
                    json!({"note": "no goal is ready; goals lists the board with blockedBy per goal"})
                }
            };
            return reply;
        }
        // beginNext claims the next ready batch in the same call, saving a round
        // trip per batch. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
        if params["arguments"]["beginNext"].as_bool() == Some(true) {
            let began = self.begin_goals(&json!({"arguments": {}}));
            if began["error"].is_null() {
                reply["began"] = began;
                // A session that reached its own stopping point after the claim
                // releases the batch untouched (docs/frontends/mcp.md#compilation-over-mcp).
                reply["beganNote"] = json!(
                    "the next batch is claimed; if this commit was your last, abandon_goals releases it untouched and leaves no trace"
                );
                return reply;
            }
        }
        // `next` names the next ready batch by id with its goals, never the whole
        // board: a board reply carries every open goal with its contract paragraph
        // and once ran to 169k characters. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
        reply["next"] = match board.batches.first() {
            Some(b) => {
                let mut v = board.batch_json(b);
                v["note"] = json!(format!("begin_goals claims the next batch ({})", b.id));
                v
            }
            None => {
                json!({"note": "no goal is ready; goals lists the board with blockedBy per goal"})
            }
        };
        reply
    }

    // Land a batch whose gates passed: release the lease, apply the staged work with
    // the resolutions on the journal entry, clear the resolved goals' records,
    // persist the failed goals, un-park, and record the goals the commit opened.
    // Shared by `done` and the ephemeral end-of-input finish.
    fn commit_open(&self, o: &mut OpenBatch) -> Value {
        crate::control::release_lease(&self.out, &o.id);
        if let Some(h) = self.worker.lock().unwrap().as_mut() {
            h.refresh(Some(""));
        }
        let staged = std::mem::take(&mut o.session.staged);
        let commit = o.session.commit(o.rounds, None);
        let resolved_ids: Vec<String> = commit.resolved.iter().map(|r| r.goal.clone()).collect();
        let mut s = Store::load(&self.out);
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        s.sync_docs(&parsed);
        let mut reply = json!({"committed": true, "batch": o.id, "applied": 0});
        let mut generation = None;
        // A zero-mutation batch with resolutions still commits: the journal is where
        // a resolution's justification lives (docs/compiler/graph.md#journal).
        if !staged.is_empty() || !commit.resolved.is_empty() {
            let report = s.apply(staged, &commit);
            generation = Some(report.generation);
            reply["applied"] = json!(report.applied);
            reply["generation"] = json!(report.generation);
            if !report.skipped.is_empty() {
                reply["skipped"] = json!(report.skipped);
            }
        }
        // The counter rides every done reply, mutations or not
        // (docs/frontends/mcp.md#compilation-over-mcp).
        if generation.is_none() {
            reply["generation"] = json!(s.status.generation);
        }
        // A resolution clears the change records its goal stood on, mutations or not
        // (a pair's created and revised records stay for their other candidates).
        if !resolved_ids.is_empty() {
            let clear: Vec<String> = resolved_ids
                .iter()
                .flat_map(|id| o.records.get(id).cloned().unwrap_or_default())
                .collect();
            s.clear_resolved(&clear);
            reply["resolved"] = json!(resolved_ids);
        }
        // Failed goals persist by id with their change payloads, so they survive
        // re-derivation. Mirrors docs/compiler/reconciler.md#parked-and-failed.
        let failed = o.session.failed_goals();
        for (id, reason) in &failed {
            if let Some(g) = o.goals.iter().find(|g| g.id == *id) {
                s.status.failed.retain(|f| f.goal.id != *id);
                s.status.failed.push(crate::model::FailedGoal {
                    goal: g.clone(),
                    reason: reason.clone(),
                });
            }
        }
        if !failed.is_empty() {
            reply["failed"] = json!(failed.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>());
        }
        // A resumed parked goal is no longer parked.
        let batch_ids: BTreeSet<&String> = o.goals.iter().map(|g| &g.id).collect();
        let parked_before = s.status.parked.len();
        s.status.parked.retain(|p| !batch_ids.contains(&p.goal.id));
        if !failed.is_empty() || parked_before != s.status.parked.len() {
            s.save_status();
        }
        // The board re-derives; the goals this commit opened land on its journal
        // entry. Mirrors docs/compiler/graph.md#journal.
        let board = crate::board::Board::compute(&self.project, &self.out);
        let opened: Vec<OpenedGoal> = board
            .open_goals()
            .iter()
            .filter(|g| !o.known_open.contains(&g.id))
            .map(|g| OpenedGoal {
                goal: g.id.clone(),
                cause: g.cause.clone().unwrap_or_default(),
            })
            .collect();
        // Derived facts this commit landed carry ratification proposals: the ratify goals
        // they open wait on a human and swell the blocked count, so the reply says so
        // (docs/frontends/mcp.md#compilation-over-mcp).
        let ratify_new = generation
            .map(|g| {
                let build = format!("g{}", g);
                Store::load(&self.out)
                    .graph
                    .diagnostics
                    .values()
                    .filter(|d| {
                        d.rule == "ratification-pending"
                            && d.created.as_deref() == Some(build.as_str())
                    })
                    .count()
            })
            .unwrap_or(0);
        if ratify_new > 0 {
            reply["ratificationNote"] = json!(format!(
                "{} derived fact(s) this commit landed (criteria, groupings, sub-entities) carry ratification proposals: {} ratify goal(s) wait on a human and join the blocked count; nothing is wrong with them",
                ratify_new, ratify_new
            ));
        }
        if !opened.is_empty() {
            reply["opened"] = json!(opened.iter().map(|x| x.goal.clone()).collect::<Vec<_>>());
            if let Some(g) = generation {
                let mut s2 = Store::load(&self.out);
                s2.record_opened_goals(g, opened);
            }
        }
        reply
    }

    // End of input with an open batch: the agent's session ended without the
    // finishing call. Valid staged work still lands, under the same gates the budget
    // path uses. Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
    fn eof_finish(&self) {
        let mut open = self.open.lock().unwrap();
        let Some(mut o) = open.take() else { return };
        if o.session
            .finish_implicit("(implicit: the agent session ended)")
        {
            let reply = self.commit_open(&mut o);
            self.trace.event(crate::session::TraceEvent::SessionDone {
                label: o.id.clone(),
                goals: o.goals.iter().map(|g| g.id.clone()).collect(),
                staged: reply["applied"].as_u64().unwrap_or(0) as usize,
                rounds: o.rounds,
                mode: "implicit".into(),
                summary: String::new(),
            });
        } else {
            crate::control::release_lease(&self.out, &o.id);
        }
    }

    fn abandon_goals(&self, params: &Value) -> Value {
        if let Some(v) = self.session_complete() {
            return v;
        }
        let mut open = self.open.lock().unwrap();
        let Some(o) = open.take() else {
            return json!({"error": {"rule": "no-open-batch", "message": "no batch is open"}});
        };
        crate::control::release_lease(&self.out, &o.id);
        if let Some(h) = self.worker.lock().unwrap().as_mut() {
            h.refresh(Some(""));
        }
        let reason = params["arguments"]["reason"].as_str().unwrap_or("");
        json!({
            "abandoned": o.id,
            "goals": o.goals.iter().map(|g| g.id.clone()).collect::<Vec<_>>(),
            "dropped": o.session.staged.len(),
            "reason": reason,
            "note": "the staged changeset is gone; the goals return to open",
        })
    }

    // ---- the agent-run benchmark ----
    // Mirrors docs/benchmark/benchmark.md#agent-run-benchmarks.

    #[cfg(not(target_arch = "wasm32"))]
    fn benchmark_cases(&self) -> Value {
        let cases = crate::benchmark::parse_cases();
        let b = self.bench.lock().unwrap();
        let scored: std::collections::BTreeMap<String, &Value> = b
            .scored
            .iter()
            .map(|e| (e["name"].as_str().unwrap_or("").to_string(), e))
            .collect();
        let rows: Vec<Value> = cases
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let state = if b.open.as_ref().map(|o| o.idx) == Some(i) {
                    json!("open")
                } else if let Some(e) = scored.get(&c.name) {
                    json!(format!("scored {}", e["score"]))
                } else {
                    json!("pending")
                };
                json!(format!(
                    "{} ({}, {}): {}",
                    c.name,
                    c.tier,
                    c.task_type,
                    state.as_str().unwrap_or("")
                ))
            })
            .collect();
        let done = b.scored.len();
        json!({
            "cases": rows,
            "scored": done,
            "total": cases.len(),
            "next": if done >= cases.len() {
                "all cases scored; call benchmark_report with an honest model name"
            } else {
                "begin_case {case: <name>} claims that case; with no name, the first pending one"
            },
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn benchmark_cases(&self) -> Value {
        json!({"error": "the browser backend does not serve the benchmark"})
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn begin_case(&self, params: &Value) -> Value {
        let cases = crate::benchmark::parse_cases();
        let mut b = self.bench.lock().unwrap();
        if let Some(o) = &b.open {
            return json!({"error": {"rule": "case-open", "message": format!(
                "case `{}` is already open; finish_case first", cases[o.idx].name)}});
        }
        let scored: std::collections::BTreeSet<String> = b
            .scored
            .iter()
            .filter_map(|e| e["name"].as_str().map(String::from))
            .collect();
        let want = params["arguments"]["case"].as_str();
        let Some((idx, case)) = cases.iter().enumerate().find(|(_, c)| match want {
            Some(w) => c.name == w,
            None => !scored.contains(&c.name),
        }) else {
            return json!({"error": {"rule": "no-pending-case", "message": "no pending case; benchmark_cases shows the run, benchmark_report closes it"}});
        };
        let tmp = std::env::temp_dir().join(format!(
            "jazyk-mcp-bench-{}-{}",
            crate::sys::process_id(),
            case.name
        ));
        crate::vfs::remove_dir_all(&tmp).ok();
        // A derived-goal case seeds through a real commit and takes its goal off the
        // board, as an endpoint run does (docs/benchmark/benchmark.md#agent-run-benchmarks).
        let mut derived: Option<crate::model::Goal> = None;
        let store = if case.derives_goal() {
            match crate::benchmark::seed_derived(case, &tmp) {
                Ok((s, g)) => {
                    derived = Some(g);
                    s
                }
                Err(e) => {
                    return json!({"error": {"rule": "fixture", "message": e}});
                }
            }
        } else {
            if let Err(error) = crate::benchmark::prepare_runtime_fixture(case, &tmp) {
                return json!({"error": {"rule": "fixture", "message": error}});
            }
            crate::benchmark::sandbox(case, &tmp)
        };
        let gs = match crate::benchmark::fixture_gen_settings(&tmp) {
            Ok(gs) => gs,
            Err(error) => return json!({"error":{"rule":"fixture","message":error}}),
        };
        crate::vfs::create_dir_all(&gs.deliverable).ok();
        let item = match &derived {
            Some(g) => WorkItem::from_goal(g),
            None => WorkItem {
                task: case.task_type.clone(),
                target: case.target.clone(),
                dirty_sections: crate::benchmark::case_dirty_sections(&store, case),
                stale_anchors: Vec::new(),
                proposals: Vec::new(),
            },
        };
        let mut reply = json!({
            "case": {"name": case.name, "tier": case.tier, "task": case.task_type, "target": case.target},
            "next": "do the work, then finish_case",
        });
        match case.task_type.as_str() {
            "verify-requirement" => {
                if let Err(e) = crate::benchmark::seed_verification(case, &store, &gs) {
                    return json!({"error": {"rule": "fixture", "message": e}});
                }
                let r = &store.graph.requirements[&case.target];
                reply["instructions"] = json!("Judge whether the implementing files satisfy the statement. Read them with your own tools. finish_case with verdict pass or fail and one-line evidence.");
                reply["package"] = json!({
                    "statement": r.statement,
                    "quote": r.source.as_ref().map(|s| s.quote.clone()).unwrap_or_default(),
                    "files": case.deliverable.keys().map(|f| gs.deliverable.join(f).to_string_lossy().to_string()).collect::<Vec<_>>(),
                });
            }
            _ => {
                if case.task_type == "generate-entity" {
                    reply["deliverableDir"] = json!(gs.deliverable.to_string_lossy());
                    reply["note"] = json!("write real files into deliverableDir with your own tools; record_generation and run_tests act on this sandbox while the case is open");
                }
            }
        }
        // One code path builds the case session, so an agent and an endpoint face the
        // same prompt, scope, loaded set, and skills
        // (docs/benchmark/benchmark.md#agent-run-benchmarks).
        let crate::benchmark::CaseSession {
            mut session,
            prompt,
            ..
        } = crate::benchmark::case_session(store.clone(), &item, &case.lint, &gs, derived.as_ref());
        if case.task_type != "verify-requirement" {
            reply["instructions"] = json!(prompt);
            reply["package"] = json!(session.render_status_full());
        }
        session.gen = gs.clone();
        session.caller = self.caller("benchmark", &case.name);
        b.open = Some(OpenCase {
            idx,
            item,
            derived,
            store,
            session,
            calls: 0,
            tmp,
            gs,
        });
        reply
    }

    #[cfg(target_arch = "wasm32")]
    fn begin_case(&self, params: &Value) -> Value {
        json!({"error": "the browser backend does not serve the benchmark"})
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn finish_case(&self, params: &Value) -> Value {
        let cases = crate::benchmark::parse_cases();
        let mut b = self.bench.lock().unwrap();
        let Some(mut o) = b.open.take() else {
            return json!({"error": {"rule": "no-open-case", "message": "no case is open; begin_case first"}});
        };
        let case = &cases[o.idx];
        // Turn cases face the same done gates a turn does; a rejection keeps the case
        // open, same contract as done on a batch.
        let mut staged_count = 0usize;
        // Every case but generation and verification commits through done
        // (docs/benchmark/benchmark.md#agent-run-benchmarks).
        if !matches!(
            case.task_type.as_str(),
            "generate-entity" | "verify-requirement"
        ) {
            let summary = params["arguments"]["summary"]
                .as_str()
                .unwrap_or("(finish)")
                .to_string();
            if let Err(e) = o.session.dispatch("done", &json!({"summary": summary})) {
                let v = e.to_value();
                b.open = Some(o);
                return v;
            }
            let staged = std::mem::take(&mut o.session.staged);
            staged_count = staged.len();
            if !staged.is_empty() {
                let commit = match &o.derived {
                    Some(_) => o.session.commit(o.calls, None),
                    None => o.item.commit(o.calls, None),
                };
                o.store.apply(staged, &commit);
            }
        }
        if case.task_type == "verify-requirement" {
            let verdict = params["arguments"]["verdict"].as_str().unwrap_or("");
            if verdict != "pass" && verdict != "fail" {
                b.open = Some(o);
                return json!({"error": {"rule": "bad-argument", "message": "finish_case on a verification case needs verdict: pass or fail"}});
            }
            let evidence = params["arguments"]["evidence"].as_str().unwrap_or("");
            if let Err(e) = crate::verify::mark(
                &o.store,
                &case.target,
                verdict,
                None,
                Some(evidence),
                None,
                &o.gs,
            ) {
                b.open = Some(o);
                return json!({"error": {"rule": "bad-argument", "message": e}});
            }
        }
        // Grade: the same deterministic checks an endpoint run faces, over the
        // session's real staged count. A goal the model marked failed fails the case
        // with the model's reason; the checks are skipped and count as failed, as on
        // an abort: an untouched fixture satisfying a check is not evidence.
        // Mirrors docs/benchmark/benchmark.md#runs.
        let goal_failed = o
            .session
            .failed_goals()
            .into_iter()
            .next()
            .map(|(_, reason)| format!("goal marked failed: {}", reason));
        let (mut passed, mut fail): (usize, Option<String>) = (0, goal_failed);
        if fail.is_none() {
            for (kind, arg) in &case.checks {
                let verdict = if crate::benchmark::WORKFLOW_CHECKS.contains(&kind.as_str()) {
                    crate::benchmark::eval_workflow_check(kind, arg, &o.store, &o.gs, &case.target)
                } else {
                    crate::benchmark::eval_check(kind, arg, &o.store, staged_count)
                };
                match verdict {
                    None => passed += 1,
                    Some(why) => {
                        if fail.is_none() {
                            fail = Some(format!("{}: {}", kind, why));
                        }
                    }
                }
            }
        }
        let score = if case.checks.is_empty() {
            0.0
        } else {
            passed as f64 / case.checks.len() as f64
        };
        let efficiency = (case.par_rounds as f64 / o.calls.max(1) as f64).min(1.0);
        crate::vfs::remove_dir_all(&o.tmp).ok();
        let entry = json!({
            "name": case.name,
            "tier": case.tier,
            "score": (score * 100.0).round() / 100.0,
            "checks": format!("{}/{}", passed, case.checks.len()),
            "calls": o.calls,
            "parRounds": case.par_rounds,
            "efficiency": (efficiency * 100.0).round() / 100.0,
            "fail": fail.clone().unwrap_or_default(),
        });
        b.scored.retain(|e| e["name"] != entry["name"]);
        b.scored.push(entry.clone());
        let remaining = cases.len().saturating_sub(b.scored.len());
        json!({
            "scored": entry,
            "next": if remaining > 0 {
                format!("{} case(s) pending; begin_case claims the next", remaining)
            } else {
                "all cases scored; call benchmark_report with an honest model name".into()
            },
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn finish_case(&self, params: &Value) -> Value {
        json!({"error": "the browser backend does not serve the benchmark"})
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn benchmark_report(&self, params: &Value) -> Value {
        let mut b = self.bench.lock().unwrap();
        if b.open.is_some() {
            return json!({"error": {"rule": "case-open", "message": "a case is open; finish_case first"}});
        }
        if b.scored.is_empty() {
            return json!({"error": {"rule": "nothing-scored", "message": "no cases scored yet; begin_case starts the run"}});
        }
        let model = params["arguments"]["model"]
            .as_str()
            .map(String::from)
            .or_else(|| {
                self.client
                    .lock()
                    .unwrap()
                    .clone()
                    .map(|c| format!("{} (agent)", c))
            })
            .unwrap_or_else(|| "unnamed-agent".into());
        let mut tier_sum: std::collections::BTreeMap<&str, (f64, usize)> = Default::default();
        let mut tier_ok: std::collections::BTreeMap<&str, bool> = Default::default();
        let (mut eff_sum, mut checks_p, mut checks_t) = (0.0f64, 0usize, 0usize);
        for e in &b.scored {
            let t = crate::benchmark::tier_key(e["tier"].as_str().unwrap_or(""));
            let sc = e["score"].as_f64().unwrap_or(0.0);
            let en = tier_sum.entry(t).or_insert((0.0, 0));
            en.0 += sc;
            en.1 += 1;
            if sc < 1.0 {
                tier_ok.insert(t, false);
            }
            eff_sum += e["efficiency"].as_f64().unwrap_or(0.0);
            let parts: Vec<usize> = e["checks"]
                .as_str()
                .unwrap_or("0/0")
                .split('/')
                .filter_map(|x| x.parse().ok())
                .collect();
            if parts.len() == 2 {
                checks_p += parts[0];
                checks_t += parts[1];
            }
        }
        // A tier never attempted is unmeasured, not capable: a partial run says what
        // it graded and nothing more.
        let ran = |t: &str| tier_sum.contains_key(t);
        let ok = |t: &str| ran(t) && *tier_ok.get(t).unwrap_or(&true);
        let ts = |t: &str| {
            tier_sum
                .get(t)
                .map(|(s, n)| {
                    if *n == 0 {
                        0.0
                    } else {
                        ((s / *n as f64) * 100.0).round() / 100.0
                    }
                })
                .unwrap_or(0.0)
        };
        let report = json!({
            "verdicts": {
                "compilation": if !ran("extraction") { "unmeasured" } else {
                    match (ok("extraction"), ok("review") && ran("review")) {
                        (true, true) => "review", (true, false) => "extraction", _ => "not-capable",
                    }
                },
                "generation": if !ran("generation") { "unmeasured" } else if ok("generation") { "capable" } else { "not-capable" },
                "verification": if !ran("verification") { "unmeasured" } else if ok("verification") { "capable" } else { "not-capable" },
            },
            "scores": {"extraction": ts("extraction"), "review": ts("review"), "generation": ts("generation"), "verification": ts("verification")},
            "checks": format!("{}/{}", checks_p, checks_t),
            "efficiency": ((eff_sum / b.scored.len() as f64) * 100.0).round() / 100.0,
            "tokens": Value::Null,
            "throughput": Value::Null,
            "cases": b.scored.iter().map(|e| (e["name"].as_str().unwrap_or("").to_string(), e.clone())).collect::<std::collections::BTreeMap<String, Value>>(),
        });
        crate::benchmark::append_history(&model, "agent", &[("agent".to_string(), report.clone())]);
        b.scored.clear();
        json!({
            "model": model,
            "codec": "agent",
            "report": report,
            "verdictsLegend": "compilation verdicts order not-capable < extraction < review; review is the highest (extraction plus review judgment). generation and verification are capable or not-capable. unmeasured means the tier was never attempted.",
            "recorded": "appended to ~/.jazyk/benchmarks/history.yaml",
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn benchmark_report(&self, params: &Value) -> Value {
        json!({"error": "the browser backend does not serve the benchmark"})
    }

    // A user conversation multiplexes set-local sessions. There is no ambient selection.
    fn cancelled(&self) -> bool {
        self.bridge
            .cancel_file
            .as_ref()
            .is_some_and(|p| crate::vfs::exists(p))
    }

    pub async fn handle_async(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        if method != "tools/call" || params["name"] != "compile_document_sets" {
            return self.handle(method, params);
        }
        if self.cancelled() { return Err((-32602, "the build was stopped".into())); }
        if self.bridge.reader || !(self.write || self.modes.iter().any(|m| m != "graph")) {
            return Err((-32602, "this serving is read-only".into()));
        }
        if self.project.set_id.is_some() || self.bridge.only.is_some() || self.bridge.build_token.is_some() {
            return Err((-32602, "nested compilation is unavailable in a scoped worker".into()));
        }
        let workspace = crate::document_sets::Workspace::load(&self.project.root).map_err(|e| (-32602, e.1))?;
        let response = crate::webapi::dispatch::request_at_async(&workspace.root, "POST", "/api/compile", &params["arguments"].to_string()).await;
        let response: Value = serde_json::from_str(&response).map_err(|e| (-32603, e.to_string()))?;
        if let Some(reason) = compile_interruption(&response) { return Err((-32603,reason)); }
        Ok(result_value(response))
    }

    pub fn handle(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        let _runtime_owner = crate::task_runtime::CancellationScope::enter(self.bridge.cancel_file.as_deref());
        if self.cancelled() && method == "tools/call" && !matches!(params["name"].as_str(), Some("abandon_goals" | "cancel_run" | "inspect_run" | "wait_run" | "read_run_log" | "search_run_log" | "read_run_changes" | "task_context")) {
            return Err((-32602, "the build was stopped; this session cannot commit".into()));
        }
        if self.project.set_id.is_some()
            || self.bridge.only.is_some()
            || self.bridge.build_token.is_some()
        {
            if params["arguments"]["setId"]
                .as_str()
                .is_some_and(|id| Some(id) != self.project.set_id.as_deref())
            {
                return Err((
                    -32602,
                    "this worker is scoped to another document set".into(),
                ));
            }
            return self.handle_local(method, params);
        }
        if method == "tools/call" && crate::task_runtime::TOOLS.contains(&params["name"].as_str().unwrap_or_default()) {
            return self.handle_local(method, params);
        }
        if self.modes.len() == 1 && self.modes[0] == "runtime" {
            return self.handle_local(method, params);
        }
        let w =
            crate::document_sets::Workspace::load(&self.project.root).map_err(|e| (-32602, e.1))?;
        if method == "initialize" {
            // Registration belongs to each set-local child, created lazily.
            if let Some(name) = params["clientInfo"]["name"].as_str() {
                *self.client.lock().unwrap() = Some(name.into());
            }
            return Ok(
                json!({"protocolVersion":params["protocolVersion"].as_str().unwrap_or("2024-11-05"),
                "capabilities":{"tools":{}},"serverInfo":{"name":"jazyk","version":env!("CARGO_PKG_VERSION")},
                "instructions":format!("This project has independent document sets. Use list_document_sets when a set-local operation needs a set ID. General answers and named document reads need no set discovery. Pass setId on set-local tools. Never reuse a graph ID in another set. {}",if self.bridge.reader {reader_instructions()} else {instructions_for(&self.modes,self.write,w.exists)})}),
            );
        }
        let writable =
            !self.bridge.reader && (self.write || self.modes.iter().any(|m| m != "graph"));
        if method == "tools/list" {
            let mut result = self.handle_local(method, params)?;
            let tools = result["tools"].as_array_mut().unwrap();
            for t in tools.iter_mut() {
                t["inputSchema"]["properties"]["setId"] = json!({"type":"string","description":"Document set ID from list_document_sets. Required when multiple sets exist."});
            }
            tools.push(json!({"name":"list_document_sets","description":"List every document set, its file selections, state and configuration hash.","inputSchema":{"type":"object","properties":{}}}));
            tools.push(json!({"name":"read_document","description":"Read a project document as compiler text. DOCX and PDF include source locations and diagnostics. Never overwrite binary documents with text.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}}));
            if writable && self.modes.iter().any(|m| m == "chat") {
                tools.push(json!({"name":"write_document","description":"Create or replace one prose document without compilation. Read first and send its hash as baseHash. Omit baseHash only to create a missing document. Preserves document-save dirtiness; does not release or run compile work.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"text":{"type":"string"},"baseHash":{"type":"string"}},"required":["path","text"],"additionalProperties":false}}));
            }
            if writable {
                for (name,description) in [("preview_document_set","Preview a document-set edit without writing."),("save_document_set","Create or edit one set, preserving its identity and graph."),("create_document_set","Create a named document set."),("update_document_set","Update a document set while preserving its identity."),("remove_document_set","Remove a set from the project. Documents and its retained graph remain on disk.")] {
                    tools.push(json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":{
                        "baseHash":{"type":"string"},"remove":{"type":"string"},"set":{"type":"object","properties":{
                            "id":{"type":"string"},"name":{"type":"string"},"docs":{"type":"array","items":{"type":"string"}},
                            "roots":{"type":"array","items":{"type":"string"}},"source":{"type":"array","items":{"type":"string"}}}}},"required":["baseHash"]}}));
                }
            }
            if writable {
                tools.push(json!({"name":"compile_document_sets","description":"Compile selected set IDs sequentially, or all dirty sets when setIds is absent.","inputSchema":{"type":"object","properties":{"setIds":{"type":"array","items":{"type":"string"}}}}}));
            }
            return Ok(result);
        }
        if method != "tools/call" {
            return self.handle_local(method, params);
        }
        let name = params["name"].as_str().unwrap_or_default();
        if crate::task_runtime::TOOLS.contains(&name) { return self.handle_local(method,params); }
        if name == "list_document_sets" {
            return Ok(result_value(w.summary()));
        }
        if name == "write_document" {
            if !writable || !self.modes.iter().any(|m| m == "chat") {
                return Err((-32602, "document edits require a writable chat serving".into()));
            }
            let args = &params["arguments"];
            let path = args["path"].as_str().ok_or((-32602, "path is required".into()))?;
            let text = args["text"].as_str().ok_or((-32602, "text is required".into()))?;
            let project = w.files_project();
            let base = args["baseHash"].as_str();
            // Validate and version-check before asking an attached editor to write.
            if let Some(sink) = &self.bridge.edit_sink {
                let abs = crate::webapi::docs::safe_doc_path_why(&project, path)
                    .map_err(|m| (-32602, m))?;
                if std::path::Path::new(path).extension().and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("docx") || e.eq_ignore_ascii_case("pdf")) {
                    return Err((-32602, "binary documents require a format-specific editor".into()));
                }
                let old = match crate::vfs::read_to_string(&abs) {
                    Ok(text) => Some(text),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound && !crate::vfs::exists(&abs) => None,
                    Err(error) => return Err((-32602,format!("cannot read existing document before editor delegation: {error}"))),
                };
                let hash = old.as_deref().map(crate::model::hash_hex);
                if base != hash.as_deref() || (base.is_none() && old.is_some()) {
                    return Ok(text_result(json!({"error":{"rule":"stale-document","message":"document changed; read_document before retrying","diskHash":hash}}), true));
                }
                sink_write(sink, &abs, old.as_deref().unwrap_or_default(), text, text)
                    .map_err(|m| (-32603, m))?;
                return Ok(result_value(json!({"path":path,"hash":crate::model::hash_hex(text),"delegated":true})));
            }
            return match crate::webapi::docs::doc_write(&project, path, text, base) {
                crate::webapi::docs::DocWrite::Written(v) => Ok(result_value(v)),
                crate::webapi::docs::DocWrite::Conflict(v) => Ok(text_result(json!({"error":{"rule":"stale-document","message":v}}), true)),
                crate::webapi::docs::DocWrite::Invalid(m) | crate::webapi::docs::DocWrite::Failed(m) => Err((-32602,m)),
            };
        }
        if name == "read_document" {
            let path = params["arguments"]["path"].as_str().ok_or((-32602, "path is required".into()))?;
            let project = w.files_project();
            let binary = std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("docx") || e.eq_ignore_ascii_case("pdf"));
            let mut value = if binary {
                crate::webapi::binary_documents::read(&project, path).map_err(|(_, message)| (-32602, message))?
            } else {
                match crate::webapi::docs::doc_content(&project, path) {
                    crate::webapi::docs::DocRead::Found(value) => value,
                    crate::webapi::docs::DocRead::Missing(message) | crate::webapi::docs::DocRead::Invalid(message) => return Err((-32602, message)),
                }
            };
            if let Some(object) = value.as_object_mut() { object.remove("base64"); }
            return Ok(result_value(value));
        }
        if name == "compile_document_sets" {
            if !writable {
                return Err((-32602, "this serving is read-only".into()));
            }
            #[cfg(target_arch = "wasm32")]
            return Err((-32602, "compilation requires the asynchronous MCP entry point".into()));
            #[cfg(not(target_arch = "wasm32"))]
            {
            let response = crate::webapi::dispatch::request_at(
                &w.root,
                "POST",
                "/api/compile",
                &params["arguments"].to_string(),
            );
            let response: Value =
                serde_json::from_str(&response).map_err(|e| (-32603, e.to_string()))?;
            if let Some(reason) = compile_interruption(&response) { return Err((-32603,reason)); }
            return Ok(result_value(response));
            }
        }
        if matches!(
            name,
            "preview_document_set"
                | "save_document_set"
                | "create_document_set"
                | "update_document_set"
                | "remove_document_set"
        ) {
            if !writable {
                return Err((-32602, "this serving is read-only".into()));
            }
            return crate::document_sets::edit(
                &w.root,
                &params["arguments"],
                name == "preview_document_set",
            )
            .map(result_value)
            .map_err(|e| (-32602, e.1));
        }
        if matches!(name, "init_project" | "update_project_settings") {
            return self.handle_local(method, params);
        }
        let routed = crate::webapi::sets::route("/", params["arguments"].clone())
            .map_err(|e| (-32602, e.1))?;
        let ids = if let Some(id) = routed.selector.as_deref() {
            vec![w.select(id).map_err(|e| (-32602, e.1))?.id.clone()]
        } else if w.sets.len() == 1 {
            vec![w.sets[0].id.clone()]
        } else if matches!(
            name,
            "search"
                | "goals"
                | "graph_status"
                | "diagnostics"
                | "binding_tasks"
                | "generation_tasks"
                | "verification_tasks"
                | "decompile_tasks"
        ) {
            w.sets.iter().map(|s| s.id.clone()).collect()
        } else {
            return Err((
                -32602,
                "pass setId from list_document_sets for this operation".into(),
            ));
        };
        let mut replies = Vec::new();
        let mut children = self.children.lock().unwrap();
        for id in ids {
            if name != "abandon_goals"
                && children
                    .get(&id)
                    .is_some_and(|s| s.project.config_hash != w.hash)
            {
                if children[&id].open.lock().unwrap().is_some() {
                    return Err((
                        -32602,
                        "configuration changed during the open batch; abandon it before continuing"
                            .into(),
                    ));
                }
                children.remove(&id);
            }
            if !children.contains_key(&id) {
                let p = w.resolve(Some(&id)).map_err(|e| (-32602, e.1))?;
                let child = Self::with_bridge(
                    p.clone(),
                    p.out.clone(),
                    self.modes.clone(),
                    self.write,
                    self.bridge.clone(),
                );
                child.handle_local("initialize",&json!({"clientInfo":{"name":self.client.lock().unwrap().clone().unwrap_or_else(||"agent".into())}}))?;
                children.insert(id.clone(), child);
            }
            let mut local = params.clone();
            local["arguments"] = routed.body.clone();
            if let Some(o) = local["arguments"].as_object_mut() {
                o.remove("setId");
            }
            let reply = children[&id].handle_local(method, &local)?;
            let text = reply["content"][0]["text"].as_str().unwrap_or_default();
            let result = serde_json::from_str(text).unwrap_or_else(|_| json!(text));
            replies.push(json!({"setId":id,"name":w.select(&id).unwrap().name,"result":crate::webapi::sets::qualify_value(result,&id),"isError":reply["isError"]}));
        }
        if replies.len() == 1 {
            let mut reply = result_value(replies[0]["result"].clone());
            reply["isError"] = replies[0]["isError"].clone();
            Ok(reply)
        } else {
            Ok(result_value(json!({"sets":replies})))
        }
    }

    fn handle_local(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                if let Some(name) = params["clientInfo"]["name"].as_str() {
                    *self.client.lock().unwrap() = Some(name.to_string());
                }
                // A task-lifecycle serving is a worker among workers: register it and
                // heartbeat while the process lives. An ephemeral serving is part of a
                // run that already answers for itself, so it never registers. Mirrors
                // docs/frontends/mcp.md#the-control-plane-over-mcp.
                if !self.bridge.ephemeral
                    && !crate::vfs::is_scoped()
                    && self.modes.iter().any(|m| {
                        m == "compile" || m == "generate" || m == "verify" || m == "decompile"
                    })
                {
                    let client = self
                        .client
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(|| "agent".into());
                    let mut w = self.worker.lock().unwrap();
                    match w.as_mut() {
                        Some(h) => h.set_client(&client),
                        None => {
                            *w = Some(crate::control::register(&self.out, "agent", &client));
                            // The heartbeat keeps the worker registration fresh so
                            // the sweep does not call it stale. A browser tab has one
                            // backend and no thread to spare, and nothing sweeps it
                            // (docs/frontends/wasm.md#the-seams).
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                // A weak handle: the thread ends when the serving
                                // that registered the worker is gone, so a replaced
                                // child serving stops heartbeating as a live worker.
                                let wref = std::sync::Arc::downgrade(&self.worker);
                                std::thread::spawn(move || loop {
                                    std::thread::sleep(std::time::Duration::from_secs(30));
                                    let Some(w) = wref.upgrade() else { break };
                                    let mut guard = w.lock().unwrap_or_else(|p| p.into_inner());
                                    match guard.as_mut() {
                                        Some(h) => h.refresh(None),
                                        None => break,
                                    }
                                });
                            }
                        }
                    }
                }
                // The serving is version-lenient line JSON; echoing the client's
                // requested protocol version keeps strict clients (rmcp) from
                // treating the reply as a downgrade.
                let requested = params["protocolVersion"].as_str().unwrap_or("2024-11-05");
                Ok(json!({
                    "protocolVersion": requested,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "jazyk", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": if self.bridge.reader { reader_instructions() } else { instructions_for(&self.modes, self.write, self.initialized()) }
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => {
                let enabled = self.enabled_tools();
                let mut tools: Vec<Value> = catalog()
                    .iter()
                    .filter(|t| enabled.contains(&t.name))
                    .map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.parameters}))
                    .collect();
                if self.modes.len() == 1 && self.modes[0] == "runtime" {
                    return Ok(json!({"tools": tools}));
                }
                #[cfg(not(target_arch = "wasm32"))]
                if !self.bridge.reader && self.modes.iter().any(|m| m == "chat") {
                    tools.push(json!({
                        "name": "release_work",
                        "description": "Approve the selected set's current compile or generate stage only when the user explicitly requested that work. Leaves manual workflow settings unchanged. Does not approve human decisions, credentials, or publication.",
                        "inputSchema": {"type":"object","properties":{"stage":{"type":"string","enum":["compile","generate"]}},"required":["stage"],"additionalProperties":false}
                    }));
                }
                // The compilation lifecycle: the compile serving's own, and the chat
                // serving carries it too (docs/compiler/tools.md#toolsets). The
                // catalog's own `done` (the goal tools) yields to the lifecycle entry:
                // a client that forwards the list as is (Claude Code) is refused by
                // its API when a name repeats.
                if self.modes.iter().any(|m| m == "compile" || m == "chat") {
                    tools.retain(|t| !LIFECYCLE.contains(&t["name"].as_str().unwrap_or_default()));
                    tools.push(json!({
                        "name": "goals",
                        "description": "The goal board, compact: the counts, the batches the scheduler would form (each with its goals), the ready goals outside a batch, and the goals not ready summarized by kind and blocking reason with their ids. full: true lists every goal with its hints, change, cause, readiness, gated and claimedBy. Zero open goals carries the build verdict with its counts. Next: begin_goals.",
                        "inputSchema": {"type": "object", "properties": {"full": {"type": "boolean", "description": "every goal whole, hints included"}}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "begin_goals",
                        "description": "Claim the named batch (batch, an id from goals), the named goals as one batch (goals, when one scheduled batch holds them all), or the next ready batch. Opens the changeset and returns the assembled session prompt as instructions and the initially loaded set as package. One batch open at a time.",
                        "inputSchema": {"type": "object", "properties": {"batch": {"type": "string", "description": "a batch id from goals, e.g. b412-1"}, "goals": {"type": "array", "items": {"type": "string"}, "description": "goal ids sharing one locality and one executor"}, "full": {"type": "boolean", "description": "repeat the agent contract and the skills even when this serving already delivered them"}}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "done",
                        "description": "Finish the open batch: run the batch gates (every goal marked done or failed, every dirty section marked, every stale anchor addressed) and commit the changeset atomically. A gate failure names the repair and keeps the changeset open; repair and call done again. The reply names the next ready batch (beginNext: true also claims it in the same call); the finish that empties the board reports the verdict with its counts.",
                        "inputSchema": {"type": "object", "properties": {"summary": {"type": "string"}, "beginNext": {"type": "boolean"}}, "required": ["summary"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "abandon_goals",
                        "description": "Drop the open changeset without committing. The batch's goals return to open.",
                        "inputSchema": {"type": "object", "properties": {"reason": {"type": "string"}}, "additionalProperties": false}
                    }));
                }
                // A ledger session's serving answers the protocol line's lifecycle
                // calls with acknowledgements; the ledger records are the commit.
                // Mirrors docs/compiler/tools.md#compilation-tools.
                if !self.modes.iter().any(|m| m == "compile")
                    && self.bridge.only.is_some()
                    && self.modes.iter().any(|m| m == "generate" || m == "verify")
                {
                    tools.push(json!({
                        "name": "begin_goals",
                        "description": "Acknowledge the batch this serving already holds. The work runs through the ledger tools; finish with done.",
                        "inputSchema": {"type": "object", "properties": {"batch": {"type": "string"}}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "done",
                        "description": "Acknowledge the end of the batch. The ledger records (record_binding, record_generation, record_verdict) are the commit; a row recorded resolves its goal.",
                        "inputSchema": {"type": "object", "properties": {"summary": {"type": "string"}}, "additionalProperties": false}
                    }));
                }
                if self.modes.iter().any(|m| m == "benchmark") {
                    tools.push(json!({
                        "name": "benchmark_cases",
                        "description": "The benchmark case list with each case's tier and state, and the run's progress. Next: begin_case.",
                        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "begin_case",
                        "description": "Claim the named case or the first pending one, against a throwaway sandbox. The reply carries the same instructions and package an in-process turn gets; do the work exactly as if it were real. One case open at a time.",
                        "inputSchema": {"type": "object", "properties": {"case": {"type": "string"}}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "finish_case",
                        "description": "Grade the open case with its deterministic checks and discard the sandbox. Turn cases face the done gates first (a rejection keeps the case open; repair and finish again). Verification cases pass verdict (pass|fail) and evidence.",
                        "inputSchema": {"type": "object", "properties": {"summary": {"type": "string"}, "verdict": {"type": "string", "enum": ["pass", "fail"]}, "evidence": {"type": "string"}}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "benchmark_report",
                        "description": "Close the run: tier scores, workflow verdicts, efficiency over the scored cases, appended to the machine-wide history under the given model name (name the agent honestly, e.g. claude-sonnet-4.6 (agent)).",
                        "inputSchema": {"type": "object", "properties": {"model": {"type": "string"}}, "additionalProperties": false}
                    }));
                }
                if self.modes.iter().any(|m| m == "decompile") {
                    tools.push(json!({
                        "name": "decompile_tasks",
                        "description": "Draft tasks derived from the unclaimed report (deliverable files no binding names), grouped by scope, each ready or gated behind a decompile release (`jazyk decompile` or the GUI records one). Next: begin_decompile on a released scope.",
                        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "begin_decompile",
                        "description": "The draft package for one scope: the inventory slice with file contents (tests first: they are the primary evidence), the lint rules, the suggested path, and the drafting contract. Read the code with your own tools, draft one markdown document stating what it observably does, then submit_draft.",
                        "inputSchema": {"type": "object", "properties": {"scope": {"type": "string"}}, "required": ["scope"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "submit_draft",
                        "description": "Validate and land the draft in the docs tree: path is project-relative and must match the docs glob; content is the full markdown (one H1, short declarative sentences, no em dashes, every statement citing its evidence in backticks). Records the draft hash for ratification and consumes the scope's release. The compiler picks the file up like any hand-written document.",
                        "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}, "scope": {"type": "string"}}, "required": ["path", "content"], "additionalProperties": false}
                    }));
                }
                if self.modes.iter().any(|m| m == "chat") {
                    tools.push(json!({
                        "name": "revise_requirement",
                        "description": "Change one requirement: the new prose replaces the old verbatim quote in its source document, and the graph node updates in the same atomic commit. Optional statement carries the new statement (defaults to keeping the old one).",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "new_text": {"type": "string", "description": "the new prose sentence, written into the document"}, "statement": {"type": "string"}}, "required": ["id", "new_text"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "add_requirement",
                        "description": "Add one requirement: the prose sentence is inserted into the named section (after after_quote when given, else at the section's end) and the requirement lands in the same atomic commit.",
                        "inputSchema": {"type": "object", "properties": {"doc": {"type": "string"}, "section": {"type": "string"}, "text": {"type": "string", "description": "the prose sentence inserted into the document"}, "statement": {"type": "string"}, "entities": {"type": "array", "items": {"type": "string"}}, "after_quote": {"type": "string"}}, "required": ["doc", "section", "text", "statement", "entities"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "retract_requirement",
                        "description": "Remove one requirement: its sentence leaves the prose and the node leaves the graph, one atomic commit. The deletion writes its change records, so a view or instance that referenced it gets a retrace goal.",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "reason": {"type": "string"}}, "required": ["id", "reason"], "additionalProperties": false}
                    }));
                    if let Some(t) = crate::tools::catalog()
                        .into_iter()
                        .find(|t| t.name == "edit_fact")
                    {
                        tools.push(json!({"name": t.name, "description": t.description, "inputSchema": t.parameters}));
                    }
                    // The two decree tools beside edit_fact: undo one, raise one
                    // threshold. Mirrors docs/compiler/tools.md#chat-tools.
                    tools.push(json!({
                        "name": "retract_decree",
                        "description": "Undo one decree: a node created by decree is deleted; a field decreed over a quoted fact returns to the prior value and source its journal entry recorded. The open ratify goal closes. One atomic commit.",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "reason": {"type": "string"}}, "required": ["id", "reason"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "bump_limit",
                        "description": "Raise one entity's or view's own threshold above the built-in limit (limits: {<limit>: value}), recorded with decree provenance. limit is a registry name that applies to the node's kind, value positive. This dismisses a size goal: it stops deriving until the count crosses the raised threshold.",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "limit": {"type": "string"}, "value": {"type": "integer", "minimum": 1}, "note": {"type": "string"}}, "required": ["id", "limit", "value"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "answer_diagnostic",
                        "description": "Record a human answer to a diagnostic's prompt, relayed from conversation. Pass option (index) for a chosen option or text for a freeform reply. An edit option applies as a dual write and resolves the finding before this returns; any other answer is recorded and the reply hands the handling contract back to you: act on it with the tools, then resolve_diagnostic.",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "option": {"type": "integer", "minimum": 0}, "text": {"type": "string"}}, "required": ["id"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "update_diagnostic",
                        "description": "Replace the question attached to an open diagnostic (null prompt removes it). Never touches a human answer or triage.",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "prompt": crate::tools::prompt_schema()}, "required": ["id"], "additionalProperties": false}
                    }));
                    // Scaffolding is offered only where there is something to scaffold,
                    // and settings only where a jazyk.toml holds them. Listing both
                    // everywhere spends a call to earn a refusal.
                    // Mirrors docs/frontends/acp.md#project-tools.
                    if !self.initialized() {
                        tools.push(json!({
                            "name": "init_project",
                            "description": "Scaffold a jazyk project in this directory: jazyk.toml, docs/ with a placeholder root document, and deliverable/. This directory has none yet.",
                            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
                        }));
                    } else {
                        tools.push(json!({
                            "name": "update_project_settings",
                            "description": "Edit jazyk.toml keys as minimal line edits. Supported keys: workflow.compile, workflow.generate, workflow.worker, acp.agent, gen.deliverable, gen.worker, llm.model, llm.base_url.",
                            "inputSchema": {"type": "object", "properties": {"settings": {"type": "object", "additionalProperties": {"type": "string"}}}, "required": ["settings"], "additionalProperties": false}
                        }));
                    }
                }
                // An ephemeral serving exists for one task; a long poll there is a
                // stall wearing a tool's name, so it is not offered.
                // Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
                if !self.bridge.ephemeral {
                    tools.push(json!({
                        "name": "await_changes",
                        "description": "Long poll: returns when the graph moves, a documentation file changes, or the ledger or a watched deliverable file changes, or at the timeout (default 300s; 0 waits indefinitely, use only when your client does not bound tool calls). Carries the task counts per queue and which tool lists the work. Never call it with a task open; finish with done first.",
                        "inputSchema": {"type": "object", "properties": {"timeout_seconds": {"type": "integer", "description": "seconds before returning unchanged; 0 = wait indefinitely"}}, "additionalProperties": false}
                    }));
                }
                // One name, one tool, whatever the modes: the union of servings must
                // not list a shared tool twice.
                let mut seen: BTreeSet<String> = BTreeSet::new();
                tools.retain(|t| seen.insert(t["name"].as_str().unwrap_or_default().to_string()));
                Ok(json!({"tools": tools}))
            }
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default().to_string();
                // The transcript row: the call under its task's label, condensed the
                // same way a turn's rows are. Mirrors docs/frontends/mcp.md#transcripts.
                let label = match self.open.lock().unwrap().as_ref() {
                    Some(o) => o.id.clone(),
                    None => format!("mcp {}", self.modes.join(",")),
                };
                self.trace.event(crate::session::TraceEvent::ToolCall {
                    activity: crate::live_preview::activity(&name, &params["arguments"], None),
                    label: label.clone(),
                    name: name.clone(),
                    summary: crate::session::condense(&params["arguments"], 160),
                    full: crate::session::full_payload(&params["arguments"]),
                });
                let reply = self.tool_call(&name, params, &label);
                if let Ok(v) = &reply {
                    let is_err = v["isError"] == true;
                    let text = v["content"][0]["text"].as_str().unwrap_or_default();
                    let parsed: Value = serde_json::from_str(text).unwrap_or_else(|_| json!(text));
                    if is_err {
                        let (rule, message) = crate::session::refusal(text, "error");
                        self.trace.event(crate::session::TraceEvent::ToolError {
                            label: label.clone(),
                            rule,
                            message,
                        });
                    } else {
                        self.trace.event(crate::session::TraceEvent::ToolResult {
                            activity: crate::live_preview::activity(
                                &name,
                                &params["arguments"],
                                Some(&parsed),
                            ),
                            label: label.clone(),
                            name: name.clone(),
                            summary: crate::session::condense(&parsed, 160),
                            full: crate::session::full_payload(&parsed),
                        });
                    }
                }
                self.update_preview(
                    &name,
                    &label,
                    reply.as_ref().is_ok_and(|v| v["isError"] != true),
                );
                reply
            }
            // Not part of MCP (clients end a serving by closing stdin), but agents
            // hand-driving the transport send it out of LSP habit; answering it is
            // cheaper than teaching every agent the difference.
            "shutdown" => Ok(Value::Null),
            _ => Err((-32601, format!("method not found: {}", method))),
        }
    }

    fn tool_call(&self, name: &str, params: &Value, _label: &str) -> Result<Value, (i64, String)> {
        {
            {
                let name = name.to_string();
                match name.as_str() {
                    #[cfg(not(target_arch = "wasm32"))]
                    "release_work" if !self.bridge.reader && self.modes.iter().any(|m| m == "chat") => {
                        let stage = params["arguments"]["stage"].as_str().unwrap_or_default();
                        if !matches!(stage, "compile" | "generate") {
                            return Ok(text_result(json!({"error":{"rule":"bad-argument","message":"stage must be compile or generate"}}), true));
                        }
                        if self.open.lock().unwrap().is_some() || crate::control::build_lease(&self.out).is_some() {
                            return Ok(text_result(json!({"error":{"rule":"work-running","message":"Finish the open batch or wait for the running build before releasing work."}}), true));
                        }
                        let _lock = crate::document_sets::ProjectLock::acquire(&self.project.root, "project.lock")
                            .map_err(|e| (-32602, e.1))?;
                        crate::control::release(&self.project, &self.out, Some(stage));
                        return Ok(text_result(json!({"released":stage,"generation":Store::load(&self.out).status.generation,"next":if stage == "generate" {"binding_tasks, then generation_tasks and verification_tasks"} else {"goals"}}), false));
                    }
                    "await_changes" => {
                        // Waiting makes no sense mid-task, and on an ephemeral
                        // serving there is no between-task to wait for.
                        if self.bridge.ephemeral {
                            return Ok(text_result(
                                json!({"error": {"rule": "not-served",
                                "message": "this serving exists for one task; there is nothing to await. Finish the open task with done."}}),
                                true,
                            ));
                        }
                        if self.open.lock().unwrap().is_some() {
                            return Ok(text_result(
                                json!({"error": {"rule": "batch-open",
                                "message": "a batch is open; awaiting changes now is a stall. Finish it with done (or abandon_goals), then await."}}),
                                true,
                            ));
                        }
                        return Ok(text_result(self.await_changes(params), false));
                    }
                    "goals" if self.modes.iter().any(|m| m == "compile" || m == "chat") => {
                        let full = params["arguments"]["full"].as_bool() == Some(true);
                        let v = self.goals_answer(full);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    "begin_goals" if self.modes.iter().any(|m| m == "compile" || m == "chat") => {
                        let v = self.begin_goals(params);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    // `done` is what every batch's instructions say; on a compile
                    // serving it is the finish. One verb everywhere.
                    "done"
                        if self.modes.iter().any(|m| m == "compile" || m == "chat")
                            && self.bench.lock().unwrap().open.is_none() =>
                    {
                        if let Some(v) = self.session_complete() {
                            return Ok(text_result(v, true));
                        }
                        if self.open.lock().unwrap().is_none() {
                            return Ok(text_result(
                                json!({"error": {"rule": "no-open-batch", "message": "no batch is open; begin_goals first"}}),
                                true,
                            ));
                        }
                        let v = self.done_batch(params);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    "abandon_goals" if self.modes.iter().any(|m| m == "compile" || m == "chat") => {
                        let v = self.abandon_goals(params);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    // A ledger session's serving already holds its batch: the
                    // lifecycle calls answer with acknowledgements, and the ledger
                    // records are the commit.
                    // Mirrors docs/compiler/tools.md#compilation-tools.
                    "goals" | "begin_goals" | "done" | "abandon_goals"
                        if self.bridge.only.is_some()
                            && self.modes.iter().any(|m| m == "generate" || m == "verify") =>
                    {
                        let only = self.bridge.only.clone().unwrap_or_default();
                        let v = match name.as_str() {
                            "begin_goals" => json!({"batch": only,
                                "note": "this serving already holds its batch; work through the ledger tools, then done"}),
                            "done" => json!({"ok": true, "batch": only,
                                "note": "the ledger records are the commit for this batch; a row recorded resolves its goal"}),
                            "abandon_goals" => json!({"abandoned": only,
                                "note": "nothing was staged here; unrecorded rows stay owed"}),
                            _ => crate::board::Board::compute(&self.project, &self.out).answer(),
                        };
                        return Ok(text_result(v, false));
                    }
                    "benchmark_cases" if self.modes.iter().any(|m| m == "benchmark") => {
                        return Ok(text_result(self.benchmark_cases(), false))
                    }
                    "begin_case" if self.modes.iter().any(|m| m == "benchmark") => {
                        return Ok(text_result(self.begin_case(params), false))
                    }
                    "finish_case" if self.modes.iter().any(|m| m == "benchmark") => {
                        return Ok(text_result(self.finish_case(params), false))
                    }
                    "benchmark_report" if self.modes.iter().any(|m| m == "benchmark") => {
                        return Ok(text_result(self.benchmark_report(params), false))
                    }
                    "decompile_tasks" if self.modes.iter().any(|m| m == "decompile") => {
                        let store = Store::load(&self.out);
                        let gs = crate::gen::GenSettings::resolve(&self.project);
                        let control = crate::control::Control::load(&self.project, &self.out);
                        let tasks = crate::decompile::pending(&self.project, &store, &gs, &control);
                        let reply = if tasks.is_empty() {
                            json!({"tasks": [], "note": "no unclaimed files; every deliverable file is named by a binding"})
                        } else {
                            json!({"tasks": tasks, "next": "begin_decompile on a released scope; gated scopes await `jazyk decompile` or the GUI"})
                        };
                        return Ok(text_result(reply, false));
                    }
                    "begin_decompile" if self.modes.iter().any(|m| m == "decompile") => {
                        let Some(scope) = params["arguments"]["scope"].as_str() else {
                            return Ok(text_result(
                                json!({"error": {"rule": "missing-argument", "message": "scope is required; decompile_tasks lists the scopes"}}),
                                true,
                            ));
                        };
                        let store = Store::load(&self.out);
                        let gs = crate::gen::GenSettings::resolve(&self.project);
                        let control = crate::control::Control::load(&self.project, &self.out);
                        let released = control
                            .released
                            .decompile
                            .iter()
                            .any(|s| s == scope || s == ".");
                        if !released {
                            return Ok(text_result(
                                json!({"error": {"rule": "awaiting-release", "message": format!(
                                "scope `{}` is not released for decompilation; `jazyk decompile {}` or the GUI's decompile action approves it", scope, scope)}}),
                                true,
                            ));
                        }
                        let reply = match crate::decompile::task(&self.project, &store, &gs, scope)
                        {
                            Ok(v) => v,
                            Err(e) => json!({"error": {"rule": "unknown-scope", "message": e}}),
                        };
                        let is_err = !reply["error"].is_null();
                        return Ok(text_result(reply, is_err));
                    }
                    "revise_requirement" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.revise_requirement(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "add_requirement" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.add_requirement(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "retract_requirement" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.retract_requirement(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "edit_fact" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.edit_fact(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "retract_decree" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.retract_decree(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "bump_limit" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.bump_limit(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "answer_diagnostic" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.answer_diagnostic(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "update_diagnostic" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.update_diagnostic_chat(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "init_project" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.init_project();
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "update_project_settings" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.update_project_settings(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "submit_draft" if self.modes.iter().any(|m| m == "decompile") => {
                        let path = params["arguments"]["path"].as_str().unwrap_or_default();
                        let content = params["arguments"]["content"].as_str().unwrap_or_default();
                        let scope = params["arguments"]["scope"].as_str();
                        let reply = match crate::decompile::submit(
                            &self.project,
                            &self.out,
                            path,
                            content,
                            scope,
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                return Ok(text_result(
                                    json!({"error": {"rule": "bad-draft", "message": e}}),
                                    true,
                                ))
                            }
                        };
                        return Ok(text_result(reply, false));
                    }
                    _ => {}
                }
                let args = params["arguments"].clone();
                let enabled = self.enabled_tools();
                if !enabled.contains(&name.as_str()) {
                    return Err((-32602, format!("unknown or disabled tool `{}`", name)));
                }
                let is_write = !crate::tools::READ_TOOLS.contains(&name.as_str())
                    && name != crate::tools::FEEDBACK_TOOL;
                let is_graph_write = is_write
                    && !crate::tools::GEN_TOOLS.contains(&name.as_str())
                    && !crate::tools::BIND_TOOLS.contains(&name.as_str())
                    && !crate::tools::VERIFY_TOOLS.contains(&name.as_str())
                    && !crate::tools::FILE_TOOLS.contains(&name.as_str())
                    && !crate::task_runtime::TOOLS.contains(&name.as_str());

                // An open benchmark case takes every call first: the sandbox is the
                // world under test. Mirrors docs/benchmark/benchmark.md#agent-run-benchmarks.
                {
                    let mut b = self.bench.lock().unwrap();
                    if let Some(o) = b.open.as_mut() {
                        let allowed = {
                            let mut t = toolset(&o.item.task);
                            t.extend(crate::tools::GEN_TOOLS);
                            t.push("run_tests");
                            t
                        };
                        if !allowed.contains(&name.as_str()) && name != crate::tools::FEEDBACK_TOOL
                        {
                            return Ok(text_result(
                                json!({"error": {"rule": "wrong-toolset", "message": format!(
                                    "`{}` is not part of a {} case", name, o.item.task)}}),
                                true,
                            ));
                        }
                        o.calls += 1;
                        return match o.session.dispatch(&name, &args) {
                            Ok(v) => Ok(text_result(v, false)),
                            Err(e) => Ok(text_result(e.to_value(), true)),
                        };
                    }
                }
                // Graph writes and goal tools stage into the open batch's changeset,
                // narrowed to the union of the batch's goal kinds' toolsets. Mirrors
                // docs/frontends/mcp.md#compilation-over-mcp.
                let mut open = self.open.lock().unwrap();
                if let Some(o) = open.as_mut() {
                    let kinds = o.session.scope.kinds();
                    let kind_refs: Vec<&str> = kinds.iter().map(String::as_str).collect();
                    let allowed = toolset_for_kinds(&kind_refs);
                    if is_graph_write && !allowed.contains(&name.as_str()) {
                        return Ok(text_result(
                            json!({"error": {"rule": "wrong-toolset", "message": format!(
                                "`{}` is not part of a {} batch; this batch's write tools: {}",
                                name, kinds.join("+"),
                                allowed.iter().filter(|t| !crate::tools::READ_TOOLS.contains(t) && !crate::tools::GOAL_TOOLS.contains(t) && **t != crate::tools::FEEDBACK_TOOL && **t != crate::tools::SPEND_TOOL).cloned().collect::<Vec<_>>().join(", "))}}),
                            true,
                        ));
                    }
                    o.rounds += 1;
                    // Activity on the open batch keeps its lease alive.
                    crate::control::refresh_lease(&self.out, &o.id);
                    return match o.session.dispatch(&name, &args) {
                        Ok(v) => Ok(text_result(v, false)),
                        Err(e) => Ok(text_result(e.to_value(), true)),
                    };
                }
                if is_graph_write && self.modes.iter().any(|m| m == "compile" || m == "chat") {
                    if let Some(v) = self.session_complete_closed() {
                        return Ok(text_result(v, true));
                    }
                    // Binding and generation have no batch: a contradiction the tests
                    // expose reaches the author through the failing-binding check
                    // (docs/frontends/mcp.md#generation-and-verification-over-mcp).
                    let lifecycle = self.modes.iter().any(|m| m == "generate" || m == "bind");
                    let message = if lifecycle {
                        "no batch is open; begin_goals first, then stage writes into it. During binding or generation there is no batch: record the row's verdict, and a contradiction the tests expose reaches the author through the failing-binding check at the next check run; report_diagnostic belongs to compile sessions"
                    } else {
                        "no batch is open; begin_goals first, then stage writes into it"
                    };
                    return Ok(text_result(
                        json!({"error": {"rule": "no-open-batch", "message": message}}),
                        true,
                    ));
                }
                drop(open);

                // The ledger task tools answer from the board: the open bind,
                // generate, and verify goals, which the ledger lifecycles claim and
                // recording resolves. Mirrors
                // docs/frontends/mcp.md#generation-and-verification-over-mcp.
                if matches!(
                    name.as_str(),
                    "binding_tasks" | "generation_tasks" | "verification_tasks"
                ) {
                    let kind = match name.as_str() {
                        "binding_tasks" => "bind",
                        "generation_tasks" => "generate",
                        _ => "verify",
                    };
                    let board = crate::board::Board::compute(&self.project, &self.out);
                    let rows: Vec<Value> = board
                        .goals
                        .iter()
                        .filter(|g| g.kind == kind)
                        .map(|g| {
                            let mut v = json!({
                                "goal": g.id,
                                "reason": g.change["reason"],
                                "state": g.state,
                                "gated": board.gated.contains(&g.id),
                                "ready": board.is_ready(&g.id),
                            });
                            if let Some(r) = board.readiness.get(&g.id).and_then(|r| r.reason()) {
                                v["blockedBy"] = json!(r);
                            }
                            if kind == "generate" {
                                v["entity"] = json!(g.target);
                            } else {
                                v["requirement"] = json!(g.target);
                                v["entity"] = g.change["entity"].clone();
                            }
                            v
                        })
                        .collect();
                    let begin = match kind {
                        "bind" => "begin_binding",
                        "generate" => "begin_generation",
                        _ => "begin_verification (or run_tests for programmatic rows)",
                    };
                    // Rows that went stale under a generation (a shared test file, an
                    // implementation file) hold no generate goal until run_tests
                    // re-baselines them; say so instead of letting the list read as
                    // complete (docs/frontends/mcp.md#generation-and-verification-over-mcp).
                    let stale_note = if kind == "generate" {
                        let gs = crate::gen::GenSettings::resolve(&self.project);
                        let ledger = crate::gen::Ledger::load(&self.out);
                        let store = Store::load(&self.out);
                        let stale = ledger
                            .requirements
                            .iter()
                            .filter(|(rid, row)| {
                                store.graph.requirements.contains_key(*rid)
                                    && matches!(
                                        crate::verify::status_of(&store, rid, row, &gs).0.as_str(),
                                        "stale-test" | "stale-code"
                                    )
                            })
                            .count();
                        (stale > 0).then(|| format!(
                            "{} row(s) went stale since their last run (a test or implementation file changed); run_tests re-baselines them, and any generation work they hold returns to this list afterwards",
                            stale
                        ))
                    } else {
                        None
                    };
                    let binding = if kind == "generate" { board.goals.iter().filter(|g| g.kind == "bind").count() } else { 0 };
                    let mut reply = json!({
                        "tasks": rows,
                        "count": rows.len(),
                        "next": if rows.is_empty() && binding > 0 {
                            format!("{} binding goal(s) remain; call binding_tasks first. No generation goal is currently derived. This does not mean the deliverable exists or is verified.", binding)
                        } else if rows.is_empty() {
                            "nothing owed".to_string()
                        } else if rows.iter().all(|r| r["gated"] == json!(true)) {
                            format!("every row is gated on the generate release: `jazyk release generate` (or the GUI) approves them, then {} claims one", begin)
                        } else {
                            format!("{} claims one ready row (blockedBy says what a row that is not ready waits for); recording it resolves its goal", begin)
                        },
                    });
                    if kind == "generate" {
                        reply["prerequisites"] = json!({"binding":binding});
                    }
                    if matches!(kind, "bind" | "generate")
                        && !crate::gen::GenSettings::resolve(&self.project).configured
                    {
                        reply["prerequisites"]["sourceMapping"] = json!(false);
                        reply["next"] = json!("The document set has no source mapping. For the requested generation, call list_document_sets, then update_document_set with its hash as baseHash and set {id, source: [narrow implementation, test, and support-file patterns]}. Preserve documents and existing settings. Reload goals to resolve criteria, then binding_tasks. An empty generation list is not a completed deliverable.");
                    }
                    if let Some(n) = stale_note {
                        reply["staleNote"] = json!(n);
                    }
                    return Ok(text_result(reply, false));
                }

                // The control plane over the stateless generation lifecycle: manual
                // mode gates begins behind a release, begin claims the entity's
                // lease, record frees it. Mirrors docs/frontends/mcp.md#the-control-plane-over-mcp.
                if name == "begin_generation" {
                    let c = crate::control::Control::load(&self.project, &self.out);
                    if self.bridge.build_token.is_none() {
                        if c.generate == "manual"
                            && c.released.generate != Store::load(&self.out).status.generation
                        {
                            return Ok(text_result(
                                json!({"error": {"rule": "awaiting-release", "message":
                                    "generation is gated: the workflow is manual and the graph's changes are not released yet; `jazyk release generate` or the GUI's generate action approves them"}}),
                                true,
                            ));
                        }
                        if let Some(l) = crate::control::build_lease(&self.out) {
                            return Ok(text_result(
                                json!({"error": {"rule": "build-running", "message": format!(
                                    "an internal build is running (lease `{}`, expires {}s after its last 30s heartbeat); await_changes returns when it clears", l.worker, crate::control::LEASE_TTL_SECS)}}),
                                true,
                            ));
                        }
                    }
                    if let Some(ent) = args["entity"].as_str() {
                        // A generate goal that is not ready (its requirements still owe
                        // a binding, a compile goal open in its cone) is refused with
                        // the board's reason: a Codex session once bound one requirement
                        // and generated the whole entity next.
                        // Mirrors docs/compiler/goals/generate.md#readiness.
                        let mut board = crate::board::Board::compute(&self.project, &self.out);
                        board.mark_own(&self.worker_id());
                        let gid = format!("g:generate:{}", ent);
                        // A lease (this serving's own retry, or another worker's) is the
                        // claim below's to answer.
                        // An internal run's session is the approval: the release gate
                        // never refuses it, only real readiness does, since the run's own
                        // commits advance the generation past the release it stamped
                        // (docs/compiler/control-plane.md#modes-and-releases).
                        let only_gated =
                            self.bridge.build_token.is_some() && board.gated.contains(&gid);
                        if board.goals.iter().any(|g| g.id == gid)
                            && !board.is_ready(&gid)
                            && !board.claimed.contains_key(&gid)
                            && !only_gated
                        {
                            if let Some(r) = board.readiness.get(&gid).and_then(|r| r.reason()) {
                                return Ok(text_result(
                                    json!({"error": {"rule": "not-ready", "message": format!(
                                        "`{}` is not ready to generate: {}; generation_tasks lists the rows that are", ent, r)}}),
                                    true,
                                ));
                            }
                        }
                        if let Err(holder) =
                            crate::control::claim(&self.out, ent, &self.worker_id())
                        {
                            return Ok(text_result(
                                json!({"error": {"rule": "claimed", "message": format!(
                                    "`{}` is claimed by worker `{}`; pick another entity or wait for the lease to lapse ({}s after its last activity)", ent, holder, crate::control::LEASE_TTL_SECS)}}),
                                true,
                            ));
                        }
                    }
                }
                // Binding rides the same control plane: it writes test files into the
                // deliverable, so the generate release gates it, and the requirement
                // lease makes the claim exclusive.
                // Mirrors docs/consumers/bind.md#when-binding-runs.
                if name == "begin_binding" {
                    let c = crate::control::Control::load(&self.project, &self.out);
                    if self.bridge.build_token.is_none() {
                        if c.generate == "manual"
                            && c.released.generate != Store::load(&self.out).status.generation
                        {
                            return Ok(text_result(
                                json!({"error": {"rule": "awaiting-release", "message":
                                    "binding is gated: the workflow is manual and the graph's changes are not released yet; `jazyk release generate` or the GUI's generate action approves them"}}),
                                true,
                            ));
                        }
                        if let Some(l) = crate::control::build_lease(&self.out) {
                            return Ok(text_result(
                                json!({"error": {"rule": "build-running", "message": format!(
                                    "an internal build is running (lease `{}`, expires {}s after its last 30s heartbeat); await_changes returns when it clears", l.worker, crate::control::LEASE_TTL_SECS)}}),
                                true,
                            ));
                        }
                    }
                    if let Some(rid) = args["requirement"].as_str() {
                        if let Err(holder) =
                            crate::control::claim(&self.out, rid, &self.worker_id())
                        {
                            return Ok(text_result(
                                json!({"error": {"rule": "claimed", "message": format!(
                                    "`{}` is claimed by worker `{}`; pick another requirement or wait for the lease to lapse ({}s after its last activity)", rid, holder, crate::control::LEASE_TTL_SECS)}}),
                                true,
                            ));
                        }
                    }
                }

                if matches!(name.as_str(), "record_generation" | "record_binding") {
                    if let Some(target) = args[if name == "record_generation" { "entity" } else { "requirement" }].as_str() {
                        if let Some(lease) = crate::control::leases(&self.out).get(target) {
                            if lease.worker != self.worker_id() {
                                return Ok(text_result(json!({"error":{"rule":"claimed","message":format!("{target} is claimed by worker {}; wait for that claim to finish.", lease.worker)}}), true));
                            }
                        }
                    }
                }
                if crate::task_runtime::TOOLS.contains(&name.as_str()) {
                    return match crate::task_runtime::call(&self.project.root,&name,&args) {
                        Ok(value)=>Ok(text_result(value,false)),
                        Err(error)=>Ok(text_result(json!({"error":{"rule":"task-runtime","message":error}}),true)),
                    };
                }
                let store = Store::load(&self.out);
                if store.docs.is_empty()
                    && store.graph.entities.is_empty()
                    && !self.modes.iter().any(|m| m == "compile")
                    && name != "report_feedback"
                    && name != crate::tools::SPEND_TOOL
                {
                    return Ok(text_result(
                        json!({"error": {"rule": "no-build", "message": "no graph found; run `jazyk compile` first (or connect a compile serving)"}}),
                        true,
                    ));
                }
                let mut scope = WorkScope::serving(if is_write { "mcp-write" } else { "mcp-read" });
                // A ledger session's serving holds its batch by id: the batch target is
                // whose component group the file tools write for, so a group session
                // extends its members' files and a regeneration rewrites its own.
                // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
                if let Some((_, target)) = self
                    .bridge
                    .only
                    .as_deref()
                    .and_then(crate::goals::parse_goal_id)
                {
                    scope.target = target.to_string();
                } else if let Some(target) = self.ledger_target.lock().unwrap().clone() {
                    if name == "write_text_file" {
                        if let Err(holder) = crate::control::claim(&self.out, &target, &self.worker_id()) {
                            return Ok(text_result(json!({"error":{"rule":"claimed","message":format!("{target} is claimed by worker {holder}; wait or begin another entity.")}}), true));
                        }
                    }
                    scope.target = target;
                }
                let mut session =
                    ToolSession::new(store, scope, self.mutation_limit, self.context_budget);
                session.gen = crate::gen::GenSettings::resolve(&self.project);
                session.lint = self.project.linting.clone();
                session.caller =
                    self.caller(if self.write { "mcp-write" } else { "mcp-read" }, &name);
                match session.dispatch(&name, &args) {
                    Ok(v) => {
                        if matches!(name.as_str(), "begin_generation" | "begin_binding") {
                            if let Some(target) = args[if name == "begin_generation" { "entity" } else { "requirement" }].as_str() {
                                let previous = self.ledger_target.lock().unwrap().replace(target.to_owned());
                                if let Some(previous) = previous.filter(|p| p != target) {
                                    self.release_own_ledger_claim(&previous);
                                }
                            }
                        } else if matches!(name.as_str(), "record_generation" | "record_binding") {
                            if let Some(target) = args[if name == "record_generation" { "entity" } else { "requirement" }].as_str() {
                                self.release_own_ledger_claim(target);
                                let mut current = self.ledger_target.lock().unwrap();
                                if current.as_deref() == Some(target) { *current = None; }
                            }
                        }
                        if is_graph_write && !session.staged.is_empty() {
                            // graph --write: each call commits as its own changeset.
                            // The commit writes its change records, so the next
                            // build derives the goals the write opened.
                            let mut s = Store::load(&self.out);
                            let report = s.apply(
                                session.staged,
                                &crate::store::Commit::session(Vec::new(), 1, None),
                            );
                            let mut v = v;
                            v["committed"] = json!(report.applied);
                            if !report.skipped.is_empty() {
                                v["skipped"] = json!(report.skipped);
                            }
                            return Ok(text_result(v, false));
                        }
                        Ok(text_result(v, false))
                    }
                    Err(e) => {
                        if matches!(name.as_str(), "begin_generation" | "begin_binding") {
                            if let Some(target) = args[if name == "begin_generation" { "entity" } else { "requirement" }].as_str() {
                                self.release_own_ledger_claim(target);
                            }
                        }
                        Ok(text_result(e.to_value(), true))
                    }
                }
            }
        }
    }
}

// ---- the chat tools: dual writes and project setup ----
// A requirement lives in the prose; a chat edit moves the document and the graph in
// one atomic commit. Mirrors docs/compiler/tools.md#chat-tools and
// docs/frontends/acp.md#dual-write-tools.
impl McpServer {
    fn release_own_ledger_claim(&self, target: &str) {
        if let Some(claim) = crate::control::owned_lease(&self.out, target, &self.worker_id()) {
            let _ = crate::control::release_owned_lease(&claim);
        }
    }

    // Write a document (or jazyk.toml) edit: through the delegating sink when the
    // spawning proxy listens, straight to disk otherwise.
    // Mirrors docs/frontends/acp.md#doc-edit-delegation.
    fn write_edit(
        &self,
        rel: &str,
        old_text: &str,
        new_text: &str,
        full: &str,
    ) -> Result<(), String> {
        let path = self.project.root.join(rel);
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("docx") || e.eq_ignore_ascii_case("pdf")) {
            return Err("DOCX and PDF cannot be changed by a text edit; use the format editor".into());
        }
        if let Some(sink) = &self.bridge.edit_sink {
            if sink_write(sink, &path, old_text, new_text, full).is_ok() {
                return Ok(());
            }
            // Nothing listening: fall through to the direct write.
        }
        crate::vfs::write(&path, full).map_err(|e| format!("write {}: {}", path.display(), e))
    }

    // A tool session for one chat call over a synced snapshot (the prose edit already
    // absorbed when one is given, so the usual gates validate the new quote).
    fn chat_session(
        &self,
        parsed: &std::collections::BTreeMap<
            String,
            (
                String,
                std::collections::BTreeMap<String, crate::model::Section>,
            ),
        >,
        edit: Option<&ProseEdit>,
        target: &str,
    ) -> ToolSession {
        let mut snapshot = Store::load(&self.out);
        snapshot.sync_docs(parsed);
        if let Some(e) = edit {
            snapshot.absorb_doc_edit(&e.doc, &e.full);
        }
        let mut scope = WorkScope::serving("mcp-write");
        scope.target = target.to_string();
        let mut session =
            ToolSession::new(snapshot, scope, self.mutation_limit, self.context_budget);
        session.gen = crate::gen::GenSettings::resolve(&self.project);
        session.lint = self.project.linting.clone();
        session.caller = self.caller("chat", target);
        session
    }

    // The shared tail of every dual write: the staged graph mutations commit with
    // the prose replacement as one changeset through the store, the file written
    // first (through the delegating sink when the proxy listens) and put back when
    // the commit skips. Mirrors docs/compiler/compilation.md#edit-paths.
    fn commit_dual_write(&self, edit: &ProseEdit, ops: Vec<crate::store::Op>, kind: &str) -> Value {
        if ops.is_empty() {
            return json!({"error": {"rule": "edit-needs-mutation", "message": "the call staged no graph mutation; a prose edit never lands alone"}});
        }
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        let mut s = Store::load(&self.out);
        s.sync_docs(&parsed);
        let write =
            |doc: &str, old: &str, new: &str, full: &str| self.write_edit(doc, old, new, full);
        match s.dual_write(
            &self.project.root,
            edit,
            ops,
            &crate::store::Commit::store(kind),
            Some(&write),
        ) {
            Ok(report) => json!({"committed": true, "applied": report.applied, "doc": edit.doc,
                   "note": "the prose and the graph moved together; no recompile is owed for this edit"}),
            Err(e) => json!({"error": {"rule": "commit-skipped", "message": e}}),
        }
    }

    // Validate one graph call through the session gates against a snapshot that
    // absorbed the edit, then commit the pair.
    fn dual_commit(&self, edit: ProseEdit, graph_call: (&str, Value), target: &str) -> Value {
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        let mut session = self.chat_session(&parsed, Some(&edit), target);
        let (name, args) = graph_call;
        if let Err(e) = session.dispatch(name, &args) {
            return e.to_value();
        }
        let ops = std::mem::take(&mut session.staged);
        self.commit_dual_write(&edit, ops, "dual-write")
    }

    // The section's stored body, for locating a prose edit inside it.
    fn section_raw(&self, doc: &str, section: &str) -> Option<String> {
        Store::load(&self.out)
            .docs
            .get(doc)
            .and_then(|d| d.sections.get(section))
            .map(|s| s.raw.clone())
    }

    // One authored field on one node: a dual write when the fact is quoted and a
    // sentence rewrite was accepted, a decree with its ratification proposal
    // otherwise. Mirrors docs/compiler/tools.md#chat-tools.
    fn edit_fact(&self, args: &Value) -> Value {
        let target = args["id"].as_str().unwrap_or_default().to_string();
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        let mut session = self.chat_session(&parsed, None, &target);
        let reply = match session.dispatch("edit_fact", args) {
            Ok(v) => v,
            Err(e) => return e.to_value(),
        };
        let ops = std::mem::take(&mut session.staged);
        if reply["prose"].is_object() {
            let p = &reply["prose"];
            let (doc, section) = (
                p["doc"].as_str().unwrap_or_default().to_string(),
                p["section"].as_str().unwrap_or_default().to_string(),
            );
            let path = self.project.root.join(&doc);
            let old_full = match crate::vfs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", doc, e)}})
                }
            };
            let edit = match ProseEdit::locate(
                &doc,
                &section,
                self.section_raw(&doc, &section).as_deref(),
                &old_full,
                p["old_text"].as_str().unwrap_or_default(),
                p["new_text"].as_str().unwrap_or_default(),
            ) {
                Ok(e) => e,
                Err(e) => return json!({"error": {"rule": "stale-anchor", "message": e}}),
            };
            let mut v = self.commit_dual_write(&edit, ops, "dual-write");
            if v["error"].is_null() {
                v["path"] = json!("dual-write");
            }
            return v;
        }
        let mut s = Store::load(&self.out);
        s.sync_docs(&parsed);
        let report = s.apply(ops, &crate::store::Commit::store("decree"));
        if !report.skipped.is_empty() {
            return json!({"error": {"rule": "commit-skipped", "message": report.skipped.join("; ")}});
        }
        let mut v = reply;
        v["committed"] = json!(true);
        v["generation"] = json!(report.generation);
        v
    }

    // The decree's author: the connected client's name, else the login, else `human`,
    // the same ladder a chat ToolSession climbs for edit_fact.
    fn chat_decree_author(&self) -> String {
        self.client
            .lock()
            .unwrap()
            .clone()
            .filter(|c| !c.trim().is_empty())
            .or_else(|| crate::sys::env_var("USER").ok().filter(|u| !u.is_empty()))
            .unwrap_or_else(|| "human".to_string())
    }

    // Undo one decree as its own `decree` changeset. The store holds the gate (a
    // quoted or otherwise non-pending node is refused as `not-a-decree`) and the
    // commit closes the open ratify goal. Mirrors docs/compiler/tools.md#chat-tools.
    fn retract_decree(&self, args: &Value) -> Value {
        let id = args["id"].as_str().unwrap_or_default().trim();
        let reason = args["reason"].as_str().unwrap_or_default().trim();
        if id.is_empty() || reason.is_empty() {
            return json!({"error": {"rule": "missing-argument", "message": "pass id (the decreed node) and reason (one line, journaled)"}});
        }
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        let mut s = Store::load(&self.out);
        s.sync_docs(&parsed);
        let rid = s.resolve_id(id).to_string();
        // The store's predicate, asked first so a refusal writes no generation: a
        // decree or derivation is any provenance but a quote, on a node with no
        // sentence of its own; a default view was never decreed.
        let pending = |p: &Option<crate::model::Provenance>| {
            p.as_ref()
                .is_some_and(|p| !matches!(p, crate::model::Provenance::Quote(_)))
        };
        let is_decree = if let Some(r) = s.graph.requirements.get(&rid) {
            r.source.is_none() && pending(&r.provenance)
        } else if let Some(e) = s.graph.entities.get(&rid) {
            pending(&e.provenance)
        } else if let Some(v) = s.graph.views.get(&rid) {
            !v.default && pending(&v.provenance)
        } else if let Some(c) = s.graph.criteria.get(&rid) {
            pending(&c.provenance)
        } else {
            return json!({"error": {"rule": "unknown-id", "message": format!("unknown node `{}`", rid)}});
        };
        if !is_decree {
            return json!({"error": {"rule": "not-a-decree", "message": format!(
                "{} is quoted from the documents, not decreed; retract_requirement or edit_fact changes it", rid)}});
        }
        let report = s.apply(
            vec![crate::store::Op::RetractDecree {
                id: rid.clone(),
                reason: reason.to_string(),
            }],
            &crate::store::Commit::store("decree"),
        );
        if !report.skipped.is_empty() {
            let msg = report.skipped.join("; ");
            let rule = if msg.contains("not a decree") {
                "not-a-decree"
            } else {
                "commit-skipped"
            };
            return json!({"error": {"rule": rule, "message": msg}});
        }
        json!({
            "id": rid, "retracted": true, "path": "decree",
            "generation": report.generation, "swept": report.swept,
        })
    }

    // Raise one node's own threshold, recorded with decree provenance, as its own
    // `decree` changeset. The store holds the gates (registry name, applies to the
    // kind, positive value). Mirrors docs/compiler/tools.md#chat-tools and
    // docs/compiler/graph.md#per-node-bumps.
    fn bump_limit(&self, args: &Value) -> Value {
        let id = args["id"].as_str().unwrap_or_default().trim();
        let limit = args["limit"].as_str().unwrap_or_default().trim();
        let Some(value) = args["value"].as_u64().filter(|v| *v > 0) else {
            return json!({"error": {"rule": "bad-value", "message": "value is a positive integer: the node's new threshold"}});
        };
        if id.is_empty() || limit.is_empty() {
            return json!({"error": {"rule": "missing-argument", "message": format!(
                "pass id (an entity or view) and limit, one of: {}",
                crate::limits::LIMITS.iter().map(|l| l.name).collect::<Vec<_>>().join(", "))}});
        }
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        let mut s = Store::load(&self.out);
        s.sync_docs(&parsed);
        let rid = s.resolve_id(id).to_string();
        let is_entity = s.graph.entities.contains_key(&rid);
        let is_view = s.graph.views.contains_key(&rid);
        if !is_entity && !is_view {
            return json!({"error": {"rule": "unknown-id", "message": format!("no entity or view `{}`", rid)}});
        }
        // The store's gates, asked first so a refusal writes no generation: a
        // registry name that applies to the node's kind.
        let Some(l) = crate::limits::limit(limit) else {
            return json!({"error": {"rule": "bad-limit", "message": format!(
                "unknown limit `{}`; one of: {}", limit,
                crate::limits::LIMITS.iter().map(|l| l.name).collect::<Vec<_>>().join(", "))}});
        };
        let applies = (is_entity && crate::limits::ENTITY_LIMITS.contains(&l.name))
            || (is_view && crate::limits::VIEW_LIMITS.contains(&l.name));
        if !applies {
            return json!({"error": {"rule": "bad-limit", "message": format!(
                "`{}` does not apply to {}; {} limits: {}", limit, rid,
                if is_entity { "entity" } else { "view" },
                if is_entity { crate::limits::ENTITY_LIMITS.join(", ") } else { crate::limits::VIEW_LIMITS.join(", ") })}});
        }
        let note = args["note"]
            .as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("raised {} to {}", limit, value));
        let provenance = crate::model::Provenance::Decree {
            author: self.chat_decree_author(),
            at: crate::verify::now_iso(),
            note: Some(note),
        };
        let report = s.apply(
            vec![crate::store::Op::BumpLimit {
                id: rid.clone(),
                limit: limit.to_string(),
                value,
                provenance,
            }],
            &crate::store::Commit::store("decree"),
        );
        if !report.skipped.is_empty() {
            return json!({"error": {"rule": "bad-limit", "message": report.skipped.join("; ")}});
        }
        json!({
            "id": rid, "limit": limit, "value": value, "path": "bump",
            "generation": report.generation,
        })
    }

    // Record a human answer relayed from conversation. An edit option is applied by
    // the answer engine (through this serving's edit sink when the proxy listens);
    // any other answer is recorded and the handling contract returns to the calling
    // agent. Mirrors docs/frontends/acp.md#questions-in-chat.
    fn answer_diagnostic(&self, args: &Value) -> Value {
        let id = args["id"].as_str().unwrap_or_default().to_string();
        let reply = if let Some(i) = args["option"].as_u64() {
            crate::answer::Reply::Choice(i as usize)
        } else if let Some(t) = args["text"].as_str() {
            crate::answer::Reply::Text(t.to_string())
        } else {
            return json!({"error": {"rule": "missing-argument", "message": "pass option (an index into the prompt's options) or text (a freeform reply)"}});
        };
        let write =
            |doc: &str, old: &str, new: &str, full: &str| self.write_edit(doc, old, new, full);
        match crate::answer::answer(&self.project, &self.out, &id, reply, Some(&write)) {
            Ok(mut v) => {
                if v["status"] == "handling" {
                    if let Ok(p) = crate::answer::handling_prompt(&self.out, &id) {
                        v["next"] = json!(p);
                    }
                }
                v
            }
            Err(e) => json!({"error": {"rule": "answer-failed", "message": e}}),
        }
    }

    // Maintain the question on a finding, through a ToolSession so the same gates
    // validate the prompt, committed as its own changeset.
    fn update_diagnostic_chat(&self, args: &Value) -> Value {
        let target = args["id"].as_str().unwrap_or_default().to_string();
        let mut snapshot = Store::load(&self.out);
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        snapshot.sync_docs(&parsed);
        let mut scope = WorkScope::serving("mcp-write");
        scope.target = target.clone();
        let mut session =
            ToolSession::new(snapshot, scope, self.mutation_limit, self.context_budget);
        session.gen = crate::gen::GenSettings::resolve(&self.project);
        session.lint = self.project.linting.clone();
        session.caller = self.caller("chat", &target);
        if let Err(e) = session.dispatch("update_diagnostic", args) {
            return e.to_value();
        }
        let ops = std::mem::take(&mut session.staged);
        let mut s = Store::load(&self.out);
        s.sync_docs(&parsed);
        let item = crate::model::WorkItem {
            task: "chat".into(),
            target,
            dirty_sections: vec![],
            stale_anchors: vec![],
            proposals: Vec::new(),
        };
        let report = s.apply(ops, &item.commit(1, None));
        if !report.skipped.is_empty() {
            return json!({"error": {"rule": "commit-skipped", "message": report.skipped.join("; ")}});
        }
        json!({"updated": true})
    }

    fn revise_requirement(&self, args: &Value) -> Value {
        let rid = args["id"].as_str().unwrap_or_default();
        let new_text = args["new_text"].as_str().unwrap_or_default().trim();
        if new_text.is_empty() {
            return json!({"error": {"rule": "missing-argument", "message": "new_text is required: the prose sentence that replaces the old quote"}});
        }
        let store = Store::load(&self.out);
        let rid = store.resolve_id(rid).to_string();
        let Some(r) = store.graph.requirements.get(&rid) else {
            return json!({"error": {"rule": "unknown-id", "message": format!("unknown requirement `{}`", rid)}});
        };
        let Some(src) = r.source.as_ref() else {
            return json!({"error": {"rule": "not-quoted", "message": format!(
                "{} has no sentence in the documents yet ({}); ratify or retract it instead of revising prose", rid, crate::session::provenance_line(r))}});
        };
        let (doc, section, old_quote) = (src.doc.clone(), src.section.clone(), src.quote.clone());
        let statement = args["statement"]
            .as_str()
            .unwrap_or(&r.statement)
            .to_string();
        let section_raw = store
            .docs
            .get(&doc)
            .and_then(|d| d.sections.get(&section))
            .map(|s| s.raw.clone());
        drop(store);
        let path = self.project.root.join(&doc);
        let old_full = match crate::vfs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", doc, e)}})
            }
        };
        let edit = match ProseEdit::locate(
            &doc,
            &section,
            section_raw.as_deref(),
            &old_full,
            &old_quote,
            new_text,
        ) {
            Ok(e) => e,
            Err(_) => {
                return json!({"error": {"rule": "stale-anchor", "message": format!(
                    "the requirement's quote no longer locates in {}#{}; compile first, then revise. The section reads:\n{}",
                    doc, section, section_raw.unwrap_or_default())}})
            }
        };
        self.dual_commit(
            edit,
            ("update_requirement", json!({"id": rid, "statement": statement, "section": format!("{}#{}", doc, section), "quote": new_text})),
            &rid,
        )
    }

    fn add_requirement(&self, args: &Value) -> Value {
        let doc = args["doc"].as_str().unwrap_or_default().to_string();
        let section = args["section"]
            .as_str()
            .unwrap_or_default()
            .trim_start_matches(&format!("{}#", doc))
            .to_string();
        let text = args["text"].as_str().unwrap_or_default().trim().to_string();
        let statement = args["statement"].as_str().unwrap_or_default();
        if doc.is_empty() || section.is_empty() || text.is_empty() || statement.is_empty() {
            return json!({"error": {"rule": "missing-argument", "message": "doc, section, text, statement, and entities are required"}});
        }
        let Some(sec_raw) = self.section_raw(&doc, &section) else {
            return json!({"error": {"rule": "unknown-section", "message": format!("no section `{}#{}` in the graph; for a prose document edit use read_document then write_document with its hash. Compile only when the user requests compilation", doc, section)}});
        };
        let path = self.project.root.join(&doc);
        let old_full = match crate::vfs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", doc, e)}})
            }
        };
        // The insertion point: after the located quote, or at the section's end.
        let edit = match args["after_quote"].as_str() {
            Some(q) => match crate::md::locate_bytes(&old_full, q) {
                Some((_, at)) => ProseEdit {
                    doc: doc.clone(),
                    section: section.clone(),
                    old_text: String::new(),
                    new_text: text.clone(),
                    full: format!(
                        "{}\n\n{}{}",
                        old_full[..at].trim_end_matches('\n'),
                        text,
                        &old_full[at..]
                    ),
                    old_full,
                },
                None => {
                    return json!({"error": {"rule": "stale-anchor", "message": "after_quote does not locate in the document"}})
                }
            },
            None => match ProseEdit::locate(&doc, &section, Some(&sec_raw), &old_full, "", &text) {
                Ok(e) => e,
                Err(e) => return json!({"error": {"rule": "stale-anchor", "message": e}}),
            },
        };
        let entities: Vec<String> = args["entities"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        self.dual_commit(
            edit,
            // The author's own sentence is a stated fact: `constraint` is its honest
            // facet, and a review may sharpen it
            // (docs/compiler/model/requirement.md#facets).
            ("upsert_requirement", json!({"statement": statement, "entities": entities, "section": format!("{}#{}", doc, section), "quote": text, "facets": [{"facet": "constraint", "reasoning": "stated by the author in the document"}]})),
            &doc,
        )
    }

    fn retract_requirement(&self, args: &Value) -> Value {
        let rid = args["id"].as_str().unwrap_or_default();
        let reason = args["reason"].as_str().unwrap_or_default();
        let store = Store::load(&self.out);
        let rid = store.resolve_id(rid).to_string();
        let Some(r) = store.graph.requirements.get(&rid) else {
            return json!({"error": {"rule": "unknown-id", "message": format!("unknown requirement `{}`", rid)}});
        };
        let Some(src) = r.source.as_ref() else {
            return json!({"error": {"rule": "not-quoted", "message": format!(
                "{} has no sentence in the documents ({}); retract the decree or derivation instead", rid, crate::session::provenance_line(r))}});
        };
        let (doc, section, old_quote) = (src.doc.clone(), src.section.clone(), src.quote.clone());
        drop(store);
        let path = self.project.root.join(&doc);
        let old_full = match crate::vfs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", doc, e)}})
            }
        };
        let Some((b, e)) = crate::md::locate_bytes(&old_full, &old_quote) else {
            return json!({"error": {"rule": "stale-anchor", "message": format!(
                "the requirement's quote no longer locates in {}; compile first", doc)}});
        };
        // Take the sentence out; a bullet line loses its marker and trailing newline too.
        let mut start = b;
        let line_start = old_full[..b].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let lead = &old_full[line_start..b];
        if lead.trim_start().starts_with("- ") || lead.trim().is_empty() {
            start = line_start;
        }
        let mut end = e;
        if old_full[e..].starts_with('\n') && start == line_start {
            end = e + 1;
        }
        let full = format!("{}{}", &old_full[..start], &old_full[end..]);
        let edit = ProseEdit {
            doc,
            section,
            old_text: old_quote,
            new_text: String::new(),
            full,
            old_full,
        };
        self.dual_commit(
            edit,
            ("delete_requirement", json!({"id": rid, "reason": reason})),
            &rid,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn init_project(&self) -> Value {
        let root = &self.project.root;
        if root.join(crate::document_sets::CONFIG).vexists() {
            return json!({"error": {"rule": "already-a-project", "message": "jazyk.toml already exists here"}});
        }
        if let Err(e) = crate::vfs::write(
            {
                let path = root.join(".jazyk/jazyk.toml");
                crate::vfs::create_dir_all(path.parent().unwrap()).unwrap();
                path
            },
            crate::cli::INIT_TOML,
        ) {
            return json!({"error": {"rule": "write-failed", "message": e.to_string()}});
        }
        let made = match crate::cli::init_scaffold(root) {
            Ok(made) => made,
            Err(e) => return json!({"error": {"rule": "write-failed", "message": e}}),
        };
        json!({"initialized": root.display().to_string(), "created": made,
               "next": "edit docs/README.md, then compile"})
    }

    #[cfg(target_arch = "wasm32")]
    fn init_project(&self) -> Value {
        json!({"error": "the browser backend does not scaffold a project"})
    }

    fn update_project_settings(&self, args: &Value) -> Value {
        const KEYS: [&str; 8] = [
            "workflow.compile",
            "workflow.generate",
            "workflow.worker",
            "acp.agent",
            "gen.deliverable",
            "gen.worker",
            "llm.model",
            "llm.base_url",
        ];
        let Some(settings) = args["settings"].as_object() else {
            return json!({"error": {"rule": "missing-argument", "message": "settings is required: a map of key to value"}});
        };
        if !self.initialized() {
            return json!({"error": {"rule": "not-a-project", "message":
                "no jazyk.toml here to edit; call init_project first"}});
        }
        let path = self.project.root.join(crate::document_sets::CONFIG);
        let old_full = crate::vfs::read_to_string(&path).unwrap_or_default();
        let mut full = old_full.clone();
        for (key, value) in settings {
            if !KEYS.contains(&key.as_str()) {
                return json!({"error": {"rule": "unsupported-key", "message": format!(
                    "`{}` is not editable here; supported: {}", key, KEYS.join(", "))}});
            }
            let Some(value) = value.as_str() else {
                return json!({"error": {"rule": "bad-value", "message": format!("`{}` takes a string value", key)}});
            };
            let (section, k) = key.split_once('.').unwrap();
            full = toml_set(&full, section, k, value);
        }
        if let Err(e) = self.write_edit(crate::document_sets::CONFIG, &old_full, &full, &full) {
            return json!({"error": {"rule": "write-failed", "message": e}});
        }
        json!({"updated": settings.keys().cloned().collect::<Vec<_>>(),
               "note": "jazyk.toml edited in place; a running GUI reloads it live"})
    }
}

// Minimal line edit on a TOML text: set `key = "value"` inside `[section]`, appending
// the section or the key when missing, touching nothing else.
pub fn toml_set(text: &str, section: &str, key: &str, value: &str) -> String {
    let Ok(mut document) = text.parse::<toml_edit::DocumentMut>() else {
        return text.to_string();
    };
    if section.is_empty() {
        document[key] = toml_edit::value(value);
    } else {
        let mut item = &mut document[section.split('.').next().unwrap()];
        for part in section.split('.').skip(1) {
            item = &mut item[part];
        }
        item[key] = toml_edit::value(value);
    }
    document.to_string()
}

// One edit over the delegation socket: a JSON line out, an acknowledgement line back.
// Mirrors docs/frontends/acp.md#doc-edit-delegation.
#[cfg(not(target_arch = "wasm32"))]
fn sink_write(
    sink: &str,
    path: &std::path::Path,
    old_text: &str,
    new_text: &str,
    full: &str,
) -> Result<(), String> {
    if crate::vfs::is_scoped() {
        return Err("hosted workspaces do not connect to editor sockets".into());
    }
    use std::io::{BufRead, BufReader, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(sink).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    let line = json!({"path": path.display().to_string(), "oldText": old_text, "newText": new_text, "content": full});
    writeln!(stream, "{}", line).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(reply.trim()).map_err(|e| e.to_string())?;
    if v["ok"].as_bool().unwrap_or(false) {
        Ok(())
    } else {
        Err(v["error"]
            .as_str()
            .unwrap_or("sink refused the edit")
            .to_string())
    }
}

// A tab has no unix socket, so there is no editor to hand the edit to.
#[cfg(target_arch = "wasm32")]
fn sink_write(
    sink: &str,
    path: &std::path::Path,
    old_text: &str,
    new_text: &str,
    full: &str,
) -> Result<(), String> {
    Err("the browser backend has no editor sink".to_string())
}

// The reply pretty-printed, one field per line, so the agent's user reads the tool
// call the way the model does (docs/compiler/tools.md).
fn text_result(v: Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string());
    json!({
        "content": [{"type": "text", "text": text}],
        "isError": is_error
    })
}

fn compile_interruption(value: &Value) -> Option<String> {
    match value {
        Value::Object(object) => {
            for key in ["suspendReason","error","stopped"] {
                if let Some(reason) = object.get(key).and_then(Value::as_str)
                    .filter(|reason| crate::session_recovery::is_interruption(reason)) {
                    return Some(reason.to_owned());
                }
            }
            object.values().find_map(compile_interruption).or_else(|| {
                ["verdict","state","stop"].iter().any(|key| object.get(*key).and_then(Value::as_str) == Some("suspended"))
                    .then(|| "suspended".into())
            })
        }
        Value::Array(values) => values.iter().find_map(compile_interruption),
        _ => None,
    }
}

fn result_value(value: Value) -> Value {
    json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_restores_staged_batch_and_its_gate_state() {
        let dir = bare_dir("checkpoint-batch");
        crate::vfs::write(dir.join("docs/a.md"), "# A\n\nNavigation index.\n").unwrap();
        let project = crate::document_sets::Workspace::load(&dir).unwrap().resolve(Some("ds_main")).unwrap();
        let out = project.out.clone();
        let mut control = crate::control::Control::load(&project, &out);
        control.compile = "auto".into();
        control.save(&out);
        let make = || McpServer::with_bridge(project.clone(), out.clone(), vec!["compile".into()], true, BridgeFlags { ephemeral: true, ..Default::default() });
        let call = |server: &McpServer, name: &str, arguments: Value| {
            let result = server.handle("tools/call", &json!({"name":name,"arguments":arguments})).unwrap();
            assert_ne!(result["isError"], true, "{result}");
            serde_json::from_str::<Value>(result["content"][0]["text"].as_str().unwrap()).unwrap()
        };
        let original = make();
        call(&original, "begin_goals", json!({}));
        call(&original, "set_coverage", json!({"section":"docs/a.md#/a", "state":"non-normative", "note":"Navigation."}));
        let goals = original.open.lock().unwrap().as_ref().unwrap().goals.iter().map(|g| g.id.clone()).collect::<Vec<_>>();
        for goal in &goals {
            call(&original, "mark_goal_done", json!({"goal":goal,"justification":"The section is a navigation index and is marked non-normative."}));
        }
        let checkpoint = original.checkpoint().unwrap();
        let generation = crate::store::read_generation(&out);
        assert!(!checkpoint["open"]["session"]["staged"].as_array().unwrap().is_empty());
        drop(original);
        let restored = make();
        restored.restore(checkpoint.clone()).unwrap();
        assert_eq!(restored.checkpoint().unwrap(), checkpoint);
        let result = call(&restored, "done", json!({"summary":"Recorded the navigation coverage."}));
        assert_eq!(result["committed"], true, "{result}");
        assert!(crate::store::read_generation(&out) > generation);
        drop(restored);
        crate::vfs::remove_dir_all(dir).unwrap();
    }

    fn chat_server(dir: &std::path::Path) -> McpServer {
        let project = crate::project::Project::load(dir);
        let out = project.out.clone();
        McpServer::with_bridge(
            project,
            out,
            vec!["chat".to_string()],
            false,
            BridgeFlags::default(),
        )
    }

    // A scoped serving whose batch committed answers every later write, goal tool,
    // done, and abandon with session-complete, naming the batch; an unscoped serving
    // keeps the no-open-batch refusal. Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
    #[test]
    fn a_committed_scoped_serving_says_the_session_is_complete() {
        let dir = bare_dir("session-complete");
        let project = crate::document_sets::Workspace::load(&dir)
            .unwrap()
            .resolve(Some("ds_main"))
            .unwrap();
        let out = project.out.clone();
        let flags = BridgeFlags {
            only: Some("b7-1".into()),
            ..BridgeFlags::default()
        };
        let s = McpServer::with_bridge(project.clone(), out.clone(), vec!["compile".to_string()], true, flags);
        *s.finished.lock().unwrap() = Some("b7-1".into());
        let call = |name: &str, args: Value| {
            let r = s
                .handle("tools/call", &json!({"name": name, "arguments": args}))
                .unwrap();
            let text = r["content"][0]["text"].as_str().unwrap_or_default().to_string();
            (r["isError"].as_bool().unwrap_or(false), text)
        };
        for (name, args) in [
            ("mark_goal_done", json!({"goal": "g:reconcile-section:docs/a.md#/a", "justification": "Done."})),
            ("upsert_entity", json!({"name": "Order"})),
            ("done", json!({"summary": "again"})),
            ("abandon_goals", json!({})),
        ] {
            let (is_err, text) = call(name, args);
            assert!(is_err, "{}: {}", name, text);
            assert!(text.contains("session-complete") && text.contains("b7-1") && text.contains("end your turn"), "{}: {}", name, text);
        }
        let open = McpServer::with_bridge(project, out, vec!["compile".to_string()], true, BridgeFlags::default());
        *open.finished.lock().unwrap() = Some("b7-1".into());
        let r = open
            .handle("tools/call", &json!({"name": "done", "arguments": {"summary": "x"}}))
            .unwrap();
        assert!(r["content"][0]["text"].as_str().unwrap_or_default().contains("no-open-batch"));
    }

    fn tool_names(s: &McpServer) -> Vec<String> {
        let r = s.handle("tools/list", &json!({})).unwrap();
        r["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    #[test]
    fn runtime_serving_needs_no_graph_and_exposes_only_execution_tools() {
        let dir = bare_dir("runtime-only");
        let project = crate::project::Project::load(&dir);
        let server = McpServer::with_bridge(project, dir.join("state"), vec!["runtime".into()], false, BridgeFlags::default());
        let names = tool_names(&server);
        assert_eq!(names.len(), crate::task_runtime::TOOLS.len());
        for name in crate::task_runtime::TOOLS {
            assert!(names.iter().any(|n| n == name));
        }
        let init = server.handle("initialize", &json!({})).unwrap();
        assert_eq!(init["instructions"], crate::task_runtime::PROMPT);
        let tools = server.handle("tools/list", &json!({})).unwrap();
        assert!(tools["tools"].as_array().unwrap().iter().all(|t| t["inputSchema"]["properties"]["setId"].is_null()));
        crate::vfs::remove_dir_all(dir).ok();
    }

    // A reader conversation (a small model in a tab) gets the read tools alone and
    // the short instruction block; a write asked of it is refused.
    // Mirrors docs/frontends/wasm.md#chat.
    #[test]
    fn reader_serving_lists_only_the_read_tools() {
        let dir = bare_dir("reader");
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        let s = McpServer::with_bridge(
            project,
            out,
            vec!["graph".to_string()],
            false,
            BridgeFlags {
                ephemeral: true,
                reader: true,
                ..Default::default()
            },
        );
        let init = s
            .handle("initialize", &json!({"protocolVersion": "2024-11-05"}))
            .unwrap();
        let instructions = init["instructions"].as_str().unwrap();
        assert!(
            instructions.contains("can only read it"),
            "{}",
            instructions
        );
        assert!(
            !instructions.contains("The write tools are"),
            "{}",
            instructions
        );
        assert_eq!(
            tool_names(&s),
            crate::tools::READ_TOOLS
                .iter()
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
        );
        let r = s.handle(
            "tools/call",
            &json!({"name": "upsert_entity", "arguments": {"name": "X", "stereotype": "system"}}),
        );
        assert!(r.is_err() || r.unwrap()["isError"].as_bool().unwrap_or(false));
    }

    #[test]
    fn project_reader_keeps_read_only_instructions_and_set_discovery() {
        let dir = bare_dir("project-reader");
        let project = crate::document_sets::Workspace::load(&dir)
            .unwrap()
            .files_project();
        let s = McpServer::with_bridge(
            project.clone(),
            project.out,
            vec!["graph".into()],
            false,
            BridgeFlags { ephemeral: true, reader: true, ..Default::default() },
        );
        let init = s.handle("initialize", &json!({})).unwrap();
        let instructions = init["instructions"].as_str().unwrap();
        assert!(instructions.contains("can only read it"));
        assert!(instructions.contains("list_document_sets"));
        assert!(!instructions.contains("The write tools are"));
        let names = tool_names(&s);
        assert!(names.iter().any(|name| name == "list_document_sets"));
        assert!(names.iter().any(|name| name == "read_document"));
        assert!(names.iter().all(|name| name == "list_document_sets" || name == "read_document" || crate::tools::READ_TOOLS.contains(&name.as_str())));
        #[cfg(all(feature = "native-documents", not(target_arch = "wasm32")))]
        {
            crate::vfs::write(dir.join("docs/contract.docx"), include_bytes!("../tests/fixtures/document-proof/contract.docx")).unwrap();
            let result = s.handle("tools/call", &json!({"name":"read_document", "arguments":{"path":"docs/contract.docx"}})).unwrap();
            let value: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
            assert!(value["projection"]["text"].as_str().unwrap().contains("The gateway shall return"));
            assert!(value.get("base64").is_none());
            assert_eq!(value["diagnostics"][0]["code"], "broken-link");
            assert!(s.handle("tools/call", &json!({"name":"read_document", "arguments":{"path":"../outside.docx"}})).is_err());
        }
    }

    #[test]
    fn chat_prose_edit_works_before_compile_and_rejects_stale_or_unsafe_writes() {
        let dir=bare_dir("chat-uncompiled-prose");
        let path=dir.join("docs/hosting.md");
        crate::vfs::write(&path,"# Hosting\n\nCloudFront-S3.\n").unwrap();
        let project=crate::document_sets::Workspace::load(&dir).unwrap().files_project();
        let s=McpServer::with_bridge(project.clone(),project.out.clone(),vec!["chat".into()],false,BridgeFlags{ephemeral:true,..Default::default()});
        let call=|name:&str,args:Value|s.handle("tools/call",&json!({"name":name,"arguments":args})).unwrap();
        let read=call("read_document",json!({"path":"docs/hosting.md"}));
        let read:Value=serde_json::from_str(read["content"][0]["text"].as_str().unwrap()).unwrap();
        let changed="# Hosting\n\nUse the existing deployment task.\n";
        let written=call("write_document",json!({"path":"docs/hosting.md","text":changed,"baseHash":read["hash"]}));
        assert!(!written["isError"].as_bool().unwrap_or(false),"{written}");
        assert_eq!(crate::vfs::read_to_string(&path).unwrap(),changed);
        let stale=call("write_document",json!({"path":"docs/hosting.md","text":"must not land","baseHash":read["hash"]}));
        assert_eq!(stale["isError"],true);
        assert_eq!(crate::vfs::read_to_string(&path).unwrap(),changed);
        assert!(s.handle("tools/call",&json!({"name":"write_document","arguments":{"path":"../outside.md","text":"x"}})).is_err());
        assert!(s.handle("tools/call",&json!({"name":"write_document","arguments":{"path":"docs/a.pdf","text":"x"}})).is_err());
        assert!(Store::load(&dir.join(".jazyk/ds_main")).graph.entities.is_empty());
        assert!(crate::control::Control::load(&project,&project.out).released.compile.is_empty());
    }

    #[test]
    fn delegated_document_write_never_treats_unreadable_existing_bytes_as_missing() {
        let dir=bare_dir("delegated-unreadable-prose");
        crate::vfs::write(dir.join("docs/invalid.md"),[0xff,0xfe]).unwrap();
        let project=crate::document_sets::Workspace::load(&dir).unwrap().files_project();
        let s=McpServer::with_bridge(project.clone(),project.out,vec!["chat".into()],false,BridgeFlags{ephemeral:true,edit_sink:Some("not-a-live-editor-sink".into()),..Default::default()});
        let error=s.handle("tools/call",&json!({"name":"write_document","arguments":{"path":"docs/invalid.md","text":"must not replace bytes"}})).unwrap_err();
        assert!(error.1.contains("cannot read existing document"),"{error:?}");
        assert_eq!(crate::vfs::read(dir.join("docs/invalid.md")).unwrap(),vec![0xff,0xfe]);
    }

    #[test]
    fn conversational_contract_does_not_require_discovery_or_execution() {
        let instructions=instructions_in(&["chat".into()],false,true,false);
        assert!(instructions.contains("self-contained general question directly"));
        assert!(instructions.contains("does not authorize compiling"));
        assert!(instructions.contains("write_document"));
        assert!(!instructions.contains("Read task_context and choose"));
        assert!(!instructions.contains("The write tools are:"));
    }

    fn bare_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jazyk-mcp-{}-{}", tag, crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        dir
    }

    // The agent-run benchmark runs a derived-goal case end to end: the fixture seeds
    // and derives, the session opens holding its prompt's loaded set, and finish_case
    // commits the staged work before grading. A serving that committed only extraction
    // and review cases scored an agent zero on three correct criteria.
    // Mirrors docs/benchmark/benchmark.md#agent-run-benchmarks.
    #[test]
    fn benchmark_serving_commits_a_derived_goal_case() {
        let dir = bare_dir("bench-derived");
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        let s = McpServer::with_bridge(
            project,
            out,
            vec!["benchmark".to_string()],
            false,
            BridgeFlags::default(),
        );
        let call = |name: &str, args: Value| -> Value {
            let r = s
                .handle("tools/call", &json!({"name": name, "arguments": args}))
                .unwrap();
            let text = r["content"][0]["text"].as_str().unwrap_or_default();
            serde_json::from_str(text).unwrap_or(Value::String(text.to_string()))
        };
        let begun = call("begin_case", json!({"case": "criteria-density"}));
        assert_eq!(begun["case"]["name"], "criteria-density", "{}", begun);
        assert!(
            begun["instructions"]
                .as_str()
                .unwrap_or_default()
                .contains("derive-criteria"),
            "the prompt names the goal: {}",
            begun["instructions"]
        );
        assert!(
            begun["package"]
                .as_str()
                .unwrap_or_default()
                .contains("req:company-1"),
            "the session opens holding its prompt's loaded set: {}",
            begun["package"]
        );
        let staged = call(
            "upsert_criterion",
            json!({
                "requirement": "req:company-1",
                "kind": "positive",
                "statement": "An observer counting the company's departments finds exactly 4 departments.",
                "provenance": {"derived": {"from": ["req:company-1"], "reasoning": "covers the stated count"}},
            }),
        );
        assert!(staged["id"].is_string(), "{}", staged);
        let second = call(
            "upsert_criterion",
            json!({
                "requirement": "req:company-2",
                "kind": "boundary",
                "statement": "A claim for `500` dollars needs no approval from the head of department.",
                "provenance": {"derived": {"from": ["req:company-2"], "reasoning": "the value at the edge"}},
            }),
        );
        assert!(second["id"].is_string(), "{}", second);
        let third = call(
            "upsert_criterion",
            json!({
                "requirement": "req:company-2",
                "kind": "boundary",
                "statement": "A claim for `501` dollars needs the head of department's approval.",
                "provenance": {"derived": {"from": ["req:company-2"], "reasoning": "the value past the edge"}},
            }),
        );
        assert!(third["id"].is_string(), "{}", third);
        let marked = call(
            "mark_goal_done",
            json!({
                "goal": "g:derive-criteria:ent:company",
                "justification": "Each requirement carries the criteria its shape needs.",
                "evidence": {"requirements": [
                    {"id": "req:company-1", "criteria": [staged["id"]], "verdict": "complete"},
                    {"id": "req:company-2", "criteria": [second["id"], third["id"]], "verdict": "complete"},
                ]},
            }),
        );
        assert!(marked["marked"].is_string(), "{}", marked);
        let scored = call("finish_case", json!({"summary": "criteria derived"}));
        assert_eq!(scored["scored"]["score"], json!(1.0), "{}", scored);
        // One line per case, so a long list stays readable to an agent whose client
        // truncates replies (Codex's guard refused a case it could not see listed).
        let list = call("benchmark_cases", json!({}));
        let rows = list["cases"].as_array().unwrap();
        assert!(rows.iter().all(|r| r.is_string()), "{}", list);
        assert!(
            rows.iter()
                .any(|r| r == "criteria-density (review, derive-criteria): scored 1.0"),
            "{}",
            list
        );
        // Feedback answers with no graph in the serving's out directory.
        let fed = call(
            "report_feedback",
            json!({"kind": "wrong", "subject": "begin_case", "message": "a test note"}),
        );
        assert!(fed.get("error").is_none(), "{}", fed);
    }

    // A verify serving for an agent with no editor of its own lists the read-only
    // file tools and never the write. Mirrors docs/compiler/goals/verify.md#tools.
    #[test]
    fn verify_serving_lists_read_only_file_tools_when_serving_files() {
        let dir = bare_dir("verify-files");
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        let serve = |serve_files: bool| {
            McpServer::with_bridge(
                project.clone(),
                out.clone(),
                vec!["verify".to_string()],
                false,
                BridgeFlags {
                    serve_files,
                    ..Default::default()
                },
            )
        };
        let with = tool_names(&serve(true));
        for t in ["read_text_file", "list_files", "run_task", "run_tests", "cancel_run", "read_run_log"] {
            assert!(with.iter().any(|n| n == t), "{} missing from {:?}", t, with);
        }
        assert!(!with.iter().any(|n| matches!(n.as_str(), "write_text_file" | "run_command")), "{:?}", with);
        let without = tool_names(&serve(false));
        assert!(
            !without.iter().any(|n| n == "read_text_file"),
            "{:?}",
            without
        );
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // A ledger session's serving holds its batch by id, and its file tools write for
    // that batch's component group: a member's file is extended, another group's is
    // refused. Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    #[test]
    fn a_ledger_serving_writes_for_its_batch_group() {
        use crate::model::Entity;
        let dir = bare_dir("ledger-group-write");
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\ngen = {}\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = [\"handbook/**\"]\n",
        )
        .unwrap();
        crate::vfs::create_dir_all(dir.join("handbook")).unwrap();
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        let mut s = Store::load(&out);
        let ent = |name: &str, parent: Option<&str>| Entity {
            name: name.into(),
            parent: parent.map(String::from),
            ..Default::default()
        };
        s.graph
            .entities
            .insert("ent:area".into(), ent("Area", None));
        s.graph
            .entities
            .insert("ent:claim".into(), ent("Claim", Some("ent:area")));
        s.graph
            .entities
            .insert("ent:receipt".into(), ent("Receipt", Some("ent:claim")));
        s.graph
            .entities
            .insert("ent:tool".into(), ent("Tool", Some("ent:area")));
        s.save();
        let mut ledger = crate::gen::Ledger::load(&out);
        ledger.entities.insert(
            "receipt".into(),
            crate::gen::EntityGen {
                fact_hash: String::new(),
                requirements: Vec::new(),
                files: vec!["handbook/claim.md".into()],
                unattached: None,
                recorded_at: None,
            },
        );
        ledger.save(&out);
        let serving = |only: &str| {
            McpServer::with_bridge(
                project.clone(),
                out.clone(),
                vec!["generate".to_string()],
                true,
                BridgeFlags {
                    serve_files: true,
                    only: Some(only.into()),
                    ..Default::default()
                },
            )
        };
        let write = |srv: &McpServer| -> Value {
            srv.handle(
                "tools/call",
                &json!({"name": "write_text_file", "arguments": {"path": "handbook/claim.md", "content": "# Claim\n"}}),
            )
            .unwrap()
        };
        let r = write(&serving("g:generate:ent:claim"));
        assert!(!r["isError"].as_bool().unwrap_or(false), "{}", r);
        assert!(dir.join("handbook/claim.md").vis_file());
        let r = write(&serving("g:generate:ent:tool"));
        assert!(r["isError"].as_bool().unwrap_or(false), "{}", r);
        assert!(r.to_string().contains("file-owned"), "{}", r);

        // A general MCP serving has no pinned goal. Its begin establishes the
        // file owner until a successful record, including a later explicit repair.
        let server = McpServer::with_bridge(
            project.clone(), out.clone(), vec!["generate".into()], false,
            BridgeFlags { serve_files: true, ..Default::default() },
        );
        let call = |name: &str, args: Value| server.handle("tools/call", &json!({"name":name,"arguments":args})).unwrap();
        let begun = call("begin_generation", json!({"entity":"ent:receipt"}));
        assert!(!begun["isError"].as_bool().unwrap_or(false), "{begun}");
        let package: Value = serde_json::from_str(begun["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(!write(&server)["isError"].as_bool().unwrap_or(false));
        let bad = call("record_generation", json!({"entity":"ent:receipt","factHash":package["factHash"]}));
        assert_eq!(bad["isError"], true);
        assert!(crate::control::leases(&out).contains_key("ent:receipt"));
        assert!(!write(&server)["isError"].as_bool().unwrap_or(false));
        let args = json!({"entity":"ent:receipt","factHash":package["factHash"],"manifest":{"files":["handbook/claim.md"],"tests":[]}});
        let recorded = call("record_generation", args.clone());
        assert!(!recorded["isError"].as_bool().unwrap_or(false), "{recorded}");
        assert!(!crate::control::leases(&out).contains_key("ent:receipt"));
        assert!(write(&server).to_string().contains("begin_generation"));
        let begun = call("begin_generation", json!({"entity":"ent:receipt"}));
        assert!(!begun["isError"].as_bool().unwrap_or(false), "{begun}");
        assert!(!write(&server)["isError"].as_bool().unwrap_or(false));
        crate::control::release_lease(&out, "ent:receipt");
        crate::control::claim(&out, "ent:receipt", "another-worker").unwrap();
        assert!(write(&server).to_string().contains("claimed"));
        assert!(call("record_generation", args).to_string().contains("claimed"));
        assert_eq!(crate::control::leases(&out)["ent:receipt"].worker, "another-worker");
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // Every serving lists each tool name once: the catalog's `done` and the
    // lifecycle's `done` are one entry, and a union of modes never repeats a shared
    // tool. Claude Code forwards the list as is and its API refuses a repeated name.
    #[test]
    fn tool_listings_never_repeat_a_name() {
        let dir = bare_dir("unique-names");
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        for modes in [
            vec!["compile"],
            vec!["chat"],
            vec!["compile", "chat"],
            vec!["generate", "verify"],
            vec!["benchmark"],
            vec!["graph"],
        ] {
            let s = McpServer::with_bridge(
                project.clone(),
                out.clone(),
                modes.iter().map(|m| m.to_string()).collect(),
                false,
                BridgeFlags {
                    serve_files: true,
                    only: Some("b1-1".into()),
                    ..Default::default()
                },
            );
            let names = tool_names(&s);
            let unique: std::collections::BTreeSet<&String> = names.iter().collect();
            assert_eq!(unique.len(), names.len(), "{:?}: {:?}", modes, names);
            if modes.contains(&"compile") {
                assert!(names.iter().any(|n| n == "done"), "{:?}", names);
            }
        }
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // The chat serving carries the compilation lifecycle beside the ledger
    // lifecycles. Mirrors docs/compiler/tools.md#toolsets.
    #[test]
    fn chat_serving_lists_the_compilation_lifecycle() {
        let dir = bare_dir("chat-lifecycle");
        let names = tool_names(&chat_server(&dir));
        for t in [
            "goals",
            "begin_goals",
            "done",
            "abandon_goals",
            "begin_binding",
            "begin_generation",
            "begin_verification",
            "release_work",
            "read_text_file",
            "write_text_file",
            "list_files",
        ] {
            assert!(
                names.iter().any(|n| n == t),
                "{} missing from {:?}",
                t,
                names
            );
        }
        assert!(names.iter().any(|n| n == "upsert_entity"), "{:?}", names);
        assert!(!names.iter().any(|n| n == "run_command"), "{:?}", names);
        crate::vfs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn chat_generation_configures_source_before_deriving_criteria() {
        let dir = bare_dir("chat-source-setup");
        let project = crate::project::Project::load(&dir);
        let mut store = Store::load(&project.out);
        store.graph.entities.insert("ent:deck".into(), crate::model::Entity { name:"Deck".into(), ..Default::default() });
        store.graph.requirements.insert("req:deck".into(), crate::model::Requirement { statement:"The deck contains three slides.".into(), entities:vec!["ent:deck".into()], ..Default::default() });
        store.save();
        let workspace = crate::document_sets::Workspace::load(&dir).unwrap().files_project();
        let server = McpServer::with_bridge(workspace.clone(), workspace.out, vec!["chat".into()], false, BridgeFlags::default());
        let call = |name, args| server.handle("tools/call", &json!({"name":name,"arguments":args})).unwrap();
        let decode = |reply: Value| serde_json::from_str::<Value>(reply["content"][0]["text"].as_str().unwrap()).unwrap();
        for name in ["binding_tasks", "generation_tasks"] {
            let reply = decode(call(name, json!({})));
            assert_eq!(reply["prerequisites"]["sourceMapping"], false, "{reply}");
            assert!(reply["next"].as_str().unwrap().contains("update_document_set"), "{reply}");
        }
        let board = crate::board::Board::compute(&project, &project.out);
        assert!(!board.goals.iter().any(|g| g.kind == "derive-criteria"));
        let bind = board.goals.iter().find(|g| g.kind == "bind").unwrap();
        assert_eq!(bind.change["sourceConfigured"], false);
        let before = decode(call("list_document_sets", json!({})));
        let updated = call("update_document_set", json!({"baseHash":before["hash"],"set":{"id":"ds_main","source":["src/**","tests/**","build.py"]}}));
        assert_ne!(updated["isError"], true, "{updated}");
        let after = decode(call("list_document_sets", json!({})));
        for key in ["id", "name", "docs", "roots"] {
            assert_eq!(before["sets"][0][key], after["sets"][0][key], "{key}");
        }
        assert_eq!(after["sets"][0]["source"], json!(["src/**","tests/**","build.py"]));
        let reply = decode(call("binding_tasks", json!({})));
        assert!(reply["prerequisites"]["sourceMapping"].is_null(), "{reply}");
        let project = crate::project::Project::load(&dir);
        let board = crate::board::Board::compute(&project, &project.out);
        assert!(board.goals.iter().any(|g| g.kind == "derive-criteria" && g.target == "ent:deck"));
        assert!(!dir.join("src").vexists(), "Patterns may select future files");
        crate::vfs::remove_dir_all(dir).ok();
    }

    #[test]
    fn chat_release_unblocks_binding_and_empty_generation_names_prerequisites() {
        let dir = bare_dir("chat-release");
        crate::vfs::write(dir.join("docs/main.md"), "# Greeter\n\nThe greeter says hello.\n").unwrap();
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        let mut store = Store::load(&out);
        let (parsed, _) = crate::reconcile::parse_all(&project);
        store.sync_docs(&parsed);
        store.graph.entities.insert("ent:greeter".into(), crate::model::Entity { name:"Greeter".into(), ..Default::default() });
        store.graph.requirements.insert("req:greet".into(), crate::model::Requirement { statement:"The greeter says hello.".into(), entities:vec!["ent:greeter".into()], ..Default::default() });
        store.save();
        let server = chat_server(&dir);
        let call = |name, args| server.handle("tools/call", &json!({"name":name,"arguments":args})).unwrap();
        let decode = |reply: Value| serde_json::from_str::<Value>(reply["content"][0]["text"].as_str().unwrap()).unwrap();
        let generation = decode(call("generation_tasks", json!({})));
        assert_eq!(generation["count"], 0, "{generation}");
        assert_eq!(generation["prerequisites"]["binding"], 1, "{generation}");
        assert!(generation["next"].as_str().unwrap().contains("binding_tasks"));
        assert_eq!(call("begin_binding", json!({"requirement":"req:greet"}))["isError"], true);
        let config = crate::vfs::read_to_string(dir.join(".jazyk/jazyk.toml")).unwrap();
        assert_eq!(call("release_work", json!({"stage":"publish"}))["isError"], true);
        crate::control::claim(&out, crate::control::BUILD_LEASE, "another-worker").unwrap();
        assert!(call("release_work", json!({"stage":"generate"})).to_string().contains("work-running"));
        crate::control::release_lease(&out, crate::control::BUILD_LEASE);
        assert_ne!(call("release_work", json!({"stage":"generate"}))["isError"], true);
        assert_eq!(crate::vfs::read_to_string(dir.join(".jazyk/jazyk.toml")).unwrap(), config);
        let binding = call("begin_binding", json!({"requirement":"req:greet"}));
        assert_ne!(binding["isError"], true, "{binding}");
        let reader = McpServer::with_bridge(project, out, vec!["graph".into()], false, BridgeFlags { reader:true, ..Default::default() });
        assert!(!tool_names(&reader).iter().any(|n| n == "release_work" || n == "write_text_file"));
        let denied = reader.handle("tools/call", &json!({"name":"release_work","arguments":{"stage":"generate"}}));
        assert!(denied.is_err() || denied.unwrap()["isError"] == true);
        crate::vfs::remove_dir_all(dir).ok();
    }

    // The chat serving carries the two decree tools beside edit_fact: bump_limit
    // records a node's own threshold with decree provenance as its own changeset
    // and refuses a limit that does not apply; retract_decree deletes a node created
    // by decree and refuses a quoted one. Mirrors docs/compiler/tools.md#chat-tools.
    #[test]
    fn chat_serving_bumps_limits_and_retracts_decrees() {
        use crate::model::{Entity, Provenance, SourceRef};
        use crate::store::{Commit, Op};
        let dir = bare_dir("chat-decree");
        crate::vfs::write(dir.join("docs/shop.md"), "# Shop\n\nAn Order is placed.\n").unwrap();
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let mut s = Store::load(&out);
        s.sync_docs(&parsed);
        s.apply(
            vec![
                Op::CreateEntity {
                    id: "ent:order".into(),
                    entity: Entity {
                        name: "Order".into(),
                        mentions: vec![SourceRef {
                            doc: "docs/shop.md".into(),
                            section: "/shop".into(),
                            quote: "An Order is placed.".into(),
                        }],
                        ..Default::default()
                    },
                },
                Op::CreateEntity {
                    id: "ent:fulfilment".into(),
                    entity: Entity {
                        name: "Fulfilment".into(),
                        provenance: Some(Provenance::Decree {
                            author: "owner".into(),
                            at: "t".into(),
                            note: None,
                        }),
                        ..Default::default()
                    },
                },
            ],
            &Commit::store("decree"),
        );
        let before = s.status.generation;
        drop(s);

        let server = chat_server(&dir);
        let names = tool_names(&server);
        for t in ["bump_limit", "retract_decree"] {
            assert!(names.iter().any(|n| n == t), "{} missing from {:?}", t, names);
        }

        // A bump lands as its own changeset with the raised threshold on the node.
        let v = server.bump_limit(
            &json!({"id": "ent:order", "limit": "requirements-per-entity", "value": 90}),
        );
        assert_eq!(v["path"], "bump", "{}", v);
        let s = Store::load(&out);
        assert_eq!(s.status.generation, before + 1, "one changeset");
        assert_eq!(
            s.graph.entities["ent:order"].limits["requirements-per-entity"].value,
            90
        );
        drop(s);
        // A view limit on an entity is refused with the store's rule, nothing lands.
        let v = server.bump_limit(
            &json!({"id": "ent:order", "limit": "members-per-structural-view", "value": 40}),
        );
        assert_eq!(v["error"]["rule"], "bad-limit", "{}", v);
        assert_eq!(Store::load(&out).status.generation, before + 1);
        // A zero or missing value never reaches the store.
        let v = server.bump_limit(&json!({"id": "ent:order", "limit": "requirements-per-entity", "value": 0}));
        assert_eq!(v["error"]["rule"], "bad-value", "{}", v);

        // Retracting the decreed entity deletes it; the quoted one is not a decree.
        let v = server.retract_decree(&json!({"id": "ent:fulfilment", "reason": "never stated"}));
        assert_eq!(v["retracted"], true, "{}", v);
        let s = Store::load(&out);
        assert!(!s.graph.entities.contains_key("ent:fulfilment"));
        assert_eq!(s.status.generation, before + 2);
        drop(s);
        let v = server.retract_decree(&json!({"id": "ent:order", "reason": "no"}));
        assert_eq!(v["error"]["rule"], "not-a-decree", "{}", v);
        let s = Store::load(&out);
        assert!(s.graph.entities.contains_key("ent:order"));
        assert_eq!(s.status.generation, before + 2, "a refusal writes nothing");
        drop(s);
        let v = server.retract_decree(&json!({"id": "ent:nobody", "reason": "no"}));
        assert_eq!(v["error"]["rule"], "unknown-id", "{}", v);
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // A chat dual write moves the prose and the graph in one changeset and absorbs
    // its own hashes, so a following sync finds nothing dirty; edit_fact without an
    // accepted sentence lands a decree with its proposal instead.
    // Mirrors docs/frontends/acp.md#dual-write-tools.
    #[test]
    fn revise_requirement_commits_prose_and_graph_without_redirtying() {
        use crate::model::{Entity, Provenance, Requirement, SourceRef};
        use crate::store::{Commit, Op};
        let dir =
            std::env::temp_dir().join(format!("jazyk-mcp-revise-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        crate::vfs::write(
            dir.join("docs/pay.md"),
            "# Pay\n\nAn Order is paid within 30 days.\n",
        )
        .unwrap();
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let mut s = Store::load(&out);
        s.sync_docs(&parsed);
        let quote = "An Order is paid within 30 days.";
        s.apply(
            vec![
                Op::CreateEntity {
                    id: "ent:order".into(),
                    entity: Entity {
                        name: "Order".into(),
                        ..Default::default()
                    },
                },
                Op::CreateRequirement {
                    id: "req:pay-1".into(),
                    requirement: Requirement {
                        statement: quote.into(),
                        entities: vec!["ent:order".into()],
                        source: Some(SourceRef {
                            doc: "docs/pay.md".into(),
                            section: "/pay".into(),
                            quote: quote.into(),
                        }),
                        ..Default::default()
                    },
                },
            ],
            &Commit::store("session"),
        );
        let before = s.status.generation;
        drop(s);

        let server = chat_server(&dir);
        assert!(tool_names(&server).iter().any(|t| t == "edit_fact"));
        let new_text = "An Order is paid within 21 days.";
        let v = server.revise_requirement(
            &json!({"id": "req:pay-1", "new_text": new_text, "statement": new_text}),
        );
        assert_eq!(v["committed"], true, "{}", v);
        let text = crate::vfs::read_to_string(dir.join("docs/pay.md")).unwrap();
        assert!(text.contains(new_text), "{}", text);
        let mut s = Store::load(&out);
        assert_eq!(s.status.generation, before + 1, "one changeset");
        let r = &s.graph.requirements["req:pay-1"];
        assert_eq!(r.source.as_ref().unwrap().quote, new_text);
        assert_eq!(r.statement, new_text);
        let (parsed, _) = crate::reconcile::parse_all(&project);
        assert_eq!(s.docs["docs/pay.md"].content_hash, parsed["docs/pay.md"].0);
        let records = s.status.changes.clone();
        assert!(s.sync_docs(&parsed).is_empty(), "no re-dirtying");
        assert_eq!(s.status.changes, records);
        drop(s);

        // A decree through edit_fact lands graph-only with its proposal.
        let v = server.edit_fact(
            &json!({"id": "req:pay-1", "field": "facets", "value": [{"facet": "constraint", "reasoning": "an invariant"}]}),
        );
        assert_eq!(v["path"], "decree", "{}", v);
        let s = Store::load(&out);
        let r = &s.graph.requirements["req:pay-1"];
        assert!(r.source.is_none() && matches!(r.provenance, Some(Provenance::Decree { .. })));
        assert_eq!(r.facets.len(), 1);
        assert!(s
            .graph
            .diagnostics
            .values()
            .any(|d| d.rule == "ratification-pending"
                && d.lifecycle == "open"
                && d.subjects == vec!["req:pay-1".to_string()]
                && d.prompt.is_some()));
        assert!(s
            .status
            .has_change(crate::store::CHANGE_PROVENANCE_PENDING, "req:pay-1"));
        drop(s);
        // The prose form is refused now that the fact is decreed.
        let v = server.revise_requirement(&json!({"id": "req:pay-1", "new_text": "x"}));
        assert_eq!(v["error"]["rule"], "not-quoted");
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // The compile serving claims goal batches: begin_goals claims the next ready
    // batch under a per-batch lease naming its goals, done without a mark per
    // mandatory goal refuses with open-goal, and abandon_goals releases the claim.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    #[test]
    fn begin_goals_claims_the_next_batch_and_done_wants_marks() {
        let dir =
            std::env::temp_dir().join(format!("jazyk-mcp-goals-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        crate::vfs::write(dir.join("docs/a.md"), "# A\n\nThe body.\n").unwrap();
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        crate::vfs::create_dir_all(&out).unwrap();
        let control = crate::control::Control {
            compile: "auto".into(),
            ..Default::default()
        };
        control.save(&out);
        let server = McpServer::new(project, out.clone(), vec!["compile".into()], false);

        let v = server.begin_goals(&json!({"arguments": {}}));
        assert!(v["error"].is_null(), "{}", v);
        // The batch id is b<generation>-<n>: the sync that absorbed the fresh
        // document bumped the generation before the board derived.
        let batch = v["batch"].as_str().unwrap().to_string();
        assert!(batch.starts_with('b') && batch.contains('-'), "{}", batch);
        let instructions = v["instructions"].as_str().unwrap();
        assert!(instructions.contains("## Goals"), "{}", instructions);
        assert!(
            instructions.contains(&batch),
            "the protocol line names the batch"
        );
        assert!(v["package"].as_str().unwrap().contains("## Loaded"));
        // The claim is a per-batch lease naming its goals.
        let leases = crate::control::leases(&out);
        let lease = leases.get(&batch).expect("a batch lease");
        assert!(
            !lease.goals.is_empty() && lease.goals.iter().all(|g| g.starts_with("g:")),
            "{:?}",
            lease.goals
        );
        // A second begin refuses while the batch is open.
        let again = server.begin_goals(&json!({"arguments": {}}));
        assert_eq!(again["error"]["rule"], "batch-open");
        // done without a mark per mandatory goal refuses with open-goal and keeps
        // the changeset open.
        let d = server.done_batch(&json!({"arguments": {"summary": "did nothing"}}));
        assert_eq!(d["error"]["rule"], "open-goal", "{}", d);
        let a = server.abandon_goals(&json!({"arguments": {"reason": "test"}}));
        assert_eq!(a["abandoned"], json!(batch));
        assert!(crate::control::leases(&out).is_empty());
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // report_spend is served with every toolset, the read-only graph serving included,
    // and outside an open batch it is refused toward `jazyk spend`.
    // Mirrors docs/compiler/tools.md#spend-tool.
    #[test]
    fn report_spend_is_served_everywhere_and_refused_outside_a_batch() {
        let dir = bare_dir("spend-tool");
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        let graph = McpServer::new(
            crate::project::Project::load(&dir),
            out.clone(),
            vec!["graph".into()],
            false,
        );
        assert!(tool_names(&graph).iter().any(|n| n == "report_spend"));
        let r = graph
            .handle(
                "tools/call",
                &json!({"name": "report_spend", "arguments": {"tokens": 500}}),
            )
            .unwrap();
        assert_eq!(r["isError"], true);
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("no-open-batch") && text.contains("jazyk spend"),
            "{}",
            text
        );
        let compile = McpServer::new(project, out, vec!["compile".into()], false);
        assert!(tool_names(&compile).iter().any(|n| n == "report_spend"));
        let r = compile
            .handle(
                "tools/call",
                &json!({"name": "report_spend", "arguments": {"tokens": 500}}),
            )
            .unwrap();
        assert_eq!(r["isError"], true);
        assert!(r["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("no-open-batch"));
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // The compile serving lists the goal lifecycle, the view tools, and the goal
    // tools; a raw write tool outside an open batch is rejected toward begin_goals.
    // Mirrors docs/compiler/tools.md#toolsets.
    #[test]
    fn compile_serving_lists_goal_and_view_tools_and_gates_writes() {
        let dir =
            std::env::temp_dir().join(format!("jazyk-mcp-toolset-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        let server = McpServer::new(project, out, vec!["compile".into()], false);
        let names = tool_names(&server);
        for t in [
            "goals",
            "begin_goals",
            "done",
            "abandon_goals",
            "upsert_view",
            "update_view",
            "delete_view",
            "mark_goal_done",
            "mark_goal_failed",
            "settle_pairs",
            "load_skill",
            "load",
            "unload",
            "graph_status",
        ] {
            assert!(names.iter().any(|n| n == t), "{} missing: {:?}", t, names);
        }
        for legacy in [
            "compilation_tasks",
            "begin_compilation",
            "finish_compilation",
            "abandon_compilation",
        ] {
            assert!(
                !names.iter().any(|n| n == legacy),
                "{} should be gone",
                legacy
            );
        }
        // A raw write outside an open batch is rejected toward begin_goals.
        let r = server
            .handle(
                "tools/call",
                &json!({"name": "upsert_entity", "arguments": {"name": "Thing",
                    "mention": {"section": "docs/a.md#/a", "quote": "x"}}}),
            )
            .unwrap();
        assert_eq!(r["isError"], true);
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("no-open-batch") && text.contains("begin_goals"),
            "{}",
            text
        );
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // Chat can complete the batches it claims, without enabling direct graph surgery.
    #[test]
    fn chat_compiler_tools_stage_only_inside_the_claimed_batch() {
        let dir = bare_dir("chat-compiler-tools");
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::create_dir_all(dir.join(".jazyk")).unwrap();
        crate::vfs::write(dir.join(".jazyk/jazyk.toml"),
            "version=1\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['docs/**/*.md']\n").unwrap();
        crate::vfs::write(dir.join("docs/a.md"), "# A\n\nThe Greeter returns hello.\n").unwrap();
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        crate::vfs::create_dir_all(&out).unwrap();
        crate::control::Control { compile: "auto".into(), ..Default::default() }.save(&out);
        let server = McpServer::new(project, out.clone(), vec!["chat".into()], false);
        let names = tool_names(&server);
        for name in toolset("mcp-compile") {
            assert!(names.iter().any(|n| n == name), "missing {name}");
        }
        let call = |name: &str, args: Value| {
            server.handle_local("tools/call", &json!({"name":name,"arguments":args})).unwrap()
        };
        let args = json!({"name":"Greeter", "mention":{
            "section":"docs/a.md#/a", "quote":"The Greeter returns hello."}});
        let rejected = call("upsert_entity", args.clone());
        assert_eq!(rejected["isError"], true);
        assert!(rejected["content"][0]["text"].as_str().unwrap().contains("no-open-batch"));
        assert!(Store::load(&out).graph.entities.is_empty());
        let batch = server.begin_goals(&json!({"arguments":{}}));
        assert!(batch["error"].is_null(), "{batch}");
        let staged = call("upsert_entity", args);
        assert_ne!(staged["isError"], true, "{staged}");
        assert!(!server.open.lock().unwrap().as_ref().unwrap().session.staged.is_empty());
        assert!(Store::load(&out).graph.entities.is_empty(), "writes must remain staged");
        let wrong = call("report_diagnostic", json!({"rule":"contradiction", "message":"test"}));
        assert_eq!(wrong["isError"], true);
        assert!(wrong["content"][0]["text"].as_str().unwrap().contains("wrong-toolset"), "{wrong}");
        server.abandon_goals(&json!({"arguments":{"reason":"test cleanup"}}));
        assert!(Store::load(&out).graph.entities.is_empty());
        assert!(crate::control::leases(&out).is_empty());
        crate::vfs::remove_dir_all(dir).unwrap();
    }

    // A project tool is offered only where it can do something: scaffolding in a bare
    // directory, settings in a project. The instructions say which case it is, so the
    // agent never spends a call to learn it.
    // Mirrors docs/frontends/acp.md#project-tools.
    #[test]
    fn project_tools_follow_the_project_state() {
        let base =
            std::env::temp_dir().join(format!("jazyk-mcp-init-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&base).ok();
        let bare = base.join("bare");
        let proj = base.join("proj");
        crate::vfs::create_dir_all(&bare).unwrap();
        crate::vfs::create_dir_all(&proj).unwrap();
        crate::vfs::write(
            { let path = proj.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();

        let bare_tools = tool_names(&chat_server(&bare));
        assert!(
            bare_tools.iter().any(|t| t == "init_project"),
            "{:?}",
            bare_tools
        );
        assert!(
            !bare_tools.iter().any(|t| t == "update_project_settings"),
            "{:?}",
            bare_tools
        );

        let proj_tools = tool_names(&chat_server(&proj));
        assert!(
            !proj_tools.iter().any(|t| t == "init_project"),
            "{:?}",
            proj_tools
        );
        assert!(
            proj_tools.iter().any(|t| t == "update_project_settings"),
            "{:?}",
            proj_tools
        );

        assert!(instructions_for(&["chat".to_string()], false, false).contains("NO PROJECT HERE"));
        // A tab's chat and reader say builds run from the Build button, since neither
        // can start one (docs/frontends/wasm.md#chat); a native chat has its own path.
        let note = "You cannot start a build: the Build button at the top of the page builds \
                    the documents into the graph. When asked to build, say so.";
        assert!(instructions_in(&["chat".to_string()], false, true, true)
            .contains("compile_document_sets"));
        assert!(reader_instructions().contains(note));
        assert!(!instructions_in(&["chat".to_string()], false, true, false).contains(note));
        assert!(
            instructions_for(&["chat".to_string()], false, true).contains("already initialized")
        );

        // The refusal survives as a floor under the listing: settings without a file.
        let r =
            chat_server(&bare).update_project_settings(&json!({"settings": {"llm.model": "x"}}));
        assert_eq!(r["error"]["rule"], "not-a-project");
        crate::vfs::remove_dir_all(&base).ok();
    }
    #[test]
    fn stop_fence_discards_implicit_finish_and_rejects_late_done() {
        let dir = bare_dir("stop-fence");
        crate::vfs::write(dir.join("docs/a.md"), "# A\n\nNavigation index.\n").unwrap();
        let project = crate::document_sets::Workspace::load(&dir)
            .unwrap()
            .resolve(Some("ds_main"))
            .unwrap();
        let out = project.out.clone();
        let mut control = crate::control::Control::load(&project, &out);
        control.compile = "auto".into();
        control.save(&out);
        let fence = out.join("cancel-test");
        let server = McpServer::with_bridge(
            project,
            out.clone(),
            vec!["compile".into()],
            true,
            BridgeFlags {
                ephemeral: true,
                cancel_file: Some(fence.clone()),
                ..Default::default()
            },
        );
        let call = |name: &str, arguments: Value| {
            let v = server
                .handle("tools/call", &json!({"name":name, "arguments":arguments}))
                .unwrap();
            assert_ne!(v["isError"], true, "{v}");
            serde_json::from_str::<Value>(v["content"][0]["text"].as_str().unwrap()).unwrap()
        };
        call("begin_goals", json!({}));
        call(
            "set_coverage",
            json!({"section":"docs/a.md#/a", "state":"non-normative", "note":"Navigation."}),
        );
        let generation = crate::store::read_generation(&out);
        assert!(!server
            .open
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .session
            .staged
            .is_empty());
        crate::vfs::write(&fence, "stopped").unwrap();
        assert!(server
            .handle("tools/call", &json!({"name":"done", "arguments":{}}))
            .is_err());
        server.finish();
        assert!(server.open.lock().unwrap().is_none());
        assert_eq!(
            crate::store::read_generation(&out),
            generation,
            "EOF committed cancelled work"
        );
        crate::vfs::remove_dir_all(dir).unwrap();
    }

}
