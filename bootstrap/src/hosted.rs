//! Filesystem-free native compiler embedding. See docs/compiler/hosted-runtime.md.
use crate::session::{Trace, TraceLevel};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub type Snapshot = BTreeMap<String, Vec<u8>>;
pub type ModelTransport = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;
pub type ModelStreamTransport = Arc<dyn Fn(&str, &mut dyn FnMut(&str, &str)) -> Result<String, String> + Send + Sync>;
pub type InputSource = Arc<dyn Fn(&str) -> Vec<Value> + Send + Sync>;
pub type InputClose = Arc<dyn Fn(&str) -> bool + Send + Sync>;
pub use crate::session::TraceSink;
pub type RuntimeTransport = Arc<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;
pub type EventSink = Arc<dyn Fn(&Value) + Send + Sync>;
pub type ModelAsyncTransport = Arc<
    dyn Fn(
            String,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
>;
pub type ModelDeltaSink = Arc<dyn Fn(&str, &str) + Send + Sync>;
pub type ModelAsyncStreamTransport = Arc<
    dyn Fn(String, ModelDeltaSink)
        -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>
        + Send + Sync,
>;
pub type CheckpointSink = Arc<
    dyn Fn(
            Value,
            Snapshot,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64, String>> + Send>>
        + Send
        + Sync,
>;


/// Text projections for authored revisions and delivery, not a verification verdict.
#[derive(Debug, serde::Serialize)]
pub struct Projection {
    pub source: BTreeMap<String, String>,
    pub files: BTreeMap<String, String>,
    pub criteria: BTreeMap<String, Value>,
    pub graph_generation: u64,
}

/// The host must enforce a deadline and cancellation within its model callback.
#[derive(Clone, Default)]
pub struct Execution {
    pub model: String,
    pub transport: Option<ModelTransport>,
    pub transport_stream: Option<ModelStreamTransport>,
    pub transport_async: Option<ModelAsyncTransport>,
    pub transport_async_stream: Option<ModelAsyncStreamTransport>,
    pub resume: Option<Value>,
    pub replay_events: Vec<Value>,
    pub checkpoint: Option<CheckpointSink>,
    pub suspend: Arc<AtomicBool>,
    pub inputs: Option<InputSource>,
    pub input_close: Option<InputClose>,
    pub runtime: Option<RuntimeTransport>,
    pub trace: Option<TraceSink>,
    pub events: Option<EventSink>,
    pub cancel: Arc<AtomicBool>,
    pub pause_reason: Arc<std::sync::Mutex<Option<String>>>,
    /// The host's global agent context and remote MCP servers for conversations
    /// (docs/frontends/agent-context.md#hosted-context).
    pub agent_context: Option<Arc<crate::agent_context::HostContext>>,
}

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}
impl Reply {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            content_type: "application/json".into(),
            body: body.to_string().into_bytes(),
        }
    }
}

pub struct Workspace {
    files: Snapshot,
}
impl Workspace {
    pub fn new(files: Snapshot) -> Result<Self, String> {
        for name in files.keys() {
            if name.is_empty()
                || name.contains(['\\', '\0', ':'])
                || name
                    .split('/')
                    .any(|c| c.is_empty() || c == "." || c == "..")
            {
                return Err(format!("invalid workspace path: {name}"));
            }
            // A flat snapshot cannot represent a file and a directory at one path.
            let mut parent = std::path::Path::new(name).parent();
            while let Some(p) = parent {
                if p.to_str().is_some_and(|s| files.contains_key(s)) {
                    return Err(format!("file conflicts with directory: {}", p.display()));
                }
                parent = p.parent();
            }
        }
        Ok(Self { files })
    }

    /// Apply retained task outputs through the same isolated workspace filesystem.
    pub fn apply_task_result(&mut self, task: &crate::task_runtime::Task, input: &std::collections::BTreeMap<String,String>, result: &Value, id: &str) -> Result<Value,String> {
        let fs = crate::vfs::Scope::enter(&self.files);
        let value = crate::task_runtime::apply_result(&crate::portable::root(),task,input,result,id)?;
        self.files = fs.snapshot();
        Ok(value)
    }

    pub fn snapshot(&self) -> Snapshot {
        self.files.clone()
    }

    /// Ensure the jazyk section of AGENTS.md exists and is current, along with
    /// the CLAUDE.md and GEMINI.md imports. Answers one line per change; warnings
    /// start with `warning: `. Mirrors docs/compiler/hosted-runtime.md.
    pub fn sync_agents_md(&mut self, _create: bool) -> Vec<String> {
        let fs = crate::vfs::Scope::enter(&self.files);
        let outcome = crate::agents_md::sync(&crate::portable::root());
        self.files = fs.snapshot();
        outcome.changed.into_iter().chain(outcome.warnings.into_iter().map(|w| format!("warning: {w}"))).collect()
    }

    /// The MCP servers the project's files propose, for the host to list and an
    /// administrator to enable. Variables stay as written. `hosted` says whether the
    /// hosted profile can use the entry at all (HTTPS only); `hash` changes whenever
    /// the entry does. Mirrors docs/frontends/agent-context.md#hosted-context.
    pub fn agent_mcp_proposals(&self) -> Vec<Value> {
        let _fs = crate::vfs::Scope::enter(&self.files);
        let keep = |name: &str| Some(format!("${{{name}}}"));
        let (servers, warnings) = crate::agent_context::servers(&crate::portable::root(), None, &keep);
        let mut out: Vec<Value> = servers.into_iter().map(|s| {
            let (kind, url) = match &s.transport {
                crate::agent_context::Transport::Stdio { .. } => ("stdio", None),
                crate::agent_context::Transport::Http { url, .. } => ("http", Some(url.clone())),
            };
            json!({"name": s.name, "source": s.source, "transport": kind, "url": url,
                   "hosted": url.as_deref().is_some_and(|u| u.starts_with("https://")), "hash": s.hash})
        }).collect();
        out.extend(warnings.into_iter().map(|w| json!({"warning": w})));
        out
    }

    /// Finalize Stop against a protected checkpoint without running a harness.
    /// Only committed VFS bytes exist here; staged MCP operations stay in the
    /// capsule and are discarded by the host after its fenced publication.
    pub fn stop_recovery(&mut self, capsule: &Value, events: &[Value]) -> Result<(), String> {
        crate::session_recovery::validate(capsule)?;
        let fs = crate::vfs::Scope::enter(&self.files);
        let _state = crate::scoped_state::Scope::enter();
        let _execution = ExecutionScope::enter(Execution::default());
        let _recovery = crate::session_recovery::Scope::enter(None);
        let conversations = crate::conversation::Scope::enter()?;
        crate::session_recovery::stop_owned(capsule)?;
        crate::conversation::replay_updates(events)?;
        crate::conversation::stop_recovered();
        conversations.save()?;
        self.files = fs.snapshot();
        Ok(())
    }

    /// Preserve authored membership explicitly. Merely writing a generated file
    /// does not make it source or a delivery candidate.
    pub fn projection(&self, authored_paths: &[String]) -> Result<Projection, String> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _fs = crate::vfs::Scope::enter(&self.files);
            project_projection(&self.files, authored_paths)
        }))
        .unwrap_or_else(|_| Err("hosted projection panicked; workspace was unchanged".into()))
    }

    /// Synchronous. The caller owns the project lease and atomic persistence.
    /// Ordinary compiler failures retain committed state. Unwinding rolls back.
    pub fn request(
        &mut self,
        method: &str,
        path: &str,
        body: &str,
        execution: &Execution,
    ) -> Result<Reply, String> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let fs = crate::vfs::Scope::enter(&self.files);
            let _state = crate::scoped_state::Scope::enter();
            let _execution = ExecutionScope::enter(execution.clone());
            let _recovery = crate::session_recovery::Scope::enter(execution.resume.as_ref());
            drop_graph_snapshots();
            let conversations = crate::conversation::Scope::enter()?;
            if execution.resume.is_some() {
                crate::conversation::replay_updates(&execution.replay_events)?;
            }
            let reply = dispatch(method, path, body)?;
            if method != "GET" && (path.contains("/chat/") || path.ends_with("/jobs") || path.ends_with("/compile")) {
                conversations.save()?;
            }
            Ok::<_, String>((reply, fs.snapshot()))
        }));
        match result {
            Ok(Ok((reply, files))) => {
                self.files = files;
                Ok(reply)
            }
            Ok(Err(error)) => Err(error),
            Err(_) => Err("hosted compiler panicked; request snapshot was rolled back".into()),
        }
    }
}

// A hosted workspace keeps no replay snapshots; remove what an earlier version left.
// Mirrors docs/compiler/hosted-runtime.md#workspace-snapshots.
fn drop_graph_snapshots() {
    let Ok(w) = crate::document_sets::Workspace::load(&root()) else { return };
    for set in &w.sets {
        let Ok(p) = w.resolve(Some(&set.id)) else { continue };
        let dir = p.out.join("snapshots");
        if crate::vfs::is_dir(&dir) {
            crate::vfs::remove_dir_all(&dir).ok();
        }
    }
}

