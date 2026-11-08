use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicU64, Arc},
};

use anyhow::Result;
use async_trait::async_trait;
use futures::FutureExt;
use goose_agent::{
    events::AgentEvent as GdkEvent,
    inference::{InferenceEffect, InferenceRunner},
    machine::{MachineSession, StateMachine, Step},
    operation::{
        self, ConversationEffect, Emitter, MachineEffect, Operation, OperationResult, RunStatus,
    },
    tool::{ToolOperation, ToolProvider},
};
use goose_context_management::{ProviderModel, Templates};
use goose_provider_types::{
    conversation::{
        message::{Message, MessageContent},
        token_usage::ProviderUsage,
        Conversation,
    },
    formats::openai,
    images::ImageFormat,
    model::ModelConfig,
};
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock, ErrorData, Role, Tool};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::{
    provider::{ChannelProvider, DefaultModelHost, HostAction},
    *,
};

const SYSTEM: &str = include_str!("../../../docs/compiler/goals/prompts/embedded-agent.md");
const LEDGER_CONTINUATION: &str =
    include_str!("../../../docs/compiler/goals/prompts/ledger-continuation.md");
const COMPACTION_REQUEST: &str =
    include_str!("../../../docs/compiler/goals/prompts/compaction-request.md");
const COMPACTION_CONTINUATION: &str =
    include_str!("../../../docs/compiler/goals/prompts/compaction-continuation.md");

#[derive(Clone)]
struct Session {
    conversation: Conversation,
}
impl MachineSession for Session {
    fn id(&self) -> &str {
        "jazyk"
    }
    fn conversation(&self) -> Option<&Conversation> {
        Some(&self.conversation)
    }
}

enum Effect {
    Conversation(ConversationEffect),
    Usage(ProviderUsage),
}
impl From<Message> for Effect {
    fn from(message: Message) -> Self {
        Self::Conversation(message.into())
    }
}
impl MachineEffect for Effect {
    fn ensure_message_ids(&mut self) {
        if let Self::Conversation(effect) = self {
            effect.ensure_message_ids();
        }
    }
}
impl InferenceEffect for Effect {
    fn record_usage(usage: ProviderUsage) -> Self {
        Self::Usage(usage)
    }
}

struct DynamicTools {
    definitions: Vec<Tool>,
    tx: mpsc::UnboundedSender<HostAction>,
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl ToolProvider<Session> for DynamicTools {
    async fn tools(&self, _: &Session) -> Result<Vec<Tool>> {
        Ok(self.definitions.clone())
    }
    async fn call(
        &self,
        _: &Session,
        request_id: &str,
        call: CallToolRequestParams,
        _: &Emitter,
    ) -> Result<CallToolResult, ErrorData> {
        let (reply, response) = oneshot::channel();
        self.tx
            .send(HostAction::Tool {
                id: request_id.into(),
                name: call.name.into_owned(),
                args: Value::Object(call.arguments.unwrap_or_default()),
                reply,
            })
            .map_err(|_| ErrorData::internal_error("tool host closed", None))?;
        match response
            .await
            .map_err(|_| ErrorData::internal_error("tool host closed", None))?
        {
            Ok(text) => Ok(tool_result(text, true)),
            Err(text) => Ok(tool_result(text, false)),
        }
    }
}

fn tool_result(text: String, ok: bool) -> CallToolResult {
    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        if value.get("content").is_some_and(Value::is_array)
            || value.get("structuredContent").is_some()
        {
            if let Ok(mut result) = serde_json::from_value::<CallToolResult>(value) {
                if !ok {
                    result.is_error = Some(true);
                }
                return result;
            }
        }
    }
    if ok {
        CallToolResult::success(vec![ContentBlock::text(text)])
    } else {
        CallToolResult::error(vec![ContentBlock::text(text)])
    }
}

fn interruption_stop(error: &str) -> Option<Stop> {
    if matches!(error, "suspended" | "execution suspended") {
        Some(Stop::Suspended)
    } else if crate::session_recovery::is_interruption(error) {
        Some(Stop::Error(error.into()))
    } else {
        None
    }
}

struct Policy {
    system: String,
    names: Vec<String>,
}
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Operation<Session, Effect> for Policy {
    fn name(&self) -> &'static str {
        "jazyk_policy"
    }
    async fn prompt_parts(&self, _: &Session, _: &Conversation) -> Result<Vec<(String, String)>> {
        Ok(vec![("jazyk".into(), self.system.clone())])
    }
    async fn run(
        &self,
        _: &Session,
        conversation: &Conversation,
        emit: &Emitter,
    ) -> Result<OperationResult<Effect>> {
        let mut response = Message::user();
        for request in unanswered(conversation) {
            if let Ok(call) = &request.tool_call {
                if !self.names.contains(&call.name.to_string()) {
                    response.add_tool_response_with_metadata(
                        request.id.clone(),
                        Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                            "Tool '{}' is not available in this session",
                            call.name
                        ))])),
                        request.metadata.as_ref(),
                    );
                }
            }
        }
        if response.content.is_empty() {
            operation::not_applicable()
        } else {
            operation::applied([Effect::from(emit.message(response))])
        }
    }
}

