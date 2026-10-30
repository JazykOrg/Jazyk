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
