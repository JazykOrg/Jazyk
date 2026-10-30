//! Application checkpoints around GDK. The host atomically saves this capsule and VFS.
use crate::harness::{CheckpointBoundary, HarnessState, LocalFuture, Recovery, ToolHost};
use crate::mcp::McpServer;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap};

const GDK_VERSION: &str = "0.1.0-alpha.13";
const OPERATIONS: &str = "jazyk-gdk-1";

#[derive(Clone, Serialize, Deserialize)]
struct Frame {
    key: String,
    harness: HarnessState,
    mcp: Value,
}

#[derive(Clone, Default)]
struct State {
    frames: Vec<Frame>,
    parent: BTreeMap<String, Value>,
    delivered: Vec<String>,
    error: Option<String>,
}

thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

pub(crate) struct Scope(State);
impl Scope {
    pub(crate) fn enter(resume: Option<&Value>) -> Self {
        let state = resume.map(decode).transpose().unwrap_or_else(|error| {
            Some(State { error: Some(error), ..Default::default() })
        }).unwrap_or_default();
        Self(STATE.with(|s| s.replace(state)))
    }
}
impl Drop for Scope {
    fn drop(&mut self) { STATE.with(|s| s.replace(std::mem::take(&mut self.0))); }
}

fn incompatible(message: impl std::fmt::Display) -> String {
    format!("checkpoint-incompatible: {message}")
}

fn decode(value: &Value) -> Result<State, String> {
    if value["version"] != 1 || value["gdkVersion"] != GDK_VERSION
        || value["operationSet"] != OPERATIONS {
        return Err(incompatible("Unsupported execution capsule, GDK version or operation set"));
    }
    let frames: Vec<Frame> = serde_json::from_value(value["frames"].clone()).map_err(incompatible)?;
    if frames.iter().any(|frame| frame.harness.version != 1) {
        return Err(incompatible("Unsupported GDK state version"));
    }
    Ok(State {
        frames,
        parent: serde_json::from_value(value["parent"].clone()).map_err(incompatible)?,
        delivered: serde_json::from_value(value["deliveredInputs"].clone()).map_err(incompatible)?,
        error: None,
    })
}

/// Release preflight uses the same validation as an executing worker.
pub fn validate(value: &Value) -> Result<(), String> { decode(value).map(|_| ()) }

pub(crate) fn parent(key: &str) -> Result<Option<Value>, String> {
    STATE.with(|s| {
        let state = s.borrow();
        if let Some(error) = &state.error { return Err(error.clone()); }
        Ok(state.parent.get(key).cloned())
    })
}

pub(crate) fn set_parent(key: &str, value: Value) {
    STATE.with(|s| { s.borrow_mut().parent.insert(key.into(), value); });
}

pub(crate) fn remove_parent(key: &str) {
    STATE.with(|s| { s.borrow_mut().parent.remove(key); });
}

pub(crate) fn remove_frame(key: &str) {
    STATE.with(|s| { s.borrow_mut().frames.retain(|frame| frame.key != key); });
}

pub(crate) fn command_key(base: &str) -> String {
    STATE.with(|s| {
        let state = s.borrow();
        // Only the command tool creates a new compiler invocation. A saved
        // inner batch can have its own pending mutation when the command resumes.
        match state.frames.iter().rev().filter_map(|frame| frame.harness.pending_tool.as_ref())
            .find(|tool| tool.name == "compile_document_sets") {
            Some(tool) => format!("{base}:{}", tool.id),
            None => base.to_string(),
        }
    })
}

pub(crate) fn is_interruption(reason: &str) -> bool {
    reason == "suspended" || ["checkpoint-incompatible:", "checkpoint-failed:", "execution-uncertain:"]
        .iter().any(|prefix| reason.starts_with(prefix))
}

pub(crate) fn batch_key(out: &std::path::Path, id: &str) -> String {
    format!("batch:{}:{id}", out.display())
}

pub(crate) fn saved_usage(key: &str) -> Option<(u64, u64, u32)> {
    STATE.with(|s| {
        let state = s.borrow();
        let frame = state.frames.iter().find(|f| f.key == key)?;
        let tokens = frame.harness.usage.iter().map(|u| u.usage.total_tokens
            .unwrap_or_else(|| u.usage.input_tokens.unwrap_or(0) + u.usage.output_tokens.unwrap_or(0))
            .max(0) as u64).sum();
        let cached = frame.harness.usage.iter().map(|u| u.usage.cache_read_input_tokens.unwrap_or(0).max(0) as u64).sum();
        Some((tokens, cached, frame.harness.rounds))
    })
}

