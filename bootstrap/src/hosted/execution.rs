//! The execution profile a host request runs under: models, events, inputs, and
//! durable checkpoints. See docs/compiler/hosted-runtime.md#execution-profile.
use super::{Execution, ModelDeltaSink};
use crate::session::{Trace, TraceLevel};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::sync::{atomic::Ordering, Arc};

thread_local! { static EXECUTION: RefCell<Option<Execution>> = const { RefCell::new(None) }; }
thread_local! { static CHECKPOINT_SEQUENCE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
pub(super) struct ExecutionScope(Option<Execution>, u64);
#[cfg(test)]
pub(crate) fn test_execution_scope(execution: Execution) -> impl Drop {
    ExecutionScope::enter(execution)
}
impl ExecutionScope {
    pub(super) fn enter(execution: Execution) -> Self {
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
pub(super) fn execution() -> Execution {
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

pub(crate) async fn transport_async(
    attempt_id: &str,
    body: &str,
    deltas: Option<ModelDeltaSink>,
) -> Result<String, String> {
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
    EXECUTION.with(|e| {
        e.borrow()
            .as_ref()
            .and_then(|execution| execution.pause_reason.lock().unwrap().clone())
    })
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
    crate::llm::Llm {
        api: "chat_completions".into(),
        reasoning_summary: None,
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
    EXECUTION
        .with(|e| e.borrow().as_ref().and_then(|e| e.inputs.clone()))
        .map(|source| source(session))
        .unwrap_or_default()
}
pub fn close_inputs(session: &str) -> bool {
    EXECUTION
        .with(|e| e.borrow().as_ref().and_then(|e| e.input_close.clone()))
        .map(|close| close(session))
        .unwrap_or(true)
}
