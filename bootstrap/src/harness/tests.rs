use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use serde_json::{json, Value};

use super::*;

fn llm() -> crate::llm::Llm {
    crate::llm::Llm {
        base_url: "http://127.0.0.1:3625/v1".into(),
        model: "fixture".into(),
        api_key: String::new(),
        temperature: None,
        trace: None,
    }
}

fn tool() -> GenericTool {
    GenericTool {
        name: "fixture".into(),
        description: "Read the session fixture".into(),
        parameters: json!({"type":"object","properties":{},"additionalProperties":false}),
    }
}

fn response(text: &str) -> Value {
    json!({"id":"provider-response","choices":[{"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}})
}

fn call() -> Value {
    json!({"choices":[{"message":{"role":"assistant","content":"","reasoning_content":"retained reasoning","tool_calls":[{"id":"tool-1","type":"function","function":{"name":"fixture","arguments":"{}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}})
}

struct Host {
    replies: VecDeque<Value>,
    requests: Rc<RefCell<Vec<ModelRequest>>>,
}
impl Host {
    fn new(replies: Vec<Value>) -> Self {
        Self {
            replies: replies.into(),
            requests: Rc::default(),
        }
    }
}
impl ModelHost for Host {
    fn infer<'a>(
        &'a mut self,
        request: ModelRequest,
    ) -> LocalFuture<'a, Result<ModelResponse, String>> {
        Box::pin(async move {
            self.requests.borrow_mut().push(request);
            Ok(ModelResponse {
                body: self.replies.pop_front().ok_or("unexpected inference")?,
            })
        })
    }
}

#[derive(Default)]
struct Store {
    saved: Option<HarnessState>,
    fail: Option<CheckpointPhase>,
    suspend_on: Option<CheckpointPhase>,
    suspended: bool,
    phases: Vec<CheckpointPhase>,
}
impl Recovery for Store {
    fn load<'a>(&'a mut self) -> LocalFuture<'a, Result<Option<HarnessState>, String>> {
        Box::pin(async move { Ok(self.saved.clone()) })
    }
    fn checkpoint<'a>(
        &'a mut self,
        boundary: CheckpointBoundary,
        state: &'a HarnessState,
    ) -> LocalFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.saved =
                Some(serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap());
            self.phases.push(boundary.phase);
            if self.suspend_on == Some(boundary.phase) {
                self.suspended = true;
            }
            if self.fail == Some(boundary.phase) {
                self.fail = None;
                return Err("simulated process interruption".into());
            }
            Ok(())
        })
    }
    fn suspend_requested(&self) -> bool {
        self.suspended
    }
}

fn run(
    host: &mut dyn ModelHost,
    store: &mut dyn Recovery,
    history: &mut Vec<Value>,
    dispatch: &mut dyn FnMut(&str, &Value) -> Result<String, String>,
    emit: &mut dyn FnMut(AgentEvent),
) -> Stop {
    runtime::run_with_host(
        LoopArgs {
            llm: &llm(),
            history,
            tools: &[tool()],
            dispatch,
            emit,
            cancelled: &|| false,
            max_rounds: 8,
            label: "test".into(),
            conversation: true,
            inputs: None,
            input_close: None,
        },
        "fixture instructions",
        host,
        store,
    )
}

#[test]
fn gdk_dispatches_dynamic_tools_and_preserves_native_reasoning() {
    let mut host = Host::new(vec![call(), response("finished")]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Run the fixture"})];
    let mut calls = 0;
    let mut events = Vec::new();
    let stop = run(
        &mut host,
        &mut store,
        &mut history,
        &mut |name, args| {
            assert_eq!(name, "fixture");
            assert_eq!(args, &json!({}));
            calls += 1;
            Ok("real tool result".into())
        },
        &mut |event| events.push(event),
    );
    assert_eq!(stop, Stop::EndTurn);
    assert_eq!(calls, 1);
    let requests = host.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body["tools"][0]["function"]["name"], "fixture");
    assert!(requests[0].body["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("fixture instructions"));
    assert!(requests[1].body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["reasoning_content"] == "retained reasoning"));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, AgentEvent::Usage { tokens: 13, .. }))
            .count(),
        2
    );
    assert!(store.phases.contains(&CheckpointPhase::SavedModel));
    assert!(store.phases.contains(&CheckpointPhase::AfterTool));
    assert!(history.iter().any(|m| m.get("_jazykGdk").is_some()));
}

#[test]
fn saved_model_response_is_reused_after_restart() {
    let mut host = Host::new(vec![call(), response("finished")]);
    let mut store = Store {
        fail: Some(CheckpointPhase::SavedModel),
        ..Default::default()
    };
    let mut history = vec![json!({"role":"user","content":"Run the fixture"})];
    let mut calls = 0;
    let mut dispatch = |_: &str, _: &Value| {
        calls += 1;
        Ok("result".into())
    };
    assert!(matches!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut dispatch,
            &mut |_| {}
        ),
        Stop::Error(_)
    ));
    assert_eq!(host.requests.borrow().len(), 1);
    assert_eq!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut dispatch,
            &mut |_| {}
        ),
        Stop::EndTurn
    );
    assert_eq!(calls, 1);
    assert_eq!(
        host.requests.borrow().len(),
        2,
        "saved model call must not be repeated"
    );
    assert_eq!(store.saved.as_ref().unwrap().usage.len(), 2);
}

