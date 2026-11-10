//! Filesystem-free native compiler embedding. See docs/compiler/hosted-runtime.md.
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};

mod dispatch;
mod execution;
mod projection;
#[cfg(test)]
mod tests;

use dispatch::dispatch;
#[cfg(test)]
pub(crate) use execution::test_execution_scope;
use execution::ExecutionScope;
pub use execution::{
    cancelled, close_inputs, emit_event, host_context, llm, model_name, pause_reason, runtime,
    take_inputs, trace,
};
pub(crate) use execution::{checkpoint, suspension_requested, transport_async};
use projection::project_projection;

pub type Snapshot = BTreeMap<String, Vec<u8>>;
pub type ModelTransport = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;
pub type ModelStreamTransport =
    Arc<dyn Fn(&str, &mut dyn FnMut(&str, &str)) -> Result<String, String> + Send + Sync>;
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
    dyn Fn(
            String,
            ModelDeltaSink,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
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
    pub fn apply_task_result(
        &mut self,
        task: &crate::task_runtime::Task,
        input: &std::collections::BTreeMap<String, String>,
        result: &Value,
        id: &str,
    ) -> Result<Value, String> {
        let fs = crate::vfs::Scope::enter(&self.files);
        let value =
            crate::task_runtime::apply_result(&crate::portable::root(), task, input, result, id)?;
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
        outcome
            .changed
            .into_iter()
            .chain(
                outcome
                    .warnings
                    .into_iter()
                    .map(|w| format!("warning: {w}")),
            )
            .collect()
    }

    /// The MCP servers the project's files propose, for the host to list and an
    /// administrator to enable. Variables stay as written. `hosted` says whether the
    /// hosted profile can use the entry at all (HTTPS only); `hash` changes whenever
    /// the entry does. Mirrors docs/frontends/agent-context.md#hosted-context.
    pub fn agent_mcp_proposals(&self) -> Vec<Value> {
        let _fs = crate::vfs::Scope::enter(&self.files);
        let keep = |name: &str| Some(format!("${{{name}}}"));
        let (servers, warnings) =
            crate::agent_context::servers(&crate::portable::root(), None, &keep);
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
            if method != "GET"
                && (path.contains("/chat/")
                    || path.ends_with("/jobs")
                    || path.ends_with("/compile"))
            {
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
    let Ok(w) = crate::document_sets::Workspace::load(&root()) else {
        return;
    };
    for set in &w.sets {
        let Ok(p) = w.resolve(Some(&set.id)) else {
            continue;
        };
        let dir = p.out.join("snapshots");
        if crate::vfs::is_dir(&dir) {
            crate::vfs::remove_dir_all(&dir).ok();
        }
    }
}

pub fn root() -> PathBuf {
    PathBuf::from("/project")
}
