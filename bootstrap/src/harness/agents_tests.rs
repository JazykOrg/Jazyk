// Sub-agent tools, delivery, limits, scope, and cancellation, with scripted models
// and scripted sub-agent turns. Mirrors docs/frontends/subagents.md.
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use goose_agent::subagent::SubagentOutcome;
use serde_json::{json, Value};

use super::super::{
    runtime::sleep_tick, AgentEvent, GenericTool, LocalFuture, LoopArgs, MemoryRecovery, ModelHost,
    ModelRequest, ModelResponse, Stop, ToolHost,
};
use super::*;

fn llm() -> crate::llm::Llm {
    crate::llm::Llm {
        api: "chat_completions".into(),
        reasoning_summary: None,
        base_url: "http://127.0.0.1:3625/v1".into(),
        model: "fixture".into(),
        api_key: String::new(),
        temperature: None,
        trace: None,
    }
}

fn reply(text: &str) -> Value {
    json!({"choices":[{"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})
}

fn call(id: &str, name: &str, args: Value) -> Value {
    json!({"choices":[{"message":{"role":"assistant","content":"","tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})
}

struct Model {
    replies: VecDeque<Value>,
    requests: Rc<RefCell<Vec<Value>>>,
}
impl ModelHost for Model {
    fn infer<'a>(
        &'a mut self,
        request: ModelRequest,
    ) -> LocalFuture<'a, Result<ModelResponse, String>> {
        Box::pin(async move {
            self.requests.borrow_mut().push(request.body);
            Ok(ModelResponse {
                body: self.replies.pop_front().ok_or("unexpected inference")?,
            })
        })
    }
}

#[derive(Default)]
struct Serving {
    calls: Vec<(String, Value)>,
}
fn serving_tool(name: &str) -> GenericTool {
    GenericTool {
        name: name.into(),
        description: name.into(),
        parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":true}),
    }
}
impl ToolHost for Serving {
    fn catalog(&mut self) -> LocalFuture<'_, Result<Option<Vec<GenericTool>>, String>> {
        Box::pin(async {
            Ok(Some(vec![
                serving_tool("read_document"),
                serving_tool("edit_doc_prose"),
            ]))
        })
    }
    fn call<'a>(
        &'a mut self,
        name: &'a str,
        args: &'a Value,
    ) -> LocalFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.calls.push((name.to_string(), args.clone()));
            Ok("served".into())
        })
    }
}

// Scripted sub-agent turns: each finishes after a few ticks with its reply, or
// holds until stopped.
#[derive(Default)]
struct Kids {
    created: RefCell<Vec<String>>,
    prompts: Rc<RefCell<Vec<(String, String)>>>,
    hold: bool,
}
impl ChildHost for Kids {
    fn create(&self, request: &ChildRequest<'_>) -> Result<String, String> {
        let id = format!("c{}", self.created.borrow().len() + 1);
        assert!(!request.task.is_empty());
        self.created.borrow_mut().push(id.clone());
        Ok(id)
    }
    fn run_turn(&self, child: RunSpec, text: String) -> LocalFuture<'static, SubagentOutcome> {
        let hold = self.hold;
        self.prompts
            .borrow_mut()
            .push((child.id.clone(), text.clone()));
        Box::pin(async move {
            for _ in 0..3 {
                if child.cancelled() {
                    return SubagentOutcome::Cancelled;
                }
                sleep_tick().await;
            }
            if hold {
                loop {
                    if child.cancelled() {
                        return SubagentOutcome::Cancelled;
                    }
                    sleep_tick().await;
                }
            }
            let ask = text.lines().last().unwrap_or_default().to_string();
            SubagentOutcome::Completed(format!("{} reports on {ask}", child.id))
        })
    }
}

struct Outcome {
    stop: Stop,
    requests: Vec<Value>,
    events: Vec<AgentEvent>,
    history: Vec<Value>,
    served: Vec<(String, Value)>,
}

fn drive(replies: Vec<Value>, run: &AgentRun, cancel_after_requests: Option<usize>) -> Outcome {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let mut model = Model {
        replies: replies.into(),
        requests: requests.clone(),
    };
    let mut serving = Serving::default();
    let mut history = vec![json!({"role":"user","content":"Delegate the work"})];
    let mut events = Vec::new();
    let watched = requests.clone();
    let cancelled = move || cancel_after_requests.is_some_and(|n| watched.borrow().len() >= n);
    let stop = crate::harness::run_with_agents(
        LoopArgs {
            llm: &llm(),
            history: &mut history,
            tools: &[],
            dispatch: &mut |_, _| Err("no dispatch".into()),
            emit: &mut |event| events.push(event),
            cancelled: &cancelled,
            max_rounds: 20,
            label: "parent".into(),
            conversation: true,
            inputs: None,
            input_close: None,
        },
        "",
        &mut model,
        &mut serving,
        &mut MemoryRecovery,
        run,
    );
    let requests = requests.borrow().clone();
    Outcome {
        stop,
        requests,
        events,
        history,
        served: serving.calls,
    }
}

fn root_run(kids: Kids) -> (AgentRun, Rc<RefCell<Vec<(String, String)>>>) {
    let prompts = kids.prompts.clone();
    (
        AgentRun::new(RunSpec::root("parent", Tree::default()), Rc::new(kids)),
        prompts,
    )
}

fn sent(request: &Value) -> String {
    request["messages"].to_string()
}

fn sources(events: &[AgentEvent]) -> Vec<Value> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Input(input) => Some(input["source"]["agentId"].clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_finished_sub_agent_reports_when_the_parent_ends_its_reply() {
    let (run, prompts) = root_run(Kids::default());
    let out = drive(
        vec![
            call(
                "s1",
                "spawn_agent",
                json!({"task":"Check the glossary links","title":"Links"}),
            ),
            reply("Waiting for the links report."),
            reply("The links are fine."),
        ],
        &run,
        None,
    );
    assert_eq!(out.stop, Stop::EndTurn);
    assert_eq!(out.requests.len(), 3, "the result continues the turn once");
    let tools: Vec<&str> = out.requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["function"]["name"].as_str())
        .collect();
    for name in TOOLS {
        assert!(tools.contains(&name), "{name} is listed: {tools:?}");
    }
    // The opening prompt frames the task and the scope.
    let prompts = prompts.borrow();
    assert_eq!(prompts[0].0, "c1");
    assert!(prompts[0].1.contains("Check the glossary links"));
    assert!(prompts[0].1.contains("none (read-only)"));
    assert!(sent(&out.requests[2]).contains("Subagent c1 completed: c1 reports on"));
    assert!(!sent(&out.requests[1]).contains("Subagent c1"));
    assert_eq!(sources(&out.events), vec![json!("c1")]);
    let node = run.spec.tree.get("c1").unwrap();
    assert_eq!(node.state, AgentState::Idle);
    assert_eq!(
        (node.parent.as_str(), node.root.as_str(), node.depth),
        ("parent", "parent", 1)
    );
    assert!(out
        .history
        .iter()
        .any(|m| m.to_string().contains("Subagent c1 completed")));
}

#[test]
fn a_waited_result_is_returned_once_and_not_delivered_again() {
    let (run, _) = root_run(Kids::default());
    let out = drive(
        vec![
            call(
                "s1",
                "spawn_agent",
                json!({"task":"Count the requirements","wait":true}),
            ),
            reply("There are many."),
        ],
        &run,
        None,
    );
    assert_eq!(out.stop, Stop::EndTurn);
    assert_eq!(out.requests.len(), 2, "no delivery inference follows");
    let tool_result = sent(&out.requests[1]);
    assert!(tool_result.contains("c1 reports on"), "{tool_result}");
    assert!(tool_result.contains("returned"));
    assert!(!out
        .history
        .iter()
        .any(|m| m.to_string().contains("Subagent c1 completed")));
    assert!(sources(&out.events).is_empty());
}

#[test]
fn message_agent_starts_a_new_turn_that_reports_under_its_own_id() {
    let (run, prompts) = root_run(Kids::default());
    let out = drive(
        vec![
            call("s1", "spawn_agent", json!({"task":"Read the intro"})),
            reply("Waiting."),
            call(
                "m1",
                "message_agent",
                json!({"id":"c1","text":"Now read the outro"}),
            ),
            reply("Waiting again."),
            reply("Both read."),
        ],
        &run,
        None,
    );
    assert_eq!(out.stop, Stop::EndTurn);
    assert_eq!(out.requests.len(), 5);
    assert!(sent(&out.requests[4])
        .contains("Subagent c1#2 completed: c1 reports on Now read the outro"));
    assert_eq!(
        prompts.borrow()[1],
        ("c1".into(), "Now read the outro".into())
    );
    assert_eq!(run.spec.tree.get("c1").unwrap().turns, 2);
    assert_eq!(sources(&out.events), vec![json!("c1"), json!("c1")]);
}

#[test]
fn stopping_the_parent_stops_its_sub_agents_and_records_them_cancelled() {
    let (run, _) = root_run(Kids {
        hold: true,
        ..Default::default()
    });
    let out = drive(
        vec![
            call("s1", "spawn_agent", json!({"task":"Work until stopped"})),
            reply("Waiting for it."),
        ],
        &run,
        Some(2),
    );
    assert_eq!(out.stop, Stop::Cancelled);
    let node = run.spec.tree.get("c1").unwrap();
    assert!(
        node.cancel.is_cancelled(),
        "the child's token follows the parent's"
    );
    assert_eq!(node.state, AgentState::Cancelled);
    assert!(out.history.iter().any(|m| m
        .to_string()
        .contains("Subagent c1 was cancelled before it finished.")));
    assert_eq!(sources(&out.events), vec![json!("c1")]);
}

#[test]
fn cancel_agent_reports_cancelled_and_the_parent_continues() {
    let (run, _) = root_run(Kids {
        hold: true,
        ..Default::default()
    });
    let out = drive(
        vec![
            call("s1", "spawn_agent", json!({"task":"Work until stopped"})),
            call("x1", "cancel_agent", json!({"id":"c1"})),
            reply("Stopped it."),
            reply("Done without it."),
        ],
        &run,
        None,
    );
    assert_eq!(out.stop, Stop::EndTurn);
    assert!(sent(&out.requests[3]).contains("Subagent c1 was cancelled before it finished."));
    assert_eq!(
        run.spec.tree.get("c1").unwrap().state,
        AgentState::Cancelled
    );
}

#[test]
fn a_scoped_sub_agent_dispatches_only_inside_its_scope() {
    let tree = Tree::default();
    let spec = RunSpec {
        id: "c9".into(),
        depth: 1,
        write: WriteScope::Paths(vec!["docs".into()]),
        tree,
        cancel: Default::default(),
    };
    let run = AgentRun::new(spec, Rc::new(Kids::default()));
    let out = drive(
        vec![
            call("w1", "edit_doc_prose", json!({"path":"src/main.rs"})),
            call("w2", "edit_doc_prose", json!({"path":"docs/a.md"})),
            reply("Edited the docs only."),
        ],
        &run,
        None,
    );
    assert_eq!(out.stop, Stop::EndTurn);
    assert_eq!(
        out.served,
        vec![("edit_doc_prose".to_string(), json!({"path":"docs/a.md"}))]
    );
    let refused = sent(&out.requests[1]);
    assert!(
        refused.contains("outside this sub-agent's write scope (only docs)"),
        "{refused}"
    );

    // A read-only sub-agent is not offered the write tools at all.
    let spec = RunSpec {
        id: "c8".into(),
        depth: 1,
        write: WriteScope::None,
        tree: Tree::default(),
        cancel: Default::default(),
    };
    let run = AgentRun::new(spec, Rc::new(Kids::default()));
    let out = drive(vec![reply("Nothing to do.")], &run, None);
    let tools = out.requests[0]["tools"].to_string();
    assert!(tools.contains("read_document") && !tools.contains("edit_doc_prose"));
}

fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    futures::executor::block_on(future)
}