#[test]
fn saved_tool_receipt_is_reused_after_restart() {
    let mut host = Host::new(vec![call(), response("finished")]);
    let mut store = Store {
        fail: Some(CheckpointPhase::AfterTool),
        ..Default::default()
    };
    let mut history = vec![json!({"role":"user","content":"Run the fixture"})];
    let mut calls = 0;
    let mut dispatch = |_: &str, _: &Value| {
        calls += 1;
        Ok("result".into())
    };
    assert!(matches!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut dispatch,
            &mut |_| {}
        ),
        Stop::Error(_)
    ));
    assert_eq!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut dispatch,
            &mut |_| {}
        ),
        Stop::EndTurn
    );
    assert_eq!(calls, 1);
    assert_eq!(host.requests.borrow().len(), 2);
}

#[test]
fn unfinished_tool_intention_requires_reconciliation() {
    let mut host = Host::new(vec![call(), response("finished")]);
    let mut store = Store {
        fail: Some(CheckpointPhase::BeforeTool),
        ..Default::default()
    };
    let mut history = vec![json!({"role":"user","content":"Run the fixture"})];
    let mut calls = 0;
    let mut dispatch = |_: &str, _: &Value| {
        calls += 1;
        Ok("result".into())
    };
    assert!(matches!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut dispatch,
            &mut |_| {}
        ),
        Stop::Error(_)
    ));
    let stop = run(
        &mut host,
        &mut store,
        &mut history,
        &mut dispatch,
        &mut |_| {},
    );
    assert!(matches!(stop, Stop::Error(error) if error.contains("uncertain")));
    assert_eq!(calls, 0);
}

#[test]
fn suspension_preserves_pending_tool_without_cancelling_it() {
    let mut host = Host::new(vec![call(), response("finished")]);
    let mut store = Store {
        suspend_on: Some(CheckpointPhase::SavedModel),
        ..Default::default()
    };
    let mut history = vec![json!({"role":"user","content":"Run the fixture"})];
    let mut calls = 0;
    let mut dispatch = |_: &str, _: &Value| {
        calls += 1;
        Ok("result".into())
    };
    assert_eq!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut dispatch,
            &mut |_| {}
        ),
        Stop::Suspended
    );
    store.suspended = false;
    store.suspend_on = None;
    assert_eq!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut dispatch,
            &mut |_| {}
        ),
        Stop::EndTurn
    );
    assert_eq!(calls, 1);
    assert_eq!(host.requests.borrow().len(), 2);
}

#[test]
fn explicit_upstream_compaction_replaces_and_checkpoints_history() {
    let mut host = Host::new(vec![
        response("summary of previous work"),
        response("finished"),
    ]);
    let mut store = Store::default();
    let mut history = vec![
        json!({"role":"user","content":"u".repeat(45000)}),
        json!({"role":"assistant","content":"a".repeat(45000)}),
        json!({"role":"user","content":"Continue"}),
    ];
    let stop = run(
        &mut host,
        &mut store,
        &mut history,
        &mut |_, _| panic!("unexpected tool"),
        &mut |_| {},
    );
    assert_eq!(stop, Stop::EndTurn);
    let requests = host.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].purpose, ModelPurpose::Compaction);
    assert_eq!(requests[1].purpose, ModelPurpose::Inference);
    assert!(store.phases.contains(&CheckpointPhase::Compacted));
    assert!(serde_json::to_string(&history).unwrap().len() < 5000);
    assert_eq!(store.saved.as_ref().unwrap().usage.len(), 2);
}

