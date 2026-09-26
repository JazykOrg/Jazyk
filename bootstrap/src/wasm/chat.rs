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
    serving: Option<(McpServer, Vec<GenericTool>, String)>,
}

#[derive(Default)]
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
                       "pending": []})
            })
            .collect()
    })
}

fn publish_sessions() {
    super::emit_event("chat.sessions", json!({"sessions": snapshot()}));
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
        super::emit_event("chat.update", json!({"sessionId": id, "n": n, "update": update}));
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

pub fn create() -> Value {
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
        Some(json!({"id": c.id, "title": c.title, "state": c.state, "pending": [],
                    "updates": updates}))
    })
}

// The chat serving for one conversation, as a native chat session gets it: `chat`
// mode, ephemeral, no build token. Answers the serving, its tools in the shape the
// loop hands the model, and its instructions.
fn open_serving() -> Result<(McpServer, Vec<GenericTool>, String), String> {
    let project = crate::project::Project::load(&super::root());
    let out = project.out.clone();
    let server = McpServer::with_bridge(
        project,
        out,
        vec!["chat".to_string()],
        false,
        crate::mcp::BridgeFlags {
            ephemeral: true,
            ..Default::default()
        },
    );
    let init = server
        .handle(
            "initialize",
            &json!({"protocolVersion": "2024-11-05", "clientInfo": {"name": "jazyk-wasm-chat"}}),
        )
        .map_err(|e| e.1)?;
    let instructions = init["instructions"].as_str().unwrap_or_default().to_string();
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
    let too_big = ["context", "too long", "too large", "exceed", "quota", "maximum"]
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
        return (400, json!({"error": "text is required: the prompt to send"}));
    }
    if state == "running" {
        return (
            409,
            json!({"error": "a turn is already running; cancel it or wait for it to end"}),
        );
    }

    append(id, json!({"sessionUpdate": "user_message", "text": text}));
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
    super::llm_bridge::clear_cancel();

    // The serving opens on the first prompt and lives with the conversation.
    let taken = BOOK.with(|b| {
        let mut b = b.borrow_mut();
        let c = b.chats.get_mut(id)?;
        Some((c.serving.take(), std::mem::take(&mut c.history)))
    });
    let (serving, mut history) = taken.unwrap_or((None, Vec::new()));
    let (server, tools, instructions) = match serving {
        Some(s) => s,
        None => match open_serving() {
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
    // The first turn opens with the serving's instructions: an agent that reads
    // nothing else still knows the project and which tools move the prose.
    if history.is_empty() && !instructions.is_empty() {
        push_user(&mut history, instructions.clone());
    }
    push_user(&mut history, text);

    let out = crate::project::Project::load(&super::root()).out;
    let mut generation = crate::store::read_generation(&out);
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
            let now = crate::store::read_generation(&out);
            if now != generation {
                generation = now;
                super::emit_event("store.generation", json!({"generation": now}));
            }
        }
        AgentEvent::Usage { .. } => {}
    };
    let cancelled = || super::llm_bridge::cancel_requested();
    let stop = agent_loop::run_loop(LoopArgs {
        llm: &super::llm(),
        history: &mut history,
        tools: &tools,
        dispatch: &mut dispatch,
        emit: &mut emit,
        cancelled: &cancelled,
        max_rounds: crate::limits::AGENT_MAX_ROUNDS,
        label: format!("chat {}", id),
        conversation: true,
    });

    let (update, state) = match stop {
        Stop::EndTurn => (json!({"sessionUpdate": "turn_end", "stop": "end_turn"}), "idle"),
        Stop::MaxRounds => (
            json!({"sessionUpdate": "turn_end", "stop": "max_turn_requests"}),
            "idle",
        ),
        Stop::Cancelled => (json!({"sessionUpdate": "turn_end", "stop": "cancelled"}), "idle"),
        // The cancel failed the model call in flight; that error is the cancel.
        Stop::Error(_) if super::llm_bridge::cancel_requested() => {
            (json!({"sessionUpdate": "turn_end", "stop": "cancelled"}), "idle")
        }
        Stop::Error(e) => (
            json!({"sessionUpdate": "turn_end", "stop": "error", "error": explain(&e)}),
            "failed",
        ),
    };
    BOOK.with(|b| {
        if let Some(c) = b.borrow_mut().chats.get_mut(id) {
            c.history = history;
            c.serving = Some((server, tools, instructions));
        }
    });
    append(id, update);
    set_state(id, state);
    (202, json!({"ok": true}))
}
