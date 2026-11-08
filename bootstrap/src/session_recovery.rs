//! Application checkpoints around GDK. The host atomically saves this capsule and VFS.
use crate::harness::{CheckpointBoundary, HarnessState, LocalFuture, Recovery, ToolHost};
use crate::mcp::McpServer;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap};

const GDK_VERSION: &str = "0.1.0-alpha.13";
const OPERATIONS: &str = "jazyk-gdk-3";

#[derive(Clone, Serialize, Deserialize)]
struct Frame {
    key: String,
    boundary: CheckpointBoundary,
    harness: HarnessState,
    mcp: Value,
}

#[derive(Clone, Default)]
struct State {
    frames: Vec<Frame>,
    parent: BTreeMap<String, Value>,
    delivered: Vec<String>,
    inputs: BTreeMap<String, Value>,
    claims: Vec<crate::control::OwnedLease>,
    error: Option<String>,
}

thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
thread_local! { static ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

pub(crate) struct Scope(State, bool);
impl Scope {
    pub(crate) fn enter(resume: Option<&Value>) -> Self {
        let state = resume
            .map(decode)
            .transpose()
            .unwrap_or_else(|error| {
                Some(State {
                    error: Some(error),
                    ..Default::default()
                })
            })
            .unwrap_or_default();
        let scope = Self(
            STATE.with(|s| s.replace(state)),
            ACTIVE.with(|a| a.replace(true)),
        );
        if resume.is_some() && crate::vfs::is_scoped() {
            let claims = STATE.with(|s| {
                let state = s.borrow();
                if state.error.is_none() {
                    state.claims.clone()
                } else {
                    Default::default()
                }
            });
            for claim in claims {
                if let Err(error) = crate::control::release_owned_lease(&claim) {
                    STATE.with(|s| {
                        s.borrow_mut().error = Some(incompatible(format!(
                            "Cannot hand over saved control claim: {error}"
                        )))
                    });
                    break;
                }
            }
        }
        scope
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        STATE.with(|s| s.replace(std::mem::take(&mut self.0)));
        ACTIVE.with(|a| a.set(self.1));
    }
}

pub(crate) fn active() -> bool {
    ACTIVE.with(|a| a.get())
}

fn incompatible(message: impl std::fmt::Display) -> String {
    format!("checkpoint-incompatible: {message}")
}

fn decode(value: &Value) -> Result<State, String> {
    if value["version"] != 1
        || value["gdkVersion"] != GDK_VERSION
        || value["operationSet"] != OPERATIONS
    {
        return Err(incompatible(
            "Unsupported execution capsule, GDK version or operation set",
        ));
    }
    let frames: Vec<Frame> =
        serde_json::from_value(value["frames"].clone()).map_err(incompatible)?;
    if frames.iter().any(|frame| frame.harness.version != 1) {
        return Err(incompatible("Unsupported GDK state version"));
    }
    Ok(State {
        frames,
        parent: serde_json::from_value(value["parent"].clone()).map_err(incompatible)?,
        delivered: serde_json::from_value(value["deliveredInputs"].clone())
            .map_err(incompatible)?,
        inputs: serde_json::from_value(
            value
                .get("deliveredInputValues")
                .cloned()
                .unwrap_or_else(|| json!({})),
        )
        .map_err(incompatible)?,
        claims: serde_json::from_value(value.get("claims").cloned().unwrap_or_else(|| json!([])))
            .map_err(incompatible)?,
        error: None,
    })
}

/// Release preflight uses the same validation as an executing worker.
pub fn validate(value: &Value) -> Result<(), String> {
    decode(value).map(|_| ())
}

/// Claim receipts cover parent builds, nested MCP batches and ledger workers.
pub(crate) fn claimed(claim: crate::control::OwnedLease) {
    if !active() || !crate::vfs::is_scoped() {
        return;
    }
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state
            .claims
            .retain(|old| old.out != claim.out || old.lease.task != claim.lease.task);
        state.claims.push(claim);
    });
}