fn capsule() -> Result<Value, String> {
    STATE.with(|s| {
        let state = s.borrow();
        if let Some(error) = &state.error { return Err(error.clone()); }
        let active = state.frames.last();
        Ok(json!({"version":1,"gdkVersion":GDK_VERSION,"operationSet":OPERATIONS,
            "engineVersion":env!("CARGO_PKG_VERSION"), "frames":state.frames,
            "harness":active.map(|f| &f.harness),"mcp":active.map(|f| &f.mcp),
            "parent":state.parent,"deliveredInputs":state.delivered}))
    })
}

async fn save() -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if crate::vfs::is_scoped() {
            // A running turn is absent from the ordinary history until it returns.
            // Its GDK frame is authoritative, while accepted input and event cursors
            // belong to the application book and must survive the same checkpoint.
            let history = crate::conversation::export_history();
            crate::vfs::write("/project/.jazyk/conversations.json", history["book"].to_string())
                .map_err(|e| e.to_string())?;
            crate::hosted::checkpoint(capsule()?).await?;
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn checkpoint_parent() -> Result<(), String> { async_io::block_on(save()) }
#[cfg(target_arch = "wasm32")]
pub(crate) async fn checkpoint_parent() -> Result<(), String> { save().await }

pub(crate) fn suspension_requested() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    { crate::hosted::suspension_requested() }
    #[cfg(target_arch = "wasm32")]
    { false }
}

pub(crate) struct McpRecovery<'a> {
    server: &'a McpServer,
    key: String,
    loaded: Option<HarnessState>,
}
impl<'a> McpRecovery<'a> {
    pub(crate) fn new(server: &'a McpServer, key: String) -> Result<Self, String> {
        let frame = STATE.with(|s| {
            let state = s.borrow();
            if let Some(error) = &state.error { return Err(error.clone()); }
            Ok(state.frames.iter().find(|f| f.key == key).cloned())
        })?;
        let loaded = match frame {
            Some(frame) => {
                server.restore(frame.mcp).map_err(incompatible)?;
                let mut harness = frame.harness;
                // The saved VFS and MCP state precede this local operation. No
                // later local effect exists outside that atomic checkpoint. A nested
                // compile retains its own frame and parent continuation in the capsule.
                // Remote task effects need destination receipts and remain uncertain.
                if harness.pending_tool.as_ref().is_some_and(|tool|
                    !matches!(tool.name.as_str(), "run_task" | "cancel_run")) {
                    harness.pending_tool = None;
                }
                Some(harness)
            }
            None => None,
        };
        Ok(Self { server, key, loaded })
    }
    pub(crate) fn resumed(&self) -> bool { self.loaded.is_some() }
    pub(crate) fn finish(&self) {
        STATE.with(|s| s.borrow_mut().frames.retain(|f| f.key != self.key));
    }
}
impl Recovery for McpRecovery<'_> {
    fn load<'a>(&'a mut self) -> LocalFuture<'a, Result<Option<HarnessState>, String>> {
        Box::pin(async move { Ok(self.loaded.take()) })
    }
    fn checkpoint<'a>(&'a mut self, boundary: CheckpointBoundary, harness: &'a HarnessState)
        -> LocalFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let frame = Frame { key: self.key.clone(), harness: harness.clone(), mcp: self.server.checkpoint()? };
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                for id in &harness.delivered_inputs {
                    if !state.delivered.contains(id) { state.delivered.push(id.clone()); }
                }
                if let Some(previous) = state.frames.iter_mut().find(|f| f.key == self.key) {
                    *previous = frame;
                } else { state.frames.push(frame); }
            });
            let _ = boundary;
            save().await
        })
    }
    fn suspend_requested(&self) -> bool { suspension_requested() }
}

pub(crate) struct McpTools<'a>(pub &'a McpServer);
impl ToolHost for McpTools<'_> {
    fn call<'a>(&'a mut self, name: &'a str, args: &'a Value) -> LocalFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let result = self.0.handle_async("tools/call", &json!({"name":name,"arguments":args}))
                .await.map_err(|(_, message)| message)?;
            // Preserve structured content and metadata as well as display text.
            let text = result.to_string();
            if result["isError"].as_bool().unwrap_or(false) { Err(text) } else { Ok(text) }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parent_only_capsule_and_nested_scopes_preserve_progress_and_reject_unknown_versions() {
        let _outer = Scope::enter(None);
        set_parent("compile:one", json!({"sessions":7,"activeBatch":"batch1"}));
        let saved = capsule().unwrap();
        assert!(saved["harness"].is_null());
        validate(&saved).unwrap();
        { let _inner = Scope::enter(Some(&saved));
          assert_eq!(parent("compile:one").unwrap().unwrap()["sessions"], 7);
          set_parent("compile:one", json!({"sessions":8})); }
        assert_eq!(parent("compile:one").unwrap().unwrap()["sessions"], 7);
        let mut future = saved;
        future["operationSet"] = json!("future");
        assert!(validate(&future).unwrap_err().starts_with("checkpoint-incompatible:"));
        let _incompatible = Scope::enter(Some(&future));
        assert!(parent("compile:one").is_err());
    }
}