#[test]
fn owner_thread_scope_and_handle_block_on_work_inside_spawn_blocking() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let handle = runtime.handle().clone();
    runtime.block_on(async move {
        tokio::task::spawn_blocking(move || {
            let scope = crate::vfs::Scope::enter(&BTreeMap::from([(
                "fixture.txt".into(),
                b"tenant data".to_vec(),
            )]));
            let owner = std::thread::current().id();
            let mut host = Host::new(vec![call(), response("finished")]);
            let mut history = vec![json!({"role":"user","content":"Run the fixture"})];
            let stop = run(
                &mut host,
                &mut Store::default(),
                &mut history,
                &mut |_, _| {
                    assert_eq!(owner, std::thread::current().id());
                    handle.block_on(async {
                        assert!(crate::vfs::is_scoped());
                        crate::vfs::read_to_string("/project/fixture.txt")
                            .map_err(|e| e.to_string())
                    })
                },
                &mut |_| {
                    handle.block_on(async {});
                },
            );
            assert_eq!(stop, Stop::EndTurn);
            assert_eq!(scope.snapshot()["fixture.txt"], b"tenant data");
        })
        .await
        .unwrap();
    });
}

#[test]
fn stop_interrupts_pending_local_model_future() {
    struct PendingHost;
    impl ModelHost for PendingHost {
        fn infer<'a>(
            &'a mut self,
            _: ModelRequest,
        ) -> LocalFuture<'a, Result<ModelResponse, String>> {
            Box::pin(std::future::pending())
        }
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    let task = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(40));
        signal.store(true, Ordering::Relaxed);
    });
    let mut history = vec![json!({"role":"user","content":"Run the fixture"})];
    let started = std::time::Instant::now();
    let stop = runtime::run_with_host(
        LoopArgs {
            llm: &llm(),
            history: &mut history,
            tools: &[tool()],
            dispatch: &mut |_, _| panic!("cancelled tool"),
            emit: &mut |_| {},
            cancelled: &|| cancelled.load(Ordering::Relaxed),
            max_rounds: 8,
            label: "cancel".into(),
            conversation: true,
            inputs: None,
            input_close: None,
        },
        "",
        &mut PendingHost,
        &mut Store::default(),
    );
    task.join().unwrap();
    assert_eq!(stop, Stop::Cancelled);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn provider_call_ids_are_scoped_to_each_durable_inference() {
    let mut host = Host::new(vec![call(), call(), response("finished")]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Run twice"})];
    let mut calls = 0;
    assert_eq!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut |_, _| {
                calls += 1;
                Ok("result".into())
            },
            &mut |_| {}
        ),
        Stop::EndTurn
    );
    assert_eq!(
        calls, 2,
        "a provider's reused call id must not hide later work"
    );
    assert_eq!(store.saved.as_ref().unwrap().tool_receipts.len(), 2);
}

#[test]
fn steering_skips_stale_batch_calls_and_is_checkpointed_once() {
    let mut batch = call();
    let mut second = batch["choices"][0]["message"]["tool_calls"][0].clone();
    second["id"] = json!("tool-2");
    batch["choices"][0]["message"]["tool_calls"]
        .as_array_mut()
        .unwrap()
        .push(second);
    let mut host = Host::new(vec![batch, response("steering applied")]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Original request"})];
    let inbox = Rc::new(RefCell::new(Vec::new()));
    let mut inputs = || std::mem::take(&mut *inbox.borrow_mut());
    let mut calls = 0;
    let mut events = Vec::new();
    let stop = runtime::run_with_host(
        LoopArgs {
            llm: &llm(),
            history: &mut history,
            tools: &[tool()],
            dispatch: &mut |_, _| {
                calls += 1;
                inbox
                    .borrow_mut()
                    .push(json!({"messageId":"input-1","text":"Use the replacement request"}));
                Ok("first result".into())
            },
            emit: &mut |event| events.push(event),
            cancelled: &|| false,
            max_rounds: 8,
            label: "steering".into(),
            conversation: true,
            inputs: Some(&mut inputs),
            input_close: None,
        },
        "",
        &mut host,
        &mut store,
    );
    assert_eq!(stop, Stop::EndTurn);
    assert_eq!(calls, 1);
    let state = store.saved.as_ref().unwrap();
    assert_eq!(state.delivered_inputs, vec!["input-1"]);
    assert!(state.pending_inputs.is_empty());
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::Input(_)))
            .count(),
        1
    );
    assert!(host.requests.borrow()[1].body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["content"] == "Use the replacement request"));
    let receipts: Vec<_> = state.tool_receipts.values().collect();
    assert_eq!(receipts.iter().filter(|receipt| receipt.ok).count(), 1);
    assert_eq!(receipts.iter().filter(|receipt| !receipt.ok).count(), 1);
}