#[derive(Default)]
pub struct MemoryRecovery;
impl Recovery for MemoryRecovery {
    fn load<'a>(&'a mut self) -> LocalFuture<'a, Result<Option<HarnessState>, String>> {
        Box::pin(async { Ok(None) })
    }
    fn checkpoint<'a>(
        &'a mut self,
        _: CheckpointBoundary,
        _: &'a HarnessState,
    ) -> LocalFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_loop(args: LoopArgs<'_>) -> Stop {
    run_loop_with_instructions(args, "")
}

#[cfg(target_arch = "wasm32")]
pub async fn run_loop(args: LoopArgs<'_>) -> Stop {
    run_loop_async(args).await
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_loop_with_instructions(args: LoopArgs<'_>, instructions: &str) -> Stop {
    let mut host = DefaultModelHost::new(args.llm);
    let mut recovery = MemoryRecovery;
    run_with_host(args, instructions, &mut host, &mut recovery)
}

#[cfg(target_arch = "wasm32")]
pub async fn run_loop_with_instructions(args: LoopArgs<'_>, instructions: &str) -> Stop {
    let mut host = DefaultModelHost::new(args.llm);
    let mut recovery = MemoryRecovery;
    run_with_host_async(args, instructions, &mut host, &mut recovery).await
}

pub async fn run_loop_async(args: LoopArgs<'_>) -> Stop {
    let mut host = DefaultModelHost::new(args.llm);
    let mut recovery = MemoryRecovery;
    run_with_host_async(args, "", &mut host, &mut recovery).await
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_with_host(
    args: LoopArgs<'_>,
    instructions: &str,
    host: &mut dyn ModelHost,
    recovery: &mut dyn Recovery,
) -> Stop {
    drive_native(run_with_host_async(args, instructions, host, recovery))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_with_hosts(
    args: LoopArgs<'_>,
    instructions: &str,
    host: &mut dyn ModelHost,
    tools: &mut dyn ToolHost,
    recovery: &mut dyn Recovery,
) -> Stop {
    drive_native(run_with_hosts_async(
        args,
        instructions,
        host,
        tools,
        recovery,
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn drive_native(future: impl std::future::Future<Output = Stop>) -> Stop {
    let drive = || async_io::block_on(future);
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(drive)
        }
        Ok(_) => {
            Stop::Error("Use run_with_host_async inside a current-thread Tokio runtime".into())
        }
        Err(_) => drive(),
    }
}

#[cfg(target_arch = "wasm32")]
pub async fn run_with_hosts(
    args: LoopArgs<'_>,
    instructions: &str,
    host: &mut dyn ModelHost,
    tools: &mut dyn ToolHost,
    recovery: &mut dyn Recovery,
) -> Stop {
    run_with_hosts_async(args, instructions, host, tools, recovery).await
}

#[cfg(target_arch = "wasm32")]
pub async fn run_with_host(
    args: LoopArgs<'_>,
    instructions: &str,
    host: &mut dyn ModelHost,
    recovery: &mut dyn Recovery,
) -> Stop {
    run_with_host_async(args, instructions, host, recovery).await
}

pub async fn run_with_host_async(
    args: LoopArgs<'_>,
    instructions: &str,
    host: &mut dyn ModelHost,
    recovery: &mut dyn Recovery,
) -> Stop {
    run_inner_async(args, instructions, host, None, recovery).await
}

pub async fn run_with_hosts_async(
    args: LoopArgs<'_>,
    instructions: &str,
    host: &mut dyn ModelHost,
    tools: &mut dyn ToolHost,
    recovery: &mut dyn Recovery,
) -> Stop {
    run_inner_async(args, instructions, host, Some(tools), recovery).await
}

async fn run_inner_async(
    mut args: LoopArgs<'_>,
    instructions: &str,
    host: &mut dyn ModelHost,
    mut tools: Option<&mut dyn ToolHost>,
    recovery: &mut dyn Recovery,
) -> Stop {
    let mut state = match recovery.load().await {
        Ok(Some(state)) if state.version == 1 => state,
        Ok(Some(_)) => {
            return Stop::Error(
                "checkpoint-incompatible: Unsupported GDK recovery state version".into(),
            )
        }
        Ok(None) => match initial_state(args.history) {
            Ok(state) => state,
            Err(error) => return Stop::Error(error),
        },
        Err(error) => return Stop::Error(error),
    };
    if let Some(pending) = &state.pending_tool {
        if !state.tool_receipts.contains_key(&pending.id) {
            return Stop::Error(format!(
                "execution-uncertain: Tool outcome is uncertain for '{}'; reconcile its intention before resuming",
                pending.id
            ));
        }
        state.pending_tool = None;
    }
    let result = drive(
        &mut args,
        instructions,
        host,
        &mut tools,
        recovery,
        &mut state,
    )
    .await;
    if result == Stop::Cancelled {
        preserve_interrupted_turn(&mut state);
        if let Err(error) = checkpoint(recovery, &state, CheckpointPhase::Finished, None).await {
            return Stop::Error(error);
        }
    }
    *args.history = export_history(&state.conversation);
    result
}

fn preserve_interrupted_turn(state: &mut HarnessState) {
    // GDK applies its conversation effect after tools return. The accepted model
    // receipt and tool receipts can already be checkpointed before that effect.
    let prefix = format!("{}:{}:Inference:", state.run_id, state.transition);
    let mut receipts = state.model_receipts.iter().filter_map(|(id, value)| {
        id.strip_prefix(&prefix)?.parse::<u64>().ok().map(|ordinal| (ordinal, id, value))
    }).collect::<Vec<_>>();
    receipts.sort_by_key(|(ordinal, _, _)| *ordinal);
    for (_, id, value) in receipts {
        if state.conversation.iter().any(|message|
            message.metadata.operation_note("jazyk.runtime", "inferenceId").and_then(Value::as_str) == Some(id.as_str())) {
            continue;
        }
        let model = state.model_requests.get(id).and_then(|body| body["model"].as_str()).unwrap_or("recovered");
        if let Ok((message, _)) = super::provider::response_message(id, value.clone(), model) {
            let ids = message.content.iter().filter_map(MessageContent::as_tool_request)
                .map(|request| request.id.as_str()).collect::<Vec<_>>();
            if !ids.is_empty() && ids.iter().all(|id| state.conversation.iter().flat_map(|message| &message.content)
                .filter_map(MessageContent::as_tool_request).any(|request| request.id == *id)) {
                continue;
            }
            state.conversation.push(message);
        }
    }

    let mut response = Message::user();
    for request in unanswered(&state.conversation) {
        let result = match state.tool_receipts.get(&request.id) {
            Some(receipt) => tool_result(receipt.result.clone(), receipt.ok),
            None => CallToolResult::error(vec![ContentBlock::text(
                "Tool call was interrupted before completing",
            )]),
        };
        response.add_tool_response_with_metadata(request.id, Ok(result), request.metadata.as_ref());
    }
    if !response.content.is_empty() {
        state.conversation.push(response);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn stopped_history(state: &HarnessState) -> Vec<Value> {
    let mut stopped = state.clone();
    preserve_interrupted_turn(&mut stopped);
    export_history(&stopped.conversation)
}

async fn checkpoint(
    recovery: &mut dyn Recovery,
    state: &HarnessState,
    phase: CheckpointPhase,
    id: Option<String>,
) -> Result<(), String> {
    recovery
        .checkpoint(
            CheckpointBoundary {
                phase,
                operation_id: id,
            },
            state,
        )
        .await
        .map_err(|error| format!("checkpoint-failed: {error}"))
}

type LiveDeltas = ModelDeltaProgress;

impl LiveDeltas {
    fn emit(&mut self, args: &mut LoopArgs<'_>, field: &str, text: String) {
        if field == "content" {
            self.content.push_str(&text);
            (args.emit)(AgentEvent::Message(text));
        } else if field == "reasoning_summary" {
            self.summary.push_str(&text);
            (args.emit)(AgentEvent::ReasoningSummary(text));
        } else {
            self.thought.push_str(&text);
            (args.emit)(AgentEvent::Thought(text));
        }
    }

    fn remainder(prefix: &mut String, text: String) -> String {
        if let Some(rest) = prefix.strip_prefix(&text) {
            *prefix = rest.to_string();
            String::new()
        } else if let Some(rest) = text.strip_prefix(prefix.as_str()) {
            let rest = rest.to_string();
            prefix.clear();
            rest
        } else {
            prefix.clear();
            text
        }
    }
}

#[derive(Default)]
struct PendingDeltas {
    chunks: std::collections::VecDeque<(String, String)>,
    bytes: usize,
    overflow: bool,
}

fn flush_deltas(
    pending: &std::sync::Mutex<PendingDeltas>,
    live: &mut LiveDeltas,
    args: &mut LoopArgs<'_>,
) -> Result<(), Stop> {
    let pending = std::mem::take(&mut *pending.lock().unwrap_or_else(|p| p.into_inner()));
    if pending.overflow
        || live
            .content
            .len()
            .saturating_add(live.thought.len())
            .saturating_add(pending.bytes)
            > 16 * 1024 * 1024
    {
        return Err(Stop::Error("model delta buffer exceeds 16 MiB".into()));
    }
    for (field, text) in pending.chunks {
        live.emit(args, &field, text);
    }
    Ok(())
}

async fn drive(
    args: &mut LoopArgs<'_>,
    instructions: &str,
    host: &mut dyn ModelHost,
    tools_host: &mut Option<&mut dyn ToolHost>,
    recovery: &mut dyn Recovery,
    state: &mut HarnessState,
) -> Stop {
    let mut live = LiveDeltas::default();
    let config = ModelConfig::new(&args.llm.model)
        .with_temperature(args.llm.temperature.map(|t| t as f32))
        .with_max_tokens(Some(
            crate::llm::max_completion_tokens().min(i32::MAX as u64) as i32,
        ))
        .with_context_limit(Some(crate::limits::MODEL_CONTEXT_TOKENS))
        .with_vision_support(false);
    loop {
        // A successful domain gate completes a worker batch. Input accepted after
        // that commit belongs to the next batch, and needs no summary inference.
        if !args.conversation && completed_batch(state) {
            pair_completed_batch(state);
            return match checkpoint(recovery, state, CheckpointPhase::Finished, None).await {
                Ok(()) => Stop::EndTurn,
                Err(error) => Stop::Error(error),
            };
        }
        if (args.cancelled)() {
            return Stop::Cancelled;
        }
        if recovery.suspend_requested() {
            return match checkpoint(recovery, state, CheckpointPhase::Suspended, None).await {
                Ok(()) => Stop::Suspended,
                Err(error) => Stop::Error(error),
            };
        }
        if let Err(error) = deliver_inputs(args, recovery, state).await {
            return Stop::Error(error);
        }
        if state.consecutive_tool_errors >= crate::limits::AGENT_TOOL_ERROR_LIMIT {
            return Stop::Error(format!("Stopped after {} consecutive tool errors; inspect the failed calls before retrying", crate::limits::AGENT_TOOL_ERROR_LIMIT));
        }
        if let Some(last) = state.conversation.last() {
            if last.error_kind().is_some() {
                let errors = last
                    .content
                    .iter()
                    .filter_map(MessageContent::as_error)
                    .map(|error| error.message.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                return Stop::Error(if errors.is_empty() {
                    format!("Model inference failed: {:?}", last.error_kind())
                } else {
                    errors
                });
            }
        }
        if operation::ends_turn(state.conversation.messages()) {
            let ledger_worker = !args.conversation
                && args.tools.iter().any(|tool| tool.name == "done")
                && args.tools.iter().any(|tool| matches!(tool.name.as_str(),
                    "record_generation" | "record_binding" | "record_verdict"));
            if ledger_worker && !state.ledger_finish_reminded {
                state.ledger_finish_reminded = true;
                state.conversation.push(Message::user().with_text(LEDGER_CONTINUATION));
                state.transition += 1;
                if let Err(error) = checkpoint(recovery, state, CheckpointPhase::Transition, None).await {
                    return Stop::Error(error);
                }
                continue;
            }
            if state.model_output_limited {
                return Stop::Error("The model reached its output token limit before completing the turn".into());
            }
            if args.input_close.as_mut().is_some_and(|close| !close()) {
                continue;
            }
            return match checkpoint(recovery, state, CheckpointPhase::Finished, None).await {
                Ok(()) => Stop::EndTurn,
                Err(error) => Stop::Error(error),
            };
        }
        let (actions, mut requests) = mpsc::unbounded_channel();
        let (events, mut event_rx) = mpsc::unbounded_channel();
        let cancellation = CancellationToken::new();
        let emitter = Emitter::new(events, cancellation.clone());
        let ordinal = Arc::new(AtomicU64::new(0));
        let provider = Arc::new(ChannelProvider {
            tx: actions.clone(),
            purpose: ModelPurpose::Inference,
            ordinal: ordinal.clone(),
        });
        let catalog = match tools_host.as_deref_mut() {
            Some(host) => match host.catalog().await {
                Ok(tools) => tools,
                Err(error) => return Stop::Error(error),
            },
            None => None,
        };
        let current_tools = catalog.as_deref().unwrap_or(args.tools);
        let definitions = match definitions(current_tools) {
            Ok(tools) => tools,
            Err(e) => return Stop::Error(e),
        };
        let tools = ToolOperation::<Session>::new().with_provider(Arc::new(DynamicTools {
            definitions,
            tx: actions.clone(),
        }));
        let inference = InferenceRunner::<Session, Effect>::new(provider, config.clone());
        let policy = Policy {
            system: format!("{SYSTEM}\n\n{instructions}"),
            names: current_tools.iter().map(|t| t.name.clone()).collect(),
        };
        let machine = StateMachine::new(
            vec![
                Step::Operation(Arc::new(policy)),
                Step::Operation(Arc::new(tools)),
                Step::Inference(Arc::new(inference)),
            ],
            cancellation.clone(),
        );
        let session = Session {
            conversation: state.conversation.clone(),
        };

        let estimate = serde_json::to_vec(&state.conversation)
            .map(|v| v.len() / 4)
            .unwrap_or(0);
        let compact = unanswered(&state.conversation).is_empty()
            && state.conversation.len() > 2
            && estimate
                > (crate::llm::model_input_tokens() as f64
                    * goose_context_management::DEFAULT_COMPACTION_THRESHOLD)
                    as usize
            && state
                .compacted_at_round
                .is_none_or(|round| state.rounds > round + 1)
            && state.compacted_size.is_none_or(|size|
                estimate >= size.saturating_add(crate::llm::model_input_tokens() / 5));
        let outcome = if compact {
            let provider = Arc::new(ChannelProvider {
                tx: actions,
                purpose: ModelPurpose::Compaction,
                ordinal,
            });
            let model = ProviderModel::new(provider, config.clone());
            let parts = super::compaction::context(state.conversation.messages(), args.conversation);
            let mut templates = Templates::default();
            templates.compaction = COMPACTION_REQUEST.to_string();
            let future = goose_context_management::summarize(&model, None, &templates, &parts.summary);
            match drive_future(
                future,
                &mut requests,
                &mut event_rx,
                args,
                host,
                tools_host,
                recovery,
                state,
                &cancellation,
                &mut live,
            )
            .await
            {
                Ok(summary) => {
                    let mut compacted: Vec<Message> = parts.opening.into_iter().collect();
                    compacted.push(summary.message);
                    compacted.push(Message::user()
                        .with_text(COMPACTION_CONTINUATION.replace("{SESSION}", &args.label)));
                    compacted.extend(parts.retained);
                    state.conversation = Conversation::new_unvalidated(compacted);
                    state.compacted_size = Some(serde_json::to_vec(&state.conversation)
                        .map(|bytes| bytes.len() / 4).unwrap_or(0));
                    record_usage(args, state, summary.usage);
                    state.compacted_at_round = Some(state.rounds);
                    state.transition += 1;
                    if let Err(error) =
                        checkpoint(recovery, state, CheckpointPhase::Compacted, None).await
                    {
                        return Stop::Error(error);
                    }
                    continue;
                }
                Err(stop) => return stop,
            }
        } else {
            match drive_future(
                machine.step(&session, &emitter),
                &mut requests,
                &mut event_rx,
                args,
                host,
                tools_host,
                recovery,
                state,
                &cancellation,
                &mut live,
            )
            .await
            {
                Ok(result) => result,
                Err(stop) => return stop,
            }
        };
        while let Ok(event) = event_rx.try_recv() {
            emit_gdk(args, event, &mut live);
        }
        for effect in outcome.effects {
            match effect {
                Effect::Usage(usage) => record_usage(args, state, usage),
                Effect::Conversation(ConversationEffect::AppendMessage(message)) => {
                    state.conversation.push(message)
                }
                Effect::Conversation(ConversationEffect::ReplaceConversation(conversation)) => {
                    state.conversation = conversation
                }
                _ => return Stop::Error("Unexpected GDK conversation effect".into()),
            }
        }
        let mut pending_inputs = std::mem::take(&mut state.pending_inputs);
        let accepted_inputs = if !args.conversation && completed_batch(state) {
            state.pending_inputs = pending_inputs;
            Vec::new()
        } else {
            accept_inputs(state, &mut pending_inputs)
        };
        state.model_deltas.clear();
        state.transition += 1;
        if let Err(error) = checkpoint(recovery, state, CheckpointPhase::Transition, None).await {
            return Stop::Error(error);
        }
        for input in accepted_inputs {
            (args.emit)(AgentEvent::Input(input));
        }
        if outcome.status == RunStatus::Cancelled {
            return Stop::Cancelled;
        }
    }
}

async fn drive_future<F, T>(
    future: F,
    requests: &mut mpsc::UnboundedReceiver<HostAction>,
    events: &mut mpsc::UnboundedReceiver<GdkEvent>,
    args: &mut LoopArgs<'_>,
    host: &mut dyn ModelHost,
    tools_host: &mut Option<&mut dyn ToolHost>,
    recovery: &mut dyn Recovery,
    state: &mut HarnessState,
    cancellation: &CancellationToken,
    live: &mut LiveDeltas,
) -> Result<T, Stop>
where
    F: std::future::Future<Output = Result<T>>,
{
    let future = future.fuse();
    futures::pin_mut!(future);
    loop {
        let request = requests.recv().fuse();
        let event = events.recv().fuse();
        let tick = sleep_tick().fuse();
        futures::pin_mut!(request, event, tick);
        futures::select_biased! {
            result = future => return result.map_err(|e| Stop::Error(e.to_string())),
            action = request => if let Some(action) = action {
                handle_action(action, args, host, tools_host, recovery, state, cancellation, live).await?;
            },
            event = event => if let Some(event) = event { emit_gdk(args, event, live); },
            _ = tick => if (args.cancelled)() { cancellation.cancel(); return Err(Stop::Cancelled); },
        }
    }
}

async fn handle_action(
    action: HostAction,
    args: &mut LoopArgs<'_>,
    host: &mut dyn ModelHost,
    tools_host: &mut Option<&mut dyn ToolHost>,
    recovery: &mut dyn Recovery,
    state: &mut HarnessState,
    cancellation: &CancellationToken,
    live: &mut LiveDeltas,
) -> Result<(), Stop> {
    if (args.cancelled)() {
        cancellation.cancel();
        return Err(Stop::Cancelled);
    }
    if recovery.suspend_requested() {
        checkpoint(recovery, state, CheckpointPhase::Suspended, None)
            .await
            .map_err(Stop::Error)?;
        return Err(Stop::Suspended);
    }
    match action {
        HostAction::Model {
            mut body,
            purpose,
            ordinal,
            reply,
        } => {
            let started = crate::sys::Instant::now();
            body["_jazykApi"] = json!(args.llm.api);
            body["_jazykReasoningSummary"] = json!(args.llm.reasoning_summary);
            let id = format!(
                "{}:{}:{purpose:?}:{ordinal}",
                state.run_id, state.transition
            );
            let body = if let Some(recorded) = state.model_requests.get(&id) {
                // The protected intention owns its provider body. A newer catalog
                // or prompt applies only to the next inference identity.
                recorded.clone()
            } else {
                if state.rounds >= args.max_rounds {
                    return Err(Stop::MaxRounds);
                }
                state.rounds += 1;
                state.model_requests.insert(id.clone(), body.clone());
                checkpoint(
                    recovery,
                    state,
                    CheckpointPhase::BeforeModel,
                    Some(id.clone()),
                )
                .await
                .map_err(Stop::Error)?;
                body
            };
            let value = if let Some(value) = state.model_receipts.get(&id) {
                *live = state.model_deltas.get(&id).cloned().unwrap_or_default();
                value.clone()
            } else {
                *live = state.model_deltas.get(&id).cloned().unwrap_or_default();
                let pending = Arc::new(std::sync::Mutex::new(PendingDeltas::default()));
                let (notice, mut notices) = mpsc::channel(1);
                let buffer = pending.clone();
                let sink: ModelDeltaSink = Arc::new(move |field, text| {
                    if text.is_empty()
                        || !matches!(field, "content" | "reasoning" | "reasoning_content" | "reasoning_summary")
                    {
                        return;
                    }
                    let mut pending = buffer.lock().unwrap_or_else(|p| p.into_inner());
                    if pending.bytes.saturating_add(text.len()) > 16 * 1024 * 1024 {
                        pending.overflow = true;
                    } else {
                        pending.bytes += text.len();
                        if let Some((_, value)) = pending
                            .chunks
                            .back_mut()
                            .filter(|(previous, _)| previous == field)
                        {
                            value.push_str(text);
                        } else {
                            pending
                                .chunks
                                .push_back((field.to_string(), text.to_string()));
                        }
                    }
                    let _ = notice.try_send(());
                });
                let request = ModelRequest {
                    body,
                    attempt_id: id.clone(),
                    purpose,
                    deltas: if cfg!(target_arch = "wasm32") || purpose == ModelPurpose::Compaction {
                        None
                    } else {
                        Some(sink.clone())
                    },
                };
                let inference = host.infer(request).fuse();
                futures::pin_mut!(inference);
                let response = loop {
                    if (args.cancelled)() {
                        cancellation.cancel();
                        return Err(Stop::Cancelled);
                    }
                    let tick = sleep_tick().fuse();
                    let notice = notices.recv().fuse();
                    futures::pin_mut!(tick, notice);
                    futures::select_biased! {
                        result = inference => {
                            flush_deltas(&pending, live, args)?;
                            match result {
                                Ok(response) => break response,
                                Err(error) => {
                                    if let Some(stop) = interruption_stop(&error) { return Err(stop); }
                                    let _ = reply.send(Err(error)); return Ok(());
                                }
                            }
                        },
                        _ = notice => flush_deltas(&pending, live, args)?,
                        _ = tick => if (args.cancelled)() { cancellation.cancel(); return Err(Stop::Cancelled); },
                    }
                };
                state.model_deltas.insert(id.clone(), live.clone());
                state
                    .model_receipts
                    .insert(id.clone(), response.body.clone());
                checkpoint(
                    recovery,
                    state,
                    CheckpointPhase::SavedModel,
                    Some(id.clone()),
                )
                .await
                .map_err(Stop::Error)?;
                response.body
            };
            (args.emit)(AgentEvent::Timing(
                json!({"kind":"provider","requestId":id,"purpose":purpose,"round":state.rounds,"ms":started.elapsed().as_millis() as u64}),
            ));
            if purpose == ModelPurpose::Inference {
                // A host may return the Responses envelope itself; its output limit
                // reads from the status, not a chat finish reason.
                state.model_output_limited = value["choices"][0]["finish_reason"] == "length"
                    || (value["object"] == "response"
                        && crate::model_api::responses_output_limited(&value));
            }
            let _ = reply.send(Ok((id, value)));
        }
        HostAction::Tool {
            id,
            name,
            args: parameters,
            reply,
        } => {
            let started = crate::sys::Instant::now();
            if let Some(receipt) = state.tool_receipts.get(&id) {
                if receipt.args.as_ref().is_some_and(|digest| *digest != super::arguments_digest(&parameters)) {
                    return Err(Stop::Error(format!(
                        "execution-uncertain: Tool call '{id}' replays with arguments that differ from its recorded receipt; reconcile it before resuming"
                    )));
                }
                let _ = reply.send(if receipt.ok {
                    Ok(receipt.result.clone())
                } else {
                    Err(receipt.result.clone())
                });
                return Ok(());
            }
            if !args.conversation && completed_batch(state) {
                let _ = reply.send(Err(
                    "Skipped: this goal batch already committed successfully".into(),
                ));
                return Ok(());
            }
            let steering = args
                .inputs
                .as_mut()
                .map(|source| source())
                .unwrap_or_default();
            state.pending_inputs.extend(steering);
            let skipped = !state.pending_inputs.is_empty();
            state.pending_tool = Some(ToolIntention {
                id: id.clone(),
                name: name.clone(),
                args: parameters.clone(),
            });
            checkpoint(
                recovery,
                state,
                CheckpointPhase::BeforeTool,
                Some(id.clone()),
            )
            .await
            .map_err(Stop::Error)?;
            (args.emit)(AgentEvent::ToolCallStart {
                id: id.clone(),
                name: name.clone(),
                args: parameters.clone(),
            });
            let result = if skipped {
                Err(
                    "Skipped: new human input arrived before dispatch; this call did not execute"
                        .into(),
                )
            } else if (args.cancelled)() {
                Err("Skipped: the session was cancelled before this call started".into())
            } else if state.failed_tool_transition == Some(state.transition) {
                Err("Skipped: an earlier call in this reply failed; inspect its result before resubmitting".into())
            } else if let Some(tools) = tools_host.as_deref_mut() {
                let call = tools.call(&name, &parameters).fuse();
                futures::pin_mut!(call);
                loop {
                    let tick = sleep_tick().fuse();
                    futures::pin_mut!(tick);
                    futures::select_biased! {
                        result = call => break result,
                        _ = tick => if (args.cancelled)() { cancellation.cancel(); return Err(Stop::Cancelled); },
                    }
                }
            } else {
                (args.dispatch)(&name, &parameters)
            };
            if let Err(error) = &result {
                if let Some(stop) = interruption_stop(error) {
                    return Err(stop);
                }
            }
            let ok = result
                .as_ref()
                .is_ok_and(|text| tool_result(text.clone(), true).is_error != Some(true));
            if !ok && !skipped && state.failed_tool_transition != Some(state.transition) {
                state.consecutive_tool_errors += 1;
                state.failed_tool_transition = Some(state.transition);
            } else if ok {
                state.consecutive_tool_errors = 0;
            }
            let digest = Some(super::arguments_digest(&parameters));
            let receipt = match &result {
                Ok(result) => ToolReceipt {
                    result: result.clone(),
                    ok,
                    args: digest,
                },
                Err(error) => ToolReceipt {
                    result: error.clone(),
                    ok: false,
                    args: digest,
                },
            };
            state.tool_receipts.insert(id.clone(), receipt.clone());
            state.pending_tool = None;
            checkpoint(
                recovery,
                state,
                CheckpointPhase::AfterTool,
                Some(id.clone()),
            )
            .await
            .map_err(Stop::Error)?;
            (args.emit)(AgentEvent::ToolCallEnd {
                id,
                result: receipt.result,
                ok: receipt.ok,
            });
            (args.emit)(AgentEvent::Timing(
                json!({"kind":"tool","name":name,"round":state.rounds,"ms":started.elapsed().as_millis() as u64}),
            ));
            let _ = reply.send(result);
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
async fn sleep_tick() {
    async_io::Timer::after(std::time::Duration::from_millis(25)).await;
}
#[cfg(target_arch = "wasm32")]
async fn sleep_tick() {
    gloo_timers::future::TimeoutFuture::new(25).await;
}

fn definitions(tools: &[GenericTool]) -> Result<Vec<Tool>, String> {
    tools
        .iter()
        .map(|tool| {
            let schema = tool
                .parameters
                .as_object()
                .cloned()
                .ok_or_else(|| format!("Tool '{}' schema is not an object", tool.name))?;
            Ok(Tool::new(
                tool.name.clone(),
                tool.description.clone(),
                Arc::new(schema),
            ))
        })
        .collect()
}

fn unanswered(
    conversation: &Conversation,
) -> Vec<goose_provider_types::conversation::message::ToolRequest> {
    let answered: std::collections::HashSet<_> = conversation
        .iter()
        .flat_map(Message::get_tool_response_ids)
        .collect();
    conversation
        .iter()
        .flat_map(|m| &m.content)
        .filter_map(MessageContent::as_tool_request)
        .filter(|r| !answered.contains(r.id.as_str()))
        .cloned()
        .collect()
}

fn completed_batch(state: &HarnessState) -> bool {
    state
        .conversation
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(MessageContent::as_tool_request)
        .filter(|request| {
            request
                .tool_call
                .as_ref()
                .is_ok_and(|call| call.name == "done")
        })
        .any(|request| {
            state.tool_receipts.get(&request.id).is_some_and(|receipt| {
                receipt.ok && tool_result(receipt.result.clone(), true).is_error != Some(true)
            })
        })
}

fn pair_completed_batch(state: &mut HarnessState) {
    let mut response = Message::user();
    for request in unanswered(&state.conversation) {
        let result = match state.tool_receipts.get(&request.id) {
            Some(receipt) => tool_result(receipt.result.clone(), receipt.ok),
            None => CallToolResult::error(vec![ContentBlock::text(
                "Skipped: this goal batch already committed successfully",
            )]),
        };
        response.add_tool_response_with_metadata(request.id, Ok(result), request.metadata.as_ref());
    }
    if !response.content.is_empty() {
        state.conversation.push(response);
    }
}

fn emit_gdk(args: &mut LoopArgs<'_>, event: GdkEvent, live: &mut LiveDeltas) {
    if let GdkEvent::Message(message) = event {
        if !message.is_user_visible() {
            return;
        }
        let summary = message.metadata.operation_note("jazyk.responses", "output").is_some();
        for content in message.content {
            match content {
                MessageContent::Text(text) if message.role == Role::Assistant => {
                    let text = LiveDeltas::remainder(&mut live.content, text.text);
                    if !text.is_empty() {
                        (args.emit)(AgentEvent::Message(text));
                    }
                }
                MessageContent::Thinking(thought) => {
                    let text = LiveDeltas::remainder(if summary { &mut live.summary } else { &mut live.thought }, thought.thinking);
                    if !text.is_empty() {
                        (args.emit)(if summary { AgentEvent::ReasoningSummary(text) } else { AgentEvent::Thought(text) });
                    }
                }
                _ => {}
            }
        }
    }
}

fn record_usage(args: &mut LoopArgs<'_>, state: &mut HarnessState, usage: ProviderUsage) {
    let tokens = usage
        .usage
        .total_tokens
        .or_else(|| {
            Some(usage.usage.input_tokens.unwrap_or(0) + usage.usage.output_tokens.unwrap_or(0))
        })
        .unwrap_or(0)
        .max(0) as u64;
    let cached = usage.usage.cache_read_input_tokens.unwrap_or(0).max(0) as u64;
    crate::llm::add_tokens(usage.usage.output_tokens.unwrap_or(0).max(0) as u64);
    (args.emit)(AgentEvent::Usage { tokens, cached });
    state.usage.push(usage);
}

async fn deliver_inputs(
    args: &mut LoopArgs<'_>,
    recovery: &mut dyn Recovery,
    state: &mut HarnessState,
) -> Result<(), String> {
    let mut inputs = std::mem::take(&mut state.pending_inputs);
    inputs.extend(
        args.inputs
            .as_mut()
            .map(|source| source())
            .unwrap_or_default(),
    );
    let accepted = accept_inputs(state, &mut inputs);
    if !accepted.is_empty() {
        checkpoint(recovery, state, CheckpointPhase::Input, None).await?;
        for input in accepted {
            (args.emit)(AgentEvent::Input(input));
        }
    }
    Ok(())
}

fn accept_inputs(state: &mut HarnessState, inputs: &mut Vec<Value>) -> Vec<Value> {
    let mut accepted = Vec::new();
    for input in inputs.drain(..) {
        let id = input["messageId"]
            .as_str()
            .or_else(|| input["id"].as_str())
            .map(str::to_string)
            .unwrap_or_else(|| crate::model::hash_hex(&input.to_string()));
        if state.delivered_inputs.contains(&id) {
            continue;
        }
        let mut skipped = Message::user();
        for pending in unanswered(&state.conversation) {
            let result = match state.tool_receipts.get(&pending.id) {
                Some(receipt) => tool_result(receipt.result.clone(), receipt.ok),
                None => CallToolResult::error(vec![ContentBlock::text(
                    "Skipped: new human input arrived before dispatch; this call did not execute",
                )]),
            };
            skipped.add_tool_response_with_metadata(
                pending.id,
                Ok(result),
                pending.metadata.as_ref(),
            );
        }
        if !skipped.content.is_empty() {
            state.conversation.push(skipped);
        }
        state
            .conversation
            .push(Message::user().with_text(input["text"].as_str().unwrap_or_default()));
        state
            .delivered_input_values
            .insert(id.clone(), input.clone());
        state.delivered_inputs.push(id);
        accepted.push(input);
    }
    accepted
}

fn initial_state(history: &[Value]) -> Result<HarnessState, String> {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let run_id = random.iter().map(|b| format!("{b:02x}")).collect();
    let mut conversation = Conversation::empty();
    for value in history {
        if value["_jazykSkip"].as_bool().unwrap_or(false) {
            continue;
        }
        if let Some(native) = value.get("_jazykGdk") {
            conversation.push(
                serde_json::from_value(native.clone())
                    .map_err(|e| format!("Invalid native GDK history: {e}"))?,
            );
            continue;
        }
        match value["role"].as_str().unwrap_or_default() {
            "assistant" => conversation.push(
                openai::response_to_message(&json!({"choices":[{"message":value}]}))
                    .map_err(|e| e.to_string())?,
            ),
            "tool" => {
                let mut message = Message::user();
                message.add_tool_response_with_metadata(
                    value["tool_call_id"].as_str().unwrap_or_default(),
                    Ok(CallToolResult::success(vec![ContentBlock::text(
                        value["content"].as_str().unwrap_or_default(),
                    )])),
                    None,
                );
                conversation.push(message);
            }
            "user" => conversation
                .push(Message::user().with_text(value["content"].as_str().unwrap_or_default())),
            _ => {}
        }
    }
    Ok(HarnessState {
        version: 1,
        run_id,
        transition: 0,
        rounds: 0,
        conversation,
        model_requests: BTreeMap::new(),
        model_receipts: BTreeMap::new(),
        model_deltas: BTreeMap::new(),
        tool_receipts: BTreeMap::new(),
        pending_tool: None,
        delivered_inputs: Vec::new(),
        delivered_input_values: BTreeMap::new(),
        pending_inputs: Vec::new(),
        consecutive_tool_errors: 0,
        failed_tool_transition: None,
        usage: Vec::new(),
        compacted_at_round: None,
        compacted_size: None,
        ledger_finish_reminded: false,
        model_output_limited: false,
    })
}

fn export_history(conversation: &Conversation) -> Vec<Value> {
    let mut history = Vec::new();
    for message in conversation {
        let formatted = openai::format_messages_with_options(
            std::slice::from_ref(message),
            &ImageFormat::OpenAi,
            openai::OpenAiFormatOptions {
                supports_vision: false,
                preserve_thinking_context: true,
                ..Default::default()
            },
        );
        if formatted.is_empty() {
            history.push(json!({"role":"assistant","content":"","_jazykGdk":message}));
        }
        for (index, mut value) in formatted.into_iter().enumerate() {
            if index == 0 {
                value["_jazykGdk"] = serde_json::to_value(message).unwrap_or(Value::Null);
            } else {
                value["_jazykSkip"] = json!(true);
            }
            history.push(value);
        }
    }
    history
}
