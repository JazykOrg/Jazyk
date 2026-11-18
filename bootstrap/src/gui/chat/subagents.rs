// Sub-agents of the embedded agent as chat sessions. Their updates arrive on the
// root conversation's ACP session with `_meta.jazyk.agent`; each sub-agent becomes
// a chat session with its tree fields, and its inputs and stop travel back through
// `_session/steering` on the root session. Mirrors docs/frontends/acp.md#sub-agent-updates
// and docs/frontends/gui.md#agents.
use super::*;

impl ChatSession {
    // The tree fields every conversation carries; a conversation a person opened is
    // its own root.
    pub(super) fn with_tree(&self, mut row: Value) -> Value {
        let fields = match &self.tree {
            Some(tree) => json!({"parentId": tree["parentId"], "rootId": tree["rootId"],
                "depth": tree["depth"], "task": tree["task"], "write": tree["write"]}),
            None => {
                json!({"parentId": null, "rootId": self.id, "depth": 0, "task": null, "write": null})
            }
        };
        if let (Some(row), Value::Object(fields)) = (row.as_object_mut(), fields) {
            row.extend(fields);
        }
        row
    }

    // The file the conversation's latest tool call named.
    fn location(&self) -> Option<Value> {
        self.ring.iter().rev().find_map(|(_, u)| {
            if u["sessionUpdate"] != "tool_call" {
                return None;
            }
            let input = &u["rawInput"];
            let path = u["locations"][0]["path"]
                .as_str()
                .or_else(|| input["path"].as_str())
                .or_else(|| input["doc"].as_str())?;
            let mut location = json!({"path": path});
            if let Some(section) = input["section"].as_str() {
                location["section"] = json!(section);
            }
            Some(location)
        })
    }

    fn row(&self) -> Value {
        let state = match self.state.as_str() {
            "running" if self.stopped => "cancelled",
            "running" | "failed" | "cancelled" => self.state.as_str(),
            _ => "idle",
        };
        let mut row = self.with_tree(json!({"id": self.id, "agentId": self.id,
            "kind": if self.tree.is_some() { "subagent" } else { "chat" },
            "title": self.title, "state": state}));
        if let Some(location) = self.location() {
            row["location"] = location;
        }
        if let Some(result) = &self.result {
            row["result"] = result.clone();
        }
        row
    }
}

impl ChatManager {
    // Running conversations first, then the most recent idle ones.
    pub fn agent_rows(&self) -> Vec<Value> {
        let mut rows: Vec<(u64, Value)> = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .map(|s| {
                let s = s.lock().unwrap();
                let n = s.id.trim_start_matches("chat-").parse().unwrap_or(0);
                (n, s.row())
            })
            .collect();
        rows.sort_by(|(a, ra), (b, rb)| {
            (rb["state"] == "running")
                .cmp(&(ra["state"] == "running"))
                .then(b.cmp(a))
        });
        let mut idle = 0;
        rows.into_iter()
            .filter(|(_, row)| {
                row["state"] == "running" || {
                    idle += 1;
                    idle <= super::super::agent_tree::RECENT_IDLE
                }
            })
            .map(|(_, row)| row)
            .collect()
    }

    fn by_agent(&self, agent: &str) -> Option<Arc<Mutex<ChatSession>>> {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .find(|s| {
                s.lock()
                    .unwrap()
                    .agent
                    .as_ref()
                    .is_some_and(|(_, a)| a == agent)
            })
            .cloned()
    }
}