#[test]
fn cancellation_at_tool_start_pairs_the_unexecuted_request() {
    let mut host = Host::new(vec![call()]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let cancel = std::cell::Cell::new(false);
    let stop = runtime::run_with_host(
        LoopArgs {
            llm: &llm(),
            history: &mut history,
            tools: &[tool()],
            dispatch: &mut |_, _| panic!("cancelled tool executed"),
            emit: &mut |event| {
                if matches!(event, AgentEvent::ToolCallStart { .. }) {
                    cancel.set(true);
                }
            },
            cancelled: &|| cancel.get(),
            max_rounds: 8,
            label: "cancel".into(),
            conversation: true,
            inputs: None,
            input_close: None,
        },
        "",
        &mut host,
        &mut store,
    );
    assert_eq!(stop, Stop::Cancelled);
    let assistant = history
        .iter()
        .find(|message| message["tool_calls"].is_array())
        .unwrap();
    let call_id = assistant["tool_calls"][0]["id"].as_str().unwrap();
    assert!(history
        .iter()
        .any(|message| message["role"] == "tool" && message["tool_call_id"] == call_id));
}

#[test]
fn unavailable_tools_are_rejected_without_host_dispatch() {
    let mut unavailable = call();
    unavailable["choices"][0]["message"]["tool_calls"][0]["function"]["name"] = json!("shell");
    let mut host = Host::new(vec![unavailable, response("finished")]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Run"})];
    assert_eq!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut |_, _| panic!("unavailable tool executed"),
            &mut |_| {}
        ),
        Stop::EndTurn
    );
    assert!(host.requests.borrow()[1].body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["role"] == "tool"
            && message["content"]
                .as_str()
                .unwrap_or_default()
                .contains("not available")));
}

#[test]
fn repeated_tool_failures_stop_with_paired_results() {
    let mut host = Host::new(vec![call(); 5]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let mut calls = 0;
    let stop = run(
        &mut host,
        &mut store,
        &mut history,
        &mut |_, _| {
            calls += 1;
            Err("gate refused".into())
        },
        &mut |_| {},
    );
    assert!(matches!(stop, Stop::Error(error) if error.contains("five consecutive")));
    assert_eq!(calls, 5);
    assert_eq!(
        history
            .iter()
            .filter(|message| message["role"] == "tool")
            .count(),
        5
    );
}

#[test]
fn rejected_native_tool_capability_returns_an_explicit_error() {
    struct Reject;
    impl ModelHost for Reject {
        fn infer<'a>(
            &'a mut self,
            _: ModelRequest,
        ) -> LocalFuture<'a, Result<ModelResponse, String>> {
            Box::pin(async { Err("tools-rejected: this endpoint does not support tools".into()) })
        }
    }
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let stop = run(
        &mut Reject,
        &mut Store::default(),
        &mut history,
        &mut |_, _| panic!("rejected tool ran"),
        &mut |_| {},
    );
    assert!(
        matches!(stop, Stop::Error(error) if error.contains("native tool calling is required"))
    );
}

#[test]
fn duplicate_call_ids_in_one_response_cannot_dispatch_any_tools() {
    let mut duplicate = call();
    let call = duplicate["choices"][0]["message"]["tool_calls"][0].clone();
    duplicate["choices"][0]["message"]["tool_calls"]
        .as_array_mut()
        .unwrap()
        .push(call);
    let mut host = Host::new(vec![duplicate]);
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let stop = run(
        &mut host,
        &mut Store::default(),
        &mut history,
        &mut |_, _| panic!("duplicate call executed"),
        &mut |_| {},
    );
    assert!(matches!(stop, Stop::Error(error) if error.contains("duplicate tool-call id")));
}

#[test]
fn async_tool_host_awaits_without_moving_the_workspace() {
    struct AsyncTools {
        owner: std::thread::ThreadId,
        calls: u32,
    }
    impl ToolHost for AsyncTools {
        fn call<'a>(
            &'a mut self,
            name: &'a str,
            _: &'a Value,
        ) -> LocalFuture<'a, Result<String, String>> {
            Box::pin(async move {
                async_io::Timer::after(std::time::Duration::from_millis(1)).await;
                assert_eq!(self.owner, std::thread::current().id());
                assert_eq!(name, "fixture");
                self.calls += 1;
                crate::vfs::read_to_string("/project/fixture.txt").map_err(|e| e.to_string())
            })
        }
    }
    let _scope = crate::vfs::Scope::enter(&BTreeMap::from([(
        "fixture.txt".into(),
        b"async tenant data".to_vec(),
    )]));
    let mut tools = AsyncTools {
        owner: std::thread::current().id(),
        calls: 0,
    };
    let mut host = Host::new(vec![call(), response("finished")]);
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let stop = run_with_hosts(
        LoopArgs {
            llm: &llm(),
            history: &mut history,
            tools: &[tool()],
            dispatch: &mut |_, _| panic!("sync dispatch used"),
            emit: &mut |_| {},
            cancelled: &|| false,
            max_rounds: 8,
            label: "async tool".into(),
            conversation: true,
            inputs: None,
            input_close: None,
        },
        "",
        &mut host,
        &mut tools,
        &mut Store::default(),
    );
    assert_eq!(stop, Stop::EndTurn);
    assert_eq!(tools.calls, 1);
    assert!(host.requests.borrow()[1].body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["content"] == "async tenant data"));
}

