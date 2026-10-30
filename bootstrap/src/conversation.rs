// The chat pane's backend in a tab. Natively a chat session is an ACP session with an
// agent process and a `jazyk mcp chat` serving injected into it. A tab has nothing to
// spawn, so a conversation is the pieces a goal batch uses in `runner`: the generic
// agent loop, an in-process `chat` serving it calls tools on, and the page's model
// behind `crate::llm`. The worker is inside the turn until it ends, so every update
// is posted to the page as it happens, in the native event stream's shapes.
// See docs/frontends/wasm.md#chat.

use crate::harness::{self, AgentEvent, GenericTool, LoopArgs, Stop};
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
    history_context_version: u32,
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
    static BUILD_CONVERSATION: RefCell<Option<String>> = const { RefCell::new(None) };
}

// A job owns the worker, while its ordinary conversation owns the human timeline.
// This scope never opens a conversational serving or grants a worker new tools.
pub(crate) struct BuildScope(Option<String>);

impl BuildScope {
    pub(crate) fn enter(body: &Value) -> Result<Self, (u16, String)> {
        let owner = match body.get("conversationId") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) if !id.is_empty() => Some(id.clone()),
            _ => return Err((400, "conversationId must name a conversation".into())),
        };
        if let Some(id) = &owner {
            let job = body["jobId"].as_u64().filter(|id| *id > 0)
                .ok_or_else(|| (400, "an owned job requires a positive numeric jobId".into()))?;
            let after = body["afterN"].as_u64();
            if after.is_some_and(|n| n > (1u64 << 53) - 10_000) {
                return Err((400, "conversation event watermark is too large".into()));
            }
            let existing = BOOK.with(|b| {
                let mut book = b.borrow_mut();
                let chat = book.chats.get_mut(id).ok_or_else(|| (404, "unknown conversation".into()))?;
                if chat.state == "running" {
                    return Err((409, "the conversation already has a running turn".into()));
                }
                if let Some(after) = after {
                    chat.next_n = chat.next_n.max(after + 1);
                }
                Ok(chat.updates.iter().any(|(_, u)| u["sessionUpdate"] == "workflow" && u["jobId"] == job))
            })?;
            if !existing {
                append(id, json!({"sessionUpdate":"workflow","jobId":job}));
            }
        }
        Ok(Self(BUILD_CONVERSATION.with(|current| current.replace(owner))))
    }
}

impl Drop for BuildScope {
    fn drop(&mut self) {
        BUILD_CONVERSATION.with(|current| current.replace(self.0.take()));
    }
}

pub(crate) fn build_conversation() -> Option<String> {
    BUILD_CONVERSATION.with(|current| current.borrow().clone())
}

pub(crate) fn build_inputs(id: &str) -> Vec<Value> {
    let mut seen = std::collections::BTreeSet::new();
    crate::portable::take_inputs(id).into_iter().filter(|input| {
        input["messageId"].as_str().is_some_and(|mid| {
            seen.insert(mid.to_owned()) && BOOK.with(|b| {
                b.borrow().chats.get(id).is_some_and(|c| !c.inputs.contains_key(mid))
            })
        })
    }).collect()
}