// The tree a stored sub-agent transcript records first, and its last result.
pub(super) fn restored(
    ring: &VecDeque<(u64, Value)>,
) -> (Option<Value>, Option<(String, String)>, Option<Value>) {
    let Some(recorded) = ring
        .iter()
        .find(|(_, u)| u["sessionUpdate"] == "agent_tree")
        .map(|(_, u)| u)
    else {
        return (None, None, None);
    };
    let result = ring
        .iter()
        .rev()
        .find_map(|(_, u)| (u["sessionUpdate"] == "turn_end").then(|| u["result"].clone()))
        .filter(|r| !r.is_null());
    let agent = recorded["agentId"].as_str().map(|a| {
        (
            recorded["tree"]["rootId"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            a.to_string(),
        )
    });
    (Some(recorded["tree"].clone()), agent, result)
}

// One update of a sub-agent of the root conversation `root`: the first creates its
// chat session.
pub(super) fn child_update(st: &SharedState, root: &str, mut update: Value) {
    let agent = update["_meta"]["jazyk"]["agent"].take();
    let Some(agent_id) = agent["id"].as_str() else {
        return;
    };
    if let Some(jazyk) = update["_meta"]["jazyk"].as_object_mut() {
        jazyk.remove("agent");
    }
    let sess = match st.chat.by_agent(agent_id) {
        Some(sess) => sess,
        // Only a sub-agent the agent still holds carries its tree fields.
        None if agent["parentId"].is_null() => return,
        None => {
            let parent = agent["parentId"]
                .as_str()
                .and_then(|p| st.chat.by_agent(p))
                .map(|p| p.lock().unwrap().id.clone())
                .unwrap_or_else(|| root.to_string());
            let n = st.chat.seq.fetch_add(1, Ordering::Relaxed) + 1;
            let id = format!("chat-{n}");
            let tree = json!({"parentId": parent, "rootId": root, "depth": agent["depth"],
                "task": agent["task"], "write": agent["write"]});
            let title = agent["title"].as_str().unwrap_or("Sub-agent").to_string();
            let session = Arc::new(Mutex::new(ChatSession {
                id: id.clone(),
                title: title.clone(),
                state: "running".into(),
                handle: None,
                host: None,
                binding: None,
                agent_selection: None,
                selection_id: None,
                ring: VecDeque::new(),
                next_n: 0,
                pending: Vec::new(),
                inputs: Vec::new(),
                turn_id: None,
                stopped: false,
                tree: Some(tree.clone()),
                agent: Some((root.to_string(), agent_id.to_string())),
                result: None,
            }));
            crate::acp::sessions::open(&st.out, &id, &st.proj().root, "gui");
            st.chat
                .sessions
                .lock()
                .unwrap()
                .insert(id.clone(), session.clone());
            append(
                st,
                &session,
                json!({"sessionUpdate":"agent_tree","agentId":agent_id,"title":title,"tree":tree}),
            );
            super::super::agent_tree::sessions_changed(st);
            session
        }
    };
    if agent["turn"] == "started" {
        let text = update["content"]["text"].clone();
        {
            let mut s = sess.lock().unwrap();
            s.result = None;
            s.stopped = false;
        }
        let mut message = json!({"sessionUpdate":"user_message","text":text});
        // A turn a person started names the input that started it.
        if !agent["messageId"].is_null() {
            message["messageId"] = agent["messageId"].clone();
        }
        append(st, &sess, message);
        set_state(st, &sess, "running");
    } else if let Some(end) = agent.get("turnEnd") {
        let stop = end["stop"].as_str().unwrap_or("error");
        sess.lock().unwrap().result = Some(end["result"].clone());
        let mut turn_end = json!({"sessionUpdate":"turn_end","stop":stop,"result":end["result"]});
        if end["result"]["status"] == "failed" {
            turn_end["error"] = end["result"]["text"].clone();
        }
        append(st, &sess, turn_end);
        set_state(
            st,
            &sess,
            match stop {
                "end_turn" => "idle",
                "cancelled" => "cancelled",
                _ => "failed",
            },
        );
    } else if let Some(input) = update["_meta"]["jazyk"]["input"].as_object() {
        let message_id = input.get("messageId").cloned().unwrap_or_default();
        delivered_result(st, &sess, &update["_meta"]["jazyk"]["input"]);
        update_input(st, &sess, &message_id, "delivered", None);
    } else {
        append(st, &sess, update);
    }
}

// A sub-agent's result delivered to `sess`: an input and a user message naming it
// (docs/frontends/subagents.md#results).
pub(super) fn delivered_result(st: &SharedState, sess: &Arc<Mutex<ChatSession>>, input: &Value) {
    let Some(agent) = input["source"]["agentId"].as_str() else {
        return;
    };
    // The agent process names its own sub-agents; the transcript names the chat.
    let source = st
        .chat
        .by_agent(agent)
        .map(|s| s.lock().unwrap().id.clone())
        .unwrap_or_else(|| agent.to_string());
    let record = json!({"messageId": input["messageId"], "text": input["text"],
        "state": "delivered", "source": {"agentId": source}});
    sess.lock().unwrap().inputs.push(record.clone());
    append(st, sess, json!({"sessionUpdate":"input","input":record}));
    append(
        st,
        sess,
        json!({"sessionUpdate":"user_message","text":input["text"],"messageId":input["messageId"],"source":{"agentId":source}}),
    );
}

fn root_handle(
    st: &SharedState,
    sess: &Arc<Mutex<ChatSession>>,
) -> Option<(SessionHandle, String)> {
    let (root, agent) = sess.lock().unwrap().agent.clone()?;
    let root = st.chat.sessions.lock().unwrap().get(&root).cloned()?;
    let handle = root.lock().unwrap().handle.clone()?;
    Some((handle, agent))
}

// Text for a sub-agent: steering for its running turn, or the next turn of an idle
// one, which its parent did not request
// (docs/frontends/subagents.md#prompting-a-sub-agent).
pub(super) async fn child_input(
    st: &SharedState,
    sess: &Arc<Mutex<ChatSession>>,
    body: &Value,
) -> Response {
    let text = body["text"].as_str().unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return super::super::api::err(StatusCode::BAD_REQUEST, "text is required");
    }
    let running = sess.lock().unwrap().state == "running";
    let Some((handle, agent)) = root_handle(st, sess) else {
        return super::super::api::err(
            StatusCode::CONFLICT,
            "the agent session that ran this sub-agent has ended; prompt its root conversation instead",
        );
    };
    if !running {
        return child_prompt(st, sess, handle, agent, body["messageId"].as_str(), text).await;
    }
    let message_id = body["messageId"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "native-{}-{}",
                crate::sys::process_id(),
                st.chat.seq.fetch_add(1, Ordering::Relaxed)
            )
        });
    let input = {
        let mut s = sess.lock().unwrap();
        if let Some(existing) = s.inputs.iter().find(|i| i["messageId"] == message_id) {
            return (StatusCode::ACCEPTED, Json(existing.clone())).into_response();
        }
        let input = json!({"messageId":message_id,"text":text,"sequence":s.inputs.len()+1,
            "state":"queued","turnId":s.turn_id,"deliveryMode":"steer"});
        s.inputs.push(input.clone());
        input
    };
    append(st, sess, json!({"sessionUpdate":"input","input":input}));
    append(
        st,
        sess,
        json!({"sessionUpdate":"user_message","messageId":message_id,"text":text}),
    );
    let meta = json!({"agentId": agent, "messageId": message_id});
    let reply = tokio::task::spawn_blocking(move || handle.steer(&text, meta)).await;
    match reply {
        Ok(Ok(result)) if result["outcome"] == "injected" => {}
        Ok(Ok(_)) => update_input(
            st,
            sess,
            &json!(message_id),
            "failed",
            Some("the sub-agent's turn ended before the input arrived"),
        ),
        Ok(Err(error)) => update_input(
            st,
            sess,
            &json!(message_id),
            "delivery_unknown",
            Some(&error),
        ),
        Err(_) => update_input(
            st,
            sess,
            &json!(message_id),
            "delivery_unknown",
            Some("steering did not complete"),
        ),
    }
    (StatusCode::ACCEPTED, Json(input)).into_response()
}