#[test]
fn incompatible_checkpoint_refuses_takeover_with_recovery_signal() {
    let mut host = Host::new(vec![response("finished")]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Run"})];
    assert_eq!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut |_, _| panic!(),
            &mut |_| {}
        ),
        Stop::EndTurn
    );
    store.saved.as_mut().unwrap().version = 77;
    let stop = run(
        &mut host,
        &mut store,
        &mut history,
        &mut |_, _| panic!(),
        &mut |_| {},
    );
    assert!(matches!(stop, Stop::Error(error) if error.starts_with("checkpoint-incompatible:")));
    assert_eq!(host.requests.borrow().len(), 1);
}

#[test]
fn dynamic_tool_provider_preserves_complete_mcp_result_blocks() {
    let mut host = Host::new(vec![call(), response("finished")]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let result = json!({"content":[{"type":"text","text":"visible text"}],"structuredContent":{"value":7},"_meta":{"provenance":"fixture"},"isError":false});
    assert_eq!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut |_, _| Ok(result.to_string()),
            &mut |_| {}
        ),
        Stop::EndTurn
    );
    let state = store.saved.as_ref().unwrap();
    let response = state
        .conversation
        .iter()
        .flat_map(|message| &message.content)
        .find_map(goose_provider_types::conversation::message::MessageContent::as_tool_response)
        .unwrap();
    let result = serde_json::to_value(response.tool_result.as_ref().unwrap()).unwrap();
    assert_eq!(result["structuredContent"]["value"], 7);
    assert_eq!(result["_meta"]["provenance"], "fixture");
    assert_eq!(result["content"][0]["text"], "visible text");
}

#[test]
fn native_compile_tool_can_drive_an_inner_gdk_session_on_the_same_thread() {
    struct CompileTool;
    impl ToolHost for CompileTool {
        fn call<'a>(
            &'a mut self,
            _: &'a str,
            _: &'a Value,
        ) -> LocalFuture<'a, Result<String, String>> {
            Box::pin(async {
                let mut host = Host::new(vec![response("inner compile completed")]);
                let mut history = vec![json!({"role":"user","content":"Compile the project"})];
                let owner = std::thread::current().id();
                let stop = run(
                    &mut host,
                    &mut Store::default(),
                    &mut history,
                    &mut |_, _| panic!("unexpected inner tool"),
                    &mut |_| {
                        assert_eq!(owner, std::thread::current().id());
                        assert!(crate::vfs::is_scoped());
                    },
                );
                assert_eq!(stop, Stop::EndTurn);
                Ok("inner compile completed".into())
            })
        }
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        tokio::task::spawn_blocking(|| {
            let _scope = crate::vfs::Scope::enter(&BTreeMap::new());
            let mut host = Host::new(vec![call(), response("finished")]);
            let mut history = vec![json!({"role":"user","content":"Run the compile tool"})];
            let stop = run_with_hosts(
                LoopArgs {
                    llm: &llm(),
                    history: &mut history,
                    tools: &[tool()],
                    dispatch: &mut |_, _| panic!("sync dispatch used"),
                    emit: &mut |_| {},
                    cancelled: &|| false,
                    max_rounds: 8,
                    label: "outer chat".into(),
                    conversation: true,
                    inputs: None,
                    input_close: None,
                },
                "",
                &mut host,
                &mut CompileTool,
                &mut Store::default(),
            );
            assert_eq!(stop, Stop::EndTurn);
        })
        .await
        .unwrap();
    });
}

#[test]
fn native_stream_envelope_preserves_gdk_usage_and_tools() {
    let native = goose_provider_types::formats::openai::response_to_message(&call()).unwrap();
    let usage = goose_provider_types::conversation::token_usage::ProviderUsage::new(
        "fixture".into(),
        goose_provider_types::formats::openai::get_usage(&call()),
    )
    .with_response_id("native-response".into());
    let mut host = Host::new(vec![
        json!({"_jazykGdkResponse":{"message":native,"usage":usage}}),
        response("finished"),
    ]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let mut calls = 0;
    assert_eq!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut |_, _| {
                calls += 1;
                Ok("result".into())
            },
            &mut |_| {}
        ),
        Stop::EndTurn
    );
    assert_eq!(calls, 1);
    assert_eq!(
        store.saved.as_ref().unwrap().usage[0]
            .response_id
            .as_deref(),
        Some("native-response")
    );
    assert!(host.requests.borrow()[1].body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["reasoning_content"] == "retained reasoning"));
}

