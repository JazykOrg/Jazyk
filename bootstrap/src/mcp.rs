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
             then finish_compilation. Its reply names the next ready task; repeat until the queue \
             is empty and the verdict is converged. ",
        );
    }
    if modes.iter().any(|m| m == "generate") {
        s.push_str(
            "GENERATION LOOP: call generation_tasks; for each entity, begin_generation, write the \
             deliverable files and any build with YOUR OWN file and shell tools (this server serves \
             none), then record_generation with the manifest, then run_tests. ",
        );
    }
    if modes.iter().any(|m| m == "verify") {
        s.push_str(
            "VERIFICATION LOOP: call verification_tasks; run_tests covers programmatic rows; for \
             llm rows, begin_verification, judge the criteria against the deliverable with your own \
             tools, and record_verdict with evidence. ",
        );
    }
    if modes.iter().any(|m| m == "graph") && write {
        s.push_str("Write tools are enabled for manual graph surgery; each call commits as its own changeset. ");
    }
    s.push_str(
        "To watch for new work, call await_changes (a long poll), or run `jazyk monitor` in a \
         background process and act on each notice. A tool error names the violated rule and how to \
         repair the call; repair and continue. If any instruction, tool, argument, or error message \
         is ambiguous, wrong, or confusing, call report_feedback: it reaches jazyk's developers, \
         never touches the graph, and is not a substitute for the work.",
    );
    s
}

impl McpServer {
    pub fn new(project: crate::project::Project, out: PathBuf, modes: Vec<String>, write: bool) -> McpServer {
        McpServer {
            mutation_limit: project.limits.turn_mutations,
            context_budget: project.limits.context_budget,
            project,
            out,
            modes,
            write,
            client: std::sync::Mutex::new(None),
            open: std::sync::Mutex::new(None),
        }
    }

