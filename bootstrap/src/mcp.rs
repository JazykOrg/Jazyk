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
}

#[derive(Default)]
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
    // Serve the file and command tools, for agents with no editor of their own.
    pub serve_files: bool,
    // Delegate document and settings writes to the spawning process (the IDE proxy).
    pub edit_sink: Option<String>,
    // The bridge already sent the batch's instructions and package as the session
    // prompt; begin_goals answers with a short ack instead of repeating them.
    pub packaged: bool,
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
    // The sandbox master: staged work applies here at finish, checks read it.
    store: Store,
    session: ToolSession,
    calls: u32,
    tmp: std::path::PathBuf,
    gs: crate::gen::GenSettings,
}

struct OpenBatch {
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
             deliverable files and any build with YOUR OWN file and shell tools (this server serves \
             none), making the bound tests pass, then record_generation with the manifest, then \
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
                "CHAT SERVING: you are in a conversation about this project. Read the graph with the \
                 read tools. A requirement lives in the prose: change one with revise_requirement (new \
                 prose, optional new statement), add one with add_requirement, remove one with \
                 retract_requirement; each moves the document and the graph in one atomic commit. \
                 update_project_settings edits jazyk.toml keys. The project is already initialized: \
                 there is no init_project tool and nothing to scaffold. The \
                 compilation, binding, generation, and verification lifecycles are available for \
                 explicit requests. ",
            );
        }
    }
    if modes.iter().any(|m| m == "graph") && write {
        s.push_str("Write tools are enabled for manual graph surgery; each call commits as its own changeset. ");
    }
    s.push_str(
        "The write tools are: upsert_entity, update_entity, delete_entity, merge_entities, \
         upsert_requirement, update_requirement, delete_requirement, set_coverage, \
         report_diagnostic, resolve_diagnostic, and the view tools upsert_view, update_view, \
         delete_view. They stage into the open batch's changeset; outside one they are rejected \
         toward begin_goals. To wait for new work, call await_changes (a long poll). A gated \
         batch says `awaiting release`; `jazyk release` (or the GUI) approves it. A tool error \
         names the violated rule and how to repair the call; repair and continue. If any \
         instruction, tool, argument, or error message is ambiguous, wrong, or confusing, call \
         report_feedback: it reaches jazyk's developers, never touches the graph, and is not a \
         substitute for the work.",
    );
    s
}