pub(crate) fn build_input_delivered(id: &str, input: Value) {
    BOOK.with(|b| {
        if let Some(chat) = b.borrow_mut().chats.get_mut(id) {
            crate::harness::push_user_text(&mut chat.history, input["text"].as_str().unwrap_or_default());
        }
    });
    append(id, json!({"sessionUpdate":"user_message","text":input["text"],
        "messageId":input["messageId"],"turnId":input["turnId"]}));
    delivered(id, input);
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
                history_context_version: 2,
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

// These are exact generated bytes from known released harnesses, not patterns
// for editing arbitrary user prose. Recorded human text must prove the suffix.
const LEGACY_CONTEXTS: [&str; 2] = [
    include_str!("../fixtures/legacy-chat-context-v1.txt"),
    include_str!("../fixtures/legacy-chat-context-v2.txt"),
];
const CONTEXT_RESET: &str = include_str!("../../docs/compiler/goals/prompts/chat-context-reset.md");
const CHAT_TURN: &str = include_str!("../../docs/compiler/goals/prompts/chat-turn.md");

fn verified_human_sequence(text: &str, humans: &[String]) -> bool {
    for end in 0..humans.len() {
        if humans[end].is_empty() || !text.ends_with(&humans[end]) {
            continue;
        }
        let mut candidate = String::new();
        for start in (0..=end).rev() {
            candidate = if candidate.is_empty() {
                humans[start].clone()
            } else {
                format!("{}\n\n{candidate}", humans[start])
            };
            if candidate == text {
                return true;
            }
            if candidate.len() > text.len() {
                break;
            }
        }
    }
    false
}

fn refresh_context(history: &mut [Value], humans: &[String], instructions: &str) -> usize {
    let mut removed = 0;
    for message in history {
        if message["role"] != "user" {
            continue;
        }
        let Some(content) = message["content"].as_str() else {
            continue;
        };
        // A user who actually submitted a full quoted contract keeps it intact.
        if verified_human_sequence(content, humans) {
            continue;
        }
        let candidate = LEGACY_CONTEXTS
            .iter()
            .copied()
            .chain(std::iter::once(instructions))
            .find_map(|prefix| {
                if prefix.is_empty() {
                    return None;
                }
                let suffix = content.strip_prefix(prefix)?.strip_prefix("\n\n")?;
                let suffix = suffix
                    .strip_prefix(CONTEXT_RESET)
                    .and_then(|rest| rest.strip_prefix("\n\n"))
                    .unwrap_or(suffix);
                verified_human_sequence(suffix, humans).then(|| suffix.to_string())
            });
        if let Some(human) = candidate {
            removed += content.len() - human.len();
            message["content"] = json!(human);
        }
    }
    removed
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
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn prompt(id: &str, body: &Value) -> (u16, Value) {
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
    let message_id = body["messageId"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| BOOK.with(|b| format!("{}-{}", id, b.borrow().chats[id].next_n)));
    let turn_id = body["turnId"].as_str().unwrap_or(&message_id).to_owned();
    let recovery_key = format!("chat:{id}:{turn_id}");
    let saved_turn = match crate::session_recovery::parent(&recovery_key) {
        Ok(value) => value,
        Err(error) => return (202, json!({"ok":false,"stop":"suspended","state":"running","error":error})),
    };
    if let Some(saved) = &saved_turn {
        if saved["messageId"] != message_id || saved["textHash"] != crate::model::hash_hex(&text) {
            return (409, json!({"error":"checkpoint-incompatible: accepted chat input changed"}));
        }
        if let Some(result) = saved.get("result") { return (202, result.clone()); }
    }
    let resuming = saved_turn.is_some();
    let prior = BOOK.with(|b| b.borrow().chats[id].inputs.get(&message_id).cloned());
    if let Some(prior) = prior.filter(|_| !resuming) {
        return if prior["text"].as_str() == Some(text.as_str()) {
            (
                202,
                json!({"ok":true,"messageId":message_id,"state":prior["state"],"turnId":prior["turnId"]}),
            )
        } else {
            (
                409,
                json!({"error":"messageId already identifies different text"}),
            )
        };
    }
    if state == "running" && !resuming {
        return (
            409,
            json!({"error": "a turn is already running; cancel it or wait for it to end"}),
        );
    }

    if let Some(after) = body["afterN"].as_u64() {
        if after > (1u64 << 53) - 10_000 {
            return (
                400,
                json!({"error":"conversation event watermark is too large"}),
            );
        }
        BOOK.with(|b| {
            let mut b = b.borrow_mut();
            let c = b.chats.get_mut(id).unwrap();
            c.next_n = c.next_n.max(after + 1);
        });
    }
    BOOK.with(|b| b.borrow_mut().chats.get_mut(id).unwrap().turn_id = Some(turn_id.clone()));
    if !resuming {
        append(id, json!({"sessionUpdate":"user_message","text":text,"messageId":message_id,"turnId":turn_id}));
    }
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
                delivered(
                    id,
                    json!({"messageId":message_id,"text":text,"turnId":turn_id,"state":"failed","reason":format!("The chat serving failed to start: {e}")}),
                );
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
    let mut recovery = match crate::session_recovery::McpRecovery::new(&server, recovery_key.clone()) {
        Ok(recovery) => recovery,
        Err(error) => return (202, json!({"ok":false,"stop":"suspended","state":"running","error":error})),
    };
    let model_llm = crate::portable::llm();
    let mut model = harness::provider::DefaultModelHost::new(&model_llm);
    let mut mcp_tools = crate::session_recovery::McpTools(&server);
    let instruction_hash = crate::model::hash_hex(&instructions);
    let humans = BOOK.with(|b| {
        b.borrow().chats[id]
            .updates
            .iter()
            .filter_map(|(_, update)| {
                (update["sessionUpdate"] == "user_message")
                    .then(|| update["text"].as_str().map(str::to_owned))
                    .flatten()
            })
            .collect::<Vec<_>>()
    });
    refresh_context(&mut history, &humans, &instructions);
    BOOK.with(|b| {
        let mut b = b.borrow_mut();
        let c = b.chats.get_mut(id).unwrap();
        c.instruction_hash = instruction_hash;
        c.history_context_version = 2;
    });
    // Current host instructions are request context, not another human turn.
    let context = format!(
        "{instructions}\n\n{CHAT_TURN}{}",
        if crate::vfs::is_scoped() {
            format!("\n\n{CONTEXT_RESET}")
        } else {
            String::new()
        }
    );
    if !resuming {
        push_user(&mut history, text.clone());
        delivered(id, json!({"messageId":message_id,"text":text,"turnId":turn_id}));
    }
    crate::session_recovery::set_parent(&recovery_key, json!({"messageId":message_id,"textHash":crate::model::hash_hex(&text)}));

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
            append(
                &chat_id,
                json!({"sessionUpdate":"user_message","text":display,"messageId":value["messageId"],"turnId":value["turnId"]}),
            );
            delivered(&chat_id, value);
        }
        AgentEvent::Timing(value) => {
            crate::portable::emit_event("chat.timing", json!({"sessionId":chat_id,"timing":value}))
        }
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
    let stop = harness::run_with_hosts(
        LoopArgs {
            llm: &model_llm,
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
        },
        &context, &mut model, &mut mcp_tools, &mut recovery,
    ).await;
    drop(recovery);
    drop(mcp_tools);

    BOOK.with(|b| {
        if let Some(c) = b.borrow_mut().chats.get_mut(id) {
            c.history = history;
            c.serving = Some((server, tools, instructions));
        }
    });
    let interruption = match &stop {
        Stop::Suspended => Some("suspended".to_owned()),
        Stop::Error(error) if crate::session_recovery::is_interruption(error) => Some(error.clone()),
        _ => None,
    };
    if let Some(error) = interruption {
        set_state(id, "running");
        return (202, json!({"ok":true,"messageId":message_id,"turnId":turn_id,"stop":"suspended","state":"running","error":error}));
    }
    let (update, state) = match stop {
        Stop::Suspended => unreachable!("suspension returned above"),
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
        (
            json!({"sessionUpdate":"turn_end","stop":"paused","error":reason}),
            "paused",
        )
    } else {
        (update, state)
    };
    let stop_reason = update["stop"].clone();
    append(id, update);
    set_state(id, state);
    let result = json!({"ok":true,"messageId":message_id,"turnId":turn_id,"stop":stop_reason,"state":state});
    crate::session_recovery::set_parent(&recovery_key, json!({"messageId":message_id,"textHash":crate::model::hash_hex(&text),"result":result}));
    if let Err(error) = crate::session_recovery::checkpoint_parent().await {
        return (202, json!({"ok":false,"stop":"suspended","state":"running","error":format!("checkpoint-failed: {error}")}));
    }
    (202, result)
}

// Record delivery only after the loop inserted the human text into history.
fn delivered(id: &str, mut input: Value) {
    if input["state"] != "failed" {
        input["state"] = json!("delivered");
    }
    if let Some(display) = input.get("displayText").cloned() {
        input["text"] = display;
    }
    input["sessionId"] = json!(id);
    if let Some(message_id) = input["messageId"].as_str() {
        crate::session_recovery::input_delivered(message_id);
    }
    BOOK.with(|b| {
        let mut b = b.borrow_mut();
        if let Some(c) = b.chats.get_mut(id) {
            if input["turnId"].is_null() {
                input["turnId"] = json!(c.turn_id);
            }
            if let Some(mid) = input["messageId"].as_str() {
                c.inputs.insert(mid.to_owned(), input.clone());
            }
        }
    });
    crate::portable::emit_event("chat.input", input);
}

pub fn export_history() -> Value {
    BOOK.with(|b| json!({"version":1,"book": &*b.borrow()}))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn stop_recovered() {
    let running = BOOK.with(|b| b.borrow().chats.values()
        .filter(|chat| chat.state == "running").map(|chat| chat.id.clone()).collect::<Vec<_>>());
    for id in running {
        append(&id, json!({"sessionUpdate":"turn_end","stop":"cancelled"}));
        set_state(&id, "idle");
    }
}
pub fn import_history(body: &Value) -> Result<Value, String> {
    if body["version"] != 1 {
        return Err("unsupported conversation history version".into());
    }
    if BOOK.with(|b| b.borrow().chats.values().any(|c| c.state == "running")) {
        return Err("cannot replace history while a turn is running".into());
    }
    let mut book: Book = serde_json::from_value(body["book"].clone())
        .map_err(|e| format!("invalid conversation history: {e}"))?;
    for (id, c) in &mut book.chats {
        if id != &c.id {
            return Err("conversation identity does not match its history key".into());
        }
        if c.state == "running" {
            c.state = "interrupted".into();
            c.updates.push((c.next_n,json!({"sessionUpdate":"turn_end","stop":"interrupted","error":"The tab closed before this turn finished. Review saved changes before retrying."})));
            c.next_n += 1;
        }
    }
    BOOK.with(|b| *b.borrow_mut() = book);
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

#[cfg(test)]
mod context_tests {
    use super::*;

    #[test]
    fn exact_legacy_context_is_removed_only_with_recorded_human_suffix() {
        for prefix in LEGACY_CONTEXTS {
            let human = "Update the relevant section in hosting.md.";
            let mut history = vec![
                json!({"role":"user","content":format!("{prefix}\n\n{human}"),"custom":"preserved"}),
            ];
            assert_eq!(
                refresh_context(&mut history, &[human.into()], "current contract"),
                prefix.len() + 2
            );
            assert_eq!(
                history[0],
                json!({"role":"user","content":human,"custom":"preserved"})
            );
            assert_eq!(
                refresh_context(&mut history, &[human.into()], "current contract"),
                0
            );
        }
    }

    #[test]
    fn merged_human_messages_and_context_reset_survive_migration() {
        let humans = vec![
            "Update hosting.md.".to_string(),
            "Keep its existing title.".to_string(),
        ];
        let content = format!(
            "{}\n\n{}\n\n{}",
            LEGACY_CONTEXTS[1],
            CONTEXT_RESET,
            humans.join("\n\n")
        );
        let mut history = vec![json!({"role":"user","content":content})];
        assert!(refresh_context(&mut history, &humans, "current contract") > 0);
        assert_eq!(history[0]["content"], humans.join("\n\n"));
        let serialized = serde_json::to_string(&history).unwrap();
        let mut reloaded = serde_json::from_str::<Vec<Value>>(&serialized).unwrap();
        assert_eq!(
            refresh_context(&mut reloaded, &humans, "current contract"),
            0
        );
        assert_eq!(reloaded, history);
    }

    #[test]
    fn unproven_contracts_and_user_quotations_stay_verbatim() {
        let human = "Update hosting.md.".to_string();
        let full_quote = format!("{}\n\n{human}", LEGACY_CONTEXTS[0]);
        let middle_quote = format!(
            "Explain this quoted contract:\n{}\n\n{human}",
            LEGACY_CONTEXTS[0]
        );
        let unknown = format!("{} changed instructions\n\n{human}", LEGACY_CONTEXTS[0]);
        let repeated = format!(
            "{}\n\n{human}\n\n{}\n\nKeep the title.",
            LEGACY_CONTEXTS[0], LEGACY_CONTEXTS[1]
        );
        for (content, humans) in [
            (full_quote.clone(), vec![full_quote]),
            (middle_quote.clone(), vec![middle_quote]),
            (unknown, vec![human.clone()]),
            (repeated, vec![human.clone(), "Keep the title.".into()]),
            (format!("{}\n\n{human}", LEGACY_CONTEXTS[0]), vec![]),
        ] {
            let mut history = vec![json!({"role":"user","content":content})];
            let before = history.clone();
            assert_eq!(
                refresh_context(&mut history, &humans, "current contract"),
                0
            );
            assert_eq!(history, before);
        }
    }

    #[test]
    fn merged_real_quotations_are_not_mistaken_for_generated_prefixes() {
        let quoted = format!("{}\n\nUpdate hosting.md.", LEGACY_CONTEXTS[0]);
        let followup = "Keep the title.".to_string();
        let humans = vec![
            quoted.clone(),
            followup.clone(),
            "Update hosting.md.".to_string(),
            followup.clone(),
        ];
        let mut history = vec![json!({"role":"user","content":format!("{quoted}\n\n{followup}")})];
        let before = history.clone();
        assert_eq!(
            refresh_context(&mut history, &humans, "current contract"),
            0
        );
        assert_eq!(history, before);
    }

    #[test]
    fn migration_never_rewrites_tool_pairs_or_provider_reasoning() {
        let human = "Update hosting.md.";
        let call = json!({"role":"assistant","content":null,"reasoning_content":"Earlier worker reasoning.","tool_calls":[{"id":"old-call","type":"function","function":{"name":"begin_goals","arguments":"{}"}}]});
        let result = json!({"role":"tool","tool_call_id":"old-call","content":format!("{}\n\n{human}",LEGACY_CONTEXTS[0])});
        let mut history = vec![
            json!({"role":"user","content":format!("{}\n\n{human}",LEGACY_CONTEXTS[0])}),
            call.clone(),
            result.clone(),
        ];
        refresh_context(&mut history, &[human.into()], "current contract");
        assert_eq!(history[1], call);
        assert_eq!(history[2], result);
    }
}
