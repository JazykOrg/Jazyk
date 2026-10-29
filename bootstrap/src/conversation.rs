// The chat pane's backend in a tab. Natively a chat session is an ACP session with an
// agent process and a `jazyk mcp chat` serving injected into it. A tab has nothing to
// spawn, so a conversation is the pieces a goal batch uses in `runner`: the generic
// agent loop, an in-process `chat` serving it calls tools on, and the page's model
// behind `crate::llm`. The worker is inside the turn until it ends, so every update
// is posted to the page as it happens, in the native event stream's shapes.
// See docs/frontends/wasm.md#chat.

use crate::agent_loop::{self, AgentEvent, GenericTool, LoopArgs, Stop};
use crate::mcp::McpServer;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(serde::Serialize, serde::Deserialize)]
struct Chat {
    id: String,
    title: String,
    // The title becomes the first prompt line (docs/frontends/gui.md#chat).
    titled: bool,
    // idle | running | failed
    state: String,
    updates: Vec<(u64, Value)>,
    next_n: u64,
    // The running conversation, kept across prompts. The serving and its tools live
    // as long as the conversation, so the loaded set carries from turn to turn. Both
    // are taken out for the length of a turn.
    history: Vec<Value>,
    #[serde(skip)]
    serving: Option<(McpServer, Vec<GenericTool>, String)>,
    // A reader conversation, for a model of the `reader` tier: the `graph` serving
    // with the read tools alone (docs/frontends/wasm.md#chat).
    reader: bool,
    #[serde(default)]
    instruction_hash: String,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    inputs: BTreeMap<String, Value>,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Book {
    seq: u64,
    chats: BTreeMap<String, Chat>,
}

thread_local! {
    static BOOK: RefCell<Book> = RefCell::new(Book::default());
}

// A new project starts with no conversations.
pub fn reset() {
    BOOK.with(|b| *b.borrow_mut() = Book::default());
}

fn snapshot() -> Vec<Value> {
    BOOK.with(|b| {
        b.borrow()
            .chats
            .values()
            .map(|c| {
                json!({"id": c.id, "title": c.title, "state": c.state, "updates": c.next_n,
                       "pending": [], "turnId":c.turn_id, "inputs":c.inputs.values().collect::<Vec<_>>()})
            })
            .collect()
    })
}

fn publish_sessions() {
    crate::portable::emit_event("chat.sessions", json!({"sessions": snapshot()}));
}

// One numbered update into the transcript, and to the page.
fn append(id: &str, update: Value) {
    let n = BOOK.with(|b| {
        let mut b = b.borrow_mut();
        let c = b.chats.get_mut(id)?;
        let n = c.next_n;
        c.next_n += 1;
        c.updates.push((n, update.clone()));
        Some(n)
    });
    if let Some(n) = n {
        crate::portable::emit_event(
            "chat.update",
            json!({"sessionId": id, "n": n, "update": update}),
        );
    }
}

fn set_state(id: &str, state: &str) {
    BOOK.with(|b| {
        if let Some(c) = b.borrow_mut().chats.get_mut(id) {
            c.state = state.to_string();
        }
    });
    publish_sessions();
}

// The page passes the loaded model's tier: `{"tier": "reader"}` opens a reader
// conversation (docs/frontends/wasm.md#chat).
pub fn create(body: &Value) -> Value {
    let reader = body["tier"].as_str() == Some("reader");
    let id = BOOK.with(|b| {
        let mut b = b.borrow_mut();
        b.seq += 1;
        let n = b.seq;
        let id = format!("chat-{}", n);
        b.chats.insert(
            id.clone(),
            Chat {
                id: id.clone(),
                title: format!("Chat {}", n),
                titled: false,
                state: "idle".into(),
                updates: Vec::new(),
                next_n: 0,
                history: Vec::new(),
                serving: None,
                reader,
                instruction_hash: String::new(),
                turn_id: None,
                inputs: BTreeMap::new(),
            },
        );
        id
    });
    publish_sessions();
    json!({"id": id})
}

// The list, with the slash command catalog the native server sends beside it. The
// catalog lives in `acp`, which a tab does not build, so it is empty here.
pub fn list() -> Value {
    json!({"sessions": snapshot(), "commands": []})
}

pub fn get(id: &str) -> Option<Value> {
    BOOK.with(|b| {
        let b = b.borrow();
        let c = b.chats.get(id)?;
        let updates: Vec<Value> = c
            .updates
            .iter()
            .map(|(n, u)| json!({"n": n, "update": u}))
            .collect();
        Some(
            json!({"id": c.id, "title": c.title, "state": c.state, "pending": [],
                    "updates": updates, "turnId":c.turn_id,"inputs":c.inputs.values().collect::<Vec<_>>()}),
        )
    })
}

// Remove a conversation (docs/frontends/gui.md#chat). A turn cannot be running: the
// worker answers nothing while it is inside one. Answers whether it existed.
pub fn remove(id: &str) -> bool {
    let gone = BOOK.with(|b| b.borrow_mut().chats.remove(id).is_some());
    if gone {
        publish_sessions();
    }
    gone
}

// The chat serving for one conversation, as a native chat session gets it: `chat`
// mode, ephemeral, no build token. A reader conversation gets the `graph` serving
// with the read tools alone and a short instruction block instead, so the prompt
// fits a small model's window. Answers the serving, its tools in the shape the loop
// hands the model, and its instructions.
fn open_serving(reader: bool) -> Result<(McpServer, Vec<GenericTool>, String), String> {
    let project = crate::document_sets::Workspace::load(&crate::portable::root())
        .map_err(|e| e.1)?
        .files_project();
    let out = project.out.clone();
    let server = McpServer::with_bridge(
        project,
        out,
        vec![if reader { "graph" } else { "chat" }.to_string()],
        false,
        crate::mcp::BridgeFlags {
            ephemeral: true,
            reader,
            ..Default::default()
        },
    );
    let init = server
        .handle(
            "initialize",
            &json!({"protocolVersion": "2024-11-05", "clientInfo": {"name": "jazyk-wasm-chat"}}),
        )
        .map_err(|e| e.1)?;
    let instructions = init["instructions"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let list = server.handle("tools/list", &json!({})).map_err(|e| e.1)?;
    let tools = list["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|t| GenericTool {
            name: t["name"].as_str().unwrap_or_default().to_string(),
            description: t["description"].as_str().unwrap_or_default().to_string(),
            parameters: t["inputSchema"].clone(),
        })
        .collect();
    Ok((server, tools, instructions))
}

// A user message into the history. Two user messages in a row are merged: some chat
// templates refuse a conversation whose roles do not alternate, and a turn that
// failed mid-way can leave a tool result as the last message.
fn push_user(history: &mut Vec<Value>, text: String) {
    if let Some(last) = history.last_mut() {
        if last["role"] == "user" {
            let prev = last["content"].as_str().unwrap_or_default().to_string();
            last["content"] = json!(format!("{}\n\n{}", prev, text));
            return;
        }
    }
    history.push(json!({"role": "user", "content": text}));
}

// A model error, with the remedy when it reads as a context window too small for
// the conversation. See docs/frontends/wasm.md#chat.
fn explain(err: &str) -> String {
    let e = err.to_lowercase();
    // A rate limit or a spent quota says so first: it exceeds a limit too, but a
    // new conversation would not help.
    let limited = ["rate limit", "rate_limit", "429", "quota"]
        .iter()
        .any(|k| e.contains(k));
    if limited {
        return format!("{} (the model source is rate limited or out of quota. Wait, or choose another source.)", err);
    }
    let too_big = ["context", "too long", "too large", "exceed", "maximum"]
        .iter()
        .any(|k| e.contains(k));
    if too_big {
        format!(
            "{} (the conversation does not fit the model's context: the instructions and the \
             chat tools travel with every call. Start a new conversation, or choose a model \
             source with a larger window.)",
            err
        )
    } else {
        err.to_string()
    }
}

// Answers the status and body the native handler would: 202 once the turn has run,
// or the refusal. The turn's progress has already reached the page as events.
pub fn prompt(id: &str, body: &Value) -> (u16, Value) {
    let text = body["text"].as_str().unwrap_or("").trim().to_string();
    let found = BOOK.with(|b| b.borrow().chats.get(id).map(|c| c.state.clone()));
    let Some(state) = found else {
        return (404, json!({"error": format!("no chat session {}", id)}));
    };
    if text.is_empty() {
        return (
            400,
            json!({"error": "text is required: the prompt to send"}),
        );
    }
    let message_id = body["messageId"].as_str().map(str::to_owned)
        .unwrap_or_else(|| BOOK.with(|b| format!("{}-{}",id,b.borrow().chats[id].next_n)));
    let turn_id = body["turnId"].as_str().unwrap_or(&message_id).to_owned();
    let prior = BOOK.with(|b| b.borrow().chats[id].inputs.get(&message_id).cloned());
    if let Some(prior) = prior {
        return if prior["text"].as_str() == Some(text.as_str()) {
            (202, json!({"ok":true,"messageId":message_id,"state":prior["state"],"turnId":prior["turnId"]}))
        } else { (409,json!({"error":"messageId already identifies different text"})) };
    }
    if state == "running" {
        return (
            409,
            json!({"error": "a turn is already running; cancel it or wait for it to end"}),
        );
    }

    if let Some(after)=body["afterN"].as_u64() {
        if after > (1u64 << 53) - 10_000 {
            return (400,json!({"error":"conversation event watermark is too large"}));
        }
        BOOK.with(|b| {
            let mut b=b.borrow_mut();
            let c=b.chats.get_mut(id).unwrap();
            c.next_n=c.next_n.max(after+1);
        });
    }
    BOOK.with(|b| b.borrow_mut().chats.get_mut(id).unwrap().turn_id=Some(turn_id.clone()));
    append(id, json!({"sessionUpdate": "user_message", "text": text,"messageId":message_id,"turnId":turn_id}));
    delivered(id,json!({"messageId":message_id,"text":text,"turnId":turn_id}));
    BOOK.with(|b| {
        if let Some(c) = b.borrow_mut().chats.get_mut(id) {
            if !c.titled {
                c.titled = true;
                let line = text.lines().next().unwrap_or_default().trim();
                c.title = crate::llm::truncate(line, 60);
            }
        }
    });
    set_state(id, "running");
    crate::portable::clear_cancel();

    // The serving opens on the first prompt and lives with the conversation.
    let taken = BOOK.with(|b| {
        let mut b = b.borrow_mut();
        let c = b.chats.get_mut(id)?;
        Some((c.serving.take(), std::mem::take(&mut c.history), c.reader))
    });
    let (serving, mut history, reader) = taken.unwrap_or((None, Vec::new(), false));
    let (server, tools, instructions) = match serving {
        Some(s) => s,
        None => match open_serving(reader) {
            Ok(s) => s,
            Err(e) => {
                BOOK.with(|b| {
                    if let Some(c) = b.borrow_mut().chats.get_mut(id) {
                        c.history = history;
                    }
                });
                append(
                    id,
                    json!({"sessionUpdate": "turn_end", "stop": "error",
                           "error": format!("the chat serving failed to start: {}", e)}),
                );
                set_state(id, "failed");
                return (202, json!({"ok": true}));
            }
        },
    };
    let instruction_hash = crate::model::hash_hex(&instructions);
    let changed = BOOK.with(|b| {
        let mut b = b.borrow_mut();
        let c = b.chats.get_mut(id).unwrap();
        let changed = c.instruction_hash != instruction_hash;
        c.instruction_hash = instruction_hash;
        changed
    });
    if (history.is_empty() || changed) && !instructions.is_empty() {
        push_user(&mut history, instructions.clone());
    }
    if crate::vfs::is_scoped() && !history.is_empty() {
        push_user(&mut history, include_str!("../../docs/compiler/goals/prompts/chat-context-reset.md").into());
    }
    push_user(&mut history, text);

    let generations = || {
        crate::document_sets::Workspace::load(&crate::portable::root())
            .map(|w| {
                w.sets
                    .iter()
                    .map(|s| {
                        (
                            s.id.clone(),
                            crate::store::read_generation(&w.root.join(".jazyk").join(&s.id)),
                        )
                    })
                    .collect::<std::collections::BTreeMap<_, _>>()
            })
            .unwrap_or_default()
    };
    let mut generation = generations();
    let mut dispatch = |name: &str, args: &Value| -> Result<String, String> {
        match server.handle("tools/call", &json!({"name": name, "arguments": args})) {
            Ok(v) => {
                let text = v["content"][0]["text"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| v.to_string());
                if v["isError"].as_bool().unwrap_or(false) {
                    Err(text)
                } else {
                    Ok(text)
                }
            }
            Err((_, m)) => Err(m),
        }
    };
    // The loop's events as the ACP session updates a native chat streams.
    let chat_id = id.to_string();
    let mut emit = |ev: AgentEvent| match ev {
        AgentEvent::Thought(s) => append(
            &chat_id,
            json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": s}}),
        ),
        AgentEvent::Message(s) => append(
            &chat_id,
            json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": s}}),
        ),
        AgentEvent::ToolCallStart { id, name, args } => append(
            &chat_id,
            json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": name,
                   "status": "in_progress", "rawInput": args}),
        ),
        AgentEvent::ToolCallEnd { id, result, ok } => {
            append(
                &chat_id,
                json!({"sessionUpdate": "tool_call_update", "toolCallId": id,
                       "status": if ok { "completed" } else { "failed" }, "rawOutput": result}),
            );
            // A dual write committed: the graph views refresh on it, mid-turn.
            let now = generations();
            for (set_id, value) in &now {
                if generation.get(set_id) != Some(value) {
                    crate::portable::emit_event(
                        "store.generation",
                        json!({"setId":set_id,"generation":value}),
                    );
                }
            }
            generation = now;
            crate::portable::emit_event("sets.changed", json!({}));
        }
        AgentEvent::Input(value) => {
            let display = value.get("displayText").unwrap_or(&value["text"]);
            append(&chat_id,json!({"sessionUpdate":"user_message","text":display,"messageId":value["messageId"],"turnId":value["turnId"]}));
            delivered(&chat_id,value);
        }
        AgentEvent::Timing(value) => crate::portable::emit_event("chat.timing", json!({"sessionId":chat_id,"timing":value})),
        AgentEvent::Usage { .. } => {}
    };
    let cancelled = || crate::portable::cancel_requested();
    let mut input_source = || {
        let mut seen = std::collections::BTreeSet::new();
        crate::portable::take_inputs(id).into_iter().filter(|v| {
            v["messageId"].as_str().is_some_and(|mid| seen.insert(mid.to_owned()) && BOOK.with(|b| !b.borrow().chats[id].inputs.contains_key(mid)))
        }).map(|mut value| {
            if value["operation"] == "answer_diagnostic" {
                let result = server.handle("tools/call", &json!({"name":"answer_diagnostic","arguments":value["arguments"]}));
                let (ok, reply) = match result {
                    Ok(v) => (!v["isError"].as_bool().unwrap_or(false),v),
                    Err((code,message)) => (false,json!({"error":message,"code":code})),
                };
                crate::portable::emit_event("chat.answer",json!({"sessionId":id,"messageId":value["messageId"],"status":if ok {"recorded"}else{"failed"},"result":reply}));
                value["displayText"] = value["text"].clone();
                if !ok { value["state"]=json!("failed"); value["reason"]=json!("The diagnostic answer was rejected. Inspect the recorded tool result before retrying."); }
                value["text"] = json!(include_str!("../../docs/compiler/goals/prompts/chat-answer-input.md").replace("{RESULT}",&reply.to_string()));
            }
            value
        }).collect::<Vec<_>>()
    };
    let mut close_inputs = || crate::portable::close_inputs(id);
    let stop = agent_loop::run_loop(LoopArgs {
        llm: &crate::portable::llm(),
        history: &mut history,
        tools: &tools,
        dispatch: &mut dispatch,
        emit: &mut emit,
        cancelled: &cancelled,
        max_rounds: crate::limits::AGENT_MAX_ROUNDS,
        label: format!("chat {}", id),
        conversation: true,
        inputs: Some(&mut input_source),
        input_close: Some(&mut close_inputs),
    });

    let (update, state) = match stop {
        Stop::EndTurn => (
            json!({"sessionUpdate": "turn_end", "stop": "end_turn"}),
            "idle",
        ),
        Stop::MaxRounds => (
            json!({"sessionUpdate": "turn_end", "stop": "max_turn_requests"}),
            "idle",
        ),
        Stop::Cancelled => (
            json!({"sessionUpdate": "turn_end", "stop": "cancelled"}),
            "idle",
        ),
        // The cancel failed the model call in flight; that error is the cancel.
        Stop::Error(_) if crate::portable::cancel_requested() => (
            json!({"sessionUpdate": "turn_end", "stop": "cancelled"}),
            "idle",
        ),
        Stop::Error(e) => (
            json!({"sessionUpdate": "turn_end", "stop": "error", "error": explain(&e)}),
            "failed",
        ),
    };
    #[cfg(not(target_arch = "wasm32"))]
    let (update, state) = if let Some(reason) = crate::hosted::pause_reason() {
        (json!({"sessionUpdate":"turn_end","stop":"paused","error":reason}), "paused")
    } else {
        (update, state)
    };
    BOOK.with(|b| {
        if let Some(c) = b.borrow_mut().chats.get_mut(id) {
            c.history = history;
            c.serving = Some((server, tools, instructions));
        }
    });
    let stop_reason = update["stop"].clone();
    append(id, update);
    set_state(id, state);
    (202, json!({"ok": true,"messageId":message_id,"turnId":turn_id,"stop":stop_reason,"state":state}))
}