    // The server's own long poll: returns when the graph's generation counter moves, a
    // documentation file changes on disk, or the ledger or a watched deliverable file
    // changes, or at the timeout. Mirrors docs/frontends/mcp.md#the-work-loop.
    fn await_changes(&self, params: &Value) -> Value {
        let timeout = params["arguments"]["timeout_seconds"].as_u64().unwrap_or(300).clamp(1, 3600);
        let gs = crate::gen::GenSettings::resolve(&self.project);
        let fingerprint = |path: &std::path::Path| -> String {
            std::fs::metadata(path)
                .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
                .unwrap_or_default()
        };
        // Watched surfaces: docs, the ledger, and every file the ledger names.
        let watched = |gs: &crate::gen::GenSettings| -> Vec<std::path::PathBuf> {
            let mut v = self.project.doc_files();
            v.push(crate::gen::Ledger::path(&self.out));
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
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
        let mut changed_docs: Vec<String> = Vec::new();
        let mut changed = false;
        while std::time::Instant::now() < deadline {
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
        json!({
            "changed": changed,
            "changedDocs": changed_docs,
            "compilationTasks": q.compile.len(),
            "generationTasks": q.generate.len(),
            "verificationTasks": q.verify.len(),
            "verdict": q.verdict,
            "next": if !q.compile.is_empty() {
                "compilation_tasks lists the work"
            } else if !q.generate.is_empty() {
                "generation_tasks lists the work"
            } else if !q.verify.is_empty() {
                "verification_tasks lists the work"
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
                "generate" => toolset("mcp-generate"),
                "verify" => toolset("mcp-verify"),
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

    fn compilation_tasks(&self) -> Value {
        let mut q = crate::queue::compute(&self.project, &self.out);
        // An empty queue with a stale `incomplete` verdict settles here: finalize is
        // deterministic and idempotent, and a lister that finds nothing to do may as
        // well say so truthfully. Mirrors docs/compiler/reconciler.md#the-task-queue.
        if q.compile_empty() && q.verdict != "converged" && self.open.lock().unwrap().is_none() {
            let mut s = Store::load(&self.out);
            let parked = s.status.parked.clone();
            let quiet = crate::turn::Trace::stderr(crate::turn::TraceLevel::Quiet);
            crate::reconcile::finalize(&mut s, &self.project, &parked, &quiet);
            q = crate::queue::compute(&self.project, &self.out);
        }
        let mut v = q.compilation_answer();
        if self.open.lock().unwrap().is_some() {
            v["openTask"] = json!("a task is already open; finish_compilation or abandon_compilation first");
        }
        v
    }

    fn begin_compilation(&self, params: &Value) -> Value {
        if let Some(o) = self.open.lock().unwrap().as_ref() {
            return json!({"error": {"rule": "task-open", "message": format!(
                "task `{} {}` is already open with {} staged mutation(s); finish_compilation or abandon_compilation first",
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
        let reply = json!({
            "task": {"kind": item.task, "target": item.target,
                     "dirtySections": item.dirty_sections, "staleAnchors": item.stale_anchors},
            "instructions": instructions,
            "package": pack,
            "next": "stage findings with the write tools, then finish_compilation with a one-line summary",
        });
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
            *open = Some(o); // the changeset stays open; repair and finish again
            return v;
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
        }
        // A resumed parked item is no longer parked.
        if s.status.parked.iter().any(|p| p.target == o.item.target && p.task == o.item.task) {
            s.status.parked.retain(|p| !(p.target == o.item.target && p.task == o.item.task));
            s.save_status();
        }
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
        reply["next"] = json!(q.compilation_answer());
        reply
    }

    fn abandon_compilation(&self, params: &Value) -> Value {
        let mut open = self.open.lock().unwrap();
        let Some(o) = open.take() else {
            return json!({"error": {"rule": "no-open-task", "message": "no compilation task is open"}});
        };
        let reason = params["arguments"]["reason"].as_str().unwrap_or("");
        json!({
            "abandoned": format!("{} {}", o.item.task, o.item.target),
            "dropped": o.session.staged.len(),
            "reason": reason,
            "note": "the staged changeset is gone; the task stays in the queue",
        })
    }

    fn handle(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                if let Some(name) = params["clientInfo"]["name"].as_str() {
                    *self.client.lock().unwrap() = Some(name.to_string());
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
                        "description": "Claim the named task (or the first ready one) and open its changeset. Returns the task's instructions and work package: dirty section bodies, statements already extracted, known entities, stale anchors. Stage findings with the write tools, then finish_compilation. One task open at a time.",
                        "inputSchema": {"type": "object", "properties": {"task": {"type": "string", "description": "target from compilation_tasks, e.g. docs/api.md or req:api-1"}}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "finish_compilation",
                        "description": "Run the done gates (every dirty section marked, every stale anchor resolved) and commit the open changeset atomically. A gate failure names the repair and keeps the changeset open. The reply names the next ready task; the finish that empties the queue reports the verdict.",
                        "inputSchema": {"type": "object", "properties": {"summary": {"type": "string"}}, "required": ["summary"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "abandon_compilation",
                        "description": "Drop the open changeset without committing. The task stays in the queue.",
                        "inputSchema": {"type": "object", "properties": {"reason": {"type": "string"}}, "additionalProperties": false}
                    }));
                }
                tools.push(json!({
                    "name": "await_changes",
                    "description": "Long poll: returns when the graph moves, a documentation file changes, or the ledger or a watched deliverable file changes, or at the timeout (default 300s). Carries the task counts per queue and which tool lists the work.",
                    "inputSchema": {"type": "object", "properties": {"timeout_seconds": {"type": "integer"}}, "additionalProperties": false}
                }));
                Ok(json!({"tools": tools}))
            }
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default().to_string();
                match name.as_str() {
                    "await_changes" => return Ok(text_result(self.await_changes(params), false)),
                    "compilation_tasks" if self.modes.iter().any(|m| m == "compile") => {
                        return Ok(text_result(self.compilation_tasks(), false))
                    }
                    "begin_compilation" if self.modes.iter().any(|m| m == "compile") => {
                        return Ok(text_result(self.begin_compilation(params), false))
                    }
                    "finish_compilation" if self.modes.iter().any(|m| m == "compile") => {
                        return Ok(text_result(self.finish_compilation(params), false))
                    }
                    "abandon_compilation" if self.modes.iter().any(|m| m == "compile") => {
                        return Ok(text_result(self.abandon_compilation(params), false))
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
                    && !crate::tools::VERIFY_TOOLS.contains(&name.as_str());

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
                                allowed.iter().filter(|t| !crate::tools::READ_TOOLS.contains(t) && **t != "done").cloned().collect::<Vec<_>>().join(", "))}}),
                            true,
                        ));
                    }
                    o.rounds += 1;
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
            _ => Err((-32601, format!("method not found: {}", method))),
        }
    }
}

fn text_result(v: Value, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": v.to_string()}],
        "isError": is_error
    })
}