// A person's prompt to an idle sub-agent: the agent starts its next turn, whose
// updates arrive like any of its turns.
async fn child_prompt(
    st: &SharedState,
    sess: &Arc<Mutex<ChatSession>>,
    handle: SessionHandle,
    agent: String,
    message_id: Option<&str>,
    text: String,
) -> Response {
    let message_id = message_id.map(str::to_string).unwrap_or_else(|| {
        format!(
            "native-{}-{}",
            crate::sys::process_id(),
            st.chat.seq.fetch_add(1, Ordering::Relaxed)
        )
    });
    if let Some(existing) = sess
        .lock()
        .unwrap()
        .inputs
        .iter()
        .find(|i| i["messageId"] == message_id)
    {
        return if existing["text"] == text {
            (StatusCode::ACCEPTED, Json(existing.clone())).into_response()
        } else {
            super::super::api::err(StatusCode::CONFLICT, "messageId has different text")
        };
    }
    let meta = json!({"agentId": agent, "messageId": message_id});
    let steer_text = text.clone();
    let reply = tokio::task::spawn_blocking(move || handle.steer(&steer_text, meta)).await;
    let turn = match reply {
        Ok(Ok(reply)) if reply["outcome"] == "started" => reply["turn"].clone(),
        Ok(Ok(reply)) => {
            return super::super::api::err(
                StatusCode::CONFLICT,
                format!("the sub-agent did not start a turn: {reply}"),
            )
        }
        Ok(Err(error)) => return super::super::api::err(StatusCode::CONFLICT, error),
        Err(_) => {
            return super::super::api::err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the prompt did not reach the agent",
            )
        }
    };
    let input = {
        let mut s = sess.lock().unwrap();
        let input = json!({"messageId":message_id,"text":text,"sequence":s.inputs.len()+1,
            "state":"delivered","turnId":turn,"deliveryMode":"turn"});
        s.turn_id = turn.as_str().map(str::to_string);
        s.inputs.push(input.clone());
        input
    };
    append(st, sess, json!({"sessionUpdate":"input","input":input}));
    (StatusCode::ACCEPTED, Json(input)).into_response()
}

