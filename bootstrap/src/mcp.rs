// The MCP server: the tool registry served over stdio as line-delimited JSON-RPC.
// `jazyk mcp <toolsets>` names what the serving is for (compile, generate, verify,
// graph); compilation holds an open changeset between calls, exactly one at a time.
// Mirrors docs/frontends/mcp.md.
use crate::model::WorkItem;
use crate::store::Store;
use crate::tools::{catalog, toolset, ToolSession, WorkScope};
use serde_json::{json, Value};
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
    // The open compilation task: a ToolSession holding the staged changeset across
    // calls. Single-flight per serving. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    open: std::sync::Mutex<Option<OpenTask>>,
    // The session transcript: one event per call under <out>/trace, reviewable beside
    // a build. Mirrors docs/frontends/mcp.md#transcripts.
    trace: crate::turn::Trace,
    // The agent-run benchmark: the open case's sandbox and the run's accumulated
    // scores. Mirrors docs/benchmark/benchmark.md#agent-run-benchmarks.
    bench: std::sync::Mutex<BenchRun>,
    // The serving's registration in the worker registry, heartbeated while the
    // process lives. Mirrors docs/compiler/reconciler.md#workers-and-leases.
    worker: std::sync::Arc<std::sync::Mutex<Option<crate::control::WorkerHandle>>>,
    // Bridge-spawned serving flags: this serving belongs to one ACP session.
    // Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
    bridge: BridgeFlags,
    // Task kinds whose instructions this serving already delivered: later tasks of
    // the same kind elide the repeated text. Mirrors
    // docs/frontends/mcp.md#compilation-over-mcp.
    seen_kinds: std::sync::Mutex<std::collections::HashSet<String>>,
}