#[test]
fn checkpoint_failure_and_uncertainty_have_preservable_recovery_signals() {
    let mut host = Host::new(vec![call()]);
    let mut store = Store {
        fail: Some(CheckpointPhase::BeforeTool),
        ..Default::default()
    };
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let stop = run(
        &mut host,
        &mut store,
        &mut history,
        &mut |_, _| panic!("uncheckpointed tool ran"),
        &mut |_| {},
    );
    assert!(matches!(stop, Stop::Error(error) if error.starts_with("checkpoint-failed:")));
    let stop = run(
        &mut host,
        &mut store,
        &mut history,
        &mut |_, _| panic!("uncertain tool reran"),
        &mut |_| {},
    );
    assert!(matches!(stop, Stop::Error(error) if error.starts_with("execution-uncertain:")));
}

#[test]
fn live_model_deltas_run_on_the_owner_thread_without_repeating_the_final_prefix() {
    struct StreamingHost {
        completed: Arc<AtomicBool>,
    }
    impl ModelHost for StreamingHost {
        fn infer<'a>(
            &'a mut self,
            request: ModelRequest,
        ) -> LocalFuture<'a, Result<ModelResponse, String>> {
            Box::pin(async move {
                let sink = request.deltas.expect("native inference has a delta sink");
                std::thread::spawn(move || {
                    sink("content", "Hello ");
                    sink("reasoning_content", "Think ");
                    sink("tool_calls", "must not appear");
                })
                .join()
                .unwrap();
                async_io::Timer::after(std::time::Duration::from_millis(10)).await;
                self.completed.store(true, Ordering::SeqCst);
                let mut body = response("Hello there");
                body["choices"][0]["message"]["reasoning_content"] = json!("Think more");
                Ok(ModelResponse { body })
            })
        }
    }
    let completed = Arc::new(AtomicBool::new(false));
    let mut host = StreamingHost {
        completed: completed.clone(),
    };
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let owner = std::thread::current().id();
    let mut content = String::new();
    let mut thought = String::new();
    let stop = run(
        &mut host,
        &mut Store::default(),
        &mut history,
        &mut |_, _| panic!(),
        &mut |event| {
            assert_eq!(owner, std::thread::current().id());
            match event {
                AgentEvent::Message(text) => {
                    if text == "Hello " {
                        assert!(!completed.load(Ordering::SeqCst));
                    }
                    content.push_str(&text);
                }
                AgentEvent::Thought(text) => thought.push_str(&text),
                _ => {}
            }
        },
    );
    assert_eq!(stop, Stop::EndTurn);
    assert_eq!(content, "Hello there");
    assert_eq!(thought, "Think more");
    assert!(history
        .iter()
        .any(|message| message["content"] == "Hello there"));
}

#[test]
fn input_accepted_during_a_final_model_reply_is_delivered_before_finishing() {
    struct SteeringHost {
        inbox: Rc<RefCell<Vec<Value>>>,
        calls: usize,
    }
    impl ModelHost for SteeringHost {
        fn infer<'a>(
            &'a mut self,
            request: ModelRequest,
        ) -> LocalFuture<'a, Result<ModelResponse, String>> {
            Box::pin(async move {
                self.calls += 1;
                if self.calls == 1 {
                    self.inbox
                        .borrow_mut()
                        .push(json!({"messageId":"final-input","text":"Preserve the identifiers"}));
                } else {
                    assert!(request
                        .body
                        .to_string()
                        .contains("Preserve the identifiers"));
                }
                Ok(ModelResponse {
                    body: response("finished"),
                })
            })
        }
    }
    let inbox = Rc::new(RefCell::new(Vec::new()));
    let mut host = SteeringHost {
        inbox: inbox.clone(),
        calls: 0,
    };
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let mut inputs = || std::mem::take(&mut *inbox.borrow_mut());
    let mut store = Store::default();
    let stop = runtime::run_with_host(
        LoopArgs {
            llm: &llm(),
            history: &mut history,
            tools: &[],
            dispatch: &mut |_, _| panic!(),
            emit: &mut |_| {},
            cancelled: &|| false,
            max_rounds: 8,
            label: "steering at end".into(),
            conversation: true,
            inputs: Some(&mut inputs),
            input_close: Some(&mut || true),
        },
        "",
        &mut host,
        &mut store,
    );
    assert_eq!(stop, Stop::EndTurn);
    assert_eq!(host.calls, 2);
    let saved = store.saved.unwrap();
    assert_eq!(saved.delivered_inputs, vec!["final-input"]);
    assert_eq!(
        saved.delivered_input_values["final-input"],
        json!({"messageId":"final-input","text":"Preserve the identifiers"})
    );
    assert_eq!(store.phases.last(), Some(&CheckpointPhase::Finished));
}