#[test]
fn spawns_over_a_limit_or_scope_are_refused_as_tool_results() {
    let (run, _) = root_run(Kids {
        hold: true,
        ..Default::default()
    });
    for n in 0..crate::limits::AGENT_MAX_CHILDREN {
        let reply = block(run.call("spawn_agent", &json!({"task": format!("part {n}")}))).unwrap();
        assert!(reply.contains("\"started\""));
    }
    let refused = block(run.call("spawn_agent", &json!({"task":"one more"}))).unwrap_err();
    assert!(refused.contains("AGENT_MAX_CHILDREN (4)"), "{refused}");

    let deep = AgentRun::new(
        RunSpec {
            id: "deep".into(),
            depth: crate::limits::AGENT_MAX_DEPTH,
            write: WriteScope::All,
            tree: Tree::default(),
            cancel: Default::default(),
        },
        Rc::new(Kids::default()),
    );
    let refused = block(deep.call("spawn_agent", &json!({"task":"deeper"}))).unwrap_err();
    assert!(refused.contains("AGENT_MAX_DEPTH (3)"));
    assert!(!definitions(crate::limits::AGENT_MAX_DEPTH)
        .iter()
        .any(|t| t.name == "spawn_agent"));

    let scoped = AgentRun::new(
        RunSpec {
            id: "scoped".into(),
            depth: 1,
            write: WriteScope::Paths(vec!["docs".into()]),
            tree: Tree::default(),
            cancel: Default::default(),
        },
        Rc::new(Kids::default()),
    );
    let refused =
        block(scoped.call("spawn_agent", &json!({"task":"write all","write":"all"}))).unwrap_err();
    assert!(refused.contains("at most your own (only docs)"));
    assert!(block(scoped.call(
        "spawn_agent",
        &json!({"task":"write a doc","write":["docs/a.md"]})
    ))
    .is_ok());

    let stranger = block(run.call("cancel_agent", &json!({"id":"c1"})));
    assert!(stranger.is_ok(), "the parent reaches its own child");
    // `c3` is the root run's child, in another tree than the scoped run's.
    let foreign = block(scoped.call("message_agent", &json!({"id":"c3","text":"hi"}))).unwrap_err();
    assert!(foreign.contains("`c3` is not one of your sub-agents"));
    let status: Value =
        serde_json::from_str(&block(run.call("agent_status", &json!({}))).unwrap()).unwrap();
    assert_eq!(status["agents"].as_array().unwrap().len(), 4);
    assert!(status["agents"][0].get("result").is_none());
}