// Record delivery only after the loop inserted the human text into history.
fn delivered(id: &str, mut input: Value) {
    if input["state"] != "failed" { input["state"] = json!("delivered"); }
    if let Some(display)=input.get("displayText").cloned() { input["text"]=display; }
    input["sessionId"] = json!(id);
    BOOK.with(|b| {
        let mut b = b.borrow_mut();
        if let Some(c) = b.chats.get_mut(id) {
            if input["turnId"].is_null() { input["turnId"] = json!(c.turn_id); }
            if let Some(mid) = input["messageId"].as_str() { c.inputs.insert(mid.to_owned(),input.clone()); }
        }
    });
    crate::portable::emit_event("chat.input",input);
}

pub fn export_history() -> Value {
    BOOK.with(|b| json!({"version":1,"book": &*b.borrow()}))
}
pub fn import_history(body: &Value) -> Result<Value,String> {
    if body["version"] != 1 { return Err("unsupported conversation history version".into()); }
    if BOOK.with(|b| b.borrow().chats.values().any(|c|c.state=="running")) {
        return Err("cannot replace history while a turn is running".into());
    }
    let mut book: Book = serde_json::from_value(body["book"].clone()).map_err(|e|format!("invalid conversation history: {e}"))?;
    for (id,c) in &mut book.chats {
        if id != &c.id { return Err("conversation identity does not match its history key".into()); }
        if c.state=="running" {
            c.state="interrupted".into();
            c.updates.push((c.next_n,json!({"sessionUpdate":"turn_end","stop":"interrupted","error":"The tab closed before this turn finished. Review saved changes before retrying."})));
            c.next_n+=1;
        }
    }
    BOOK.with(|b| *b.borrow_mut()=book);
    publish_sessions();
    Ok(json!({"ok":true}))
}

// A hosted request owns its conversation book. Only serializable history and
// updates survive; the MCP loaded set is reopened explicitly on the next turn.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct Scope(Book);
#[cfg(not(target_arch = "wasm32"))]
impl Scope {
    pub(crate) fn enter() -> Result<Self, String> {
        let path = "/project/.jazyk/conversations.json";
        let book = match crate::vfs::read_to_string(path) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| format!("invalid conversations: {}", e))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Book::default(),
            Err(e) => return Err(e.to_string()),
        };
        Ok(Self(BOOK.with(|b| b.replace(book))))
    }
    pub(crate) fn save(&self) -> Result<(), String> {
        let bytes = BOOK
            .with(|b| serde_json::to_vec(&*b.borrow()))
            .map_err(|e| e.to_string())?;
        crate::vfs::write("/project/.jazyk/conversations.json", bytes).map_err(|e| e.to_string())
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for Scope {
    fn drop(&mut self) {
        BOOK.with(|b| b.replace(std::mem::take(&mut self.0)));
    }
}