pub(crate) fn owned_claim(out: &std::path::Path, task: &str) -> Option<crate::control::OwnedLease> {
    STATE.with(|s| {
        s.borrow()
            .claims
            .iter()
            .find(|claim| claim.out == out && claim.lease.task == task)
            .cloned()
    })
}

pub(crate) fn released(out: &std::path::Path, task: &str) {
    if !active() || !crate::vfs::is_scoped() {
        return;
    }
    STATE.with(|s| {
        s.borrow_mut()
            .claims
            .retain(|claim| claim.out != out || claim.lease.task != task)
    });
}

/// Preserve accepted chat context and release claims without running MCP state.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn stop_owned(value: &Value) -> Result<(), String> {
    if !crate::vfs::is_scoped() {
        return Err("Stop recovery requires a scoped workspace".into());
    }
    let state = decode(value)?;
    for claim in state.claims {
        crate::control::release_owned_lease(&claim)?;
    }
    for frame in state.frames {
        if frame.key.starts_with("chat:") {
            crate::conversation::restore_stopped_history(
                &frame.key,
                crate::harness::stopped_history(&frame.harness),
            );
        }
    }
    Ok(())
}

pub(crate) fn parent(key: &str) -> Result<Option<Value>, String> {
    STATE.with(|s| {
        let state = s.borrow();
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        Ok(state.parent.get(key).cloned())
    })
}

pub(crate) fn set_parent(key: &str, value: Value) {
    STATE.with(|s| {
        s.borrow_mut().parent.insert(key.into(), value);
    });
}

pub(crate) fn input_delivered(id: &str, input: &Value) {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        if !state.delivered.iter().any(|existing| existing == id) {
            state.delivered.push(id.to_owned());
        }
        state.inputs.insert(id.to_owned(), input.clone());
    });
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn replay_delta(session: &str, field: &str, text: &str) -> Result<(), String> {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let Some(frame) = state
            .frames
            .iter_mut()
            .rev()
            .find(|frame| frame.key.starts_with(&format!("chat:{session}:")))
        else {
            return Ok(());
        };
        if !matches!(
            frame.boundary.phase,
            crate::harness::CheckpointPhase::BeforeModel
                | crate::harness::CheckpointPhase::SavedModel
        ) {
            return Ok(());
        }
        let Some(request_id) = &frame.boundary.operation_id else {
            return Ok(());
        };
        let prefix = frame
            .harness
            .model_deltas
            .entry(request_id.clone())
            .or_default();
        if prefix
            .content
            .len()
            .saturating_add(prefix.thought.len())
            .saturating_add(text.len())
            > 16 * 1024 * 1024
        {
            return Err(
                "execution-uncertain: retained model prefix exceeds the response limit".into(),
            );
        }
        match field {
            "agent_message_chunk" => prefix.content.push_str(text),
            "agent_thought_chunk" => prefix.thought.push_str(text),
            _ => {}
        }
        Ok(())
    })
}

pub(crate) fn remove_frame(key: &str) {
    STATE.with(|s| {
        s.borrow_mut().frames.retain(|frame| frame.key != key);
    });
}

pub(crate) fn command_key(base: &str) -> String {
    STATE.with(|s| {
        let state = s.borrow();
        // Only the command tool creates a new compiler invocation. A saved
        // inner batch can have its own pending mutation when the command resumes.
        match state
            .frames
            .iter()
            .rev()
            .filter_map(|frame| frame.harness.pending_tool.as_ref())
            .find(|tool| tool.name == "compile_document_sets")
        {
            Some(tool) => format!("{base}:{}", tool.id),
            None => base.to_string(),
        }
    })
}