fn project_projection(files: &Snapshot, authored: &[String]) -> Result<Projection, String> {
    let w = crate::document_sets::Workspace::load(&root()).map_err(|e| e.1)?;
    let mut combined = Projection {
        source: Default::default(),
        files: Default::default(),
        criteria: Default::default(),
        graph_generation: 0,
    };
    let contexts: Vec<_> = if w.sets.is_empty() {
        vec![w.files_project()]
    } else {
        w.sets
            .iter()
            .map(|s| w.resolve(Some(&s.id)).unwrap())
            .collect()
    };
    for p in contexts {
        let projected = set_projection(files, authored, &p)?;
        combined.source.extend(projected.source);
        combined.files.extend(projected.files);
        for (id, v) in projected.criteria {
            let id = p
                .set_id
                .as_deref()
                .map(|set| crate::webapi::sets::qualify(&id, set))
                .unwrap_or(id);
            combined.criteria.insert(
                id,
                p.set_id
                    .as_deref()
                    .map(|set| crate::webapi::sets::qualify_value(v.clone(), set))
                    .unwrap_or(v),
            );
        }
        combined.graph_generation += projected.graph_generation;
    }
    Ok(combined)
}
fn set_projection(
    files: &Snapshot,
    authored: &[String],
    p: &crate::project::Project,
) -> Result<Projection, String> {
    use crate::vfs::VPathExt;
    let store = crate::store::Store::load(&p.out);
    let gs = crate::gen::GenSettings::resolve(p);
    let ledger = crate::gen::Ledger::load(&p.out);
    // Resolve through the VFS so redirected projects and dot components use the
    // same paths as compilation. No absent or out-of-workspace file can enter.
    let relative = |path: &Path| -> Option<String> {
        if !path.vis_file() {
            return None;
        }
        let path = crate::vfs::canonicalize(path).ok()?;
        Some(path.strip_prefix(root()).ok()?.to_str()?.replace('\\', "/"))
    };
    let internal = |name: &str| {
        !matches!(name, crate::document_sets::CONFIG | ".jazyk/.gitignore")
            && (name
                .split('/')
                .any(|c| c == ".git" || c.starts_with(".jazyk") || c.starts_with("jazyk-out"))
                || root().join(name).starts_with(&p.out))
    };
    let mut owned = BTreeSet::new();
    let mut accepted = BTreeSet::new();
    for path in ledger
        .support
        .iter()
        .chain(ledger.entities.values().flat_map(|e| e.files.iter()))
        .chain(ledger.requirements.values().flat_map(|r| r.files.iter()))
    {
        if let Some(name) = relative(&gs.deliverable.join(path)) {
            owned.insert(name.clone());
            accepted.insert(name);
        }
    }
    for row in ledger.requirements.values() {
        for test in std::iter::once(&row.test).chain(row.tests.values().map(|t| &t.test)) {
            if let Some(name) = relative(&crate::gen::artifact_path(&p.out, &gs, test)) {
                owned.insert(name.clone());
                if test.kind == "programmatic" {
                    accepted.insert(name);
                }
            }
        }
    }
    for path in crate::gen::build_outputs(&ledger) {
        if let Some(name) = relative(&gs.deliverable.join(path)) {
            owned.insert(name.clone());
            accepted.remove(&name);
        }
    }
    let mut source_paths = BTreeSet::new();
    for name in authored {
        if name.is_empty()
            || name.contains(['\\', '\0', ':'])
            || name
                .split('/')
                .any(|c| c.is_empty() || c == "." || c == "..")
        {
            return Err(format!("invalid authored path: {name}"));
        }
        if files.contains_key(name) {
            source_paths.insert(name.clone());
        }
    }
    for path in p.doc_files().into_iter().chain([
        root().join(crate::document_sets::CONFIG),
        p.root.join(crate::document_sets::CONFIG),
    ]) {
        if let Some(name) = relative(&path) {
            source_paths.insert(name);
        }
    }
    source_paths.retain(|name| !internal(name) && !owned.contains(name));
    let read_text = |names: BTreeSet<String>| -> Result<BTreeMap<String, String>, String> {
        names
            .into_iter()
            .filter(|name| !internal(name))
            .map(|name| {
                let bytes = files
                    .get(&name)
                    .ok_or_else(|| format!("missing projected file: {name}"))?;
                let content = std::str::from_utf8(bytes)
                    .map_err(|_| format!("projected file is not UTF-8 text: {name}"))?;
                Ok((name, content.to_owned()))
            })
            .collect()
    };
    let source = read_text(source_paths)?;
    let mut candidate = source.clone();
    candidate.extend(read_text(accepted)?);
    // The set's committed compiler state rides with the delivery, so the receiving
    // repository compiles incrementally from it. Local state stays in the snapshot.
    // Mirrors docs/compiler/hosted-runtime.md#source-and-delivery-projections.
    let out_prefix = p
        .out
        .strip_prefix(root())
        .ok()
        .and_then(|o| o.to_str())
        .map(|o| format!("{}/", o.replace('\\', "/")));
    if let Some(text) = files
        .get(".jazyk/.gitignore")
        .and_then(|b| std::str::from_utf8(b).ok())
    {
        candidate.insert(".jazyk/.gitignore".into(), text.to_owned());
    }
    if let Some(prefix) = out_prefix {
        for (name, bytes) in files.range(prefix.clone()..) {
            if !name.starts_with(&prefix) {
                break;
            }
            if crate::document_sets::committed(name) {
                let content = std::str::from_utf8(bytes)
                    .map_err(|_| format!("projected file is not UTF-8 text: {name}"))?;
                candidate.insert(name.clone(), content.to_owned());
            }
        }
    }
    let criteria = store
        .graph
        .criteria
        .iter()
        .map(|(id, criterion)| {
            serde_json::to_value(criterion)
                .map(|value| (id.clone(), value))
                .map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    Ok(Projection {
        source,
        files: candidate,
        criteria,
        graph_generation: store.status.generation,
    })
}

thread_local! { static EXECUTION: RefCell<Option<Execution>> = const { RefCell::new(None) }; }
thread_local! { static CHECKPOINT_SEQUENCE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
struct ExecutionScope(Option<Execution>, u64);
#[cfg(test)]
pub(crate) fn test_execution_scope(execution: Execution) -> impl Drop {
    ExecutionScope::enter(execution)
}
impl ExecutionScope {
    fn enter(execution: Execution) -> Self {
        let previous = CHECKPOINT_SEQUENCE.with(|s| {
            s.replace(
                execution
                    .resume
                    .as_ref()
                    .and_then(|r| r["sequence"].as_u64())
                    .unwrap_or(0),
            )
        });
        Self(EXECUTION.with(|e| e.replace(Some(execution))), previous)
    }
}
impl Drop for ExecutionScope {
    fn drop(&mut self) {
        EXECUTION.with(|e| e.replace(self.0.take()));
        CHECKPOINT_SEQUENCE.with(|s| s.set(self.1));
    }
}
fn execution() -> Execution {
    EXECUTION.with(|e| e.borrow().clone().expect("hosted execution context"))
}

pub(crate) fn suspension_requested() -> bool {
    EXECUTION.with(|e| {
        e.borrow()
            .as_ref()
            .is_some_and(|e| e.suspend.load(Ordering::Acquire))
    })
}

pub(crate) async fn checkpoint(mut capsule: Value) -> Result<u64, String> {
    let sink = EXECUTION.with(|e| e.borrow().as_ref().and_then(|e| e.checkpoint.clone()));
    let sequence = CHECKPOINT_SEQUENCE.with(|s| s.get());
    let Some(sink) = sink else {
        return Ok(sequence);
    };
    if !crate::vfs::is_scoped() {
        return Err("Checkpoint requires an owned hosted workspace.".into());
    }
    capsule["sequence"] = json!(sequence);
    let snapshot = crate::vfs::scoped_snapshot()?;
    let next = sink(capsule, snapshot).await?;
    if next <= sequence {
        return Err("Checkpoint sequence did not advance.".into());
    }
    CHECKPOINT_SEQUENCE.with(|s| s.set(next));
    Ok(next)
}

pub(crate) async fn transport_async(attempt_id: &str, body: &str, deltas: Option<ModelDeltaSink>) -> Result<String, String> {
    let e = execution();
    if e.cancel.load(Ordering::Acquire) {
        return Err("execution cancelled".into());
    }
    if e.suspend.load(Ordering::Acquire) {
        return Err("execution suspended".into());
    }
    let mut request: Value =
        serde_json::from_str(body).map_err(|e| format!("Invalid model request: {e}"))?;
    request["_jazyk"]["requestId"] = json!(attempt_id);
    request["_jazyk"]["checkpointSequence"] = json!(CHECKPOINT_SEQUENCE.with(|s| s.get()));
    if let (Some(stream), Some(deltas)) = (e.transport_async_stream, deltas) {
        return stream(request.to_string(), deltas).await;
    }
    let transport = e
        .transport_async
        .ok_or("Hosted async model transport is unavailable.")?;
    transport(request.to_string()).await
}

pub fn runtime(request: &Value) -> Result<Value, String> {
    let transport = EXECUTION.with(|e| e.borrow().as_ref().and_then(|e| e.runtime.clone()));
    transport.ok_or("Hosted task runtime is not configured")?(request)
}

pub fn root() -> PathBuf {
    PathBuf::from("/project")
}
pub fn host_context() -> Option<Arc<crate::agent_context::HostContext>> {
    EXECUTION.with(|e| e.borrow().as_ref().and_then(|e| e.agent_context.clone()))
}
pub fn model_name() -> String {
    execution().model
}
pub fn cancelled() -> bool {
    EXECUTION.with(|e| {
        e.borrow()
            .as_ref()
            .is_some_and(|e| e.cancel.load(Ordering::Relaxed))
    })
}
pub fn pause_reason() -> Option<String> {
    EXECUTION.with(|e| e.borrow().as_ref().and_then(|execution| execution.pause_reason.lock().unwrap().clone()))
}
pub fn trace() -> Trace {
    let e = execution();
    Trace::to_sink(
        TraceLevel::Normal,
        e.trace.unwrap_or_else(|| Arc::new(|_| {})),
        e.cancel,
    )
}
pub fn llm() -> crate::llm::Llm {
    crate::llm::Llm { api: "chat_completions".into(), reasoning_summary: None,
        base_url: "host-injected".into(),
        model: model_name(),
        api_key: String::new(),
        temperature: Some(0.0),
        trace: Some(trace()),
    }
}
pub fn emit_event(kind: &str, mut body: Value) {
    body["type"] = json!(kind);
    if let Some(sink) = execution().events {
        sink(&body);
    }
}
pub fn take_inputs(session: &str) -> Vec<Value> {
    EXECUTION.with(|e| e.borrow().as_ref().and_then(|e| e.inputs.clone()))
        .map(|source| source(session)).unwrap_or_default()
}
pub fn close_inputs(session: &str) -> bool {
    EXECUTION.with(|e| e.borrow().as_ref().and_then(|e| e.input_close.clone()))
        .map(|close| close(session)).unwrap_or(true)
}
fn dispatch(method: &str, path: &str, body: &str) -> Result<Reply, String> {
    let routed =
        match crate::webapi::sets::route(path, serde_json::from_str(body).unwrap_or(Value::Null)) {
            Ok(r) => r,
            Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
        };
    let (route, query) = routed.path.split_once('?').unwrap_or((&routed.path, ""));
    // A configuration that does not load, or a project with no document set, is a
    // state of the project with its own status, never a server failure.
    // Mirrors docs/compiler/document-sets.md#discovery-and-opening.
    let workspace = match crate::document_sets::Workspace::load(&root()) {
        Ok(w) => w,
        Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
    };
    let selected = || workspace.resolve(routed.selector.as_deref());
    let route = route.strip_prefix("/api").unwrap_or(route);
    if method == "GET" && matches!(route, "/out/file" | "/project/file" | "/project/raw" | "/deliverable/raw") {
        let rel = url::form_urlencoded::parse(query.as_bytes())
            .find(|(k, _)| k == "path")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        let p =
            if matches!(route, "/project/file" | "/project/raw" | "/deliverable/raw") && routed.selector.is_none() {
                workspace.files_project()
            } else {
                match selected() {
                    Ok(p) => p,
                    Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
                }
            };
        let result = match route {
            "/out/file" => crate::webapi::outfiles::out_file(&p.out, &rel),
            "/project/file" | "/project/raw" => crate::webapi::projfiles::project_file(&p.root, &rel),
            _ => crate::webapi::deliverable::deliverable_raw(
                &crate::gen::GenSettings::resolve(&p),
                &rel,
            ),
        };
        return Ok(match result {
            Ok(body) => Reply {
                status: 200,
                content_type: crate::webapi::outfiles::content_type(&rel).into(),
                body,
            },
            Err((code, msg)) => Reply::json(code, json!({"error":msg})),
        });
    }
    if method == "POST" && route == "/jobs" {
        let request = routed.body.clone();
        let _conversation = match crate::conversation::BuildScope::enter(&request) {
            Ok(scope) => scope,
            Err((status, error)) => return Ok(Reply::json(status, json!({"error":error}))),
        };
        let kind = request["kind"].as_str().unwrap_or("compile");
        if kind == "workflow" {
            let tasks_only = crate::workflow::parse(&request["steps"]).is_ok_and(|steps|
                steps.iter().all(|s| matches!(s, crate::workflow::Step::Task { .. })));
            let p = match if tasks_only && routed.selector.is_none() { Ok(workspace.files_project()) } else { selected() } {
                Ok(p) => p, Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
            };
            let steps = match crate::workflow::parse(&request["steps"]) {
                Ok(steps) => steps, Err(e) => return Ok(Reply::json(400, json!({"error":e}))),
            };
            let result = crate::workflow::run(&p, &llm(), &steps, &trace())?;
            return Ok(Reply::json(if result["state"] == "failed" {422} else {200},
                json!({"kind":"workflow","report":result})));
        }
        if kind == "verify" && request.get("testPaths").is_some() {
            let paths=match crate::verify::artifacts::parse_paths(&request) {
                Ok(Some(paths))=>paths,Ok(None)=>unreachable!(),Err(error)=>return Ok(Reply::json(400,json!({"error":error}))),
            };
            let result=crate::verify::artifacts::run_workspace(&root(),routed.selector.as_deref(),&paths,&trace(),|project| {
                Ok(Box::new(crate::inprocess::InProcessRunner::start(project,&llm(),&project.out)?))
            });
            return Ok(match result {
                Ok(report)=>Reply::json(200,json!({"kind":"verify","report":report})),
                Err(error)=>Reply::json(422,json!({"error":error})),
            });
        }
        if matches!(kind, "verify" | "audit" | "gen") {
            let p = match selected() {
                Ok(p) => p,
                Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
            };
            let store = crate::store::Store::load(&p.out);
            let gs = crate::gen::GenSettings::resolve(&p);
            let result = if kind == "gen" {
                use crate::runner::SessionRunner;
                let _build = crate::control::begin_internal_build(&p, &p.out, "generate")?;
                let runner = crate::inprocess::InProcessRunner::start(&p, &llm(), &p.out)?;
                runner.set_build_token(Some(_build.token().to_owned()));
                let entities = request["entities"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                crate::gen::run_all(
                    &store,
                    &runner,
                    &gs,
                    &entities,
                    request["force"].as_bool().unwrap_or(false),
                    &trace(),
                )
            } else if kind == "audit" {
                Ok(crate::verify::audit(&store, &gs))
            } else {
                let runner = crate::inprocess::InProcessRunner::start(&p, &llm(), &p.out)?;
                let targets = request["targets"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                crate::verify::run_all(
                    &store,
                    &runner,
                    &gs,
                    &targets,
                    request["testKind"].as_str(),
                    request["force"].as_bool().unwrap_or(false),
                    &trace(),
                )
            };
            return Ok(match result {
                Ok(report) => Reply::json(
                    200,
                    json!({"id":1,"kind":kind,"state":"finished","report":report}),
                ),
                Err(error) => Reply::json(422, json!({"error":error})),
            });
        }
    }
    if method == "GET" && route == "/capabilities" {
        return Ok(Reply::json(
            200,
            json!({"protocol":crate::protocol::EDITOR_PROTOCOL_VERSION,"backend":"hosted","projects":"list","docs":"write","events":"pushed","lsp":false,"jobs":["workflow","compile","gen","verify","audit"],"agent":"embedded","chatInputs":true,"conversationJobs":true,"testPaths":true,"model":if execution().transport.is_some(){"configured"}else{"none"},"modelName":model_name(),"rendering":"server","persistence":"memory","shell":false,"dev":false}),
        ));
    }
    let envelope: Value =
        serde_json::from_str(&crate::webapi::dispatch::request(method, path, body))
            .map_err(|e| e.to_string())?;
    Ok(Reply::json(
        envelope["status"].as_u64().unwrap_or(500) as u16,
        envelope["body"].clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    // A project carrying only a root jazyk.toml from before document sets has no set:
    // its files stay browsable, a document read and a job each answer 409
    // no-document-set, and nothing fails the request.
    // Mirrors docs/compiler/document-sets.md#discovery-and-opening.
    #[test]
    fn a_project_without_document_sets_answers_no_document_set_instead_of_failing() {
        let mut files = Snapshot::new();
        files.insert(
            "jazyk.toml".into(),
            b"[docs]\nglob = [\"docs/**/*.md\"]\n\n[roots]\nfiles = [\"docs/main.md\"]\n".to_vec(),
        );
        files.insert("docs/main.md".into(), b"# Main\n\nThe system shall respond.\n".to_vec());
        let mut w = Workspace::new(files).unwrap();
        let e = Execution::default();
        let doc = w.request_test("GET", "/api/docs/content?path=docs/main.md", "", &e).unwrap();
        assert_eq!(doc.status, 409, "{}", String::from_utf8_lossy(&doc.body));
        assert!(String::from_utf8_lossy(&doc.body).contains("no-document-set"));
        let job = w.request_test("POST", "/api/jobs", r#"{"kind":"audit"}"#, &e).unwrap();
        assert_eq!(job.status, 409, "{}", String::from_utf8_lossy(&job.body));
        assert!(String::from_utf8_lossy(&job.body).contains("Add one under Document sets"));
        let text = w.request_test("GET", "/api/project/text?path=docs/main.md", "", &e).unwrap();
        assert_eq!(text.status, 200);
        assert!(String::from_utf8_lossy(&text.body).contains("The system shall respond."));
        // The old paths come back as the first set's suggestion, and nothing is written.
        let sets = w.request_test("GET", "/api/sets", "", &e).unwrap();
        let v: Value = serde_json::from_slice(&sets.body).unwrap();
        assert_eq!(v["suggested"]["docs"], json!(["docs/**/*.md"]), "{}", v);
        assert_eq!(v["suggested"]["roots"], json!(["docs/main.md"]), "{}", v);
        assert!(!w.snapshot().contains_key(".jazyk/jazyk.toml"));
    }

    #[test]
    fn hosted_ledger_runner_sends_the_shared_batch_contract() {
        use crate::runner::SessionRunner;
        let w = workspace("Alpha");
        let _fs = crate::vfs::Scope::enter(&w.snapshot());
        let _state = crate::scoped_state::Scope::enter();
        let observed = Arc::new(AtomicBool::new(false));
        let saw = observed.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        let e = Execution {
            model: "ledger-fixture".into(),
            cancel,
            transport_async: Some(Arc::new(move |body| {
                let saw = saw.clone();
                let cancelled = cancelled.clone();
                Box::pin(async move {
                let request: Value = serde_json::from_str(&body).unwrap();
                let user = request["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|m| m["role"] == "user")
                    .map(|m| m["content"].as_str().unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(user.contains("g:generate:ent:alpha"));
                assert!(user.contains("begin_generation"));
                assert!(user.contains("record_generation"));
                saw.store(true, Ordering::SeqCst);
                cancelled.store(true, Ordering::SeqCst);
                Ok(model_reply("Stopped after inspecting the prompt."))
                })
            })),
            ..Default::default()
        };
        let _execution = ExecutionScope::enter(e);
        let p = crate::project::Project::load(&root());
        let runner = crate::inprocess::InProcessRunner::start(&p, &llm(), &p.out).unwrap();
        runner.run_item(
            &crate::runner::BatchRun::single(crate::model::Goal {
                id: "g:generate:ent:alpha".into(),
                kind: "generate".into(),
                target: "ent:alpha".into(),
                ..Default::default()
            }),
            &trace(),
        );
        assert!(observed.load(Ordering::SeqCst));
    }

    fn workspace(name: &str) -> Workspace {
        Workspace::new(BTreeMap::from([
            (
                ".jazyk/jazyk.toml".into(),
                b"version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = [\"docs/main.md\"]\nsource = []\n"
                    .to_vec(),
            ),
            (
                "docs/main.md".into(),
                format!("# {name}\n\nNavigation index.\n").into_bytes(),
            ),
        ]))
        .unwrap()
    }

    #[test]
    fn projection_preserves_authored_membership_and_records_only_ledger_output() {
        let mut w = workspace("Alpha");
        for (path, text) in [

            (
                ".jazyk/jazyk.toml",
                "version = 1\ngen = {}\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"spec/docs/**/*.md\"]\nroots = []\nsource = [\"spec/product/**\"]\n",
            ),
            ("spec/docs/main.md", "# Main\nAuthored prose.\n"),
            ("spec/docs/new.md", "# New\nNewly authored prose.\n"),
            ("notes.txt", "An existing human file, edited."),
            (".github/workflows/test.yml", "name: tests"),
            (".git/config", "private git metadata"),
            ("spec/.jazyk.bak/status.yaml", "private backup"),
            ("spec/product/main.py", "print('accepted')"),
            ("spec/product/package.json", "{}"),
            ("spec/product/test.py", "assert True"),
            ("spec/product/test_extra.py", "assert 1 == 1"),
            ("spec/product/draft.py", "unrecorded output"),
            ("spec/product/build.zip", "built output"),
            (".jazyk/ds_main/gen/judge.md", "LLM judgment criteria"),
            (".jazyk/.gitignore", crate::document_sets::IGNORE),
            (".jazyk/ds_main/journal/g1.yaml", "build: g1\n"),
            (".jazyk/ds_main/diagrams/class/alpha.svg", "<svg/>"),
            (".jazyk/ds_main/graph/relationships/rel~a%7Eb.yaml", "members: []\n"),
            (".jazyk/ds_main/trace/run.jsonl", "{}"),
            (".jazyk/ds_main/control.yaml", "compile: auto\n"),
        ] {
            w.files.insert(path.into(), text.as_bytes().to_vec());
        }
        {
            let fs = crate::vfs::Scope::enter(&w.files);
            let project = crate::project::Project::load(&root());
            let mut store = crate::store::Store::load(&project.out);
            store.status.generation = 42;
            store.graph.criteria.insert(
                "crit:stable".into(),
                crate::model::Criterion {
                    requirement: "req:stable".into(),
                    statement: "One returns one.".into(),
                    ..Default::default()
                },
            );
            store.save();
            let programmatic = |artifact| json!({"kind":"programmatic", "label":"test", "artifact":artifact, "name":"test", "run":"python test.py"});
            let ledger: crate::gen::Ledger = serde_json::from_value(json!({
                "support":["spec/product/package.json"],
                "entities":{"alpha":{"factHash":"recorded", "requirements":[], "files":["spec/product/main.py","spec/product/build.zip","spec/product/missing.py"]}},
                "build":{"run":"python build.py","produces":["spec/product/build.zip"]},
                "requirements":{"req:stable":{
                    "entity":"ent:alpha","files":["spec/product/main.py"],
                    "test": programmatic("spec/product/test.py"),
                    "tests": {
                        "crit:extra":{"test":programmatic("spec/product/test_extra.py"),"criterion_hash":"hash"},
                        "crit:llm":{"test":{"kind":"llm","label":"judge","artifact":"judge.md","name":"judge","run":""},"criterion_hash":"hash"}
                    },
                    "hashes":{"requirement":"","test":"","files":""}
                }}
            })).unwrap();
            ledger.save(&project.out).unwrap();
            w.files = fs.snapshot();
        }
        let before = w.snapshot();
        let authored = [
            "notes.txt",
            "deleted.txt",
            ".github/workflows/test.yml",
            ".git/config",
            "spec/.jazyk/status.yaml",
            "spec/.jazyk.bak/status.yaml",
            "spec/product/main.py",
            "spec/product/build.zip",
        ]
        .map(String::from);
        let projection = w.projection(&authored).unwrap();
        assert_eq!(projection.graph_generation, 42);
        assert_eq!(
            projection.criteria["crit:ds_main::stable"]["statement"],
            "One returns one."
        );
        assert_eq!(
            projection
                .source
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec![
                ".github/workflows/test.yml",
                ".jazyk/jazyk.toml",
                "notes.txt",
                "spec/docs/main.md",
                "spec/docs/new.md",
            ]
        );
        for path in ["main.py", "package.json", "test.py", "test_extra.py"] {
            assert!(projection
                .files
                .contains_key(&format!("spec/product/{path}")));
        }
        // The set's committed compiler state rides with the delivery; local state does
        // not (docs/compiler/hosted-runtime.md#source-and-delivery-projections).
        let state: Vec<&str> = projection
            .files
            .keys()
            .map(String::as_str)
            .filter(|p| p.starts_with(".jazyk/") && *p != ".jazyk/jazyk.toml")
            .collect();
        assert_eq!(
            state,
            vec![
                ".jazyk/.gitignore",
                ".jazyk/ds_main/gen/entities/alpha.yaml",
                ".jazyk/ds_main/gen/judge.md",
                ".jazyk/ds_main/gen/ledger.yaml",
                ".jazyk/ds_main/gen/requirements/req~stable.yaml",
                ".jazyk/ds_main/graph/criteria/crit~stable.yaml",
                ".jazyk/ds_main/status.yaml",
            ]
        );
        assert_eq!(projection.files.len(), projection.source.len() + 4 + state.len());
        assert!(!projection.source.keys().any(|p| p.starts_with(".jazyk/ds_main/")));
        assert_eq!(
            projection.source["notes.txt"],
            "An existing human file, edited."
        );
        assert_eq!(w.snapshot(), before);
        assert!(!crate::vfs::is_scoped());
        let other = workspace("Beta").projection(&[]).unwrap();
        assert_eq!(other.graph_generation, 0);
        assert!(other.criteria.is_empty());
    }

    #[test]
    fn projection_rejects_invalid_membership_and_selected_binary_without_mutation() {
        let mut w = workspace("Alpha");
        w.files.insert("photo.png".into(), vec![0, 255]);
        let before = w.snapshot();
        assert!(w
            .projection(&["photo.png".into()])
            .unwrap_err()
            .contains("not UTF-8"));
        for path in ["../escape", "/absolute", "docs//main.md", "a\\b", "a:b"] {
            assert!(w
                .projection(&[path.into()])
                .unwrap_err()
                .contains("invalid authored"));
        }
        assert!(!w.projection(&[]).unwrap().files.contains_key("photo.png"));
        assert_eq!(w.snapshot(), before);
        assert!(!crate::vfs::is_scoped());
    }

    impl Workspace {
        // Existing fixture providers stay synchronous; every production invocation
        // below still exercises the new owned asynchronous host interface.
        fn request_test(&mut self, method: &str, path: &str, body: &str, execution: &Execution) -> Result<Reply, String> {
            let mut execution = execution.clone();
            if execution.transport_async.is_none() {
                execution.transport_async = execution.transport.clone().map(|source| Arc::new(move |body: String| {
                    let source = source.clone();
                    Box::pin(async move { source(&body) }) as std::pin::Pin<Box<dyn std::future::Future<Output=Result<String,String>> + Send>>
                }) as ModelAsyncTransport);
            }
            if execution.transport_async_stream.is_none() {
                execution.transport_async_stream = execution.transport_stream.clone().map(|source| Arc::new(move |body: String, sink: ModelDeltaSink| {
                    let source = source.clone();
                    Box::pin(async move { source(&body, &mut |field, text| sink(field,text)) }) as std::pin::Pin<Box<dyn std::future::Future<Output=Result<String,String>> + Send>>
                }) as ModelAsyncStreamTransport);
            }
            self.request(method,path,body,&execution)
        }
    }

    fn value(w: &mut Workspace, method: &str, path: &str, body: Value, e: &Execution) -> Value {
        let reply = w.request_test(method, path, &body.to_string(), e).unwrap();
        assert!(
            reply.status < 400,
            "{}",
            String::from_utf8_lossy(&reply.body)
        );
        serde_json::from_slice(&reply.body).unwrap()
    }
    fn model_reply(content: &str) -> String {
        json!({"choices":[{"message":{"role":"assistant","content":content}}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}).to_string()
    }
    #[test]
    fn stop_discards_staged_batch_and_releases_only_saved_claims_after_partial_commit() {
        let mut workspace = workspace("Stop");
        workspace.files.insert("docs/committed.txt".into(), b"An earlier accepted edit.\n".to_vec());
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let model_calls = calls.clone();
        let model: ModelAsyncTransport = Arc::new(move |_| {
            model_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(json!({"choices":[{"message":{"role":"assistant","tool_calls":[
                {"id":"stage","type":"function","function":{"name":"set_coverage","arguments":json!({
                    "section":"docs/main.md#/stop","state":"non-normative","note":"Staged coverage, not committed."}).to_string()}}
            ]}}]}).to_string()) })
        });
        let saved = Arc::new(std::sync::Mutex::new(None::<(Value, Snapshot)>));
        let saved_sink = saved.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let sink: CheckpointSink = Arc::new(move |capsule, files| {
            let saved = saved_sink.clone(); let stop = stop.clone();
            Box::pin(async move {
                let after_tool = capsule["frames"].as_array().and_then(|v| v.last())
                    .is_some_and(|frame| frame["boundary"]["phase"] == "after_tool");
                if after_tool {
                    *saved.lock().unwrap() = Some((capsule, files));
                    stop.store(true, Ordering::SeqCst);
                    Err("checkpoint-failed: cancellation raced with durable save".into())
                } else { Ok(capsule["sequence"].as_u64().unwrap() + 1) }
            })
        });
        let execution = Execution { model:"fixture".into(), transport_async:Some(model), checkpoint:Some(sink), cancel, ..Default::default() };
        let reply = workspace.request("POST", "/api/jobs", &json!({"kind":"compile"}).to_string(), &execution).unwrap();
        assert!(reply.status < 400, "{}", String::from_utf8_lossy(&reply.body));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "{}", String::from_utf8_lossy(&reply.body));
        assert!(!workspace.snapshot().keys().any(|path| path.contains("/leases/")), "active cancellation retained a claim");
        let (capsule, files) = saved.lock().unwrap().clone().expect("staged tool checkpoint");
        assert!(capsule["frames"][0]["mcp"]["open"]["session"]["staged"].as_array().is_some_and(|v| !v.is_empty()), "{capsule}");
        assert!(files.keys().any(|path| path.ends_with("/leases/build.yaml")));
        assert!(files.keys().any(|path| path.contains("/leases/b")));
        assert!(capsule["claims"].as_array().unwrap().len() >= 2);
        let mut recovered = Workspace::new(files.clone()).unwrap();
        recovered.stop_recovery(&capsule, &[]).unwrap();
        let stopped = recovered.snapshot();
        assert!(!stopped.keys().any(|path| path.contains("/leases/")));
        assert_eq!(stopped["docs/committed.txt"], b"An earlier accepted edit.\n");
        for (path, bytes) in &files {
            if !path.contains("/leases/") && path != ".jazyk/conversations.json" {
                assert_eq!(stopped.get(path), Some(bytes), "Stop changed committed bytes: {path}");
            }
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "Stop invoked the model");
        let _fs = crate::vfs::Scope::enter(&stopped);
        let project = crate::document_sets::Workspace::load(&root()).unwrap().resolve(Some("ds_main")).unwrap();
        let guard = crate::control::begin_internal_build(&project, &project.out, "compile").expect("fresh build must not inherit the stopped batch lease");
        drop(guard);
    }

    #[test]
    fn actual_compiler_resume_hands_over_saved_claims_and_refuses_foreign_replacements() {
        let mut workspace = workspace("Alpha");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut execution = navigation_model("Alpha", calls.clone(), None);
        let saved = Arc::new(std::sync::Mutex::new(None::<(Value, Snapshot)>));
        let save = saved.clone();
        let suspend = execution.suspend.clone();
        execution.checkpoint = Some(Arc::new(move |mut capsule, files| {
            let save = save.clone(); let suspend = suspend.clone();
            Box::pin(async move {
                let next = capsule["sequence"].as_u64().unwrap() + 1;
                capsule["sequence"] = json!(next);
                if capsule["frames"].as_array().and_then(|frames| frames.last())
                    .is_some_and(|frame| frame["boundary"]["phase"] == "after_tool") {
                    suspend.store(true, Ordering::SeqCst);
                }
                *save.lock().unwrap() = Some((capsule, files));
                Ok(next)
            })
        }));
        let stopped = value(&mut workspace,"POST","/api/jobs",json!({"kind":"compile","setIds":["ds_main"]}),&execution);
        assert!(stopped.to_string().contains("suspended"), "{stopped}");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let (mut capsule, mut files) = saved.lock().unwrap().clone().unwrap();
        let original_token = capsule["claims"].as_array().unwrap().iter()
            .find(|claim| claim["lease"]["task"] == "build").unwrap()["lease"]["worker"]
            .as_str().unwrap().to_owned();
        // Emulate a previous OS worker. Its protected receipts and VFS agree.
        // The replacement must acquire a fresh build identity and use it for its batch.
        for claim in capsule["claims"].as_array_mut().unwrap() {
            claim["lease"]["worker"] = json!("previous-process");
        }
        for (path, bytes) in files.iter_mut().filter(|(path,_)| path.contains("/leases/")) {
            let mut lease: crate::control::Lease = serde_norway::from_slice(bytes).unwrap();
            lease.worker = "previous-process".into();
            *bytes = serde_norway::to_string(&lease).unwrap().into_bytes();
            assert!(!path.is_empty());
        }
        let mut foreign_files = files.clone();
        let (foreign_path, bytes) = foreign_files.iter_mut()
            .find(|(path,_)| path.contains("/leases/") && !path.ends_with("/build.yaml")).unwrap();
        let foreign_path = foreign_path.clone();
        let mut lease: crate::control::Lease = serde_norway::from_slice(bytes).unwrap();
        lease.worker = "another-execution".into();
        *bytes = serde_norway::to_string(&lease).unwrap().into_bytes();
        let foreign_bytes = bytes.clone();
        let resume = Execution { resume:Some(capsule.clone()), ..navigation_model("Alpha",calls.clone(),None) };
        let mut foreign = Workspace::new(foreign_files).unwrap();
        let refused = value(&mut foreign,"POST","/api/jobs",json!({"kind":"compile","setIds":["ds_main"]}),&resume);
        assert!(refused.to_string().contains("incomplete"), "{refused}");
        assert_eq!(foreign.snapshot()[&foreign_path], foreign_bytes);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "Foreign claim dispatched inference");
        let observed = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let observe = observed.clone();
        let checkpoint: CheckpointSink = Arc::new(move |capsule, _| {
            let observe = observe.clone();
            Box::pin(async move {
                let next = capsule["sequence"].as_u64().unwrap() + 1;
                observe.lock().unwrap().push(capsule);
                Ok(next)
            })
        });
        let resume = Execution { checkpoint:Some(checkpoint), ..resume };
        let mut restored = Workspace::new(files).unwrap();
        let complete = value(&mut restored,"POST","/api/jobs",json!({"kind":"compile","setIds":["ds_main"]}),&resume);
        assert!(complete.to_string().contains("converged"), "{complete}");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "Saved model response was billed again");
        assert!(!restored.snapshot().keys().any(|path| path.contains("/leases/")));
        assert!(observed.lock().unwrap().iter().any(|capsule|
            capsule["claims"].as_array().is_some_and(|claims| {
                let Some(token) = claims.iter().find(|claim| claim["lease"]["task"] == "build")
                    .and_then(|claim| claim["lease"]["worker"].as_str()) else { return false };
                !token.is_empty() && token != original_token && token != "previous-process"
                    && token != "another-execution" && claims.iter().any(|claim|
                        claim["lease"]["task"] != "build" && claim["lease"]["worker"] == token)
            })),
            "Restored MCP batch was not reclaimed under the replacement's fresh build token");
    }

    #[test]
    fn stopped_before_inference_chat_sends_the_next_human_instructions_in_full() {
        let mut w = workspace("Stopped before inference");
        let chat = value(&mut w,"POST","/api/chat/sessions",json!({}),&Execution::default());
        let path = format!("/api/chat/sessions/{}/prompt",chat["id"].as_str().unwrap());
        let saved = Arc::new(std::sync::Mutex::new(None::<(Value,Snapshot)>));
        let checkpoint = saved.clone();
        let sink: CheckpointSink = Arc::new(move |mut capsule, files| {
            let checkpoint = checkpoint.clone();
            Box::pin(async move {
                let next = capsule["sequence"].as_u64().unwrap()+1;
                capsule["sequence"] = json!(next);
                let before_model = capsule["frames"].as_array().and_then(|frames|frames.last())
                    .is_some_and(|frame|frame["boundary"]["phase"] == "before_model");
                *checkpoint.lock().unwrap() = Some((capsule,files));
                if before_model { Err("checkpoint-failed: injected loss before inference".into()) }
                else { Ok(next) }
            })
        });
        let execution = Execution {
            checkpoint:Some(sink),model:"must-not-run".into(),
            transport:Some(Arc::new(|_| panic!("A protected pre-inference Stop must not invoke a model"))),
            ..Default::default()
        };
        let stopped = value(&mut w,"POST",&path,json!({"text":"Create the recovery document.","messageId":"original"}),&execution);
        assert_eq!(stopped["stop"],"suspended");
        let (capsule,files) = saved.lock().unwrap().clone().unwrap();
        let mut stopped = Workspace::new(files).unwrap();
        stopped.stop_recovery(&capsule,&[]).unwrap();
        let mut stopped = Workspace::new(stopped.snapshot()).unwrap();
        let requests = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let observed = requests.clone();
        let execution = Execution {
            model:"followup-fixture".into(),transport:Some(Arc::new(move |body| {
                observed.lock().unwrap().push(serde_json::from_str(body).unwrap());
                Ok(model_reply("The earlier request was to create the recovery document."))
            })),..Default::default()
        };
        let instruction = "Do not write any files. Just explain the earlier task.";
        value(&mut stopped,"POST",&path,json!({"text":instruction,"messageId":"followup"}),&execution);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(),1,"No stopped inference or tool may be replayed");
        let humans = requests[0]["messages"].as_array().unwrap().iter()
            .filter(|message|message["role"] == "user")
            .filter_map(|message|message["content"].as_str()).collect::<Vec<_>>().join("\n");
        assert!(humans.contains("Create the recovery document."),"{humans}");
        assert!(humans.contains(instruction),"New instructions vanished from actual provider input: {humans}");
        assert!(!stopped.snapshot().contains_key("docs/recovery.md"));
    }

    #[test]
    fn actual_gdk_chat_restores_saved_response_tool_effect_and_finished_turn() {
        for phase in ["saved_model", "before_tool", "after_tool", "finished"] {
            for response_kind in ["completion", "responses"] {
            let mut w = workspace("Recovery");
            let chat = value(&mut w, "POST", "/api/chat/sessions", json!({}), &Execution::default());
            let path = format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap());
            let body = json!({"text":"Create the recovery document.","messageId":"accepted-1","turnId":"turn-1"});
            let calls = Arc::new(AtomicUsize::new(0));
            let model_calls = calls.clone();
            let model: ModelAsyncTransport = Arc::new(move |request| {
                let model_calls = model_calls.clone();
                Box::pin(async move {
                    let request: Value = serde_json::from_str(&request).unwrap();
                    assert!(request["_jazyk"]["requestId"].is_string());
                    let n = model_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(if n == 0 && response_kind == "responses" {
                        json!({"object":"response","id":"response-fixture","model":"fixture","status":"completed",
                            "_jazykResponseRoute":"fixture-route","output":[
                                {"id":"reasoning-fixture","type":"reasoning","summary":[],"encrypted_content":"opaque-reasoning-fixture"},
                                {"id":"call-fixture","type":"function_call","call_id":"write-1","name":"write_document","status":"completed",
                                    "arguments":json!({"path":"docs/recovery.md","text":"# Recovered\n\nSaved once.\n"}).to_string()}],
                            "usage":{"input_tokens":10,"output_tokens":3,"total_tokens":13}}).to_string()
                    } else if n == 0 {
                        json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"write-1","type":"function","function":{"name":"write_document","arguments":json!({"path":"docs/recovery.md","text":"# Recovered\n\nSaved once.\n"}).to_string()}}]}}],"usage":{"total_tokens":13,"prompt_tokens":10,"completion_tokens":3}}).to_string()
                    } else {
                        assert_eq!(n, 1, "A completed provider call was replayed at {phase}");
                        model_reply("Recovery complete.")
                    })
                })
            });
            let saved = Arc::new(std::sync::Mutex::new(None::<(Value, Snapshot)>));
            let checkpoint = saved.clone();
            let injected = Arc::new(AtomicBool::new(false));
            let inject = injected.clone();
            let sink: CheckpointSink = Arc::new(move |mut capsule, files| {
                let checkpoint = checkpoint.clone();
                let inject = inject.clone();
                Box::pin(async move {
                    let next = capsule["sequence"].as_u64().unwrap() + 1;
                    capsule["sequence"] = json!(next);
                    let current_phase = capsule["frames"].as_array().and_then(|frames| frames.last())
                        .and_then(|frame| frame["boundary"]["phase"].as_str());
                    let fail = current_phase == Some(phase) && !inject.swap(true, Ordering::SeqCst);
                    *checkpoint.lock().unwrap() = Some((capsule, files));
                    if fail { Err("checkpoint-failed: injected process loss after durable save".into()) } else { Ok(next) }
                })
            });
            let execution = Execution { model:"fixture".into(),transport_async:Some(model.clone()),checkpoint:Some(sink),..Default::default() };
            let stopped = value(&mut w,"POST",&path,body.clone(),&execution);
            assert_eq!(stopped["stop"],"suspended", "{phase}: {stopped}");
            let (capsule, files) = saved.lock().unwrap().clone().unwrap();
            crate::session_recovery::validate(&capsule).unwrap();
            let mut normalized: crate::harness::HarnessState =
                serde_json::from_value(capsule["frames"][0]["harness"].clone()).unwrap();
            let stopped_history = crate::harness::stopped_history(&normalized);
            normalized.conversation = goose_provider_types::conversation::Conversation::new_unvalidated(
                stopped_history.iter().filter_map(|message| message.get("_jazykGdk"))
                    .map(|message| serde_json::from_value(message.clone()).unwrap()).collect::<Vec<_>>());
            assert_eq!(crate::harness::stopped_history(&normalized), stopped_history,
                "Normalizing an already stopped transcript duplicated evidence at {phase}");
            if response_kind == "responses" {
                assert!(stopped_history.iter().any(|message| message.to_string().contains("opaque-reasoning-fixture")),
                    "Stopped transcript lost native Responses context at {phase}");
            }
            assert!(capsule["deliveredInputs"].as_array().unwrap().contains(&json!("accepted-1")));
            let committed = matches!(phase, "after_tool" | "finished");
            assert_eq!(files.contains_key("docs/recovery.md"), committed);
            let mut cancelled = Workspace::new(files.clone()).unwrap();
            let prior = value(&mut cancelled,"GET",&path.replace("/prompt",""),Value::Null,&Execution::default());
            let next = prior["updates"].as_array().unwrap().last().map_or(0,|u|u["n"].as_u64().unwrap()+1);
            let retained = json!({"type":"chat.update","sessionId":chat["id"],"n":next,
                "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Retained partial response."}}});
            cancelled.stop_recovery(&capsule, &[retained.clone(),retained]).unwrap();
            let cancelled_history = value(&mut cancelled,"GET",&path.replace("/prompt",""),Value::Null,&Execution::default());
            assert_eq!(cancelled_history["state"],"idle", "{phase}: {cancelled_history}");
            let cancelled_updates = cancelled_history["updates"].as_array().unwrap();
            assert_eq!(cancelled_updates.iter().filter(|u|u["update"]["content"]["text"]=="Retained partial response.").count(),1,"{phase}");
            assert_eq!(cancelled_updates.last().unwrap()["n"],json!(next+1),"{phase}");
            assert_eq!(cancelled_updates.iter()
                .filter(|u| u["update"]["sessionUpdate"] == "turn_end" && u["update"]["stop"] == "cancelled").count(),1,"{phase}");
            let requests = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
            let observed = requests.clone();
            let followup = Execution {
                model: "followup-fixture".into(),
                transport: Some(Arc::new(move |body| {
                    observed.lock().unwrap().push(serde_json::from_str(body).unwrap());
                    Ok(model_reply("The prior request was to create the recovery document."))
                })),
                ..Default::default()
            };
            value(&mut cancelled, "POST", &path,
                json!({"text":"try again","messageId":"followup-1","turnId":"turn-2"}), &followup);
            let requests = requests.lock().unwrap();
            assert_eq!(requests.len(), 1, "Stop or followup replayed inference at {phase}");
            let messages = requests[0]["messages"].as_array().unwrap();
            assert!(messages.iter().any(|message| message["role"] == "user"
                && message["content"].as_str().is_some_and(|text| text.contains("Create the recovery document."))),
                "Stopped chat lost the accepted human request at {phase}: {}", requests[0]);
            {
                let inference = capsule["frames"][0]["harness"]["model_receipts"].as_object().unwrap()
                    .keys().find(|key| key.contains(":Inference:")).unwrap();
                let call_id = format!("{}:write-1", &crate::model::hash_hex(inference)[..16]);
                assert!(messages.iter().any(|message| message["role"] == "assistant"
                    && message["tool_calls"].as_array().is_some_and(|calls| calls.iter().any(|call| call["id"] == call_id))),
                    "Stopped chat lost its assistant tool request at {phase}: {messages:?}");
                assert!(messages.iter().any(|message| message["role"] == "tool" && message["tool_call_id"] == call_id),
                    "Stopped chat lost the completed tool receipt at {phase}");
                if !committed {
                    assert!(messages.iter().any(|message| message["role"] == "tool"
                        && message["content"].as_str().is_some_and(|content| content.contains("interrupted"))),
                        "An undispatched proposal needs an interrupted result");
                }
            }
            assert_eq!(cancelled.snapshot().contains_key("docs/recovery.md"), committed,
                "Following up dispatched a saved proposal at {phase}");
            let mut restored = Workspace::new(files).unwrap();
            let resumed = Execution { model:"fixture".into(),transport_async:Some(model),resume:Some(capsule),..Default::default() };
            let complete = value(&mut restored,"POST",&path,body,&resumed);
            assert_eq!(complete["stop"],"end_turn", "{phase}: {complete}");
            assert_eq!(calls.load(Ordering::SeqCst),2, "{phase}");
            assert_eq!(restored.snapshot()["docs/recovery.md"], b"# Recovered\n\nSaved once.\n");
            let timeline = value(&mut restored,"GET",&path.replace("/prompt",""),Value::Null,&Execution::default());
            let updates = timeline["updates"].as_array().unwrap();
            assert_eq!(updates.iter().filter(|u| u["update"]["sessionUpdate"] == "user_message").count(),1,"{phase}");
            assert_eq!(updates.iter().filter(|u| u["update"]["sessionUpdate"] == "turn_end").count(),1,"{phase}");
            }
        }
    }
    #[test]
    fn old_amnesic_chat_recovers_its_own_timeline_in_the_next_provider_request_once() {
        for already_amnesic in [false,true] {
        let mut w = workspace("Legacy chat");
        value(&mut w,"POST","/api/chat/sessions",json!({}),&Execution::default());
        value(&mut w,"POST","/api/chat/sessions",json!({}),&Execution::default());
        let mut book: Value = serde_json::from_slice(&w.files[".jazyk/conversations.json"]).unwrap();
        let chat = &mut book["chats"]["chat-1"];
        chat["state"] = json!("idle");
        chat["titled"] = json!(true);
        chat["history"] = if already_amnesic { json!([
            {"role":"user","content":"try again"},
            {"role":"assistant","content":"I do not have the earlier request."}
        ]) } else { json!([]) };
        chat["updates"] = json!([
            [0,{"sessionUpdate":"user_message","messageId":"original","text":"Restyle the UML diagram in deliverable/index.html"}],
            [1,{"sessionUpdate":"agent_message_chunk","content":{"text":"Updating UML cards."}}],
            [2,{"sessionUpdate":"tool_call","toolCallId":"write","title":"write_text_file","rawInput":{"path":"deliverable/index.html"}}],
            [3,{"sessionUpdate":"tool_call_update","toolCallId":"write","status":"completed","rawOutput":"Saved deliverable/index.html"}],
            [4,{"sessionUpdate":"turn_end","stop":"cancelled"}],
            [5,{"sessionUpdate":"user_message","messageId":"retry","text":"try again"}],
            [6,{"sessionUpdate":"agent_message_chunk","content":{"text":"I do not have the earlier request."}}],
            [7,{"sessionUpdate":"turn_end","stop":"end_turn"}]
        ]);
        if !already_amnesic { chat["updates"].as_array_mut().unwrap().truncate(5); }
        chat["next_n"] = json!(8);
        book["chats"]["chat-2"]["updates"] = json!([
            [0,{"sessionUpdate":"user_message","text":"DO_NOT_CROSS_OTHER_CHAT"}]
        ]);
        w.files.insert(".jazyk/conversations.json".into(),serde_json::to_vec(&book).unwrap());
        w.files.insert("deliverable/index.html".into(),b"Previously saved diagram".to_vec());
        let requests = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let observed = requests.clone();
        let execution = Execution {
            model:"legacy-repair-fixture".into(),
            transport:Some(Arc::new(move |body| {
                observed.lock().unwrap().push(serde_json::from_str(body).unwrap());
                Ok(model_reply("The earlier request was to restyle the UML diagram."))
            })),..Default::default()
        };
        value(&mut w,"POST","/api/chat/sessions/chat-1/prompt",json!({"text":"try again","messageId":"new-retry"}),&execution);
        let mut reopened = Workspace::new(w.snapshot()).unwrap();
        value(&mut reopened,"POST","/api/chat/sessions/chat-1/prompt",json!({"text":"what was the task?","messageId":"later"}),&execution);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(),2,"History repair must not invoke extra inference");
        for request in requests.iter() {
            let messages = request["messages"].as_array().unwrap();
            let receipts = messages.iter().filter(|message| message["role"] == "user"
                && message["content"].as_str().is_some_and(|text|text.starts_with("Earlier conversation evidence was recovered"))).collect::<Vec<_>>();
            assert_eq!(receipts.len(),1,"The receipt must survive reopening without duplication: {:?}",
                messages.iter().filter(|message|message["role"] != "system").collect::<Vec<_>>());
            let text = receipts[0]["content"].as_str().unwrap();
            assert!(text.contains("Restyle the UML diagram in deliverable/index.html"));
            assert!(text.contains("Updating UML cards."));
            assert!(text.contains("Saved deliverable/index.html"));
            assert!(!messages.iter().any(|message|message["role"] == "tool" || message.get("tool_calls").is_some()),
                "Historical observations must not become synthetic tool calls or outcomes");
            assert!(!request.to_string().contains("DO_NOT_CROSS_OTHER_CHAT"));
        }
        assert_eq!(reopened.snapshot()["deliverable/index.html"],b"Previously saved diagram");
        }
    }

    #[test]
    fn shared_task_suite_dispatches_once_per_pass_and_keeps_case_and_owner_evidence_separate() {
        use base64::Engine;
        use crate::harness::Recovery;
        use crate::{gen::{GenSettings, Ledger, mark as gen_mark}, model::{Criterion, Entity, Requirement, hash_hex}, store::Store, verify::{run_selected, status_of}};
        use std::{collections::BTreeMap, sync::{Arc, Mutex}};

        let submissions = Arc::new(Mutex::new(BTreeMap::<String, Value>::new()));
        for owner in ["owner-a", "owner-b"] {
            let _fs = crate::vfs::Scope::enter(&Default::default());
            let _state = crate::scoped_state::Scope::enter();
            let root = std::path::Path::new("/project");
            let out = root.join(".jazyk/ds_main");
            crate::vfs::create_dir_all(root.join(".jazyk")).unwrap();
            let mut store = Store { out:out.clone(), ..Default::default() };
            store.graph.entities.insert("ent:cart".into(), Entity { name:"Cart".into(), ..Default::default() });
            store.graph.requirements.insert("req:shop-1".into(), Requirement { statement:"The Cart shall hold items.".into(), entities:vec!["ent:cart".into()], ..Default::default() });
            crate::vfs::write(root.join(".jazyk/jazyk.toml"), "version=1\n[[sets]]\nid='ds_main'\nname='Site'\ndocs=['docs/**']\nsource=['src/**','tests/**']\n[[tasks]]\nid='check'\nname='Shared suite'\ndescription='Check related cases together'\ncommand='pytest --junitxml=reports/check.xml'\nreports=['reports/*.xml']\n[[tasks]]\nid='other'\nname='Other suite'\ndescription='Unselected checks'\ncommand='false'\n").unwrap();
            let project = crate::document_sets::Workspace::load(root).unwrap().resolve(Some("ds_main")).unwrap();
            let gs = GenSettings::resolve(&project);
            crate::vfs::create_dir_all(root.join("src")).unwrap();
            crate::vfs::create_dir_all(root.join("tests")).unwrap();
            crate::vfs::write(root.join("tests/suite.py"), "def works(): pass\ndef broken(): pass\ndef skipped(): pass\ndef absent(): pass\ndef unrelated(): pass\n").unwrap();
            let requirement = store.graph.requirements["req:shop-1"].clone();
            for id in ["req:shop-2", "req:unrelated"] {
                store.graph.requirements.insert(id.into(), Requirement { statement:format!("Statement for {id}"), ..requirement.clone() });
            }
            crate::vfs::write(root.join("src/cart.rs"), format!("// {owner}: revision 1\n")).unwrap();
            gen_mark(&store, "ent:cart", None, &json!({"files":["src/cart.rs"],"tests":[
                {"requirement":"req:shop-1","kind":"programmatic","artifact":"tests/suite.py","name":"Site::works","run":"task:check"},
                {"requirement":"req:shop-2","kind":"programmatic","artifact":"tests/suite.py","name":"Other::works","run":"task:check"},
                {"requirement":"req:unrelated","kind":"programmatic","artifact":"tests/suite.py","name":"unrelated","run":"task:other"}
            ]}), &gs).unwrap();
            let mut ledger = Ledger::load(&out);
            for (rid, cid, selector) in [
                ("req:shop-1", "crit:pass", "Site::works"),
                ("req:shop-1", "crit:fail", "Site::broken"),
                ("req:shop-1", "crit:skip", "Site::skipped"),
                ("req:shop-2", "crit:other", "Other::works"),
                ("req:shop-2", "crit:missing", "Site::absent"),
                ("req:shop-2", "crit:ambiguous", "works"),
            ] {
                store.graph.criteria.insert(cid.into(), Criterion { requirement:rid.into(), statement:cid.into(), ..Default::default() });
                let row = ledger.requirements.get_mut(rid).unwrap();
                let mut test = row.test.clone();
                test.name = selector.into();
                row.tests.insert(cid.into(), crate::gen::CritTest {
                    test, criterion_hash:hash_hex(cid), test_hash:row.hashes.test.clone(),
                    verdict:"pass".into(), last_run:None, exit_code:None, evidence:None,
                });
            }
            ledger.entities.get_mut("cart").unwrap().fact_hash = crate::gen::fact_hash(&store, "ent:cart");
            let unrelated = serde_json::to_value(&ledger.requirements["req:unrelated"]).unwrap();
            ledger.save(&out).unwrap();
            let requests = submissions.clone();
            let runtime: crate::hosted::RuntimeTransport = Arc::new(move |request| {
                let run_id = request["run_id"].as_str().unwrap_or_default();
                match request["operation"].as_str().unwrap() {
                    "task_context" => Ok(json!({"available":true})),
                    "run_task" => {
                        assert_eq!(request["task"]["id"], "check");
                        let mut requests = requests.lock().unwrap();
                        let id = format!("{owner}-run-{}", requests.len()+1);
                        requests.insert(id.clone(), request.clone());
                        Ok(json!({"id":id}))
                    }
                    "wait_run" => {
                        assert!(run_id.starts_with(owner));
                        assert!(requests.lock().unwrap().contains_key(run_id));
                        Ok(json!({"terminated":true}))
                    }
                    "result" => {
                        assert!(run_id.starts_with(owner));
                        let submitted = requests.lock().unwrap()[run_id].clone();
                        let task: crate::task_runtime::Task = serde_json::from_value(submitted["task"].clone()).unwrap();
                        let input = serde_json::from_value(submitted["files"].clone()).unwrap();
                        let xml = "<testsuite><testcase classname='Site' name='works'/><testcase classname='Site' name='broken'><failure/></testcase><testcase classname='Site' name='skipped'><skipped/></testcase><testcase classname='Other' name='works'/><testcase classname='Unbound' name='noise'/></testsuite>";
                        let raw = json!({"status":"failed","complete":true,"exit_code":1,"log":"fixture suite completed","files":{},"cache":{},"deleted":[],"products":[],"reports":{"reports/check.xml":base64::engine::general_purpose::STANDARD.encode(xml)}});
                        let mut result = crate::task_runtime::validate_result(&task, &input, raw).unwrap();
                        result["input_files"] = submitted["files"].clone();
                        Ok(result)
                    }
                    operation => panic!("Unexpected suite operation {operation}"),
                }
            });
            let saved = Arc::new(Mutex::new(None::<(Value, Snapshot)>));
            let checkpoint = saved.clone();
            let sink: CheckpointSink = Arc::new(move |mut capsule, files| {
                let checkpoint = checkpoint.clone();
                Box::pin(async move {
                    let next = capsule["sequence"].as_u64().unwrap()+1;
                    capsule["sequence"] = json!(next);
                    *checkpoint.lock().unwrap() = Some((capsule, files));
                    Ok(next)
                })
            });
            let _execution = ExecutionScope::enter(Execution { runtime:Some(runtime), checkpoint:Some(sink), ..Default::default() });
            let _recovery = crate::session_recovery::Scope::enter(None);
            let parent_key = crate::session_recovery::command_key("compiler-progress");
            for revision in [1, 2] {
                crate::vfs::write(root.join("src/cart.rs"), format!("// {owner}: revision {revision}\n")).unwrap();
                let before = submissions.lock().unwrap().len();
                // Deliberately reuse these ids in the other owner's scope.
                let invocation = format!("run-tests-{revision}");
                let state: crate::harness::HarnessState = serde_json::from_value(json!({
                    "version":1,"run_id":"suite-worker","transition":revision,"rounds":revision,
                    "conversation":[],"model_receipts":{},"tool_receipts":{},
                    "pending_tool":{"id":invocation,"name":"run_tests","args":{}},
                    "delivered_inputs":[],"usage":[]
                })).unwrap();
                let mut recovery = crate::session_recovery::McpRecovery::utility("suite-worker".into()).unwrap();
                async_io::block_on(recovery.checkpoint(crate::harness::CheckpointBoundary {
                    phase:crate::harness::CheckpointPhase::BeforeTool, operation_id:Some(invocation),
                }, &state)).unwrap();
                assert_eq!(crate::session_recovery::command_key("compiler-progress"), parent_key);
                let result = run_selected(&store, &gs, &["req:shop-1".into(), "req:shop-2".into()]).unwrap();
                assert_eq!(submissions.lock().unwrap().len(), before+1, "one shared suite, not one run per requirement or criterion");
                assert_eq!(result["runs"].as_array().unwrap().len(), 1);
                let ledger = Ledger::load(&out);
                let first = &ledger.requirements["req:shop-1"];
                assert_eq!(first.tests["crit:pass"].verdict, "pass");
                assert_eq!(first.tests["crit:fail"].verdict, "fail");
                assert_eq!(first.tests["crit:skip"].verdict, "none");
                let second = &ledger.requirements["req:shop-2"];
                assert_eq!(second.tests["crit:other"].verdict, "pass");
                assert_eq!(second.tests["crit:missing"].verdict, "none");
                assert_eq!(second.tests["crit:ambiguous"].verdict, "none");
                assert_ne!(status_of(&store, "req:shop-2", second, &gs).0, "verified");
                assert_eq!(serde_json::to_value(&ledger.requirements["req:unrelated"]).unwrap(), unrelated);
                let before_replay = submissions.lock().unwrap().len();
                let original = saved.lock().unwrap().as_ref().unwrap().0["parent"].clone();
                run_selected(&store, &gs, &["req:shop-1".into(), "req:shop-2".into()]).unwrap();
                assert_eq!(submissions.lock().unwrap().len(), before_replay, "same invocation reuses its destination receipt");
                assert_eq!(saved.lock().unwrap().as_ref().unwrap().0["parent"], original, "same intention, destination run and deadline survive replay");
                let requests = submissions.lock().unwrap();
                let submitted = requests.values().last().unwrap();
                let source = base64::engine::general_purpose::STANDARD.decode(submitted["files"]["src/cart.rs"].as_str().unwrap()).unwrap();
                assert_eq!(source, format!("// {owner}: revision {revision}\n").as_bytes());
            }
            let (capsule, files) = saved.lock().unwrap().clone().unwrap();
            let before_restore = submissions.lock().unwrap().len();
            let _restored_fs = crate::vfs::Scope::enter(&files);
            let _restored_recovery = crate::session_recovery::Scope::enter(Some(&capsule));
            run_selected(&store, &gs, &["req:shop-1".into(), "req:shop-2".into()]).unwrap();
            assert_eq!(submissions.lock().unwrap().len(), before_restore, "restoring the same tool checkpoint does not dispatch another task");
            assert_eq!(saved.lock().unwrap().as_ref().unwrap().0["parent"], capsule["parent"]);
        }
        let requests = submissions.lock().unwrap();
        assert_eq!(requests.len(), 4, "two owners each run once against each of two source revisions");
        let inputs: std::collections::BTreeSet<_> = requests.values().map(|request| request["input_digest"].as_str().unwrap()).collect();
        assert_eq!(inputs.len(), 4, "every pass uses its own frozen input");
    }
    #[test]
    fn task_restart_reuses_destination_intention_run_receipt_and_original_deadline() {
        for lose_reply in [false,true] {
            let mut initial = workspace("Tasks").snapshot();
            initial.get_mut(".jazyk/jazyk.toml").unwrap().extend_from_slice(b"\n[[tasks]]\nid='check'\nname='Check'\ndescription='Check source'\ncommand='true'\n");
            let saved = Arc::new(std::sync::Mutex::new(None::<(Value,Snapshot)>));
            let effects = Arc::new(std::sync::Mutex::new(BTreeMap::<String,Value>::new()));
            let requests = Arc::new(AtomicUsize::new(0));
            let runtime_effects = effects.clone();
            let runtime_requests = requests.clone();
            let runtime: RuntimeTransport = Arc::new(move |request| match request["operation"].as_str().unwrap() {
                "task_context" => Ok(json!({"available":true})),
                "run_task" => {
                    let n = runtime_requests.fetch_add(1,Ordering::SeqCst);
                    let key = request["operation_key"].as_str().unwrap().to_owned();
                    let mut effects = runtime_effects.lock().unwrap();
                    if let Some(previous) = effects.get(&key) { assert_eq!(previous,request); }
                    effects.insert(key,request.clone());
                    if lose_reply && n == 0 { Err("reply lost after destination accepted the task".into()) }
                    else { Ok(json!({"id":"original-run"})) }
                }
                "wait_run" => { assert_eq!(request["run_id"],"original-run"); Ok(json!({"terminated":true})) }
                "result" => {
                    let input = runtime_effects.lock().unwrap().values().next().unwrap()["files"].clone();
                    Ok(json!({"status":"succeeded","complete":true,"input_files":input,
                        "log":"done","files":{},"reports":{},"cache":{},"deleted":[],"products":[]}))
                }
                operation => panic!("Unexpected task operation {operation}"),
            });
            let checkpoint = saved.clone();
            let failed = Arc::new(AtomicBool::new(false));
            let fail = failed.clone();
            let sink: CheckpointSink = Arc::new(move |mut capsule,files| {
                let checkpoint = checkpoint.clone(); let fail = fail.clone();
                Box::pin(async move {
                    let next = capsule["sequence"].as_u64().unwrap()+1;
                    capsule["sequence"] = json!(next);
                    let has_run = capsule["parent"].as_object().unwrap().values().any(|p| p["run_id"] == "original-run");
                    *checkpoint.lock().unwrap() = Some((capsule,files));
                    if has_run && !lose_reply && !fail.swap(true,Ordering::SeqCst) {
                        Err("checkpoint-failed: process lost after the run receipt was saved".into())
                    } else { Ok(next) }
                })
            });
            {
                let _fs = crate::vfs::Scope::enter(&initial);
                let _state = crate::scoped_state::Scope::enter();
                let _execution = ExecutionScope::enter(Execution { runtime:Some(runtime.clone()),checkpoint:Some(sink),..Default::default() });
                let _recovery = crate::session_recovery::Scope::enter(None);
                let error = crate::task_runtime::execute_with_key(&root(),"check","verify").unwrap_err();
                assert!(crate::session_recovery::is_interruption(&error),"{error}");
            }
            let (capsule,files) = saved.lock().unwrap().clone().unwrap();
            let deadline = capsule["parent"].as_object().unwrap().values().find_map(|p| p["deadline"].as_u64()).unwrap();
            {
                let _fs = crate::vfs::Scope::enter(&files);
                let _state = crate::scoped_state::Scope::enter();
                let _execution = ExecutionScope::enter(Execution { runtime:Some(runtime),resume:Some(capsule.clone()),..Default::default() });
                let _recovery = crate::session_recovery::Scope::enter(Some(&capsule));
                let (id,result) = crate::task_runtime::execute_with_key(&root(),"check","verify").unwrap();
                assert_eq!(id,"original-run"); assert_eq!(result["status"],"succeeded");
                let key = capsule["parent"].as_object().unwrap().iter()
                    .find(|(_, value)| value["deadline"].as_u64().is_some()).unwrap().0;
                assert_eq!(crate::session_recovery::parent(key).unwrap().unwrap()["deadline"],deadline);
            }
            assert_eq!(effects.lock().unwrap().len(),1);
            assert_eq!(requests.load(Ordering::SeqCst),if lose_reply {2}else{1});
        }
    }
    fn navigation_model(
        name: &'static str,
        calls: Arc<AtomicUsize>,
        barrier: Option<Arc<std::sync::Barrier>>,
    ) -> Execution {
        Execution {
            model: name.into(),
            transport: Some(Arc::new(move |body| {
                let request: Value = serde_json::from_str(body).unwrap();
                assert_eq!(request["model"], name);
                let n = calls.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    if let Some(b) = &barrier {
                        b.wait();
                    }
                }
                if request["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["role"] == "tool")
                {
                    return Ok(model_reply("Done."));
                }
                let section = format!("docs/main.md#/{}", name.to_lowercase());
                let goal = format!("g:reconcile-section:{section}");
                let actions = [
                    (
                        "set_coverage",
                        json!({"section":section,"state":"non-normative","note":"This navigation index contains no product obligations."}),
                    ),
                    (
                        "mark_goal_done",
                        json!({"goal":goal,"justification":"The section is a navigation index with no normative sentences."}),
                    ),
                    (
                        "done",
                        json!({"summary":"Classified the navigation index."}),
                    ),
                ];
                Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(i,(name,args))|json!({"id":format!("call-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}).to_string())
            })),
            ..Default::default()
        }
    }

    #[test]
    fn concurrent_real_builds_keep_projects_models_and_noop_isolated() {
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = ["Alpha", "Beta"]
            .into_iter()
            .map(|name| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut w = workspace(name);
                    let calls = Arc::new(AtomicUsize::new(0));
                    let e = navigation_model(name, calls.clone(), Some(barrier));
                    let first = value(&mut w, "POST", "/api/jobs", json!({"kind":"compile"}), &e);
                    assert_eq!(
                        first["report"]["failed"].as_u64().unwrap_or(0),
                        0,
                        "{first}"
                    );
                    let before = calls.load(Ordering::SeqCst);
                    assert!(before > 0);
                    let graph = value(&mut w, "GET", "/api/graph", Value::Null, &e);
                    assert!(graph["generation"].as_u64().unwrap() > 0, "{first}");
                    let next = value(&mut w, "POST", "/api/compile", json!({}), &e);
                    assert_eq!(
                        calls.load(Ordering::SeqCst),
                        before,
                        "no-op called model: {next}"
                    );
                    let restored = w.snapshot();
                    assert!(String::from_utf8_lossy(&restored["docs/main.md"]).contains(name));
                    assert!(!restored.keys().any(|k| k.starts_with('/')));
                    assert!(!crate::vfs::is_scoped());
                    restored
                })
            })
            .collect();
        let trees: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_ne!(trees[0]["docs/main.md"], trees[1]["docs/main.md"]);
    }

    #[test]
    fn hosted_files_shell_config_and_binary_routes_are_confined() {
        let mut w = workspace("Alpha");
        w.files.insert("picture.png".into(), vec![0, 255, 13, 10]);
        let before = w.snapshot();
        {
            let scope = crate::vfs::Scope::enter(&before);
            for path in [
                "/etc/hosts",
                "/project/../../etc/hosts",
                "../outside",
                "/project-other/a",
            ] {
                assert_eq!(
                    crate::vfs::read(path).unwrap_err().kind(),
                    std::io::ErrorKind::PermissionDenied
                );
                assert!(crate::vfs::write(path, b"forbidden").is_err());
            }
            assert!(crate::sys::run_shell("exit 0", &root()).is_err());
            assert!(crate::sys::env_var("HOME").is_err());
            assert!(crate::project::load_global_llm().model.is_none());
            assert_eq!(scope.snapshot(), before);
        }
        let reply = w
            .request_test(
                "GET",
                "/api/project/file?path=picture.png",
                "",
                &Execution::default(),
            )
            .unwrap();
        assert_eq!(reply.status, 200);
        assert_eq!(reply.content_type, "image/png");
        assert_eq!(reply.body, vec![0, 255, 13, 10]);
        for path in ["../a", "/abs", "a//b", "a/./b", "a\\b", "a\0b"] {
            assert!(Workspace::new(BTreeMap::from([(path.into(), vec![])])).is_err());
        }
    }

    #[test]
    fn hosted_chat_receives_github_context_through_mcp_instructions() {
        let mut w = workspace("GitHub context");
        let e = Execution {
            model:"fixture".into(),
            runtime:Some(Arc::new(|request| {
                assert_eq!(request["operation"],"project_context");
                Ok(json!({"available":true,"location":"cloud","project":{"name":"Jazyk.org"},"github":{"url":"https://github.com/JazykOrg/jazyk-site","branch":"main","default_branch":"master"}}))
            })),
            transport:Some(Arc::new(|body| {
                let body:Value = serde_json::from_str(body).unwrap();
                let messages = body["messages"].to_string();
                for fact in ["https://github.com/JazykOrg/jazyk-site", "default_branch", "master", "main", "Jazyk.org"] { assert!(messages.contains(fact),"missing {fact}: {messages}"); }
                assert!(body["tools"].as_array().unwrap().iter().any(|t|t["function"]["name"]=="project_context"));
                Ok(model_reply("The current branch is main."))
            })), ..Default::default()
        };
        let chat = value(&mut w,"POST","/api/chat/sessions",json!({}),&e);
        value(&mut w,"POST",&format!("/api/chat/sessions/{}/prompt",chat["id"].as_str().unwrap()),json!({"messageId":"branch-question","text":"Which branch should the deploy workflow target?"}),&e);
    }

    // Hosted chat reads the snapshot's instruction files and skills and the host's
    // layer, never the host filesystem, and calls remote tools through the host.
    // Mirrors docs/frontends/agent-context.md#hosted-context.
    #[test]
    fn hosted_chat_uses_the_exact_managed_policy_and_deferred_skill_tools() {
        let mut w = workspace("Shared workflows");
        w.sync_agents_md(false);
        let policy = {
            let _fs = crate::vfs::Scope::enter(&w.files);
            crate::agent_workflows::render(&crate::portable::root()).unwrap()
        };
        let before = w.snapshot();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let expected_skill = crate::agent_workflows::read("change-code").unwrap()["body"].as_str().unwrap().to_string();
        let e = Execution { model: "fixture".into(), transport: Some(Arc::new(move |body| {
            let request: Value = serde_json::from_str(body).unwrap();
            let texts = request["messages"].as_array().unwrap().iter()
                .filter_map(|m|m["content"].as_str()).collect::<Vec<_>>();
            assert_eq!(texts.iter().map(|t|t.matches(&policy).count()).sum::<usize>(), 1);
            for name in crate::agent_workflows::TOOLS {
                assert!(request["tools"].as_array().unwrap().iter().any(|t|t["function"]["name"]==name));
            }
            if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,
                    "tool_calls":[{"id":"read-workflow","type":"function","function":{"name":"read_jazyk_skill","arguments":json!({"name":"change-code"}).to_string()}}]}}]}).to_string());
            }
            assert!(texts.iter().filter_map(|t|serde_json::from_str::<Value>(t).ok())
                .any(|v|v["body"].as_str()==Some(expected_skill.as_str())));
            Ok(model_reply("A repair stays within requirements. Changed behavior updates the docs first."))
        })), ..Default::default() };
        let chat = value(&mut w, "POST", "/api/chat/sessions", json!({}), &e);
        value(&mut w, "POST", &format!("/api/chat/sessions/{}/prompt",chat["id"].as_str().unwrap()),
            json!({"text":"Explain the workflow for a code change. Do not edit anything.","messageId":"workflow-question"}), &e);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        for (path, bytes) in before {
            assert_eq!(w.snapshot().get(&path), Some(&bytes), "read-only skill request changed {path}");
        }
        assert!(!w.snapshot().keys().any(|p|p.contains("graph/")));
    }

    #[test]
    fn hosted_chat_receives_project_and_host_agent_context() {
        let mut w = workspace("Agent context");
        w.files.insert("AGENTS.md".into(), b"Prefer short sentences.\n".to_vec());
        w.files.insert(".agents/skills/triage/SKILL.md".into(), b"---\nname: triage\ndescription: sort issues\n---\nSteps.".to_vec());
        w.files.insert(".mcp.json".into(), br#"{"mcpServers": {"web": {"url": "https://mcp.example.com/mcp"}, "local": {"command": "x"}}}"#.to_vec());
        let changed = w.sync_agents_md(true);
        assert!(changed.iter().any(|c| c == "added the jazyk section to AGENTS.md"), "{changed:?}");
        let agents = String::from_utf8(w.snapshot()["AGENTS.md"].clone()).unwrap();
        assert!(agents.starts_with("Prefer short sentences.\n\n<!-- JAZYK:BEGIN -->"));
        assert!(w.sync_agents_md(false).is_empty());
        let proposals = w.agent_mcp_proposals();
        assert_eq!(proposals.iter().map(|p| (p["name"].as_str().unwrap(), p["hosted"].as_bool().unwrap())).collect::<Vec<_>>(), vec![("local", false), ("web", true)]);
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let host = crate::agent_context::HostContext {
            instructions: vec![("organization".into(), "Use British spelling.".into())],
            mcp: vec![crate::agent_context::RemoteServer { name: "issues".into(), instructions: None, tools: vec![crate::harness::GenericTool { name: "list_issues".into(), description: "List issues".into(), parameters: json!({"type": "object"}) }] }],
            call: Some(Arc::new(|server, tool, _| Ok(format!("{server}:{tool}:42 open")))),
            ..Default::default()
        };
        let e = Execution {
            model: "fixture".into(),
            agent_context: Some(Arc::new(host)),
            transport: Some(Arc::new(move |body| {
                let body: Value = serde_json::from_str(body).unwrap();
                let messages = body["messages"].to_string();
                let n = seen.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    for fact in ["Prefer short sentences.", "Use British spelling.", "- triage: sort issues", "precedence"] {
                        assert!(messages.contains(fact), "missing {fact}: {messages}");
                    }
                    assert!(!messages.contains("JAZYK:BEGIN"), "the jazyk section stays out");
                    let names: Vec<&str> = body["tools"].as_array().unwrap().iter().filter_map(|t| t["function"]["name"].as_str()).collect();
                    assert!(names.contains(&"read_skill") && names.contains(&"list_issues"), "{names:?}");
                    return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"r1","type":"function","function":{"name":"list_issues","arguments":"{}"}}]}}]}).to_string());
                }
                assert!(messages.contains("issues:list_issues:42 open"), "{messages}");
                Ok(model_reply("42 issues are open."))
            })),
            ..Default::default()
        };
        let chat = value(&mut w, "POST", "/api/chat/sessions", json!({}), &e);
        value(&mut w, "POST", &format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap()), json!({"messageId":"q","text":"How many issues are open?"}), &e);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn hosted_input_callback_delivers_before_stale_write_and_persists_identity() {
        let mut w=workspace("Steering");
        w.files.insert("docs/hosting.md".into(),b"# Hosting\n\nOriginal.\n".to_vec());
        let incoming=Arc::new(AtomicBool::new(false));
        let incoming_model=incoming.clone();
        let calls=Arc::new(AtomicUsize::new(0));
        let model_calls=calls.clone();
        let hash=crate::model::hash_hex("# Hosting\n\nOriginal.\n");
        let events=Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let sink=events.clone();
        let e=Execution{model:"fixture".into(),transport:Some(Arc::new(move|body|{
            let n=model_calls.fetch_add(1,Ordering::SeqCst);
            if n==0 {
                incoming_model.store(true,Ordering::SeqCst);
                return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"stale-write","type":"function","function":{"name":"write_document","arguments":json!({"path":"docs/hosting.md","text":"must not land","baseHash":hash}).to_string()}}]}}]}).to_string());
            }
            assert!(body.contains("Do not change hosting.md"));
            Ok(model_reply("Kept hosting.md unchanged."))
        })),inputs:Some(Arc::new(move|session|if incoming.swap(false,Ordering::SeqCst){vec![json!({"sessionId":session,"messageId":"steer-1","text":"Do not change hosting.md","sequence":2})]}else{vec![]})),input_close:Some(Arc::new(|_|true)),events:Some(Arc::new(move|event|sink.lock().unwrap().push(event.clone()))),..Default::default()};
        let chat=value(&mut w,"POST","/api/chat/sessions",json!({}),&e);
        value(&mut w,"POST",&format!("/api/chat/sessions/{}/prompt",chat["id"].as_str().unwrap()),json!({"messageId":"initial","text":"Update hosting.md"}),&e);
        assert_eq!(w.snapshot()["docs/hosting.md"],b"# Hosting\n\nOriginal.\n");
        assert_eq!(calls.load(Ordering::SeqCst),2);
        let timeline=value(&mut w,"GET",&format!("/api/chat/sessions/{}",chat["id"].as_str().unwrap()),Value::Null,&e);
        assert!(timeline["inputs"].as_array().unwrap().iter().any(|v|v["messageId"]=="steer-1"&&v["state"]=="delivered"));
        assert!(events.lock().unwrap().iter().any(|v|v["type"]=="chat.input"&&v["messageId"]=="steer-1"));
    }

    #[test]
    fn owned_build_steering_recovers_cursor_and_preserves_conversation() {
        let mut w = workspace("Steering");
        let incoming = Arc::new(AtomicBool::new(false));
        let incoming_model = incoming.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_model = cancel.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_model = calls.clone();
        let events = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let sink = events.clone();
        let execution = Execution {
            model: "build-steering-fixture".into(),
            cancel,
            transport: Some(Arc::new(move |body| {
                if calls_model.fetch_add(1, Ordering::SeqCst) == 0 {
                    incoming_model.store(true, Ordering::SeqCst);
                    return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{
                        "id":"stale-coverage","type":"function","function":{"name":"set_coverage",
                        "arguments":json!({"section":"docs/main.md#/steering","state":"non-normative","note":"stale proposal"}).to_string()}
                    }]}}]}).to_string());
                }
                assert!(body.contains("Stop changing coverage. Inspect the source first."));
                assert!(body.contains("Skipped: new human input arrived before dispatch"));
                cancel_model.store(true, Ordering::SeqCst);
                Ok(model_reply("Stopped after receiving the correction."))
            })),
            inputs: Some(Arc::new(move |session| {
                assert_eq!(session, "chat-1", "the worker batch must drain its ordinary conversation");
                if incoming.swap(false, Ordering::SeqCst) {
                    vec![json!({"sessionId":session,"messageId":"build-steer-1","text":"Stop changing coverage. Inspect the source first.","turnId":"41","sequence":1})]
                } else { vec![] }
            })),
            input_close: Some(Arc::new(|_| panic!("a worker batch cannot close whole-job admission"))),
            events: Some(Arc::new(move |event| sink.lock().unwrap().push(event.clone()))),
            ..Default::default()
        };
        let chat = value(&mut w, "POST", "/api/chat/sessions", json!({}), &execution);
        let id = chat["id"].as_str().unwrap();
        // A reopened workspace can lack updates already committed to the event DB.
        // Its empty conversation book is behind that DB's durable cursor of 800.
        let result = w.request_test("POST", "/api/jobs", &json!({"kind":"compile","setId":"ds_main","conversationId":id,"jobId":41,"sessions":1,"afterN":800}).to_string(), &execution).unwrap();
        assert_eq!(result.status, 200, "{}", String::from_utf8_lossy(&result.body));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let timeline = value(&mut w, "GET", &format!("/api/chat/sessions/{id}"), Value::Null, &Execution::default());
        let rows = timeline["updates"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "one workflow marker and one delivered human message");
        assert_eq!(rows[0]["n"], 801);
        assert_eq!(rows[1]["n"], 802);
        assert!(rows.iter().any(|v| v["update"]["sessionUpdate"] == "workflow" && v["update"]["jobId"] == 41));
        assert_eq!(rows.iter().filter(|v| v["update"]["sessionUpdate"] == "user_message" && v["update"]["messageId"] == "build-steer-1").count(), 1);
        assert!(timeline["inputs"].as_array().unwrap().iter().any(|v| v["messageId"] == "build-steer-1" && v["state"] == "delivered"));
        assert!(events.lock().unwrap().iter().any(|v| v["type"] == "chat.input" && v["messageId"] == "build-steer-1" && v["state"] == "delivered"));
        let history = String::from_utf8(w.snapshot()[".jazyk/conversations.json"].clone()).unwrap();
        assert!(history.contains("Stop changing coverage. Inspect the source first."));
        assert!(!history.contains("stale proposal"));
        assert!(!w.snapshot().values().any(|v| String::from_utf8_lossy(v).contains("stale proposal")));
    }

    #[test]
    fn owned_jobs_reject_unknown_conversation_before_execution() {
        let mut w = workspace("Ownership");
        let reply = w.request_test("POST", "/api/jobs", r#"{"kind":"compile","conversationId":"missing","jobId":42}"#, &Execution::default()).unwrap();
        assert_eq!(reply.status, 404);
        let chat = value(&mut w, "POST", "/api/chat/sessions", json!({}), &Execution::default());
        let reply = w.request_test("POST", "/api/jobs", &json!({"kind":"compile","conversationId":chat["id"]}).to_string(), &Execution::default()).unwrap();
        assert_eq!(reply.status, 400);
        let reply = w.request_test("POST", "/api/jobs", &json!({"kind":"compile","conversationId":chat["id"],"jobId":42,"afterN":1u64 << 53}).to_string(), &Execution::default()).unwrap();
        assert_eq!(reply.status, 400, "the cursor must stay within safe JSON event integers");
        assert!(!w.snapshot().keys().any(|k| k.contains("graph/")));
    }

    #[test]
    fn chat_followup_edits_uncompiled_prose_without_lifecycle_work() {
        let mut w=workspace("Alpha");
        w.files.insert("docs/hosting.md".into(),b"# Hosting\n\nCloudFront-S3.\n".to_vec());
        let calls=Arc::new(AtomicUsize::new(0));
        let calls_for_model=calls.clone();
        let expected_hash=crate::model::hash_hex("# Hosting\n\nCloudFront-S3.\n");
        let updated="# Hosting\n\nUse the existing deployment task.\n";
        let e=Execution{model:"fixture".into(),transport:Some(Arc::new(move |body| {
            let request:Value=serde_json::from_str(body).unwrap();
            let messages=request["messages"].as_array().unwrap();
            assert!(!messages.iter().any(|m|m["content"].as_str().is_some_and(|t|t.starts_with("Read task_context and choose"))));
            let n=calls_for_model.fetch_add(1,Ordering::SeqCst);
            if n==0 { return Ok(model_reply("Credentials are sensitive. Resource identifiers are configuration.")); }
            if n==1 {
                return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"write-hosting","type":"function","function":{"name":"write_document","arguments":json!({"path":"docs/hosting.md","text":updated,"baseHash":expected_hash}).to_string()}}]}}]}).to_string());
            }
            assert_eq!(n,2,"unnecessary completion round");
            Ok(model_reply("Updated hosting.md."))
        })),..Default::default()};
        let chat=value(&mut w,"POST","/api/chat/sessions",json!({}),&e);
        let route=format!("/api/chat/sessions/{}/prompt",chat["id"].as_str().unwrap());
        value(&mut w,"POST",&route,json!({"text":"What credentials does a static deployment need?","messageId":"first"}),&e);
        value(&mut w,"POST",&route,json!({"text":"Update the relevant section in hosting.md","messageId":"second"}),&e);
        assert_eq!(calls.load(Ordering::SeqCst),3);
        assert_eq!(w.snapshot()["docs/hosting.md"],updated.as_bytes());
        let again=value(&mut w,"POST",&route,json!({"text":"Update the relevant section in hosting.md","messageId":"second"}),&e);
        assert_eq!(again["messageId"],"second");
        assert_eq!(calls.load(Ordering::SeqCst),3,"retry re-executed a completed turn");
        assert!(!w.snapshot().keys().any(|p|p.contains("graph/entity/")));
        let history=value(&mut w,"GET","/api/chat/history",Value::Null,&e);
        let chat=&history["book"]["chats"][chat["id"].as_str().unwrap()];
        assert_eq!(chat["inputs"]["second"]["state"],"delivered");
        assert_eq!(chat["instruction_hash"].as_str().unwrap().len(),16);
    }

    #[test]
    fn restored_legacy_chat_replaces_harness_context_and_edits_only_named_document() {
        let mut w=workspace("Alpha");
        let original="# Hosting\n\nCloudFront-S3.\n";
        let updated="# Hosting\n\nUse the existing deployment task.\n";
        w.files.insert("docs/hosting.md".into(),original.as_bytes().to_vec());
        let e=Execution::default();let created=value(&mut w,"POST","/api/chat/sessions",json!({}),&e);
        let id=created["id"].as_str().unwrap();
        let mut saved=value(&mut w,"GET","/api/chat/history",Value::Null,&e);
        let legacy=include_str!("../fixtures/legacy-chat-context-v1.txt");
        let first="What deployment credentials are needed?";
        let edit="Update the relevant section in hosting.md.";
        let old_pair=json!({"role":"assistant","content":null,"reasoning_content":"Earlier worker deliberation remains provider history.","tool_calls":[{"id":"old-done","type":"function","function":{"name":"done","arguments":"{}"}}]});
        let old_result=json!({"role":"tool","tool_call_id":"old-done","content":"Committed b6-1. Next unclaimed batch b7-1."});
        let chat=&mut saved["book"]["chats"][id];
        chat["history"]=json!([
            {"role":"system","content":"Old generic harness"},
            {"role":"user","content":format!("{legacy}\n\n{first}")},old_pair.clone(),old_result.clone(),
            {"role":"user","content":format!("{legacy}\n\n{edit}")},
            {"role":"assistant","content":"Cancelled before the edit."}
        ]);
        chat["updates"]=json!([[0,{"sessionUpdate":"user_message","text":first}],[1,{"sessionUpdate":"user_message","text":edit}]]);
        chat["next_n"]=json!(2);
        chat["history_context_version"]=json!(0);
        value(&mut w,"PUT","/api/chat/history",saved,&e);
        let requests=Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));let captured=requests.clone();
        let expected_hash=crate::model::hash_hex(original);
        let e=Execution{model:"fixture".into(),transport:Some(Arc::new(move|body| {
            let request:Value=serde_json::from_str(body).unwrap();
            let mut requests=captured.lock().unwrap();let round=requests.len();requests.push(request.clone());
            let messages=request["messages"].as_array().unwrap();
            let system=messages[0]["content"].as_str().unwrap();assert!(system.contains("historical evidence"));
            assert!(!messages.iter().any(|message|message["role"]=="user"&&message["content"].as_str().is_some_and(|text|text.starts_with("This project has independent document sets."))));
            assert!(messages.contains(&old_pair));assert!(messages.contains(&old_result));
            if round==0 {return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"read","type":"function","function":{"name":"read_document","arguments":json!({"path":"docs/hosting.md"}).to_string()}}]}}]}).to_string());}
            if round==1 {return Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"write","type":"function","function":{"name":"write_document","arguments":json!({"path":"docs/hosting.md","text":updated,"baseHash":expected_hash}).to_string()}}]}}]}).to_string());}
            assert_eq!(round,2,"unexpected compiler or completion round");Ok(model_reply("Updated hosting.md."))
        })),..Default::default()};
        value(&mut w,"POST",&format!("/api/chat/sessions/{id}/prompt"),json!({"text":"Try again. Do not compile; edit only hosting.md.","messageId":"retry-edit"}),&e);
        assert_eq!(requests.lock().unwrap().len(),3);
        assert_eq!(w.snapshot()["docs/hosting.md"],updated.as_bytes());
        assert!(!w.snapshot().keys().any(|path|path.contains("graph/entity/")||path.ends_with("control.yaml")));
        let saved=value(&mut w,"GET","/api/chat/history",Value::Null,&e);
        let messages=saved["book"]["chats"][id]["history"].as_array().unwrap();
        // Current instructions are supplied separately to GDK, not persisted as
        // human or provider history.
        assert_eq!(messages.iter().filter(|message|message["role"]=="system").count(),0);
        assert_eq!(messages.iter().filter(|message|message["role"]=="user").map(|message|message["content"].as_str().unwrap()).collect::<Vec<_>>(),vec![first,edit,"Try again. Do not compile; edit only hosting.md."]);
    }

    #[test]
    fn chat_histories_roundtrip_and_panics_restore_all_context() {
        let mut a = workspace("Alpha");
        let mut b = workspace("Beta");
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let e = Execution {
            model: "fixture".into(),
            transport: Some(Arc::new(|_| Ok(model_reply("Hello from the fixture.")))),
            events: Some(Arc::new(move |v| sink.lock().unwrap().push(v.clone()))),
            ..Default::default()
        };
        let chat = value(&mut a, "POST", "/api/chat/sessions", json!({}), &e);
        value(
            &mut a,
            "POST",
            &format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap()),
            json!({"text":"hello alpha"}),
            &e,
        );
        let mut restored = Workspace::new(a.snapshot()).unwrap();
        let list = value(&mut restored, "GET", "/api/chat/sessions", Value::Null, &e);
        assert_eq!(list["sessions"].as_array().unwrap().len(), 1);
        assert!(list.to_string().contains("hello alpha"));
        assert_eq!(
            value(&mut b, "GET", "/api/chat/sessions", Value::Null, &e)["sessions"],
            json!([])
        );
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .any(|v| v["type"] == "chat.update"));
        let before = restored.snapshot();
        let panic = Execution {
            transport: Some(Arc::new(|_| {
                crate::llm::add_tokens(999);
                panic!("test callback panic")
            })),
            ..Default::default()
        };
        assert!(restored
            .request_test(
                "POST",
                "/api/chat/sessions/chat-1/prompt",
                "{\"text\":\"panic\"}",
                &panic
            )
            .unwrap_err()
            .contains("rolled back"));
        assert_eq!(restored.snapshot(), before);
        assert!(!crate::vfs::is_scoped());
        let recovery_calls = AtomicUsize::new(0);
        let fresh = Execution {
            transport: Some(Arc::new(move |_| {
                if recovery_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    assert_eq!(crate::llm::tokens_spent(), 0);
                }
                Ok(model_reply("Recovered."))
            })),
            ..Default::default()
        };
        value(
            &mut restored,
            "POST",
            "/api/chat/sessions/chat-1/prompt",
            json!({"text":"recover"}),
            &fresh,
        );
    }

    #[test]
    fn no_hosted_execution_has_no_pause_reason() {
        assert!(pause_reason().is_none());
    }

    #[test]
    fn chat_provider_pause_keeps_its_reason_after_reopening() {
        let mut workspace = workspace("Pause");
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let pause_reason = Arc::new(std::sync::Mutex::new(None));
        let reason = pause_reason.clone();
        let execution = Execution {
            cancel, pause_reason,
            transport: Some(Arc::new(move |body| {
                let request: Value = serde_json::from_str(body).unwrap();
                let names: Vec<_> = request["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap()).collect();
                for tool in ["write_text_file", "read_text_file", "list_files", "release_work"] { assert!(names.contains(&tool), "{tool}: {names:?}"); }
                assert!(!names.contains(&"run_command"));
                *reason.lock().unwrap() = Some("Provider deadline reached.".into());
                flag.store(true, Ordering::Relaxed);
                Err("Provider deadline reached.".into())
            })),
            ..Default::default()
        };
        value(&mut workspace, "POST", "/api/chat/sessions", json!({}), &execution);
        value(&mut workspace, "POST", "/api/chat/sessions/chat-1/prompt", json!({"text":"Generate and test this project."}), &execution);
        let mut restored = Workspace::new(workspace.snapshot()).unwrap();
        let chat = value(&mut restored, "GET", "/api/chat/sessions/chat-1", Value::Null, &Execution::default());
        let terminal = &chat["updates"].as_array().unwrap().last().unwrap()["update"];
        assert_eq!(terminal["stop"], "paused", "{chat}");
        assert_eq!(terminal["error"], "Provider deadline reached.");
    }

    #[test]
    fn incomplete_model_writes_cannot_bypass_commit_gates() {
        let mut w = workspace("Alpha");
        w.files.insert(
            "docs/main.md".into(),
            b"# Alpha\n\nAlpha stores records.\n".to_vec(),
        );
        let errors = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = errors.clone();
        let e = Execution {
            model: "bad-fixture".into(),
            transport: Some(Arc::new(move |body| {
                let request: Value = serde_json::from_str(body).unwrap();
                let tool_messages: Vec<_> = request["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|m| m["role"] == "tool")
                    .collect();
                if !tool_messages.is_empty() {
                    seen.lock()
                        .unwrap()
                        .extend(tool_messages.iter().map(|m| m.to_string()));
                    return Ok(model_reply(
                        "Finished without repairing the missing coverage.",
                    ));
                }
                let actions = [
                    (
                        "upsert_entity",
                        json!({"name":"Alpha","mention":{"section":"docs/main.md#/alpha","quote":"Alpha stores records."}}),
                    ),
                    (
                        "mark_goal_done",
                        json!({"goal":"g:reconcile-section:docs/main.md#/alpha","justification":"This is supposedly complete, without extracting its required statement."}),
                    ),
                    (
                        "done",
                        json!({"summary":"Skip all extraction and coverage checks."}),
                    ),
                ];
                Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(i,(name,args))|json!({"id":format!("bad-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}]}).to_string())
            })),
            ..Default::default()
        };
        let result = value(&mut w, "POST", "/api/compile", json!({}), &e);
        assert!(
            result["results"][0]["body"]["report"]["failed"]
                .as_u64()
                .unwrap()
                + result["results"][0]["body"]["report"]["parked"]
                    .as_u64()
                    .unwrap()
                > 0,
            "{result}"
        );
        let graph = value(&mut w, "GET", "/api/graph", Value::Null, &e);
        assert!(
            graph["entities"].as_object().unwrap().is_empty(),
            "ungated staged entity leaked: {graph}"
        );
        assert!(errors
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.contains("coverage")));
        assert!(
            w.snapshot().contains_key(".jazyk/ds_main/status.yaml"),
            "valid failure state should remain persistable"
        );
    }

    #[test]
    fn cancelled_execution_never_calls_model_or_shell_and_svg_is_self_contained() {
        let mut w = workspace("Alpha");
        let e = Execution {
            transport: Some(Arc::new(|_| panic!("cancelled model called"))),
            cancel: Arc::new(AtomicBool::new(true)),
            ..Default::default()
        };
        value(&mut w, "POST", "/api/chat/sessions", json!({}), &e);
        value(
            &mut w,
            "POST",
            "/api/chat/sessions/chat-1/prompt",
            json!({"text":"cancelled"}),
            &e,
        );
        let chat = value(&mut w, "GET", "/api/chat/sessions/chat-1", Value::Null, &e);
        assert!(chat.to_string().contains("cancelled"));
        let _scope = crate::vfs::Scope::enter(&w.snapshot());
        assert!(crate::render::render_svg("@startuml\nclass Alpha\n@enduml")
            .unwrap()
            .contains("Alpha"));
        assert!(
            crate::render::render_svg("@startuml\n!include /etc/hosts\nclass Alpha\n@enduml")
                .is_err()
        );
        assert!(
            crate::render::render_svg("@startuml\nclass \"<img:/etc/hosts>\"\n@enduml").is_err()
        );
        assert!(crate::render::render_png("<svg />").is_err());
    }

    // A hosted workspace keeps no replay snapshots: a batch in the scope copies
    // nothing, and a request drops a directory an earlier version left behind.
    // Mirrors docs/compiler/hosted-runtime.md#workspace-snapshots.
    #[test]
    fn hosted_requests_write_no_graph_snapshots_and_drop_old_ones() {
        let mut w = workspace("Alpha");
        w.files.insert(".jazyk/ds_main/graph/entities/ent~a.yaml".into(), b"name: A\n".to_vec());
        w.files.insert(".jazyk/ds_main/status.yaml".into(), format!("version: {}\n", crate::store::STORE_VERSION).into_bytes());
        {
            let fs = crate::vfs::Scope::enter(&w.files);
            crate::store::snapshot_graph(&root().join(".jazyk/ds_main"), "b:one");
            assert!(!fs.snapshot().keys().any(|p| p.contains("/snapshots/")));
        }
        w.files.insert(
            ".jazyk/ds_main/snapshots/b_old/graph/entities/ent~a.yaml".into(),
            b"name: A\n".to_vec(),
        );
        w.files.insert(
            ".jazyk/ds_main/snapshots/b_old/status.yaml".into(),
            format!("version: {}\n", crate::store::STORE_VERSION).into_bytes(),
        );
        let reply = w
            .request_test("GET", "/api/capabilities", "", &Execution::default())
            .unwrap();
        assert_eq!(reply.status, 200);
        let files = w.snapshot();
        assert!(!files.keys().any(|p| p.contains("/snapshots/")), "{:?}", files.keys());
        assert!(files.contains_key(".jazyk/ds_main/graph/entities/ent~a.yaml"));
        assert!(files.contains_key(".jazyk/ds_main/status.yaml"));
    }

    #[test]
    fn real_extraction_commits_graph_identity_and_survives_snapshot_reload() {
        let mut w = workspace("Alpha");
        w.files.insert(
            "docs/main.md".into(),
            b"# Alpha\n\nAlpha stores records.\n".to_vec(),
        );
        let called = AtomicUsize::new(0);
        let previews = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let captured = previews.clone();
        let e = Execution {
            trace: Some(Arc::new(move |event| {
                captured
                    .lock()
                    .unwrap()
                    .push(serde_json::to_value(event).unwrap())
            })),
            model: "extract-fixture".into(),
            transport: Some(Arc::new(move |_| {
                if called.fetch_add(1, Ordering::SeqCst) > 0 {
                    return Ok(model_reply(
                        "The remaining judgment is deliberately left incomplete.",
                    ));
                }
                let actions = [
                    (
                        "upsert_entity",
                        json!({"name":"Alpha","mention":{"section":"docs/main.md#/alpha","quote":"Alpha stores records."}}),
                    ),
                    (
                        "upsert_entity",
                        json!({"name":"Records","mention":{"section":"docs/main.md#/alpha","quote":"Alpha stores records."}}),
                    ),
                    (
                        "upsert_requirement",
                        json!({"statement":"Alpha stores records.","facets":[{"facet":"behavior","reasoning":"Alpha performs the storage action."}],"entities":["ent:alpha","ent:records"],"edges":[{"a":"ent:alpha","b":"ent:records","type":"association"}],"section":"docs/main.md#/alpha","quote":"Alpha stores records."}),
                    ),
                    (
                        "set_coverage",
                        json!({"section":"docs/main.md#/alpha","state":"covered","note":"Alpha stores records is recorded as a requirement."}),
                    ),
                    (
                        "mark_goal_done",
                        json!({"goal":"g:reconcile-section:docs/main.md#/alpha","justification":"The statement Alpha stores records is extracted with its entity and verbatim source."}),
                    ),
                    (
                        "done",
                        json!({"summary":"Extracted Alpha stores records with quote provenance."}),
                    ),
                ];
                Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(i,(name,args))|json!({"id":format!("extract-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}]}).to_string())
            })),
            ..Default::default()
        };
        let result = value(&mut w, "POST", "/api/compile", json!({}), &e);
        let events = previews.lock().unwrap();
        let first_preview = events
            .iter()
            .position(|v| {
                v["kind"] == "graphPreview"
                    && v["phase"] == "staged"
                    && v["graph"]["entities"]["ent:alpha"]["name"] == "Alpha"
            })
            .expect("accepted entity is streamed before commit");
        let finished = events
            .iter()
            .position(|v| v["kind"] == "sessionDone")
            .unwrap();
        assert!(first_preview < finished);
        assert!(events
            .iter()
            .any(|v| v["kind"] == "toolResult" && v["activity"] == "Alpha created"));
        drop(events);
        let graph = value(&mut w, "GET", "/api/graph", Value::Null, &e);
        assert_eq!(
            graph["entities"]["ent:ds_main::alpha"]["name"], "Alpha",
            "{result}: {graph}"
        );
        assert_eq!(graph["requirements"].as_object().unwrap().len(), 1);
        let snapshot = w.snapshot();
        assert!(!snapshot
            .keys()
            .any(|p| p.starts_with(".jazyk/ds_main/preview/")));
        assert!(
            !snapshot.keys().any(|p| p.contains("/snapshots/")),
            "a hosted batch writes no graph snapshot"
        );
        assert!(snapshot
            .keys()
            .any(|p| p.starts_with(".jazyk/ds_main/journal/")));
        assert!(
            snapshot.keys().any(|p| p.ends_with(".svg")),
            "hosted compilation should retain diagram output"
        );
        let mut restored = Workspace::new(snapshot).unwrap();
        assert_eq!(
            value(
                &mut restored,
                "GET",
                "/api/graph",
                Value::Null,
                &Execution::default()
            ),
            graph
        );
        let mut other = workspace("Beta");
        assert!(value(
            &mut other,
            "GET",
            "/api/graph",
            Value::Null,
            &Execution::default()
        )["entities"]
            .as_object()
            .unwrap()
            .is_empty());
    }
    #[test]
    fn stop_discards_staged_coverage_and_late_done_without_retry_then_build_resumes() {
        let mut w = workspace("Alpha");
        let e = navigation_model("Alpha", Arc::new(AtomicUsize::new(0)), None);
        value(&mut w, "POST", "/api/jobs", json!({"kind":"compile"}), &e);
        let baseline = w.snapshot();
        w.files.insert(
            "docs/main.md".into(),
            b"# Alpha\n\nUpdated navigation index.\n".to_vec(),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let stopping = Execution {
            cancel: cancel.clone(),
            model: "Alpha".into(),
            transport: Some(Arc::new(move |_| {
                let n = seen.fetch_add(1, Ordering::SeqCst);
                let actions = if n == 0 {
                    vec![(
                        "set_coverage",
                        json!({"section":"docs/main.md#/alpha", "state":"non-normative", "note":"Navigation."}),
                    )]
                } else {
                    flag.store(true, Ordering::SeqCst);
                    vec![
                        (
                            "mark_goal_done",
                            json!({"goal":"g:reconcile-section:docs/main.md#/alpha", "justification":"Done."}),
                        ),
                        ("done", json!({"summary":"late completion"})),
                    ]
                };
                Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(i,(name,args))| json!({"id":format!("call-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}]}).to_string())
            })),
            ..Default::default()
        };
        value(
            &mut w,
            "POST",
            "/api/jobs",
            json!({"kind":"compile"}),
            &stopping,
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "stop must not retry or dispatch the late done"
        );
        for (path, bytes) in baseline.iter().filter(|(p, _)| p.contains("/journal/")) {
            assert_eq!(
                w.files.get(path),
                Some(bytes),
                "earlier commit changed: {path}"
            );
        }
        let stopped = value(
            &mut w,
            "GET",
            "/api/status",
            Value::Null,
            &Execution::default(),
        );
        assert_ne!(stopped["verdict"]["state"], "converged");
        let resumed = Arc::new(AtomicUsize::new(0));
        let e = navigation_model("Alpha", resumed.clone(), None);
        value(&mut w, "POST", "/api/jobs", json!({"kind":"compile"}), &e);
        assert!(
            resumed.load(Ordering::SeqCst) > 0,
            "staged coverage must have been discarded"
        );
        let before = resumed.load(Ordering::SeqCst);
        value(&mut w, "POST", "/api/jobs", json!({"kind":"compile"}), &e);
        assert_eq!(
            resumed.load(Ordering::SeqCst),
            before,
            "completed resume must be a no-op"
        );
    }

}
