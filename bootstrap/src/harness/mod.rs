//! The shared Goose GDK runtime. Host callbacks run on the session's owning thread.

use std::{collections::BTreeMap, future::Future, pin::Pin};

use goose_provider_types::conversation::Conversation;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod provider;
mod runtime;

pub use runtime::{
    run_loop, run_loop_async, run_loop_with_instructions, run_with_host, run_with_host_async,
    run_with_hosts, run_with_hosts_async, MemoryRecovery,
};

pub type ModelDeltaSink = std::sync::Arc<dyn Fn(&str, &str) + Send + Sync>;

pub type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

#[derive(Clone, Debug)]
pub struct GenericTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug)]
pub enum AgentEvent {
    Input(Value),
    Timing(Value),
    Thought(String),
    Message(String),
    ToolCallStart {
        id: String,
        name: String,
        args: Value,
    },
    ToolCallEnd {
        id: String,
        result: String,
        ok: bool,
    },
    Usage {
        tokens: u64,
        cached: u64,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Stop {
    EndTurn,
    MaxRounds,
    Cancelled,
    Suspended,
    Error(String),
}

pub struct LoopArgs<'a> {
    pub llm: &'a crate::llm::Llm,
    pub history: &'a mut Vec<Value>,
    pub tools: &'a [GenericTool],
    pub dispatch: &'a mut dyn FnMut(&str, &Value) -> Result<String, String>,
    pub emit: &'a mut dyn FnMut(AgentEvent),
    pub cancelled: &'a dyn Fn() -> bool,
    pub max_rounds: u32,
    pub label: String,
    pub conversation: bool,
    pub inputs: Option<&'a mut dyn FnMut() -> Vec<Value>>,
    pub input_close: Option<&'a mut dyn FnMut() -> bool>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelPurpose {
    Inference,
    Compaction,
}

#[derive(Clone)]
pub struct ModelRequest {
    pub body: Value,
    pub attempt_id: String,
    pub purpose: ModelPurpose,
    pub deltas: Option<ModelDeltaSink>,
}

impl std::fmt::Debug for ModelRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelRequest")
            .field("body", &self.body)
            .field("attempt_id", &self.attempt_id)
            .field("purpose", &self.purpose)
            .field("deltas", &self.deltas.is_some())
            .finish()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModelDeltaProgress {
    pub content: String,
    pub thought: String,
}

#[derive(Clone, Debug)]
pub struct ModelResponse {
    pub body: Value,
}

pub trait ModelHost {
    fn infer<'a>(
        &'a mut self,
        request: ModelRequest,
    ) -> LocalFuture<'a, Result<ModelResponse, String>>;
}

pub trait ToolHost {
    fn call<'a>(
        &'a mut self,
        name: &'a str,
        args: &'a Value,
    ) -> LocalFuture<'a, Result<String, String>>;
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointPhase {
    BeforeModel,
    SavedModel,
    BeforeTool,
    AfterTool,
    Compacted,
    Transition,
    Input,
    Suspended,
    Finished,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckpointBoundary {
    pub phase: CheckpointPhase,
    pub operation_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolIntention {
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolReceipt {
    pub result: String,
    pub ok: bool,
}

/// Owned data only. Recovery hosts pair this with their MCP and workspace capsules.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HarnessState {
    pub version: u32,
    pub run_id: String,
    pub transition: u64,
    pub rounds: u32,
    pub conversation: Conversation,
    #[serde(default)]
    pub model_requests: BTreeMap<String, Value>,
    pub model_receipts: BTreeMap<String, Value>,
    #[serde(default)]
    pub model_deltas: BTreeMap<String, ModelDeltaProgress>,
    pub tool_receipts: BTreeMap<String, ToolReceipt>,
    pub pending_tool: Option<ToolIntention>,
    pub delivered_inputs: Vec<String>,
    #[serde(default)]
    pub pending_inputs: Vec<Value>,
    #[serde(default)]
    pub consecutive_tool_errors: u32,
    #[serde(default)]
    pub failed_tool_transition: Option<u64>,
    pub usage: Vec<goose_provider_types::conversation::token_usage::ProviderUsage>,
    #[serde(default)]
    pub compacted_at_round: Option<u32>,
}

pub trait Recovery {
    fn load<'a>(&'a mut self) -> LocalFuture<'a, Result<Option<HarnessState>, String>>;
    fn checkpoint<'a>(
        &'a mut self,
        boundary: CheckpointBoundary,
        state: &'a HarnessState,
    ) -> LocalFuture<'a, Result<(), String>>;
    fn suspend_requested(&self) -> bool {
        false
    }
}

pub(crate) fn push_user_text(history: &mut Vec<Value>, text: &str) {
    if let Some(last) = history.last_mut().filter(|m| m["role"] == "user") {
        if let Some(previous) = last["content"].as_str() {
            last["content"] = Value::String(format!("{previous}\n\n{text}"));
            if let Some(object) = last.as_object_mut() {
                object.remove("_jazykGdk");
            }
            return;
        }
    }
    history.push(serde_json::json!({"role":"user","content":text}));
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
