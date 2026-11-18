// Sub-agents of the embedded agent. Each sub-agent turn runs on its own thread with
// its own connections to the session's MCP servers, so it gets its own serving.
// The sub-agent has no ACP session: its updates travel on the root session with
// `_meta.jazyk.agent`, and the client reaches it through `_session/steering`.
// Mirrors docs/frontends/acp.md#sub-agent-updates.
use super::*;
use crate::harness::agents::{self, AgentRun, ChildHost, ChildRequest, RunSpec, Tree};
use goose_agent::subagent::SubagentOutcome;

pub(super) type Notify = Arc<dyn Fn(SessionUpdate) + Send + Sync>;

// A sub-agent's conversation: its servers, tools, and history, kept across turns.
pub(super) struct ChildState {
    conns: Vec<McpServerConn>,
    skills: Vec<crate::agent_context::Skill>,
    tools: Vec<GenericTool>,
    history: Vec<Value>,
}

pub(super) type Children = Arc<Mutex<HashMap<String, Arc<Mutex<Option<ChildState>>>>>>;

static NEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(super) struct AcpChildren {
    pub root: String,
    pub servers: Vec<McpServer>,
    pub cwd: std::path::PathBuf,
    pub llm: crate::llm::Llm,
    pub notify: Notify,
    pub children: Children,
    pub tree: Tree,
}

/// An update with `_meta.jazyk.agent` merged in, so the client routes it to the
/// sub-agent's conversation.
pub(super) fn with_agent(update: SessionUpdate, agent: Value) -> SessionUpdate {
    let Ok(mut value) = serde_json::to_value(&update) else {
        return update;
    };
    if !value["_meta"].is_object() {
        value["_meta"] = json!({});
    }
    if !value["_meta"]["jazyk"].is_object() {
        value["_meta"]["jazyk"] = json!({});
    }
    value["_meta"]["jazyk"]["agent"] = agent;
    serde_json::from_value(value).unwrap_or(update)
}

fn stop_name(stop: &Stop) -> &'static str {
    match stop {
        Stop::EndTurn => "end_turn",
        Stop::Cancelled => "cancelled",
        Stop::MaxRounds => "max_turn_requests",
        Stop::Suspended | Stop::Error(_) => "error",
    }
}

impl AcpChildren {
    fn agent(&self, id: &str, extra: Value) -> Value {
        let mut agent = match self.tree.get(id) {
            Some(node) => json!({"id": node.id, "parentId": node.parent, "depth": node.depth,
                "title": node.title, "task": node.task, "write": node.write.value()}),
            None => json!({"id": id}),
        };
        if let (Some(agent), Value::Object(extra)) = (agent.as_object_mut(), extra) {
            agent.extend(extra);
        }
        agent
    }

    // A deleted sub-agent's last updates go nowhere: the client removed it.
    fn send(&self, id: &str, update: SessionUpdate, extra: Value) {
        if self.tree.get(id).is_none() {
            return;
        }
        (self.notify)(with_agent(update, self.agent(id, extra)));
    }

    fn end(&self, id: &str, stop: &str, outcome: &SubagentOutcome) {
        self.send(
            id,
            SessionUpdate::UsageUpdate(UsageUpdate::new(0, 0)),
            json!({"turnEnd": {"stop": stop, "result": agents::outcome_value(outcome)}}),
        );
    }