#[test]
fn saved_model_receipt_retains_the_already_streamed_prefix() {
    struct PrefixHost;
    impl ModelHost for PrefixHost {
        fn infer<'a>(
            &'a mut self,
            request: ModelRequest,
        ) -> LocalFuture<'a, Result<ModelResponse, String>> {
            Box::pin(async move {
                request.deltas.unwrap()("content", "Hello ");
                Ok(ModelResponse {
                    body: response("Hello there"),
                })
            })
        }
    }
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let mut store = Store {
        fail: Some(CheckpointPhase::SavedModel),
        ..Default::default()
    };
    let mut content = String::new();
    let mut emit = |event| {
        if let AgentEvent::Message(text) = event {
            content.push_str(&text);
        }
    };
    assert!(matches!(
        run(
            &mut PrefixHost,
            &mut store,
            &mut history,
            &mut |_, _| panic!(),
            &mut emit
        ),
        Stop::Error(_)
    ));
    let prefix = store
        .saved
        .as_ref()
        .unwrap()
        .model_deltas
        .values()
        .next()
        .unwrap();
    assert_eq!(prefix.content, "Hello ");
    let mut cached_host = Host::new(vec![]);
    assert_eq!(
        run(
            &mut cached_host,
            &mut store,
            &mut history,
            &mut |_, _| panic!(),
            &mut emit
        ),
        Stop::EndTurn
    );
    assert!(cached_host.requests.borrow().is_empty());
    assert_eq!(content, "Hello there");
    assert!(store.saved.unwrap().model_deltas.is_empty());
}

fn done_response(extra_call: bool) -> Value {
    let mut calls =
        vec![json!({"id":"done-1","type":"function","function":{"name":"done","arguments":"{}"}})];
    if extra_call {
        calls.push(json!({"id":"after-done","type":"function","function":{"name":"fixture","arguments":"{}"}}));
    }
    json!({"choices":[{"message":{"role":"assistant","content":"","tool_calls":calls},"finish_reason":"tool_calls"}]})
}

fn completion_run(
    conversation: bool,
    host: &mut dyn ModelHost,
    store: &mut dyn Recovery,
    history: &mut Vec<Value>,
    dispatch: &mut dyn FnMut(&str, &Value) -> Result<String, String>,
    mut inputs: Option<&mut dyn FnMut() -> Vec<Value>>,
) -> Stop {
    let mut tools = vec![tool()];
    tools.push(GenericTool {
        name: "done".into(),
        description: "Commit the validated batch".into(),
        parameters: json!({"type":"object"}),
    });
    let mut source = || inputs.as_mut().map(|source| source()).unwrap_or_default();
    let llm = llm();
    runtime::run_with_host(
        LoopArgs {
            llm: &llm,
            history,
            tools: &tools,
            dispatch,
            emit: &mut |_| {},
            cancelled: &|| false,
            max_rounds: 8,
            label: "completion".into(),
            conversation,
            inputs: Some(&mut source),
            input_close: None,
        },
        "",
        host,
        store,
    )
}