// Flags of a serving injected into an ACP session by the bridge. Not for standalone
// servings. Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
#[derive(Default, Clone)]
pub struct BridgeFlags {
    // The serving belongs to one session: no worker registration, and end of input
    // with an open task runs the implicit finish.
    pub ephemeral: bool,
    // begin_compilation accepts only this target (parallel-wave safety).
    pub only: Option<String>,
    // The serving is part of the running internal build: the build-lease refusal and
    // the release gate do not apply to its target, and leases claim under this id.
    pub build_token: Option<String>,
    // Serve the file and command tools, for agents with no editor of their own.
    pub serve_files: bool,
    // Delegate document and settings writes to the spawning process (the IDE proxy).
    pub edit_sink: Option<String>,
    // The bridge already sent the task's instructions and package as the session
    // prompt; begin_compilation answers with a short ack instead of repeating them.
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

struct OpenTask {
    item: WorkItem,
    session: ToolSession,
    rounds: u32,
}

// The compilation lifecycle: served beside the catalog, implemented on the server
// because they own the queue and the open changeset.
const LIFECYCLE: [&str; 4] = ["compilation_tasks", "begin_compilation", "finish_compilation", "abandon_compilation"];

fn instructions_for(modes: &[String], write: bool) -> String {
    let mut s = String::from(
        "This server is one jazyk project's semantic graph: entities and EARS requirements \
         reconciled from prose documentation, consumed by generation and verification. ",
    );
    if modes.iter().any(|m| m == "compile") {
        s.push_str(
            "COMPILATION LOOP: call compilation_tasks; while a task is ready, begin_compilation, \
             follow the instructions in the returned package, stage findings with the write tools, \
             then call done. Its reply names the next ready task; repeat until the queue is empty \
             and the verdict is converged. ",
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
        s.push_str(
            "CHAT SERVING: you are in a conversation about this project. Read the graph with the \
             read tools. A requirement lives in the prose: change one with revise_requirement (new \
             prose, optional new ears), add one with add_requirement, remove one with \
             retract_requirement; each moves the document and the graph in one atomic commit. \
             init_project scaffolds a project; update_project_settings edits jazyk.toml keys. The \
             compilation, binding, generation, and verification lifecycles are available for \
             explicit requests. ",
        );
    }
    if modes.iter().any(|m| m == "graph") && write {
        s.push_str("Write tools are enabled for manual graph surgery; each call commits as its own changeset. ");
    }
    s.push_str(
        "The write tools are: upsert_entity, update_entity, delete_entity, merge_entities, \
         upsert_requirement, update_requirement, delete_requirement, set_coverage, \
         report_diagnostic, resolve_diagnostic. To wait for new work, call await_changes (a long \
         poll). A gated task says `awaiting release`; `jazyk release` (or the GUI) approves it. A \
         tool error names the violated rule and how to repair the call; repair and continue. If any \
         instruction, tool, argument, or error message is ambiguous, wrong, or confusing, call \
         report_feedback: it reaches jazyk's developers, never touches the graph, and is not a \
         substitute for the work.",
    );
    s
}

impl McpServer {
    pub fn new(project: crate::project::Project, out: PathBuf, modes: Vec<String>, write: bool) -> McpServer {
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
            mutation_limit: project.limits.turn_mutations,
            context_budget: project.limits.context_budget,
            project,
            out,
            modes,
            write,
            client: std::sync::Mutex::new(None),
            open: std::sync::Mutex::new(None),
            bench: std::sync::Mutex::new(BenchRun::default()),
            worker: std::sync::Arc::new(std::sync::Mutex::new(None)),
            trace: crate::turn::Trace::stderr(crate::turn::TraceLevel::Quiet).with_transcript(&out_for_trace, "mcp"),
            bridge,
            seen_kinds: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    // The server's own long poll: returns when the graph's generation counter moves, a
    // documentation file changes on disk, or the ledger or a watched deliverable file
    // changes, or at the timeout. timeout_seconds 0 waits indefinitely; the default
    // returns because most MCP clients bound a tool call with their own timeout.
    // Mirrors docs/frontends/mcp.md#the-work-loop.
    fn await_changes(&self, params: &Value) -> Value {
        let timeout = params["arguments"]["timeout_seconds"].as_u64().unwrap_or(300);
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
        let snapshot: std::collections::BTreeMap<std::path::PathBuf, String> =
            watched(&gs).into_iter().map(|f| (f.clone(), fingerprint(&f))).collect();
        let start_gen = Store::load(&self.out).status.generation;
        let deadline = (timeout > 0)
            .then(|| std::time::Instant::now() + std::time::Duration::from_secs(timeout.clamp(1, 3600)));
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
        let q = crate::queue::compute(&self.project, &self.out);
        let c = crate::control::Control::load(&self.project, &self.out);
        let (c_act, b_act, g_act, v_act) = (
            crate::queue::actionable(&q.compile),
            crate::queue::actionable(&q.bind),
            crate::queue::actionable(&q.generate),
            crate::queue::actionable(&q.verify),
        );
        let gated =
            crate::queue::gated(&q.compile) + crate::queue::gated(&q.bind) + crate::queue::gated(&q.generate);
        json!({
            "changed": changed,
            "changedDocs": changed_docs,
            "compilationTasks": q.compile.len(),
            "bindingTasks": q.bind.len(),
            "generationTasks": q.generate.len(),
            "verificationTasks": q.verify.len(),
            "workflow": {"compile": c.compile, "generate": c.generate},
            "gatedTasks": gated,
            "verdict": q.verdict,
            "openDiagnostics": q.open_diags,
            "next": if c_act > 0 {
                "compilation_tasks lists the work"
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
                    t.extend(["benchmark_cases", "begin_case", "finish_case", "benchmark_report"]);
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
            let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
            let method = req["method"].as_str().unwrap_or_default().to_string();
            let id = req["id"].clone();
            if id.is_null() {
                continue; // notification, no response
            }
            let result = self.handle(&method, &req["params"]);
            let resp = match result {
                Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
                Err((code, msg)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}}),
            };
            let mut out = stdout.lock();
            writeln!(out, "{}", resp).ok();
            out.flush().ok();
        }
        if self.bridge.ephemeral {
            self.eof_finish();
        }
        self.trace.finish_transcript("done", &json!({"modes": self.modes}));
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

    fn compilation_tasks(&self) -> Value {
        let mut q = crate::queue::compute(&self.project, &self.out);
        // An empty queue with a stale `incomplete` verdict settles here: finalize is
        // deterministic and idempotent, and a lister that finds nothing to do may as
        // well say so truthfully. A dangling judged diagnostic settles the same way:
        // finalize resolves or re-enqueues it, and the recompute lists the reviews.
        // Mirrors docs/compiler/reconciler.md#the-task-queue.
        if q.compile_empty() && (q.verdict != "converged" || q.dangling_diags) && self.open.lock().unwrap().is_none() {
            let mut s = Store::load(&self.out);
            let parked = s.status.parked.clone();
            let quiet = crate::turn::Trace::stderr(crate::turn::TraceLevel::Quiet);
            crate::reconcile::finalize(&mut s, &self.project, &parked, &quiet);
            q = crate::queue::compute(&self.project, &self.out);
        }
        let mut v = q.compilation_answer();
        if self.open.lock().unwrap().is_some() {
            v["openTask"] = json!("a task is already open; done or abandon_compilation first");
        }
        v
    }

    fn begin_compilation(&self, params: &Value) -> Value {
        if let Some(o) = self.open.lock().unwrap().as_ref() {
            return json!({"error": {"rule": "task-open", "message": format!(
                "task `{} {}` is already open with {} staged mutation(s); done or abandon_compilation first",
                o.item.task, o.item.target, o.session.staged.len())}});
        }
        let q = crate::queue::compute(&self.project, &self.out);
        let target = params["arguments"]["task"].as_str();
        let Some(item) = q.find(target) else {
            let mut v = q.compilation_answer();
            v["error"] = json!({"rule": "no-ready-task", "message": match target {
                Some(t) => format!("`{}` is not a ready task; the queue above says what is", t),
                None => "no task is ready; the queue above says why".to_string(),
            }});
            return v;
        };
        // A bridge serving scoped to one target refuses everything else, so a
        // confused agent cannot grab a sibling wave's work.
        if let Some(only) = &self.bridge.only {
            if item.target != *only {
                return json!({"error": {"rule": "wrong-target", "message": format!(
                    "this serving is scoped to `{}`; `{}` belongs to another session", only, item.target)}});
            }
        }
        // The control plane's claims, in order: a gated task awaits its release, a
        // running internal build owns the queue, a leased task belongs to its holder.
        // A serving carrying the build's own token skips the first two: the build
        // already released this work and holds the coarse lease itself.
        // Mirrors docs/frontends/mcp.md#the-control-plane-over-mcp.
        if self.bridge.build_token.is_none() {
            if q.compile.iter().any(|e| e["target"] == item.target.as_str() && e["gated"] == true) {
                return json!({"error": {"rule": "awaiting-release", "message": format!(
                    "`{}` is awaiting release: `jazyk release compile` (or the GUI) approves it", item.target)}});
            }
            if let Some(l) = crate::control::build_lease(&self.out) {
                return json!({"error": {"rule": "build-running", "message": format!(
                    "an internal build is running (lease `{}`, heartbeated every 30s, expires {}s after the last heartbeat). \
                     await_changes returns when it clears. If you were spawned BY that build, its serving carries --build-token and never sees this error; \
                     seeing it means this serving is a bystander and the queue is not yours yet", l.worker, crate::control::LEASE_TTL_SECS)}});
            }
        }
        if let Err(holder) = crate::control::claim(&self.out, &item.target, &self.worker_id()) {
            return json!({"error": {"rule": "claimed", "message": format!(
                "`{}` is claimed by worker `{}`; pick another task or wait for the lease to lapse", item.target, holder)}});
        }
        if let Some(h) = self.worker.lock().unwrap().as_mut() {
            h.refresh(Some(&format!("{} {}", item.task, item.target)));
        }
        // The task's snapshot: the store with section trees synced against the docs on
        // disk, in memory. The commit at finish re-syncs its own fresh store.
        let mut store = Store::load(&self.out);
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        store.sync_docs(&parsed);
        let gs = crate::gen::GenSettings::resolve(&self.project);
        let (instructions, pack) = crate::turn::task_prompt(&store, &item, &self.project.limits, &self.project.linting, &gs);
        let scope = match item.task.as_str() {
            "reconcile-doc" => WorkScope {
                task: item.task.clone(),
                doc: Some(item.target.clone()),
                target: item.target.clone(),
                target_sections: item.dirty_sections.clone(),
                stale_anchors: item.stale_anchors.clone(),
            },
            _ => WorkScope {
                task: item.task.clone(),
                doc: None,
                target: item.target.clone(),
                target_sections: Vec::new(),
                stale_anchors: Vec::new(),
            },
        };
        let mut session = ToolSession::new(store, scope, self.mutation_limit, self.context_budget);
        session.gen = crate::gen::GenSettings::resolve(&self.project);
        session.caller = self.caller(&item.task, &item.target);
        let write_tools: Vec<&str> = toolset(&item.task)
            .into_iter()
            .filter(|t| !crate::tools::READ_TOOLS.contains(t) && *t != "done" && *t != crate::tools::FEEDBACK_TOOL)
            .collect();
        let reply = if self.bridge.packaged {
            // The bridge already delivered the contract as the session prompt.
            json!({
                "task": {"kind": item.task, "target": item.target},
                "note": "changeset open; stage findings with the write tools, then finish with done",
            })
        } else {
            // The first task of each kind ships the full contract; later ones elide
            // it (the agent saw it earlier in this session), which is the bulk of
            // the reply on review-heavy builds.
            let seen = !self.seen_kinds.lock().unwrap().insert(item.task.clone());
            let instructions_field = if seen {
                json!(format!("(same contract as the earlier {} task in this session; unchanged)", item.task))
            } else {
                json!(instructions)
            };
            json!({
                "task": {"kind": item.task, "target": item.target,
                         "dirtySections": item.dirty_sections.iter().map(|r| format!("{}#{}", item.target, r)).collect::<Vec<_>>(),
                         "staleAnchors": item.stale_anchors},
                "instructions": instructions_field,
                "package": pack,
                "writeTools": write_tools,
                "readTools": ["context", "expand", "search", "read_section", "get_entity", "diagnostics"],
                "finishTool": "done",
                "next": "stage findings with the write tools, then done with a one-line summary",
            })
        };
        let _ = (&instructions, &pack, &write_tools);
        *self.open.lock().unwrap() = Some(OpenTask { item, session, rounds: 0 });
        reply
    }

    fn finish_compilation(&self, params: &Value) -> Value {
        let mut open = self.open.lock().unwrap();
        let Some(mut o) = open.take() else {
            return json!({"error": {"rule": "no-open-task", "message": "no compilation task is open; begin_compilation first"}});
        };
        let summary = params["arguments"]["summary"].as_str().unwrap_or("").to_string();
        // The same done gates an in-process turn faces: coverage contract, stale anchors.
        if let Err(e) = o.session.dispatch("done", &json!({"summary": summary})) {
            let v = e.to_value();
            crate::control::refresh_lease(&self.out, &o.item.target);
            *open = Some(o); // the changeset stays open; repair and finish again
            return v;
        }
        let mut reply = self.commit_open(&mut o);
        drop(open);
        // The consumer that empties the queue runs the deterministic tail.
        let q = crate::queue::compute(&self.project, &self.out);
        if q.compile_empty() {
            let mut s2 = Store::load(&self.out);
            let parked = s2.status.parked.clone();
            let quiet = crate::turn::Trace::stderr(crate::turn::TraceLevel::Quiet);
            let report = crate::reconcile::finalize(&mut s2, &self.project, &parked, &quiet);
            reply["verdict"] = json!(report.verdict);
            reply["coveragePct"] = json!(report.coverage_pct);
            // The verdict never travels alone (docs/compiler/reconciler.md#convergence).
            let counts = s2.open_diag_counts();
            if !counts.is_empty() {
                reply["openDiagnostics"] = json!(counts);
                reply["diagnosticsNote"] = json!("open diagnostics stand in the graph; the diagnostics read tool lists them");
            }
            let q2 = crate::queue::compute(&self.project, &self.out);
            if q2.compile_empty() {
                reply["next"] = if q2.generate.is_empty() {
                    json!("compilation done; nothing pending")
                } else {
                    json!(format!(
                        "compilation done; {} generation task(s) ready (generation_tasks lists them)",
                        q2.generate.len()
                    ))
                };
                return reply;
            }
            // The checks can surface new work (rare); fall through to name it.
            reply["next"] = json!(q2.compilation_answer());
            return reply;
        }
        // beginNext claims the next ready task in the same call, saving a round trip
        // per task. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
        if params["arguments"]["beginNext"].as_bool() == Some(true) {
            let began = self.begin_compilation(&json!({"arguments": {}}));
            if began["error"].is_null() {
                reply["began"] = began;
                return reply;
            }
        }
        reply["next"] = json!(q.compilation_answer());
        reply
    }

    // Land a task whose done gates passed: release the lease, apply the staged work,
    // complete reviews, un-park. Shared by finish_compilation and the ephemeral
    // end-of-input finish.
    fn commit_open(&self, o: &mut OpenTask) -> Value {
        crate::control::release_lease(&self.out, &o.item.target);
        if let Some(h) = self.worker.lock().unwrap().as_mut() {
            h.refresh(Some(""));
        }
        let staged = std::mem::take(&mut o.session.staged);
        let mut s = Store::load(&self.out);
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        s.sync_docs(&parsed);
        let mut reply = json!({"committed": true, "applied": 0});
        if !staged.is_empty() {
            let report = s.apply(staged, &o.item, o.rounds, 0);
            reply["applied"] = json!(report.applied);
            if !report.skipped.is_empty() {
                reply["skipped"] = json!(report.skipped);
            }
        }
        if o.item.task.starts_with("review-") {
            s.complete_review(&o.item.task, &o.item.target);
            if o.item.task == "review-requirement" {
                s.complete_pair_mirrors(&o.item.target);
            }
        }
        // A resumed parked item is no longer parked.
        if s.status.parked.iter().any(|p| p.target == o.item.target && p.task == o.item.task) {
            s.status.parked.retain(|p| !(p.target == o.item.target && p.task == o.item.task));
            s.save_status();
        }
        reply
    }

    // End of input with an open task: the agent's session ended without the finishing
    // call. Valid staged work still lands, under the same gates the budget path uses.
    // Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
    fn eof_finish(&self) {
        let mut open = self.open.lock().unwrap();
        let Some(mut o) = open.take() else { return };
        if o.session.finish_implicit("(implicit: the agent session ended)") {
            let reply = self.commit_open(&mut o);
            self.trace.event(crate::turn::TraceEvent::TurnDone {
                label: format!("{} {}", o.item.task, o.item.target),
                staged: reply["applied"].as_u64().unwrap_or(0) as usize,
                rounds: o.rounds,
                mode: "implicit".into(),
                summary: String::new(),
            });
        } else {
            crate::control::release_lease(&self.out, &o.item.target);
        }
    }

    fn abandon_compilation(&self, params: &Value) -> Value {
        let mut open = self.open.lock().unwrap();
        let Some(o) = open.take() else {
            return json!({"error": {"rule": "no-open-task", "message": "no compilation task is open"}});
        };
        crate::control::release_lease(&self.out, &o.item.target);
        if let Some(h) = self.worker.lock().unwrap().as_mut() {
            h.refresh(Some(""));
        }
        let reason = params["arguments"]["reason"].as_str().unwrap_or("");
        json!({
            "abandoned": format!("{} {}", o.item.task, o.item.target),
            "dropped": o.session.staged.len(),
            "reason": reason,
            "note": "the staged changeset is gone; the task stays in the queue",
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
        let scored: std::collections::BTreeSet<String> =
            b.scored.iter().filter_map(|e| e["name"].as_str().map(String::from)).collect();
        let want = params["arguments"]["case"].as_str();
        let Some((idx, case)) = cases
            .iter()
            .enumerate()
            .find(|(_, c)| match want {
                Some(w) => c.name == w,
                None => !scored.contains(&c.name),
            })
        else {
            return json!({"error": {"rule": "no-pending-case", "message": "no pending case; benchmark_cases shows the run, benchmark_report closes it"}});
        };
        let tmp = std::env::temp_dir().join(format!("jazyk-mcp-bench-{}-{}", std::process::id(), case.name));
        std::fs::remove_dir_all(&tmp).ok();
        let store = crate::benchmark::sandbox(case, &tmp);
        let gs = crate::gen::GenSettings { deliverable: tmp.join("deliverable"), worker: "agentic".into(), code: Vec::new() };
        std::fs::create_dir_all(&gs.deliverable).ok();
        let item = WorkItem {
            task: case.task_type.clone(),
            target: case.target.clone(),
            dirty_sections: match case.task_type.as_str() {
                "reconcile-doc" => store.docs.get(&case.target).map(|r| r.sections.keys().cloned().collect()).unwrap_or_default(),
                _ => Vec::new(),
            },
            stale_anchors: Vec::new(),
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
                    "statement": r.ears,
                    "quote": r.source.quote,
                    "files": case.deliverable.keys().map(|f| gs.deliverable.join(f).to_string_lossy().to_string()).collect::<Vec<_>>(),
                });
            }
            _ => {
                let (instructions, pack) =
                    crate::turn::task_prompt(&store, &item, &self.project.limits, &case.lint, &gs);
                reply["instructions"] = json!(instructions);
                reply["package"] = json!(pack);
                if case.task_type == "generate-entity" {
                    reply["deliverableDir"] = json!(gs.deliverable.to_string_lossy());
                    reply["note"] = json!("write real files into deliverableDir with your own tools; record_generation and run_tests act on this sandbox while the case is open");
                }
            }
        }
        let scope = match item.task.as_str() {
            "reconcile-doc" => WorkScope {
                task: item.task.clone(),
                doc: Some(item.target.clone()),
                target: item.target.clone(),
                target_sections: item.dirty_sections.clone(),
                stale_anchors: Vec::new(),
            },
            _ => WorkScope {
                task: item.task.clone(),
                doc: None,
                target: item.target.clone(),
                target_sections: Vec::new(),
                stale_anchors: Vec::new(),
            },
        };
        let mut session = ToolSession::new(store.clone(), scope, self.mutation_limit, self.context_budget);
        session.gen = gs.clone();
        session.caller = self.caller("benchmark", &case.name);
        b.open = Some(OpenCase { idx, item, store, session, calls: 0, tmp, gs });
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
        // open, same contract as finish_compilation.
        if case.task_type == "reconcile-doc" || case.task_type.starts_with("review-") {
            let summary = params["arguments"]["summary"].as_str().unwrap_or("(finish)").to_string();
            if let Err(e) = o.session.dispatch("done", &json!({"summary": summary})) {
                let v = e.to_value();
                b.open = Some(o);
                return v;
            }
            let staged = std::mem::take(&mut o.session.staged);
            if !staged.is_empty() {
                o.store.apply(staged, &o.item, o.calls, 0);
            }
        }
        if case.task_type == "verify-requirement" {
            let verdict = params["arguments"]["verdict"].as_str().unwrap_or("");
            if verdict != "pass" && verdict != "fail" {
                b.open = Some(o);
                return json!({"error": {"rule": "bad-argument", "message": "finish_case on a verification case needs verdict: pass or fail"}});
            }
            let evidence = params["arguments"]["evidence"].as_str().unwrap_or("");
            if let Err(e) = crate::verify::mark(&o.store, &case.target, verdict, None, Some(evidence), &o.gs) {
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
        let score = if case.checks.is_empty() { 0.0 } else { passed as f64 / case.checks.len() as f64 };
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
            .or_else(|| self.client.lock().unwrap().clone().map(|c| format!("{} (agent)", c)))
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
            let parts: Vec<usize> = e["checks"].as_str().unwrap_or("0/0").split('/').filter_map(|x| x.parse().ok()).collect();
            if parts.len() == 2 {
                checks_p += parts[0];
                checks_t += parts[1];
            }
        }
        // A tier never attempted is unmeasured, not capable: a partial run says what
        // it graded and nothing more.
        let ran = |t: &str| tier_sum.contains_key(t);
        let ok = |t: &str| ran(t) && *tier_ok.get(t).unwrap_or(&true);
        let ts = |t: &str| tier_sum.get(t).map(|(s, n)| if *n == 0 { 0.0 } else { ((s / *n as f64) * 100.0).round() / 100.0 }).unwrap_or(0.0);
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
                    && self.modes.iter().any(|m| m == "compile" || m == "generate" || m == "verify" || m == "decompile")
                {
                    let client = self.client.lock().unwrap().clone().unwrap_or_else(|| "agent".into());
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
                Ok(json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "jazyk", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": instructions_for(&self.modes, self.write)
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
                        "name": "compilation_tasks",
                        "description": "The compilation task queue: reconcile-document tasks by document level, then review tasks, each ready or blocked with the reason. Zero tasks carries the build verdict. Next: begin_compilation.",
                        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "begin_compilation",
                        "description": "Claim the named task (or the first ready one) and open its changeset. Returns the task's instructions and work package: dirty section bodies, statements already extracted, known entities, stale anchors. Stage findings with the write tools, then done. One task open at a time.",
                        "inputSchema": {"type": "object", "properties": {"task": {"type": "string", "description": "target from compilation_tasks, e.g. docs/api.md or req:api-1"}}, "additionalProperties": false}
                    }));
                    // One finish verb: `done` is the only listed completion tool.
                    // finish_compilation stays dispatchable for older clients but is
                    // not advertised; two listed names made models wonder which is
                    // which. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
                    tools.push(json!({
                        "name": "done",
                        "description": "Finish the open task: run the done gates (every dirty section marked, every stale anchor resolved) and commit the changeset atomically. A gate failure names the repair and keeps the changeset open; repair and call done again. The reply names the next ready task (beginNext: true also claims it in the same call); the finish that empties the queue reports the verdict.",
                        "inputSchema": {"type": "object", "properties": {"summary": {"type": "string"}, "beginNext": {"type": "boolean"}}, "required": ["summary"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "abandon_compilation",
                        "description": "Drop the open changeset without committing. The task stays in the queue.",
                        "inputSchema": {"type": "object", "properties": {"reason": {"type": "string"}}, "additionalProperties": false}
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
                        "description": "Change one requirement: the new prose replaces the old verbatim quote in its source document, and the graph node updates in the same atomic commit. Optional ears carries the new EARS rephrasing (defaults to keeping the old one).",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "newText": {"type": "string", "description": "the new prose sentence, written into the document"}, "ears": {"type": "string"}}, "required": ["id", "newText"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "add_requirement",
                        "description": "Add one requirement: the prose sentence is inserted into the named section (after afterQuote when given, else at the section's end) and the requirement lands in the same atomic commit.",
                        "inputSchema": {"type": "object", "properties": {"doc": {"type": "string"}, "section": {"type": "string"}, "text": {"type": "string", "description": "the prose sentence inserted into the document"}, "ears": {"type": "string"}, "entities": {"type": "array", "items": {"type": "string"}}, "afterQuote": {"type": "string"}}, "required": ["doc", "section", "text", "ears", "entities"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "retract_requirement",
                        "description": "Remove one requirement: its sentence leaves the prose and the node leaves the graph, one atomic commit.",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "reason": {"type": "string"}}, "required": ["id", "reason"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "init_project",
                        "description": "Scaffold a jazyk project here: jazyk.toml, docs/ with a placeholder root document, and deliverable/. Refused when jazyk.toml already exists.",
                        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "update_project_settings",
                        "description": "Edit jazyk.toml keys as minimal line edits. Supported keys: workflow.compile, workflow.generate, workflow.worker, acp.agent, gen.deliverable, gen.worker, llm.model, llm.base_url.",
                        "inputSchema": {"type": "object", "properties": {"settings": {"type": "object", "additionalProperties": {"type": "string"}}}, "required": ["settings"], "additionalProperties": false}
                    }));
                }
                tools.push(json!({
                    "name": "await_changes",
                    "description": "Long poll: returns when the graph moves, a documentation file changes, or the ledger or a watched deliverable file changes, or at the timeout (default 300s; 0 waits indefinitely, use only when your client does not bound tool calls). Carries the task counts per queue and which tool lists the work.",
                    "inputSchema": {"type": "object", "properties": {"timeout_seconds": {"type": "integer", "description": "seconds before returning unchanged; 0 = wait indefinitely"}}, "additionalProperties": false}
                }));
                Ok(json!({"tools": tools}))
            }
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default().to_string();
                // The transcript row: the call under its task's label, condensed the
                // same way a turn's rows are. Mirrors docs/frontends/mcp.md#transcripts.
                let label = match self.open.lock().unwrap().as_ref() {
                    Some(o) => format!("{} {}", o.item.task, o.item.target),
                    None => format!("mcp {}", self.modes.join(",")),
                };
                self.trace.event(crate::turn::TraceEvent::ToolCall {
                    label: label.clone(),
                    name: name.clone(),
                    summary: crate::turn::condense(&params["arguments"], 160),
                    full: crate::turn::full_payload(&params["arguments"]),
                });
                let reply = self.tool_call(&name, params, &label);
                if let Ok(v) = &reply {
                    let is_err = v["isError"] == true;
                    let text = v["content"][0]["text"].as_str().unwrap_or_default();
                    let parsed: Value = serde_json::from_str(text).unwrap_or_else(|_| json!(text));
                    if is_err {
                        self.trace.event(crate::turn::TraceEvent::ToolError {
                            label,
                            rule: parsed["error"]["rule"].as_str().unwrap_or("error").to_string(),
                            message: parsed["error"]["message"].as_str().unwrap_or(text).to_string(),
                        });
                    } else {
                        self.trace.event(crate::turn::TraceEvent::ToolResult {
                            label,
                            name: name.clone(),
                            summary: crate::turn::condense(&parsed, 160),
                            full: crate::turn::full_payload(&parsed),
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
                    "await_changes" => return Ok(text_result(self.await_changes(params), false)),
                    "compilation_tasks" if self.modes.iter().any(|m| m == "compile") => {
                        {
                        let v = self.compilation_tasks();
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    }
                    "begin_compilation" if self.modes.iter().any(|m| m == "compile") => {
                        {
                        let v = self.begin_compilation(params);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    }
                    "finish_compilation" if self.modes.iter().any(|m| m == "compile") => {
                        {
                        let v = self.finish_compilation(params);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    }
                    // `done` is what every task's instructions say; on a compile
                    // serving it is the same finish. One verb everywhere.
                    "done"
                        if self.modes.iter().any(|m| m == "compile")
                            && self.bench.lock().unwrap().open.is_none()
                            && self.open.lock().unwrap().is_some() =>
                    {
                        {
                        let v = self.finish_compilation(params);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    }
                    "abandon_compilation" if self.modes.iter().any(|m| m == "compile") => {
                        {
                        let v = self.abandon_compilation(params);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
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
                            return Ok(text_result(json!({"error": {"rule": "missing-argument", "message": "scope is required; decompile_tasks lists the scopes"}}), true));
                        };
                        let store = Store::load(&self.out);
                        let gs = crate::gen::GenSettings::resolve(&self.project);
                        let control = crate::control::Control::load(&self.project, &self.out);
                        let released = control.released.decompile.iter().any(|s| s == scope || s == ".");
                        if !released {
                            return Ok(text_result(json!({"error": {"rule": "awaiting-release", "message": format!(
                                "scope `{}` is not released for decompilation; `jazyk decompile {}` or the GUI's decompile action approves it", scope, scope)}}), true));
                        }
                        let reply = match crate::decompile::task(&self.project, &store, &gs, scope) {
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
                        let reply = match crate::decompile::submit(&self.project, &self.out, path, content, scope) {
                            Ok(v) => v,
                            Err(e) => return Ok(text_result(json!({"error": {"rule": "bad-draft", "message": e}}), true)),
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
                let is_write =
                    !crate::tools::READ_TOOLS.contains(&name.as_str()) && name != crate::tools::FEEDBACK_TOOL;
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
                        if !allowed.contains(&name.as_str()) && name != crate::tools::FEEDBACK_TOOL {
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
                // Graph writes stage into the open task's changeset. Mirrors
                // docs/frontends/mcp.md#compilation-over-mcp.
                let mut open = self.open.lock().unwrap();
                if let Some(o) = open.as_mut() {
                    let allowed = toolset(&o.item.task);
                    if is_graph_write && !allowed.contains(&name.as_str()) {
                        return Ok(text_result(
                            json!({"error": {"rule": "wrong-toolset", "message": format!(
                                "`{}` is not part of a {} task; this task's write tools: {}",
                                name, o.item.task,
                                allowed.iter().filter(|t| !crate::tools::READ_TOOLS.contains(t) && **t != "done" && **t != crate::tools::FEEDBACK_TOOL).cloned().collect::<Vec<_>>().join(", "))}}),
                            true,
                        ));
                    }
                    o.rounds += 1;
                    // Activity on the open task keeps its lease alive.
                    crate::control::refresh_lease(&self.out, &o.item.target);
                    return match o.session.dispatch(&name, &args) {
                        Ok(v) => Ok(text_result(v, false)),
                        Err(e) => Ok(text_result(e.to_value(), true)),
                    };
                }
                if is_graph_write && self.modes.iter().any(|m| m == "compile") {
                    return Ok(text_result(
                        json!({"error": {"rule": "no-open-task", "message": "no compilation task is open; begin_compilation first, then stage writes into it"}}),
                        true,
                    ));
                }
                drop(open);

                // The control plane over the stateless generation lifecycle: manual
                // mode gates begins behind a release, begin claims the entity's
                // lease, record frees it. Mirrors docs/frontends/mcp.md#the-control-plane-over-mcp.
                if name == "begin_generation" {
                    let c = crate::control::Control::load(&self.project, &self.out);
                    if self.bridge.build_token.is_none() {
                        if c.generate == "manual" && c.released.generate != Store::load(&self.out).status.generation {
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
                        if let Err(holder) = crate::control::claim(&self.out, ent, &self.worker_id()) {
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
                        if c.generate == "manual" && c.released.generate != Store::load(&self.out).status.generation {
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
                        if let Err(holder) = crate::control::claim(&self.out, rid, &self.worker_id()) {
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
                if store.docs.is_empty() && store.graph.entities.is_empty() && !self.modes.iter().any(|m| m == "compile") {
                    return Ok(text_result(
                        json!({"error": {"rule": "no-build", "message": "no graph found; run `jazyk compile` first (or connect a compile serving)"}}),
                        true,
                    ));
                }
                let scope = WorkScope {
                    task: if is_write { "mcp-write".into() } else { "mcp-read".into() },
                    doc: None,
                    target: String::new(),
                    target_sections: Vec::new(),
                    stale_anchors: Vec::new(),
                };
                let mut session = ToolSession::new(store, scope, self.mutation_limit, self.context_budget);
                session.gen = crate::gen::GenSettings::resolve(&self.project);
                session.caller = self.caller(if self.write { "mcp-write" } else { "mcp-read" }, &name);
                match session.dispatch(&name, &args) {
                    Ok(v) => {
                        if is_graph_write && !session.staged.is_empty() {
                            // Legacy graph --write: each call commits as its own changeset.
                            let mut s = Store::load(&self.out);
                            let wi = WorkItem {
                                task: "mcp".into(),
                                target: name.clone(),
                                dirty_sections: vec![],
                                stale_anchors: vec![],
                            };
                            let report = s.apply(session.staged, &wi, 1, 0);
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
    fn write_edit(&self, rel: &str, old_text: &str, new_text: &str, full: &str) -> Result<(), String> {
        let path = self.project.root.join(rel);
        if let Some(sink) = &self.bridge.edit_sink {
            if sink_write(sink, &path, old_text, new_text, full).is_ok() {
                return Ok(());
            }
            // Nothing listening: fall through to the direct write.
        }
        std::fs::write(&path, full).map_err(|e| format!("write {}: {}", path.display(), e))
    }

    // The shared tail of every dual write: run the graph mutation through a real
    // ToolSession against a snapshot that already absorbed the prose edit (so the
    // usual gates validate the new quote), write the file, commit both together.
    fn dual_commit(
        &self,
        doc: &str,
        section: &str,
        old_text: &str,
        new_text: &str,
        full: &str,
        old_full: &str,
        graph_call: (&str, Value),
        target: &str,
    ) -> Value {
        let mut snapshot = Store::load(&self.out);
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        snapshot.sync_docs(&parsed);
        snapshot.absorb_doc_edit(doc, full);
        let scope = WorkScope {
            task: "mcp-write".into(),
            doc: None,
            target: target.to_string(),
            target_sections: Vec::new(),
            stale_anchors: Vec::new(),
        };
        let mut session = ToolSession::new(snapshot, scope, self.mutation_limit, self.context_budget);
        session.gen = crate::gen::GenSettings::resolve(&self.project);
        session.caller = self.caller("chat", target);
        let (name, args) = graph_call;
        if let Err(e) = session.dispatch(name, &args) {
            return e.to_value();
        }
        if let Err(e) = self.write_edit(doc, old_text, new_text, full) {
            return json!({"error": {"rule": "write-failed", "message": e}});
        }
        let mut ops = vec![crate::store::Op::EditDocProse {
            doc: doc.to_string(),
            section: section.to_string(),
            old_text: old_text.to_string(),
            new_text: new_text.to_string(),
            text: full.to_string(),
        }];
        ops.extend(std::mem::take(&mut session.staged));
        let mut s = Store::load(&self.out);
        s.sync_docs(&parsed);
        s.absorb_doc_edit(doc, full);
        let item = WorkItem { task: "chat".into(), target: target.to_string(), dirty_sections: vec![], stale_anchors: vec![] };
        let report = s.apply(ops, &item, 1, 0);
        if !report.skipped.is_empty() {
            // The graph side skipped: put the prose back so neither moved.
            let _ = self.write_edit(doc, new_text, old_text, old_full);
            return json!({"error": {"rule": "commit-skipped", "message": report.skipped.join("; ")}});
        }
        json!({"committed": true, "applied": report.applied, "doc": doc,
               "note": "the prose and the graph moved together; no recompile is owed for this edit"})
    }

    fn revise_requirement(&self, args: &Value) -> Value {
        let rid = args["id"].as_str().unwrap_or_default();
        let new_text = args["newText"].as_str().unwrap_or_default().trim();
        if new_text.is_empty() {
            return json!({"error": {"rule": "missing-argument", "message": "newText is required: the prose sentence that replaces the old quote"}});
        }
        let store = Store::load(&self.out);
        let rid = store.resolve_id(rid).to_string();
        let Some(r) = store.graph.requirements.get(&rid) else {
            return json!({"error": {"rule": "unknown-id", "message": format!("unknown requirement `{}`", rid)}});
        };
        let (doc, section, old_quote) = (r.source.doc.clone(), r.source.section.clone(), r.source.quote.clone());
        let ears = args["ears"].as_str().unwrap_or(&r.ears).to_string();
        let path = self.project.root.join(&doc);
        let old_full = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", doc, e)}}),
        };
        let Some((b, e)) = crate::md::locate_bytes(&old_full, &old_quote) else {
            return json!({"error": {"rule": "stale-anchor", "message": format!(
                "the requirement's quote no longer locates in {}; compile first, then revise", doc)}});
        };
        let full = format!("{}{}{}", &old_full[..b], new_text, &old_full[e..]);
        self.dual_commit(
            &doc,
            &section,
            &old_quote,
            new_text,
            &full,
            &old_full,
            ("update_requirement", json!({"id": rid, "ears": ears, "section": format!("{}#{}", doc, section), "quote": new_text})),
            &rid,
        )
    }

    fn add_requirement(&self, args: &Value) -> Value {
        let doc = args["doc"].as_str().unwrap_or_default().to_string();
        let section = args["section"].as_str().unwrap_or_default().trim_start_matches(&format!("{}#", doc)).to_string();
        let text = args["text"].as_str().unwrap_or_default().trim().to_string();
        let ears = args["ears"].as_str().unwrap_or_default();
        if doc.is_empty() || section.is_empty() || text.is_empty() || ears.is_empty() {
            return json!({"error": {"rule": "missing-argument", "message": "doc, section, text, ears, and entities are required"}});
        }
        let store = Store::load(&self.out);
        let Some(sec_raw) = store.docs.get(&doc).and_then(|d| d.sections.get(&section)).map(|x| x.raw.clone()) else {
            return json!({"error": {"rule": "unknown-section", "message": format!("no section `{}#{}` in the graph; compile first", doc, section)}});
        };
        let path = self.project.root.join(&doc);
        let old_full = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", doc, e)}}),
        };
        // The insertion point: after the located quote, or at the section's end.
        let at = match args["afterQuote"].as_str() {
            Some(q) => match crate::md::locate_bytes(&old_full, q) {
                Some((_, e)) => e,
                None => return json!({"error": {"rule": "stale-anchor", "message": "afterQuote does not locate in the document"}}),
            },
            None => {
                let Some((b, _)) = crate::md::locate_bytes(&old_full, sec_raw.trim()) else {
                    return json!({"error": {"rule": "stale-anchor", "message": format!(
                        "section `{}` drifted from the document on disk; compile first", section)}});
                };
                b + sec_raw.trim().len()
            }
        };
        let full = format!("{}\n\n{}{}", &old_full[..at].trim_end_matches(['\n']), text, &old_full[at..]);
        let entities: Vec<String> = args["entities"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        self.dual_commit(
            &doc,
            &section,
            "",
            &text,
            &full,
            &old_full,
            ("upsert_requirement", json!({"ears": ears, "entities": entities, "section": format!("{}#{}", doc, section), "quote": text})),
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
        let (doc, section, old_quote) = (r.source.doc.clone(), r.source.section.clone(), r.source.quote.clone());
        let path = self.project.root.join(&doc);
        let old_full = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", doc, e)}}),
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
        self.dual_commit(
            &doc,
            &section,
            &old_quote,
            "",
            &full,
            &old_full,
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
        if let Err(e) = crate::cli::init_scaffold(root) {
            return json!({"error": {"rule": "write-failed", "message": e}});
        }
        json!({"initialized": root.display().to_string(), "next": "edit docs/README.md, then compile"})
    }

    fn update_project_settings(&self, args: &Value) -> Value {
        const KEYS: [&str; 8] = [
            "workflow.compile", "workflow.generate", "workflow.worker", "acp.agent",
            "gen.deliverable", "gen.worker", "llm.model", "llm.base_url",
        ];
        let Some(settings) = args["settings"].as_object() else {
            return json!({"error": {"rule": "missing-argument", "message": "settings is required: a map of key to value"}});
        };
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
fn toml_set(text: &str, section: &str, key: &str, value: &str) -> String {
    let header = format!("[{}]", section);
    let rendered = format!("{} = \"{}\"", key, value.replace('\\', "\\\\").replace('"', "\\\""));
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
fn sink_write(sink: &str, path: &std::path::Path, old_text: &str, new_text: &str, full: &str) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(sink).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    let line = json!({"path": path.display().to_string(), "oldText": old_text, "newText": new_text, "content": full});
    writeln!(stream, "{}", line).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(reply.trim()).map_err(|e| e.to_string())?;
    if v["ok"].as_bool().unwrap_or(false) {
        Ok(())
    } else {
        Err(v["error"].as_str().unwrap_or("sink refused the edit").to_string())
    }
}

fn text_result(v: Value, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": v.to_string()}],
        "isError": is_error
    })
}