#[test]
fn write_scopes_parse_nest_and_judge_path_arguments() {
    assert_eq!(WriteScope::parse(&Value::Null).unwrap(), WriteScope::None);
    assert_eq!(WriteScope::parse(&json!("all")).unwrap(), WriteScope::All);
    let docs = WriteScope::parse(&json!(["./docs/", "src/a.rs"])).unwrap();
    assert_eq!(
        docs,
        WriteScope::Paths(vec!["docs".into(), "src/a.rs".into()])
    );
    assert!(WriteScope::parse(&json!(["../x"])).is_err());
    assert!(WriteScope::parse(&json!(["/abs"])).is_err());
    assert!(WriteScope::parse(&json!("some")).is_err());
    assert!(WriteScope::None.within(&WriteScope::None));
    assert!(!WriteScope::All.within(&docs));
    assert!(WriteScope::Paths(vec!["docs/x".into()]).within(&docs));
    assert!(!WriteScope::Paths(vec!["docsx".into()]).within(&docs));
    assert!(docs.admits("edit_doc_prose", &json!({"doc":"docs/a.md"})));
    assert!(docs.admits("write_files", &json!({"paths":["docs/a","src/a.rs"]})));
    assert!(!docs.admits("write_files", &json!({"paths":["docs/a","src/b.rs"]})));
    assert!(!docs.admits("record_verdict", &json!({})));
    assert!(docs.admits("read_document", &json!({"path":"elsewhere"})));
    assert!(WriteScope::None.admits("spawn_agent", &json!({})));
    assert!(!WriteScope::None.admits("edit_doc_prose", &json!({"path":"docs/a.md"})));
}