#[test]
fn worker_done_stops_before_more_tools_inference_or_post_commit_steering() {
    let mut host = Host::new(vec![done_response(true), response("must not request this")]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Resolve the batch"})];
    let inbox = Rc::new(RefCell::new(Vec::new()));
    let delivery = inbox.clone();
    let mut inputs = || std::mem::take(&mut *delivery.borrow_mut());
    let mut calls = 0;
    let stop = completion_run(
        false,
        &mut host,
        &mut store,
        &mut history,
        &mut |name, _| {
            assert_eq!(name, "done");
            calls += 1;
            inbox
                .borrow_mut()
                .push(json!({"messageId":"next-batch-input","text":"Change the next goal"}));
            Ok(json!({"content":[{"type":"text","text":"Committed"}],"isError":false}).to_string())
        },
        Some(&mut inputs),
    );
    assert_eq!(stop, Stop::EndTurn);
    assert_eq!(calls, 1);
    assert_eq!(host.requests.borrow().len(), 1);
    assert_eq!(inbox.borrow().len(), 1);
    assert!(store.saved.as_ref().unwrap().delivered_inputs.is_empty());
    assert_eq!(store.phases.last(), Some(&CheckpointPhase::Finished));
}

#[test]
fn a_chat_continues_after_done_to_explain_the_result() {
    let mut host = Host::new(vec![done_response(false), response("The batch committed.")]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Resolve and explain"})];
    assert_eq!(
        completion_run(
            true,
            &mut host,
            &mut store,
            &mut history,
            &mut |_, _| Ok("Committed".into()),
            None
        ),
        Stop::EndTurn
    );
    assert_eq!(host.requests.borrow().len(), 2);
    assert!(history
        .iter()
        .any(|message| message["content"] == "The batch committed."));
}

#[test]
fn a_saved_done_receipt_finishes_without_tool_or_model_replay() {
    let mut host = Host::new(vec![done_response(false)]);
    let mut store = Store {
        fail: Some(CheckpointPhase::AfterTool),
        ..Default::default()
    };
    let mut history = vec![json!({"role":"user","content":"Resolve"})];
    let mut calls = 0;
    assert!(matches!(
        completion_run(
            false,
            &mut host,
            &mut store,
            &mut history,
            &mut |_, _| {
                calls += 1;
                Ok("Committed".into())
            },
            None
        ),
        Stop::Error(_)
    ));
    assert_eq!(calls, 1);
    let mut restored_host = Host::new(vec![]);
    assert_eq!(
        completion_run(
            false,
            &mut restored_host,
            &mut store,
            &mut history,
            &mut |_, _| panic!("committed tool must not repeat"),
            None
        ),
        Stop::EndTurn
    );
    assert!(restored_host.requests.borrow().is_empty());
    assert_eq!(store.phases.last(), Some(&CheckpointPhase::Finished));
    assert_eq!(history.last().unwrap()["role"], "tool");
}

#[test]
fn a_rejected_done_gate_does_not_complete_a_worker() {
    let mut host = Host::new(vec![
        done_response(false),
        response("The gate needs repair."),
    ]);
    let mut store = Store::default();
    let mut history = vec![json!({"role":"user","content":"Resolve"})];
    assert_eq!(
        completion_run(
            false,
            &mut host,
            &mut store,
            &mut history,
            &mut |_, _| {
                Ok(
                    json!({"content":[{"type":"text","text":"Gate refused"}],"isError":true})
                        .to_string(),
                )
            },
            None
        ),
        Stop::EndTurn
    );
    assert_eq!(host.requests.borrow().len(), 2);
}

#[test]
fn input_checkpoint_preserves_the_full_payload_before_delivery_events() {
    let input = json!({"messageId":"answer-1","text":"The diagnostic has no safe resolution",
        "diagnosticAnswer":{"state":"failed","reason":"The source is contradictory"}});
    let mut inbox = Some(input.clone());
    let mut inputs = || inbox.take().into_iter().collect();
    let mut host = Host::new(vec![]);
    let mut store = Store {
        fail: Some(CheckpointPhase::Input),
        ..Default::default()
    };
    let mut history = vec![json!({"role":"user","content":"Resolve the diagnostic"})];
    let mut events = Vec::new();
    let stop = runtime::run_with_host(
        LoopArgs {
            llm: &llm(),
            history: &mut history,
            tools: &[],
            dispatch: &mut |_, _| panic!(),
            emit: &mut |event| events.push(event),
            cancelled: &|| false,
            max_rounds: 4,
            label: "input receipt".into(),
            conversation: true,
            inputs: Some(&mut inputs),
            input_close: None,
        },
        "",
        &mut host,
        &mut store,
    );
    assert!(matches!(stop,Stop::Error(ref error) if error.starts_with("checkpoint-failed:")));
    assert!(host.requests.borrow().is_empty());
    assert!(!events
        .iter()
        .any(|event| matches!(event, AgentEvent::Input(_))));
    let saved = store.saved.unwrap();
    assert_eq!(saved.delivered_inputs, vec!["answer-1"]);
    assert_eq!(saved.delivered_input_values["answer-1"], input);
}

#[test]
fn host_response_replay_retains_a_durable_prefix_before_the_engine_receipt() {
    let mut history = vec![json!({"role":"user","content":"Run"})];
    let mut store = Store {
        fail: Some(CheckpointPhase::BeforeModel),
        ..Default::default()
    };
    assert!(matches!(
        run(
            &mut Host::new(vec![]),
            &mut store,
            &mut history,
            &mut |_, _| panic!(),
            &mut |_| {}
        ),
        Stop::Error(_)
    ));
    let saved = store.saved.as_mut().unwrap();
    let request = saved.model_requests.keys().next().unwrap().clone();
    saved.model_deltas.insert(
        request,
        ModelDeltaProgress {
            content: "Hello ".into(),
            thought: String::new(),
        },
    );
    assert!(saved.model_receipts.is_empty());
    let mut host = Host::new(vec![response("Hello there")]);
    let mut content = "Hello ".to_string();
    assert_eq!(
        run(
            &mut host,
            &mut store,
            &mut history,
            &mut |_, _| panic!(),
            &mut |event| if let AgentEvent::Message(text) = event {
                content.push_str(&text);
            }
        ),
        Stop::EndTurn
    );
    assert_eq!(content, "Hello there");
    assert_eq!(host.requests.borrow().len(), 1);
}