// A task belongs to the model tool invocation that caused it, not every later
// invocation in the same compiler command. The run id also separates batches
// whose providers reuse a tool id. Both ids survive exact checkpoint replay.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn task_key(base: &str) -> String {
    let command = command_key(base);
    STATE.with(|s| {
        s.borrow()
            .frames
            .iter()
            .rev()
            .find_map(|frame| {
                frame
                    .harness
                    .pending_tool
                    .as_ref()
                    .map(|tool| format!("{command}:tool:{}:{}", frame.harness.run_id, tool.id))
            })
            .unwrap_or(command)
    })
}

pub(crate) fn is_interruption(reason: &str) -> bool {
    reason == "suspended"
        || [
            "checkpoint-incompatible:",
            "checkpoint-failed:",
            "execution-uncertain:",
        ]
        .iter()
        .any(|prefix| reason.contains(prefix))
}

pub(crate) fn batch_key(out: &std::path::Path, id: &str) -> String {
    format!("batch:{}:{id}", out.display())
}

pub(crate) fn saved_usage(key: &str) -> Option<(u64, u64, u32)> {
    STATE.with(|s| {
        let state = s.borrow();
        let frame = state.frames.iter().find(|f| f.key == key)?;
        let tokens = frame
            .harness
            .usage
            .iter()
            .map(|u| {
                u.usage
                    .total_tokens
                    .unwrap_or_else(|| {
                        u.usage.input_tokens.unwrap_or(0) + u.usage.output_tokens.unwrap_or(0)
                    })
                    .max(0) as u64
            })
            .sum();
        let cached = frame
            .harness
            .usage
            .iter()
            .map(|u| u.usage.cache_read_input_tokens.unwrap_or(0).max(0) as u64)
            .sum();
        Some((tokens, cached, frame.harness.rounds))
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn capsule() -> Result<Value, String> {
    STATE.with(|s| {
        let state = s.borrow();
        if let Some(error) = &state.error { return Err(error.clone()); }
        let active = state.frames.last();
        Ok(json!({"version":1,"gdkVersion":GDK_VERSION,"operationSet":OPERATIONS,
            "engineVersion":env!("CARGO_PKG_VERSION"), "frames":state.frames,
            "harness":active.map(|f| &f.harness),"mcp":active.map(|f| &f.mcp),
            "parent":state.parent,"claims":state.claims,"deliveredInputs":state.delivered,"deliveredInputValues":state.inputs}))
    })
}

async fn save() -> Result<(), String> {
    if !active() {
        return Ok(());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if crate::vfs::is_scoped() {
            // A running turn is absent from the ordinary history until it returns.
            // Its GDK frame is authoritative, while accepted input and event cursors
            // belong to the application book and must survive the same checkpoint.
            let history = crate::conversation::export_history();
            crate::vfs::write(
                "/project/.jazyk/conversations.json",
                history["book"].to_string(),
            )
            .map_err(|e| e.to_string())?;
            crate::hosted::checkpoint(capsule()?).await?;
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn checkpoint_parent() -> Result<(), String> {
    async_io::block_on(save())
}
#[cfg(target_arch = "wasm32")]
pub(crate) async fn checkpoint_parent() -> Result<(), String> {
    save().await
}

pub(crate) fn suspension_requested() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::hosted::suspension_requested()
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

pub(crate) struct McpRecovery<'a> {
    server: Option<&'a McpServer>,
    key: String,
    loaded: Option<HarnessState>,
}
impl<'a> McpRecovery<'a> {
    pub(crate) fn new(server: &'a McpServer, key: String) -> Result<Self, String> {
        Self::from_frame(Some(server), key)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn utility(key: String) -> Result<Self, String> {
        Self::from_frame(None, key)
    }
    fn from_frame(server: Option<&'a McpServer>, key: String) -> Result<Self, String> {
        let frame = STATE.with(|s| {
            if !active() {
                return Ok(None);
            }
            let state = s.borrow();
            if let Some(error) = &state.error {
                return Err(error.clone());
            }
            Ok(state.frames.iter().find(|f| f.key == key).cloned())
        })?;
        let loaded =
            match frame {
                Some(frame) => {
                    if let Some(server) = server {
                        server.restore(frame.mcp).map_err(incompatible)?;
                    } else if !frame.mcp.is_null() {
                        return Err(incompatible(
                            "Utility inference cannot restore an MCP session",
                        ));
                    }
                    let mut harness = frame.harness;
                    // The saved VFS and MCP state precede this local operation. No
                    // later local effect exists outside that atomic checkpoint. A nested
                    // compile retains its own frame and parent continuation in the capsule.
                    // Remote task effects need destination receipts and remain uncertain.
                    if harness.pending_tool.as_ref().is_some_and(|tool| {
                        !matches!(tool.name.as_str(), "run_task" | "cancel_run")
                    }) {
                        harness.pending_tool = None;
                    }
                    Some(harness)
                }
                None => None,
            };
        let input_values = STATE.with(|s| s.borrow().inputs.values().cloned().collect::<Vec<_>>());
        crate::conversation::restore_input_outcomes(input_values);
        Ok(Self {
            server,
            key,
            loaded,
        })
    }
    pub(crate) fn resumed(&self) -> bool {
        self.loaded.is_some()
    }
}
impl Recovery for McpRecovery<'_> {
    fn load<'a>(&'a mut self) -> LocalFuture<'a, Result<Option<HarnessState>, String>> {
        Box::pin(async move { Ok(self.loaded.take()) })
    }
    fn checkpoint<'a>(
        &'a mut self,
        boundary: CheckpointBoundary,
        harness: &'a HarnessState,
    ) -> LocalFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if !active() {
                return Ok(());
            }
            let mcp = self
                .server
                .map(McpServer::checkpoint)
                .transpose()?
                .unwrap_or(Value::Null);
            let frame = Frame {
                key: self.key.clone(),
                boundary,
                harness: harness.clone(),
                mcp,
            };
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                for id in &harness.delivered_inputs {
                    if !state.delivered.contains(id) {
                        state.delivered.push(id.clone());
                    }
                }
                for (id, input) in &harness.delivered_input_values {
                    let mut input = input.clone();
                    if input["state"] != "failed" {
                        input["state"] = json!("delivered");
                    }
                    state.inputs.insert(id.clone(), input);
                }
                if let Some(previous) = state.frames.iter_mut().find(|f| f.key == self.key) {
                    *previous = frame;
                } else {
                    state.frames.push(frame);
                }
            });
            save().await
        })
    }
    fn suspend_requested(&self) -> bool {
        suspension_requested()
    }
}