    // A turn a person started on an idle sub-agent, on its own thread. Its result
    // goes to the parent, which did not request it
    // (docs/frontends/subagents.md#prompting-a-sub-agent).
    pub(super) fn prompt(
        &self,
        root: &SessionEntry,
        id: &str,
        text: String,
        message_id: Value,
    ) -> Result<Value, String> {
        let (node, turn) = self.tree.begin_turn(id)?;
        let spec = RunSpec::of(&node, self.tree.clone());
        let host = self.clone();
        let root = root.clone();
        let reply = json!({"outcome": "started", "turn": turn});
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                host.turn_with(spec, text, message_id)
            }))
            .unwrap_or_else(|p| {
                SubagentOutcome::Failed(format!(
                    "the sub-agent panicked: {}",
                    crate::sys::panic_text(p.as_ref())
                ))
            });
            host.tree.finish(&node.id, &turn, outcome.clone());
            host.deliver(&root, &node, &turn, &outcome);
        });
        Ok(reply)
    }

    // The parent of a turn it did not request receives the result: as steering
    // while it runs, otherwise appended to its conversation for its next turn.
    fn deliver(
        &self,
        root: &SessionEntry,
        node: &agents::Node,
        turn: &str,
        outcome: &SubagentOutcome,
    ) {
        // A deleted sub-agent reports to no one.
        if self.tree.get(&node.id).is_none() {
            return;
        }
        let input = agents::result_input(&node.id, turn, outcome);
        let text = input["text"].as_str().unwrap_or_default().to_string();
        let update = event_update(AgentEvent::Input(input.clone()), &mut 0);
        if node.parent == self.root {
            {
                let mut inbox = root.inbox.lock().unwrap();
                if inbox.0 && !root.cancelled.load(Ordering::Relaxed) {
                    inbox.1.push(input);
                    return;
                }
            }
            let mut state = root.state.lock().unwrap_or_else(|p| p.into_inner());
            harness::push_user_text(&mut state.history, &text);
            drop(state);
            (self.notify)(update);
            return;
        }
        if self.tree.steer(&node.parent, input) {
            return;
        }
        let slot = self.children.lock().unwrap().get(&node.parent).cloned();
        if let Some(slot) = slot {
            if let Some(state) = slot.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                harness::push_user_text(&mut state.history, &text);
            }
        }
        self.send(&node.parent, update, json!({}));
    }

    fn turn(&self, spec: RunSpec, text: String) -> SubagentOutcome {
        self.turn_with(spec, text, Value::Null)
    }

    fn turn_with(&self, spec: RunSpec, text: String, message_id: Value) -> SubagentOutcome {
        let id = spec.id.clone();
        let slot = self
            .children
            .lock()
            .unwrap()
            .entry(id.clone())
            .or_default()
            .clone();
        let mut slot = slot.lock().unwrap_or_else(|p| p.into_inner());
        self.send(
            &id,
            SessionUpdate::UserMessageChunk(ContentChunk::new(ContentBlock::from(text.clone()))),
            json!({"turn": "started", "messageId": message_id}),
        );
        if slot.is_none() {
            let context = Some(crate::agent_context::native(&self.cwd));
            match open_session(&self.servers, &self.cwd, context) {
                Ok(Opened {
                    conns,
                    tools,
                    history,
                    skills,
                }) => {
                    *slot = Some(ChildState {
                        conns,
                        skills,
                        tools,
                        history,
                    })
                }
                Err(error) => {
                    let outcome = SubagentOutcome::Failed(error);
                    self.end(&id, "error", &outcome);
                    return outcome;
                }
            }
        }
        let state = slot.as_mut().expect("opened above");
        harness::push_user_text(&mut state.history, &text);
        let mut total_tokens = 0u64;
        let mut emit = |ev: AgentEvent| {
            self.send(&id, event_update(ev, &mut total_tokens), json!({}));
        };
        let tree = self.tree.clone();
        let inbox = id.clone();
        let mut inputs = move || tree.take_inbox(&inbox);
        let token = spec.cancel.clone();
        let cancelled = move || token.is_cancelled();
        let mut tools = ConnectedTools(&mut state.conns, &state.skills);
        let mut model = harness::provider::DefaultModelHost::new(&self.llm);
        let mut recovery = harness::MemoryRecovery;
        let mut dispatch = |_: &str, _: &Value| Err("tool host unavailable".into());
        let run = AgentRun::new(spec, std::rc::Rc::new(self.clone()));
        let stop = harness::run_with_agents(
            LoopArgs {
                llm: &self.llm,
                history: &mut state.history,
                tools: &state.tools,
                dispatch: &mut dispatch,
                emit: &mut emit,
                cancelled: &cancelled,
                max_rounds: crate::limits::agent_max_rounds(),
                label: format!("agent {id}"),
                conversation: true,
                inputs: Some(&mut inputs),
                input_close: None,
            },
            "",
            &mut model,
            &mut tools,
            &mut recovery,
            &run,
        );
        let outcome = agents::outcome_of(&stop, &state.history);
        self.end(&id, stop_name(&stop), &outcome);
        outcome
    }
}

impl ChildHost for AcpChildren {
    fn create(&self, _: &ChildRequest<'_>) -> Result<String, String> {
        Ok(format!(
            "{}/a{}",
            self.root,
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    // The turn runs on its own thread; the parent awaits its outcome.
    fn run_turn(
        &self,
        child: RunSpec,
        text: String,
    ) -> harness::LocalFuture<'static, SubagentOutcome> {
        let (tx, rx) = futures::channel::oneshot::channel();
        let host = self.clone();
        std::thread::spawn(move || {
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| host.turn(child, text)))
                    .unwrap_or_else(|p| {
                        SubagentOutcome::Failed(format!(
                            "the sub-agent panicked: {}",
                            crate::sys::panic_text(p.as_ref())
                        ))
                    });
            let _ = tx.send(outcome);
        });
        Box::pin(async move {
            rx.await.unwrap_or_else(|_| {
                SubagentOutcome::Failed("the sub-agent thread ended without a result".into())
            })
        })
    }