impl McpServer {
    // Whether this serving stands in a project or in a bare directory. Decides which
    // project tools exist. Mirrors docs/frontends/acp.md#project-tools.
    fn initialized(&self) -> bool {
        self.project.root.join("jazyk.toml").exists()
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
            trace: crate::session::Trace::stderr(crate::session::TraceLevel::Quiet)
                .with_transcript(&out_for_trace, "mcp"),
            bridge,
            delivered: std::sync::Mutex::new(Delivered::default()),
        }
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
            std::fs::metadata(path)
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
            std::time::Instant::now() + std::time::Duration::from_secs(timeout.clamp(1, 3600))
        });
        let mut changed_docs: Vec<String> = Vec::new();
        let mut changed = false;
        while deadline.is_none_or(|d| std::time::Instant::now() < d) {
            std::thread::sleep(std::time::Duration::from_secs(2));
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
        json!({
            "changed": changed,
            "changedDocs": changed_docs,
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
            "verificationTasks": board.open_of(&["verify"]),
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
        let mut v: Vec<&'static str> = Vec::new();
        for m in &self.modes {
            let set = match m.as_str() {
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
                        t.extend(crate::tools::FILE_TOOLS);
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
                "verify" => toolset("mcp-verify"),
                // The chat serving: reads, every lifecycle, and the server-implemented
                // chat tools; no raw write tools (docs/frontends/mcp.md#toolsets).
                "chat" => {
                    let mut t = toolset("mcp-read");
                    t.extend(LIFECYCLE);
                    t.extend(crate::tools::GEN_TOOLS);
                    t.extend(crate::tools::BIND_TOOLS);
                    t.extend(crate::tools::VERIFY_TOOLS);
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

    pub fn run(&self) {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(req) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let method = req["method"].as_str().unwrap_or_default().to_string();
            let id = req["id"].clone();
            if id.is_null() {
                continue; // notification, no response
            }
            let result = self.handle(&method, &req["params"]);
            let resp = match result {
                Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
                Err((code, msg)) => {
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
                }
            };
            let mut out = stdout.lock();
            writeln!(out, "{}", resp).ok();
            out.flush().ok();
        }
        if self.bridge.ephemeral {
            self.eof_finish();
        }
        self.trace
            .finish_transcript("done", &json!({"modes": self.modes}));
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
            .unwrap_or_else(|| format!("agent-{}", std::process::id()))
    }

    // The `goals` tool: the board with readiness sentences, gated and claimedBy, the
    // batches the scheduler would form, and the verdict with its counts when nothing
    // is open. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    fn goals_answer(&self) -> Value {
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
        let mut v = board.answer();
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
    fn begin_goals(&self, params: &Value) -> Value {
        if let Some(o) = self.open.lock().unwrap().as_ref() {
            return json!({"error": {"rule": "batch-open", "message": format!(
                "batch `{}` is already open with {} staged mutation(s); done or abandon_goals first",
                o.id, o.session.staged.len())}});
        }
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
                "batch `{}` is claimed by worker `{}`; pick another batch or wait for the lease to lapse", batch.id, holder)}});
        }
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
        let (loaded, skills) = crate::session::initial_loaded(&store, &goals);
        let mut pb = crate::session::ProjectBlock::compute(&store, &goals, &control.compile);
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
        let instructions = crate::session::session_prompt_elided(
            &store,
            &goals,
            &loaded,
            &skills,
            &pb,
            include_contract,
            &skip_skills,
        );
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
            })
            .collect();
        let mut session = ToolSession::new(store, scope, self.mutation_limit, self.context_budget);
        session.loaded = loaded;
        session.skills = skills;
        session.gen = crate::gen::GenSettings::resolve(&self.project);
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
        let mut reply = self.commit_open(&mut o);
        drop(open);
        // The consumer that empties the board runs the deterministic tail.
        let board = crate::board::Board::compute(&self.project, &self.out);
        let graph_kinds = crate::board::Board::graph_kinds();
        if board.ready_of(&graph_kinds) == 0 {
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
            if board2.ready_of(&graph_kinds) == 0 {
                let generate = board2.open_of(&["generate"]);
                reply["next"] = if generate == 0 {
                    json!("compilation done; nothing pending")
                } else {
                    json!(format!(
                        "compilation done; {} generation goal(s) ready (generation_tasks lists them)",
                        generate
                    ))
                };
                return reply;
            }
            // The checks can surface new work (rare); fall through to name it.
            reply["next"] = json!(board2.answer());
            return reply;
        }
        // beginNext claims the next ready batch in the same call, saving a round
        // trip per batch. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
        if params["arguments"]["beginNext"].as_bool() == Some(true) {
            let began = self.begin_goals(&json!({"arguments": {}}));
            if began["error"].is_null() {
                reply["began"] = began;
                return reply;
            }
        }
        reply["next"] = json!(board.answer());
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
        let commit = o.session.commit(o.rounds, 0);
        let resolved_ids: Vec<String> = commit.resolved.iter().map(|r| r.goal.clone()).collect();
        let mut s = Store::load(&self.out);
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        s.sync_docs(&parsed);
        let mut reply = json!({"committed": true, "batch": o.id, "applied": 0});
        let mut generation = None;
        if !staged.is_empty() {
            let report = s.apply(staged, &commit);
            generation = Some(report.generation);
            reply["applied"] = json!(report.applied);
            reply["generation"] = json!(report.generation);
            if !report.skipped.is_empty() {
                reply["skipped"] = json!(report.skipped);
            }
        }
        // A resolution clears the change records its goal stood on, mutations or not.
        if !resolved_ids.is_empty() {
            let clear: Vec<String> = resolved_ids
                .iter()
                .flat_map(|id| o.records.get(id).cloned().unwrap_or_default())
                .collect();
            s.clear_changes(&clear);
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
        s.status.parked.retain(|p| !batch_ids.contains(&p.id));
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
                json!({"name": c.name, "tier": c.tier, "task": c.task_type, "state": state})
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
                "begin_case claims the first pending case"
            },
        })
    }

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
            std::process::id(),
            case.name
        ));
        std::fs::remove_dir_all(&tmp).ok();
        let store = crate::benchmark::sandbox(case, &tmp);
        let gs = crate::gen::GenSettings {
            deliverable: tmp.join("deliverable"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        std::fs::create_dir_all(&gs.deliverable).ok();
        let item = WorkItem {
            task: case.task_type.clone(),
            target: case.target.clone(),
            dirty_sections: match case.task_type.as_str() {
                "reconcile-doc" => store
                    .docs
                    .get(&case.target)
                    .map(|r| r.sections.keys().cloned().collect())
                    .unwrap_or_default(),
                _ => Vec::new(),
            },
            stale_anchors: Vec::new(),
            proposals: Vec::new(),
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
                let goal = item.to_goal(crate::model::GoalState::Open);
                reply["instructions"] =
                    json!(crate::session::preview(&store, std::slice::from_ref(&goal)));
                if case.task_type == "generate-entity" {
                    reply["deliverableDir"] = json!(gs.deliverable.to_string_lossy());
                    reply["note"] = json!("write real files into deliverableDir with your own tools; record_generation and run_tests act on this sandbox while the case is open");
                }
            }
        }
        let scope = WorkScope::from_item(&item);
        let mut session = ToolSession::new(
            store.clone(),
            scope,
            self.mutation_limit,
            self.context_budget,
        );
        session.gen = gs.clone();
        session.caller = self.caller("benchmark", &case.name);
        b.open = Some(OpenCase {
            idx,
            item,
            store,
            session,
            calls: 0,
            tmp,
            gs,
        });
        reply
    }

    fn finish_case(&self, params: &Value) -> Value {
        let cases = crate::benchmark::parse_cases();
        let mut b = self.bench.lock().unwrap();
        let Some(mut o) = b.open.take() else {
            return json!({"error": {"rule": "no-open-case", "message": "no case is open; begin_case first"}});
        };
        let case = &cases[o.idx];
        // Turn cases face the same done gates a turn does; a rejection keeps the case
        // open, same contract as done on a batch.
        if case.task_type == "reconcile-doc" || case.task_type.starts_with("review-") {
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
            if !staged.is_empty() {
                o.store.apply(staged, &o.item.commit(o.calls, 0));
            }
        }
        if case.task_type == "verify-requirement" {
            let verdict = params["arguments"]["verdict"].as_str().unwrap_or("");
            if verdict != "pass" && verdict != "fail" {
                b.open = Some(o);
                return json!({"error": {"rule": "bad-argument", "message": "finish_case on a verification case needs verdict: pass or fail"}});
            }
            let evidence = params["arguments"]["evidence"].as_str().unwrap_or("");
            if let Err(e) =
                crate::verify::mark(&o.store, &case.target, verdict, None, Some(evidence), &o.gs)
            {
                b.open = Some(o);
                return json!({"error": {"rule": "bad-argument", "message": e}});
            }
        }
        // Grade: the same deterministic checks an endpoint run faces.
        let staged_count = 0usize;
        let (mut passed, mut fail): (usize, Option<String>) = (0, None);
        for (kind, arg) in &case.checks {
            let verdict = if crate::benchmark::WORKFLOW_CHECKS.contains(&kind.as_str()) {
                crate::benchmark::eval_workflow_check(kind, arg, &o.store, &o.gs, &case.target)
            } else {
                crate::benchmark::eval_check(kind, arg, &o.store, staged_count.max(1))
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
        let score = if case.checks.is_empty() {
            0.0
        } else {
            passed as f64 / case.checks.len() as f64
        };
        let efficiency = (case.par_rounds as f64 / o.calls.max(1) as f64).min(1.0);
        std::fs::remove_dir_all(&o.tmp).ok();
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

    fn handle(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
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
                            let wref = self.worker.clone();
                            std::thread::spawn(move || loop {
                                std::thread::sleep(std::time::Duration::from_secs(30));
                                match wref.lock().unwrap().as_mut() {
                                    Some(h) => h.refresh(None),
                                    None => break,
                                }
                            });
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
                    "instructions": instructions_for(&self.modes, self.write, self.initialized())
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
                if self.modes.iter().any(|m| m == "compile") {
                    tools.push(json!({
                        "name": "goals",
                        "description": "The goal board: every goal with its kind, class, readiness (ready, or the blocking reason as a sentence), gated and claimedBy, plus the batches the scheduler would form. Zero open goals carries the build verdict with its counts. Next: begin_goals.",
                        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
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
                        self.trace.event(crate::session::TraceEvent::ToolError {
                            label,
                            rule: parsed["error"]["rule"]
                                .as_str()
                                .unwrap_or("error")
                                .to_string(),
                            message: parsed["error"]["message"]
                                .as_str()
                                .unwrap_or(text)
                                .to_string(),
                        });
                    } else {
                        self.trace.event(crate::session::TraceEvent::ToolResult {
                            label,
                            name: name.clone(),
                            summary: crate::session::condense(&parsed, 160),
                            full: crate::session::full_payload(&parsed),
                        });
                    }
                }
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
                        let v = self.goals_answer();
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
                    && !crate::tools::FILE_TOOLS.contains(&name.as_str());

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
                                allowed.iter().filter(|t| !crate::tools::READ_TOOLS.contains(t) && !crate::tools::GOAL_TOOLS.contains(t) && **t != crate::tools::FEEDBACK_TOOL).cloned().collect::<Vec<_>>().join(", "))}}),
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
                if is_graph_write && self.modes.iter().any(|m| m == "compile") {
                    return Ok(text_result(
                        json!({"error": {"rule": "no-open-batch", "message": "no batch is open; begin_goals first, then stage writes into it"}}),
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
                            });
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
                    let reply = json!({
                        "tasks": rows,
                        "count": rows.len(),
                        "next": if rows.is_empty() {
                            "nothing owed".to_string()
                        } else {
                            format!("{} claims one row; recording it resolves its goal", begin)
                        },
                    });
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
                        if let Err(holder) =
                            crate::control::claim(&self.out, ent, &self.worker_id())
                        {
                            return Ok(text_result(
                                json!({"error": {"rule": "claimed", "message": format!(
                                    "`{}` is claimed by worker `{}`; pick another entity or wait for the lease to lapse", ent, holder)}}),
                                true,
                            ));
                        }
                    }
                }
                if name == "record_generation" {
                    if let Some(ent) = args["entity"].as_str() {
                        crate::control::release_lease(&self.out, ent);
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
                                    "`{}` is claimed by worker `{}`; pick another requirement or wait for the lease to lapse", rid, holder)}}),
                                true,
                            ));
                        }
                    }
                }
                if name == "record_binding" {
                    if let Some(rid) = args["requirement"].as_str() {
                        crate::control::release_lease(&self.out, rid);
                    }
                }

                let store = Store::load(&self.out);
                if store.docs.is_empty()
                    && store.graph.entities.is_empty()
                    && !self.modes.iter().any(|m| m == "compile")
                {
                    return Ok(text_result(
                        json!({"error": {"rule": "no-build", "message": "no graph found; run `jazyk compile` first (or connect a compile serving)"}}),
                        true,
                    ));
                }
                let scope = WorkScope::serving(if is_write { "mcp-write" } else { "mcp-read" });
                let mut session =
                    ToolSession::new(store, scope, self.mutation_limit, self.context_budget);
                session.gen = crate::gen::GenSettings::resolve(&self.project);
                session.caller =
                    self.caller(if self.write { "mcp-write" } else { "mcp-read" }, &name);
                match session.dispatch(&name, &args) {
                    Ok(v) => {
                        if is_graph_write && !session.staged.is_empty() {
                            // graph --write: each call commits as its own changeset.
                            // The commit writes its change records, so the next
                            // build derives the goals the write opened.
                            let mut s = Store::load(&self.out);
                            let report = s.apply(
                                session.staged,
                                &crate::store::Commit::session(Vec::new(), 1, 0),
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
                    Err(e) => Ok(text_result(e.to_value(), true)),
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
        if let Some(sink) = &self.bridge.edit_sink {
            if sink_write(sink, &path, old_text, new_text, full).is_ok() {
                return Ok(());
            }
            // Nothing listening: fall through to the direct write.
        }
        std::fs::write(&path, full).map_err(|e| format!("write {}: {}", path.display(), e))
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
            let old_full = match std::fs::read_to_string(&path) {
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
        let report = s.apply(ops, &item.commit(1, 0));
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
        let old_full = match std::fs::read_to_string(&path) {
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
            return json!({"error": {"rule": "unknown-section", "message": format!("no section `{}#{}` in the graph; compile first", doc, section)}});
        };
        let path = self.project.root.join(&doc);
        let old_full = match std::fs::read_to_string(&path) {
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
                        &old_full[..at].trim_end_matches('\n'),
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
            ("upsert_requirement", json!({"statement": statement, "entities": entities, "section": format!("{}#{}", doc, section), "quote": text})),
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
        let old_full = match std::fs::read_to_string(&path) {
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

    fn init_project(&self) -> Value {
        let root = &self.project.root;
        if root.join("jazyk.toml").exists() {
            return json!({"error": {"rule": "already-a-project", "message": "jazyk.toml already exists here"}});
        }
        if let Err(e) = std::fs::write(root.join("jazyk.toml"), crate::cli::INIT_TOML) {
            return json!({"error": {"rule": "write-failed", "message": e.to_string()}});
        }
        let made = match crate::cli::init_scaffold(root) {
            Ok(made) => made,
            Err(e) => return json!({"error": {"rule": "write-failed", "message": e}}),
        };
        json!({"initialized": root.display().to_string(), "created": made,
               "next": "edit docs/README.md, then compile"})
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
        let path = self.project.root.join("jazyk.toml");
        let old_full = std::fs::read_to_string(&path).unwrap_or_default();
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
        if let Err(e) = self.write_edit("jazyk.toml", &old_full, &full, &full) {
            return json!({"error": {"rule": "write-failed", "message": e}});
        }
        json!({"updated": settings.keys().cloned().collect::<Vec<_>>(),
               "note": "jazyk.toml edited in place; a running GUI reloads it live"})
    }
}

// Minimal line edit on a TOML text: set `key = "value"` inside `[section]`, appending
// the section or the key when missing, touching nothing else.
pub fn toml_set(text: &str, section: &str, key: &str, value: &str) -> String {
    let header = format!("[{}]", section);
    let rendered = format!(
        "{} = \"{}\"",
        key,
        value.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let mut in_section = false;
    let mut section_start: Option<usize> = None;
    for i in 0..lines.len() {
        let t = lines[i].trim();
        if t.starts_with('[') {
            in_section = t == header;
            if in_section {
                section_start = Some(i);
            }
            continue;
        }
        if in_section {
            if let Some((k, _)) = t.split_once('=') {
                if k.trim() == key {
                    lines[i] = rendered;
                    return lines.join("\n") + "\n";
                }
            }
        }
    }
    match section_start {
        Some(i) => {
            lines.insert(i + 1, rendered);
        }
        None => {
            if !lines.is_empty() && !lines.last().unwrap().is_empty() {
                lines.push(String::new());
            }
            lines.push(header);
            lines.push(rendered);
        }
    }
    lines.join("\n") + "\n"
}

// One edit over the delegation socket: a JSON line out, an acknowledgement line back.
// Mirrors docs/frontends/acp.md#doc-edit-delegation.
fn sink_write(
    sink: &str,
    path: &std::path::Path,
    old_text: &str,
    new_text: &str,
    full: &str,
) -> Result<(), String> {
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

fn text_result(v: Value, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": v.to_string()}],
        "isError": is_error
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn tool_names(s: &McpServer) -> Vec<String> {
        let r = s.handle("tools/list", &json!({})).unwrap();
        r["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    // A chat dual write moves the prose and the graph in one changeset and absorbs
    // its own hashes, so a following sync finds nothing dirty; edit_fact without an
    // accepted sentence lands a decree with its proposal instead.
    // Mirrors docs/frontends/acp.md#dual-write-tools.
    #[test]
    fn revise_requirement_commits_prose_and_graph_without_redirtying() {
        use crate::model::{Entity, Provenance, Requirement, SourceRef};
        use crate::store::{Commit, Op};
        let dir = std::env::temp_dir().join(format!("jazyk-mcp-revise-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(
            dir.join("jazyk.toml"),
            "[docs]\nglob = [\"docs/**/*.md\"]\n",
        )
        .unwrap();
        std::fs::write(
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
        let text = std::fs::read_to_string(dir.join("docs/pay.md")).unwrap();
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
        std::fs::remove_dir_all(&dir).ok();
    }

    // The compile serving claims goal batches: begin_goals claims the next ready
    // batch under a per-batch lease naming its goals, done without a mark per
    // mandatory goal refuses with open-goal, and abandon_goals releases the claim.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    #[test]
    fn begin_goals_claims_the_next_batch_and_done_wants_marks() {
        let dir = std::env::temp_dir().join(format!("jazyk-mcp-goals-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(
            dir.join("jazyk.toml"),
            "[docs]\nglob = [\"docs/**/*.md\"]\n",
        )
        .unwrap();
        std::fs::write(dir.join("docs/a.md"), "# A\n\nThe body.\n").unwrap();
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        std::fs::create_dir_all(&out).unwrap();
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
        std::fs::remove_dir_all(&dir).ok();
    }

    // The compile serving lists the goal lifecycle, the view tools, and the goal
    // tools; a raw write tool outside an open batch is rejected toward begin_goals.
    // Mirrors docs/compiler/tools.md#toolsets.
    #[test]
    fn compile_serving_lists_goal_and_view_tools_and_gates_writes() {
        let dir = std::env::temp_dir().join(format!("jazyk-mcp-toolset-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(
            dir.join("jazyk.toml"),
            "[docs]\nglob = [\"docs/**/*.md\"]\n",
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
        std::fs::remove_dir_all(&dir).ok();
    }

    // A project tool is offered only where it can do something: scaffolding in a bare
    // directory, settings in a project. The instructions say which case it is, so the
    // agent never spends a call to learn it.
    // Mirrors docs/frontends/acp.md#project-tools.
    #[test]
    fn project_tools_follow_the_project_state() {
        let base = std::env::temp_dir().join(format!("jazyk-mcp-init-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let bare = base.join("bare");
        let proj = base.join("proj");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("jazyk.toml"),
            "[docs]\nglob = [\"docs/**/*.md\"]\n",
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
        assert!(
            instructions_for(&["chat".to_string()], false, true).contains("already initialized")
        );

        // The refusal survives as a floor under the listing: settings without a file.
        let r =
            chat_server(&bare).update_project_settings(&json!({"settings": {"llm.model": "x"}}));
        assert_eq!(r["error"]["rule"], "not-a-project");
        std::fs::remove_dir_all(&base).ok();
    }
}
