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
    config_revision: std::sync::Mutex<String>,
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
    // What the transport saw while a call runs: cancelled request ids and end of
    // input, so a long poll stops waiting. Mirrors docs/frontends/mcp.md#the-work-loop.
    interrupts: std::sync::Arc<Interrupts>,
    // The guard and feedback cap across per-call sessions outside a batch, keyed by
    // the ledger claim they ran under. Mirrors docs/compiler/sessions.md#repeated-calls.
    call_memory: std::sync::Mutex<(Option<String>, crate::tools::CallMemory)>,
    // The document hash each read_document or write_document of a path last
    // returned: the version a write_document literal edit applies to without
    // baseHash. Mirrors docs/compiler/tools.md#conversational-document-writes.
    doc_hashes: std::sync::Mutex<std::collections::BTreeMap<String, String>>,
    // Whether the transport carries server-to-client notifications (the stdio loop
    // does), and the tool names the client last listed: the baseline a
    // `notifications/tools/list_changed` compares against.
    // Mirrors docs/frontends/mcp.md#the-work-loop.
    notifies: std::sync::atomic::AtomicBool,
    listed: std::sync::Mutex<Option<Vec<String>>>,
}

#[derive(Default)]
struct Interrupts {
    cancelled: std::sync::Mutex<BTreeSet<String>>,
    current: std::sync::Mutex<Option<String>>,
    eof: std::sync::atomic::AtomicBool,
}

impl Interrupts {
    // Record a `notifications/cancelled` the reader saw, alone or inside a batch.
    fn note(&self, message: &Value) {
        match message {
            Value::Array(items) => items.iter().for_each(|m| self.note(m)),
            m if m["method"] == "notifications/cancelled"
                && !m["params"]["requestId"].is_null() =>
            {
                self.cancelled
                    .lock()
                    .unwrap()
                    .insert(m["params"]["requestId"].to_string());
            }
            _ => {}
        }
    }

    // The running call should stop: the client cancelled it or went away.
    fn stop_current(&self) -> bool {
        self.eof.load(std::sync::atomic::Ordering::SeqCst)
            || self
                .current
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|id| self.cancelled.lock().unwrap().contains(id))
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Delivered {
    contract: bool,
    #[serde(default)]
    generation_contract: bool,
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

// The browser backend refuses the benchmark, so it never reads a case's fields.
#[derive(Default)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct BenchRun {
    open: Option<OpenCase>,
    // One entry per finished case: name, tier, score, calls, par, fail.
    scored: Vec<Value>,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
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
    #[serde(default)]
    invalidated: bool,
}

// The compilation lifecycle: served beside the catalog, implemented on the server
// because they own the board and the open changeset.
const LIFECYCLE: [&str; 4] = ["goals", "begin_goals", "done", "abandon_goals"];

mod benchmark;
mod catalog;
mod chat;
mod dispatch;
mod lifecycle;
mod router;
mod server;
mod transport;
mod watch;

pub use chat::{toml_set, try_toml_set};
pub use server::reader_instructions;
pub(crate) use server::SERVING;

// Why a delegated edit did not land: the sink cannot take edits at all (nothing
// listens, or the IDE lacks the capability), or it was asked and failed.
// Mirrors docs/frontends/acp.md#doc-edit-delegation.
#[derive(Debug)]
enum SinkError {
    Unavailable(String),
    // A tab has no sink to fail.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Failed(String),
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
) -> Result<(), SinkError> {
    if crate::vfs::is_scoped() {
        return Err(SinkError::Unavailable(
            "hosted workspaces do not connect to editor sockets".into(),
        ));
    }
    use std::io::{BufRead, BufReader, Write};
    let mut stream = crate::local_socket::UnixStream::connect(sink)
        .map_err(|e| SinkError::Unavailable(e.to_string()))?;
    let failed = |e: std::io::Error| SinkError::Failed(e.to_string());
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(failed)?;
    let line = json!({"path": path.display().to_string(), "oldText": old_text, "newText": new_text, "content": full});
    writeln!(stream, "{}", line).map_err(failed)?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(failed)?;
    let v: Value = serde_json::from_str(reply.trim())
        .map_err(|_| SinkError::Failed("the editor sink sent no acknowledgement".into()))?;
    if v["ok"].as_bool().unwrap_or(false) {
        return Ok(());
    }
    let message = v["error"]
        .as_str()
        .unwrap_or("sink refused the edit")
        .to_string();
    Err(if v["unavailable"] == true {
        SinkError::Unavailable(message)
    } else {
        SinkError::Failed(message)
    })
}

// A tab has no unix socket, so there is no editor to hand the edit to.
#[cfg(target_arch = "wasm32")]
fn sink_write(
    _sink: &str,
    _path: &std::path::Path,
    _old_text: &str,
    _new_text: &str,
    _full: &str,
) -> Result<(), SinkError> {
    Err(SinkError::Unavailable(
        "the browser backend has no editor sink".to_string(),
    ))
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
            for key in ["suspendReason", "error", "stopped"] {
                if let Some(reason) = object
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|reason| crate::session_recovery::is_interruption(reason))
                {
                    return Some(reason.to_owned());
                }
            }
            object.values().find_map(compile_interruption).or_else(|| {
                ["verdict", "state", "stop"]
                    .iter()
                    .any(|key| object.get(*key).and_then(Value::as_str) == Some("suspended"))
                    .then(|| "suspended".into())
            })
        }
        Value::Array(values) => values.iter().find_map(compile_interruption),
        _ => None,
    }
}

// The longest await_changes waits, in seconds (docs/frontends/mcp.md#the-work-loop).
const AWAIT_CAP_SECS: u64 = 3600;

// The prefix of the JSON-RPC error for a tool the serving does not serve.
const UNKNOWN_TOOL: &str = "unknown or disabled tool";

// The MCP protocol versions the serving speaks, oldest first.
const PROTOCOL_VERSIONS: [&str; 4] = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

// The requested version when supported, the latest supported otherwise, and the
// oldest for a client that names none. Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
fn negotiate_protocol(requested: &Value) -> &'static str {
    match requested.as_str() {
        None => PROTOCOL_VERSIONS[0],
        Some(v) => PROTOCOL_VERSIONS
            .iter()
            .find(|p| **p == v)
            .copied()
            .unwrap_or(PROTOCOL_VERSIONS[PROTOCOL_VERSIONS.len() - 1]),
    }
}

// A project tool's reply, pretty-printed like every other reply (docs/frontends/mcp.md#toolsets).
fn result_value(value: Value) -> Value {
    text_result(value, false)
}

#[cfg(test)]
mod dispatch_tests;

#[cfg(test)]
mod tests;