    fn root_of(&self, _: &str) -> String {
        self.root.clone()
    }
}

/// Steering for a sub-agent of `root`: text for its running turn or the next turn
/// of an idle one, its stop, or its removal (docs/frontends/acp.md#sub-agent-updates).
pub(super) fn steer(
    tree: &Tree,
    root: &str,
    jazyk: &Value,
    text: String,
    session: Option<(&AcpChildren, &SessionEntry)>,
) -> Result<Value, String> {
    let id = jazyk["agentId"].as_str().unwrap_or_default();
    if !tree.get(id).is_some_and(|node| node.root == root) {
        return Err(format!("no sub-agent {id} in this session"));
    }
    if jazyk["deleteAgent"] == true {
        let removed = tree.remove(id);
        if let Some((host, _)) = session {
            let mut children = host.children.lock().unwrap();
            for id in &removed {
                children.remove(id);
            }
        }
        return Ok(json!({"outcome":"deleted","agents":removed}));
    }
    if jazyk["cancelAgent"] == true {
        tree.cancel(id);
        return Ok(json!({"outcome":"cancelled"}));
    }
    if text.trim().is_empty() {
        return Err("text is required".into());
    }
    let mut input = jazyk.clone();
    if let Some(input) = input.as_object_mut() {
        input.remove("agentId");
        input.remove("cancelAgent");
    }
    input["text"] = json!(text);
    if input["messageId"].is_null() {
        input["messageId"] = json!(format!(
            "steer-{id}-{}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    }
    if tree.steer(id, input.clone()) {
        return Ok(json!({"outcome":"injected"}));
    }
    match session {
        Some((host, entry)) => host.prompt(entry, id, text, input["messageId"].clone()),
        None => Ok(json!({"outcome":"promptRequired","reason":"noRunningTurn"})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The meta survives on every update kind the agent streams, beside the meta the
    // update already carried.
    #[test]
    fn agent_meta_rides_on_every_streamed_update() {
        let agent = json!({"id":"sess-1/a1","parentId":"sess-1","depth":1});
        let mut total = 0;
        for event in [
            AgentEvent::Message("hi".into()),
            AgentEvent::Thought("hm".into()),
            AgentEvent::ReasoningSummary("sum".into()),
            AgentEvent::ToolCallStart {
                id: "t1".into(),
                name: "search".into(),
                args: json!({}),
            },
            AgentEvent::ToolCallEnd {
                id: "t1".into(),
                result: "ok".into(),
                ok: true,
            },
            AgentEvent::Usage {
                tokens: 3,
                cached: 0,
            },
            AgentEvent::Input(json!({"text":"x","source":{"agentId":"sess-1/a2"}})),
        ] {
            let update = with_agent(event_update(event, &mut total), agent.clone());
            let value = serde_json::to_value(&update).unwrap();
            assert_eq!(value["_meta"]["jazyk"]["agent"], agent, "{value}");
        }
        let update = with_agent(
            event_update(
                AgentEvent::Usage {
                    tokens: 2,
                    cached: 1,
                },
                &mut total,
            ),
            agent.clone(),
        );
        let value = serde_json::to_value(&update).unwrap();
        assert_eq!(value["_meta"]["jazyk"]["tokens"], 2);
    }

    #[test]
    fn steering_reaches_only_this_sessions_running_sub_agents() {
        let tree = Tree::default();
        let id = "sess-1/a1".to_string();
        tree.adopt(agents::Node {
            id: id.clone(),
            parent: "sess-1".into(),
            root: "sess-1".into(),
            depth: 1,
            title: "look".into(),
            task: "look".into(),
            write: agents::WriteScope::None,
            state: agents::AgentState::Running,
            turns: 1,
            results: Default::default(),
            inbox: Vec::new(),
            cancel: Default::default(),
        });
        assert!(steer(&tree, "sess-2", &json!({"agentId": id}), "hi".into(), None).is_err());
        assert_eq!(
            steer(&tree, "sess-1", &json!({"agentId": id}), "hi".into(), None).unwrap()["outcome"],
            "injected"
        );
        assert_eq!(
            steer(
                &tree,
                "sess-1",
                &json!({"agentId": id, "cancelAgent": true}),
                String::new(),
                None
            )
            .unwrap()["outcome"],
            "cancelled"
        );
        assert!(tree.get(&id).unwrap().cancel.is_cancelled());
    }
}