// The in-process serving's tools, plus the conversation's extra tools: skills and the
// host's remote MCP servers (docs/frontends/agent-context.md#hosted-context).
pub(crate) struct McpTools<'a>(pub &'a McpServer, pub &'a crate::agent_context::Extras);
impl<'a> McpTools<'a> {
    // The serving alone: worker sessions stay hermetic.
    pub(crate) fn plain(server: &'a McpServer) -> Self {
        static NONE: std::sync::OnceLock<crate::agent_context::Extras> = std::sync::OnceLock::new();
        Self(server, NONE.get_or_init(Default::default))
    }
}
impl ToolHost for McpTools<'_> {
    fn catalog(
        &mut self,
    ) -> LocalFuture<'_, Result<Option<Vec<crate::harness::GenericTool>>, String>> {
        Box::pin(async move {
            let reply = self
                .0
                .handle_async("tools/list", &json!({}))
                .await
                .map_err(|(_, message)| message)?;
            let mut tools: Vec<crate::harness::GenericTool> = reply["tools"]
                .as_array()
                .ok_or("Invalid MCP tool catalog")?
                .iter()
                .map(|tool| crate::harness::GenericTool {
                    name: tool["name"].as_str().unwrap_or_default().into(),
                    description: tool["description"].as_str().unwrap_or_default().into(),
                    parameters: tool["inputSchema"].clone(),
                })
                .collect();
            for extra in self.1.tools() {
                if !tools.iter().any(|t| t.name == extra.name) {
                    tools.push(extra);
                }
            }
            Ok(Some(tools))
        })
    }
    fn call<'a>(
        &'a mut self,
        name: &'a str,
        args: &'a Value,
    ) -> LocalFuture<'a, Result<String, String>> {
        Box::pin(async move {
            if self.1.handles(name) {
                return self.1.call(name, args);
            }
            let result = self
                .0
                .handle_async("tools/call", &json!({"name":name,"arguments":args}))
                .await
                .map_err(|(_, message)| message)?;
            // Preserve structured content and metadata as well as display text.
            let text = result.to_string();
            if result["isError"].as_bool().unwrap_or(false) {
                Err(text)
            } else {
                Ok(text)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stop_receipts_preserve_foreign_replacements_and_require_scoped_vfs() {
        let saved = {
            let _fs = crate::vfs::Scope::enter(&Default::default());
            let _scope = Scope::enter(None);
            let out = std::path::Path::new("/project/.jazyk/ds_main");
            crate::control::claim(out, "ledger:owned", "worker-one").unwrap();
            crate::control::claim(out, "batch:replaced", "worker-one").unwrap();
            crate::control::claim(out, "batch:renewed", "worker-one").unwrap();
            let saved = capsule().unwrap();
            crate::control::release_lease(out, "batch:replaced");
            crate::control::claim(out, "batch:replaced", "worker-two").unwrap();
            let path = out.join("leases/batch%3Arenewed.yaml");
            let mut renewed: crate::control::Lease =
                serde_norway::from_str(&crate::vfs::read_to_string(&path).unwrap()).unwrap();
            renewed.claimed_at += 1;
            crate::vfs::write(&path, serde_norway::to_string(&renewed).unwrap()).unwrap();
            crate::control::claim(out, "ledger:foreign", "worker-two").unwrap();
            let foreign = crate::vfs::read(out.join("leases/ledger%3Aforeign.yaml")).unwrap();
            stop_owned(&saved).unwrap();
            assert!(!crate::vfs::exists(&out.join("leases/ledger%3Aowned.yaml")));
            assert_eq!(crate::control::leases(out).len(), 3);
            assert_eq!(
                crate::vfs::read(out.join("leases/ledger%3Aforeign.yaml")).unwrap(),
                foreign
            );
            assert_eq!(
                crate::control::leases(out)["batch:replaced"].worker,
                "worker-two"
            );
            assert_eq!(
                crate::control::leases(out)["batch:renewed"].claimed_at,
                renewed.claimed_at
            );
            saved
        };
        assert!(stop_owned(&saved).unwrap_err().contains("scoped workspace"));
    }

    #[test]
    fn parent_only_capsule_and_nested_scopes_preserve_progress_and_reject_unknown_versions() {
        let _outer = Scope::enter(None);
        set_parent("compile:one", json!({"sessions":7,"activeBatch":"batch1"}));
        let saved = capsule().unwrap();
        assert!(saved["harness"].is_null());
        validate(&saved).unwrap();
        let mut legacy = saved.clone();
        legacy["operationSet"] = json!("jazyk-gdk-2");
        assert!(validate(&legacy)
            .unwrap_err()
            .starts_with("checkpoint-incompatible:"));
        {
            let _legacy = Scope::enter(Some(&legacy));
            assert!(parent("compile:one").is_err());
        }
        {
            let _inner = Scope::enter(Some(&saved));
            assert_eq!(parent("compile:one").unwrap().unwrap()["sessions"], 7);
            set_parent("compile:one", json!({"sessions":8}));
        }
        assert_eq!(parent("compile:one").unwrap().unwrap()["sessions"], 7);
        let mut future = saved;
        future["operationSet"] = json!("future");
        assert!(validate(&future)
            .unwrap_err()
            .starts_with("checkpoint-incompatible:"));
        let _incompatible = Scope::enter(Some(&future));
        assert!(parent("compile:one").is_err());
    }
}