// The sessions of `id`'s sub-agents and their descendants, nearest first.
pub(super) fn descendants(st: &SharedState, id: &str) -> Vec<Arc<Mutex<ChatSession>>> {
    let sessions: Vec<_> = st.chat.sessions.lock().unwrap().values().cloned().collect();
    let parents: Vec<(String, Option<String>)> = sessions
        .iter()
        .map(|s| {
            let s = s.lock().unwrap();
            let parent = s
                .tree
                .as_ref()
                .and_then(|t| t["parentId"].as_str().map(str::to_string));
            (s.id.clone(), parent)
        })
        .collect();
    let mut ids = vec![id.to_string()];
    let mut next = 0;
    while next < ids.len() {
        for (child, parent) in &parents {
            if parent.as_deref() == Some(ids[next].as_str()) && !ids.contains(child) {
                ids.push(child.clone());
            }
        }
        next += 1;
    }
    ids.iter()
        .skip(1)
        .filter_map(|id| {
            sessions
                .iter()
                .find(|s| s.lock().unwrap().id == *id)
                .cloned()
        })
        .collect()
}

// Delete a sub-agent in its agent: it stops with its descendants and leaves the
// tree (docs/frontends/acp.md#sub-agent-updates).
pub(super) fn delete_child(st: &SharedState, sess: &Arc<Mutex<ChatSession>>) {
    let Some((handle, agent)) = root_handle(st, sess) else {
        return;
    };
    sess.lock().unwrap().stopped = true;
    let meta = json!({"agentId": agent, "deleteAgent": true});
    tokio::task::spawn_blocking(move || {
        let _ = handle.steer("delete", meta);
    });
}

// Stop a sub-agent and its descendants; its parent keeps running.
pub(super) async fn child_cancel(st: &SharedState, sess: &Arc<Mutex<ChatSession>>) -> Response {
    let Some((handle, agent)) = root_handle(st, sess) else {
        return super::super::api::err(
            StatusCode::CONFLICT,
            "the sub-agent's root conversation is not running",
        );
    };
    sess.lock().unwrap().stopped = true;
    let meta = json!({"agentId": agent, "cancelAgent": true});
    let _ = tokio::task::spawn_blocking(move || handle.steer("stop", meta)).await;
    (StatusCode::ACCEPTED, Json(json!({"ok": true}))).into_response()
}
