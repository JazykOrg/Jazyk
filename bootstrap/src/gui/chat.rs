// The chat pane's backend: ACP chat sessions against the configured agent, with
// permission requests forwarded to the browser. Follow sessions need no backend of
// their own: automated jobs already stream their translated turns over `job.trace`,
// and the pane renders those. Mirrors docs/frontends/gui.md#chat.
use super::state::SharedState;
use crate::acp::config::{self, EMBEDDED};
use crate::acp::host::{AcpHost, HostEvent, McpSpec, SessionHandle};
use crate::acp::policy::PermissionPolicy;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// One update over the wire is elided like a job trace row; the ring keeps the full
// payload for replay. Mirrors docs/frontends/gui.md#jobs.
const RING: usize = 5_000;

pub struct ChatSession {
    pub id: String,
    pub title: String,
    // idle | running | failed | closed
    pub state: String,
    handle: Option<SessionHandle>,
    // Numbered updates, full payloads, bounded.
    ring: VecDeque<(u64, Value)>,
    next_n: u64,
    // Forwarded permission requests awaiting the user: {id, request}.
    pending: Vec<Value>,
    inputs: Vec<Value>,
    turn_id: Option<String>,
    stopped: bool,
}

pub struct ChatManager {
    host: Mutex<Option<AcpHost>>,
    sessions: Mutex<BTreeMap<String, Arc<Mutex<ChatSession>>>>,
    seq: AtomicU64,
}

impl Default for ChatManager {
    fn default() -> Self {
        ChatManager {
            host: Mutex::new(None),
            sessions: Mutex::new(BTreeMap::new()),
            seq: AtomicU64::new(0),
        }
    }
}

impl ChatManager {
    pub fn is_running(&self, id: &str) -> bool {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|session| {
                let session = session.lock().unwrap();
                session.state == "running" && !session.stopped
            })
    }

    // The shared agent process behind every chat session, spawned on first use.
    fn host(&self, st: &SharedState) -> Result<(), String> {
        let mut h = self.host.lock().unwrap();
        if h.is_some() {
            return Ok(());
        }
        let proj = st.proj();
        let agent =
            config::resolve_acp(None, &proj.acp, &crate::project::load_global_acp(), |n| {
                std::env::var(n).ok()
            })?;
        let llm = st.llm();
        let extra_env = if agent.name == EMBEDDED {
            let mut v = vec![
                ("JAZYK_LLM_BASE_URL".to_string(), llm.base_url.clone()),
                ("JAZYK_MODEL".to_string(), llm.model.clone()),
            ];
            if !llm.api_key.is_empty() {
                v.push(("JAZYK_API_KEY".to_string(), llm.api_key));
            }
            if let Some(t) = llm.temperature {
                v.push(("JAZYK_TEMPERATURE".to_string(), t.to_string()));
            }
            v
        } else {
            Vec::new()
        };
        *h = Some(AcpHost::start(agent, proj.root.clone(), extra_env)?);
        Ok(())
    }

    fn open_acp_session(&self, st: &SharedState, chat_id: &str) -> Result<SessionHandle, String> {
        self.host(st)?;
        let proj = st.proj();
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "jazyk".to_string());
        // The chat serving: reads, lifecycles, and the dual-write chat tools
        // (docs/frontends/mcp.md#toolsets). Scoped to this session.
        let spec = McpSpec {
            name: "jazyk".to_string(),
            command: exe,
            args: vec![
                "mcp".to_string(),
                "chat".to_string(),
                "--ephemeral".to_string(),
            ],
            env: Vec::new(),
        };
        let h = self.host.lock().unwrap();
        let host = h.as_ref().ok_or("agent host is gone")?;
        let handle = host.new_chat_session(&proj.root, vec![spec], PermissionPolicy::Forward)?;
        let _ = chat_id;
        Ok(handle)
    }

    pub fn snapshot(&self) -> Vec<Value> {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .map(|s| {
                let s = s.lock().unwrap();
                json!({"id": s.id, "title": s.title, "state": s.state, "updates": s.next_n,
                       "pending": s.pending, "inputs":s.inputs,"turnId":s.turn_id})
            })
            .collect()
    }

    // The conversations this project already had, read back from the store on first
    // use so a restarted server shows them. They have no live agent behind them
    // until prompted, which opens a fresh session under the same id.
    // Mirrors docs/frontends/acp.md#session-store.
    // The files are read before the sessions lock is taken, so a slow disk never
    // stalls the handlers that only snapshot the list.
    pub fn restore(&self, out: &std::path::Path) {
        if !self.sessions.lock().unwrap().is_empty() {
            return;
        }
        let mut restored: Vec<(String, ChatSession)> = Vec::new();
        for meta in crate::acp::sessions::list(out).into_iter().take(30) {
            let records = crate::acp::sessions::read(out, &meta.id);
            let mut ring: VecDeque<(u64, Value)> = VecDeque::new();
            for (n, r) in records.iter().enumerate() {
                let update = match r["kind"].as_str().unwrap_or("") {
                    "user" => json!({"sessionUpdate": "user_message", "text": r["text"]}),
                    "update" => r["update"].clone(),
                    _ => continue,
                };
                let cursor = update["cursor"].as_u64().unwrap_or(n as u64);
                ring.push_back((cursor, update));
            }
            let mut inputs: Vec<Value> = Vec::new();
            for (_, update) in &ring {
                if update["sessionUpdate"] == "input" {
                    let mut input = update["input"].clone();
                    if ["queued", "delivering", "accepted"]
                        .iter()
                        .any(|state| input["state"] == *state)
                    {
                        input["state"] = json!("delivery_unknown");
                        input["reason"] =
                            json!("server restarted; inspect committed changes before resuming");
                    }
                    if let Some(previous) = inputs
                        .iter_mut()
                        .find(|i| i["messageId"] == input["messageId"])
                    {
                        *previous = input;
                    } else {
                        inputs.push(input);
                    }
                }
            }
            let restored_state = ring
                .iter()
                .rev()
                .find_map(|(_, update)| match update["sessionUpdate"].as_str() {
                    Some("turn_end") => Some(if update["stop"] == "end_turn" {
                        "idle"
                    } else if update["stop"] == "cancelled" {
                        "cancelled"
                    } else {
                        "failed"
                    }),
                    Some("state") => Some(match update["state"].as_str() {
                        Some("failed") => "failed",
                        Some("cancelled") => "cancelled",
                        Some("idle") => "idle",
                        _ => "interrupted",
                    }),
                    Some("user_message" | "input") => Some("interrupted"),
                    _ => None,
                })
                .unwrap_or("idle");
            let next_n = ring.iter().map(|(n, _)| n + 1).max().unwrap_or(0);
            // A number a later append cannot collide with, whatever was trimmed.
            let seq = self.seq.load(Ordering::Relaxed);
            let n = meta
                .id
                .strip_prefix("chat-")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if n > seq {
                self.seq.store(n, Ordering::Relaxed);
            }
            restored.push((
                meta.id.clone(),
                ChatSession {
                    id: meta.id,
                    title: meta.title,
                    state: restored_state.into(),
                    handle: None,
                    ring,
                    next_n,
                    pending: Vec::new(),
                    inputs,
                    turn_id: None,
                    stopped: false,
                },
            ));
        }
        let mut sessions = self.sessions.lock().unwrap();
        // Another request restored meanwhile: its sessions may be live already.
        if !sessions.is_empty() {
            return;
        }
        for (id, session) in restored {
            sessions.insert(id, Arc::new(Mutex::new(session)));
        }
    }
}

// Restore off the runtime: the first call reads every stored conversation.
async fn restore(st: &SharedState) {
    let st = st.clone();
    tokio::task::spawn_blocking(move || st.chat.restore(&st.out))
        .await
        .expect("chat restore");
}

fn append(st: &SharedState, sess: &Arc<Mutex<ChatSession>>, mut update: Value) {
    // Serialize transcript writes with the in-memory cursor. Admissions use the
    // same session lock and a checked, flushed write before HTTP acceptance.
    let (id, n, elided) = {
        let mut s = sess.lock().unwrap();
        let n = s.next_n;
        update["cursor"] = json!(n);
        if let Err(error) =
            crate::acp::sessions::record_update_unflushed_checked(&st.out, &s.id, &update)
        {
            s.stopped = true;
            s.state = "failed".into();
            if let Some(handle) = &s.handle {
                handle.cancel();
            }
            st.events.emit(
                "chat.persistence_error",
                json!({"sessionId":s.id,"error":error,"state":"failed"}),
            );
        }
        s.next_n += 1;
        s.ring.push_back((n, update.clone()));
        while s.ring.len() > RING {
            s.ring.pop_front();
        }
        (s.id.clone(), n, super::jobs::elide(&update))
    };
    st.events
        .emit("chat.update", json!({"sessionId":id,"n":n,"update":elided}));
}

fn set_state(st: &SharedState, sess: &Arc<Mutex<ChatSession>>, state: &str) {
    {
        let mut s = sess.lock().unwrap();
        if s.stopped && s.state == "failed" {
            return;
        }
        s.state = state.to_string();
    }
    append(st, sess, json!({"sessionUpdate":"state","state":state}));
    st.events
        .emit("chat.sessions", json!({"sessions": st.chat.snapshot()}));
}

// ---- handlers ----

pub async fn post_session(State(st): State<SharedState>) -> Response {
    restore(&st).await;
    let n = st.chat.seq.fetch_add(1, Ordering::Relaxed) + 1;
    let id = format!("chat-{}", n);
    let session = ChatSession {
        id: id.clone(),
        title: format!("Chat {}", n),
        state: "idle".into(),
        handle: None,
        ring: VecDeque::new(),
        next_n: 0,
        pending: Vec::new(),
        inputs: Vec::new(),
        turn_id: None,
        stopped: false,
    };
    crate::acp::sessions::open(&st.out, &id, &st.proj().root, "gui");
    st.chat
        .sessions
        .lock()
        .unwrap()
        .insert(id.clone(), Arc::new(Mutex::new(session)));
    st.events
        .emit("chat.sessions", json!({"sessions": st.chat.snapshot()}));
    Json(json!({"id": id})).into_response()
}

pub async fn list_sessions(State(st): State<SharedState>) -> Response {
    restore(&st).await;
    // The same catalog the IDE proxy advertises; the GUI is a jazyk client like any
    // other. Mirrors docs/frontends/acp.md#slash-commands.
    let commands: Vec<Value> = crate::acp::commands::available(true)
        .map(|c| json!({"name": format!("/{}", c.name), "description": c.description, "hint": c.hint}))
        .collect();
    Json(json!({"sessions": st.chat.snapshot(), "commands": commands})).into_response()
}

pub async fn get_session(State(st): State<SharedState>, Path(id): Path<String>) -> Response {
    // A reload can ask for one conversation by id without listing first.
    restore(&st).await;
    let sess = st.chat.sessions.lock().unwrap().get(&id).cloned();
    match sess {
        Some(s) => {
            let s = s.lock().unwrap();
            let updates: Vec<Value> = s
                .ring
                .iter()
                .map(|(n, u)| json!({"n": n, "update": super::jobs::elide(u)}))
                .collect();
            Json(json!({"id": s.id, "title": s.title, "state": s.state,
                        "pending": s.pending, "updates": updates,"inputs":s.inputs,"turnId":s.turn_id}))
            .into_response()
        }
        None => super::api::err(StatusCode::NOT_FOUND, format!("no chat session {}", id)),
    }
}

// The prompt runs on a blocking thread; progress streams as chat.* events. Slash
// commands are matched before the agent sees the text: the real job runs and the
// pane follows it. Mirrors docs/frontends/gui.md#chat.
pub async fn post_prompt(
    State(st): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    // Prompting a conversation restored from the store is how it resumes.
    restore(&st).await;
    let Some(sess) = st.chat.sessions.lock().unwrap().get(&id).cloned() else {
        return super::api::err(StatusCode::NOT_FOUND, format!("no chat session {}", id));
    };
    let text = body["text"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        return super::api::err(
            StatusCode::BAD_REQUEST,
            "text is required: the prompt to send",
        );
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
        let mut session = sess.lock().unwrap();
        if let Some(existing) = session.inputs.iter().find(|i| i["messageId"] == message_id) {
            return if existing["text"] == text {
                (StatusCode::ACCEPTED, Json(existing.clone())).into_response()
            } else {
                super::api::err(StatusCode::CONFLICT, "messageId has different text")
            };
        }
        if session.state == "running" {
            return super::api::err(
                StatusCode::CONFLICT,
                "use /inputs for a running conversation",
            );
        }
        let turn_id = format!("{}:{}", id, message_id);
        let input = json!({"messageId":message_id,"text":text,"sequence":session.inputs.len()+1,"state":"queued","turnId":turn_id,"deliveryMode":"steer"});
        if let Err(error) = crate::acp::sessions::record_update_checked(
            &st.out,
            &id,
            &json!({"sessionUpdate":"input","input":input}),
        ) {
            return super::api::err(StatusCode::INTERNAL_SERVER_ERROR, error);
        }
        session.state = "running".into();
        session.stopped = false;
        session.turn_id = Some(turn_id);
        session.inputs.push(input.clone());
        input
    };
    append(&st, &sess, json!({"sessionUpdate":"input","input":input}));
    // The user's side of the transcript.
    append(
        &st,
        &sess,
        json!({"sessionUpdate": "user_message", "text": text,"messageId":message_id}),
    );

    // Slash commands arrive as prompt text, per the protocol; jazyk matches the
    // prefix and runs the real work instead of prompting the agent.
    // Mirrors docs/frontends/acp.md#slash-commands.
    if let Some(reply) = run_command(&st, &text).await {
        append(
            &st,
            &sess,
            json!({"sessionUpdate": "agent_message", "text": reply}),
        );
        set_state(&st, &sess, "idle");
        return (StatusCode::ACCEPTED, Json(json!({"ok": true}))).into_response();
    }

    set_state(&st, &sess, "running");
    let st2 = st.clone();
    let sess2 = sess.clone();
    tokio::task::spawn_blocking(move || {
        if sess2.lock().unwrap().stopped {
            set_state(&st2, &sess2, "cancelled");
            return;
        }
        // The ACP session opens on the first prompt and lives with the chat.
        let (handle, reopened) = {
            let existing = sess2.lock().unwrap().handle.clone();
            match existing {
                Some(h) => (h, false),
                None => match st2.chat.open_acp_session(&st2, &id) {
                    Ok(h) => {
                        sess2.lock().unwrap().handle = Some(h.clone());
                        (h, true)
                    }
                    Err(e) => {
                        append(
                            &st2,
                            &sess2,
                            json!({"sessionUpdate": "agent_message", "text": format!("(agent failed to start: {})", e)}),
                        );
                        set_state(&st2, &sess2, "failed");
                        return;
                    }
                },
            }
        };
        let mut turn_text = if reopened {
            let session = sess2.lock().unwrap();
            let mut context = Vec::new();
            for (_, update) in &session.ring {
                match update["sessionUpdate"].as_str() {
                    Some("user_message") if update["messageId"] != message_id => context.push(
                        format!("User: {}", update["text"].as_str().unwrap_or_default()),
                    ),
                    Some("agent_message") => context.push(format!(
                        "Agent: {}",
                        update["text"].as_str().unwrap_or_default()
                    )),
                    Some("agent_message_chunk") => context.push(
                        update["content"]["text"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                    ),
                    _ => {}
                }
            }
            let context = context.join("\n");
            let tail = context
                .chars()
                .rev()
                .take(24_000)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>();
            if tail.is_empty() {
                text
            } else {
                format!("Previous conversation context (already completed turns, not fresh authorization):\n{tail}\n\nCurrent user message:\n{text}")
            }
        } else {
            text
        };
        let mut current_input_id = message_id;
        loop {
            let cb_st = st2.clone();
            let cb_sess = sess2.clone();
            let initial_id = current_input_id.clone();
            let outcome = handle.prompt_cancellable(
            &turn_text,
            Arc::new(move |ev: &HostEvent| match ev {
                HostEvent::Update(u) => {
                    update_input(&cb_st,&cb_sess,&json!(initial_id),"delivered",None);
                    if let Ok(v) = serde_json::to_value(u) {
                        if let Some(delivered) = v["_meta"]["jazyk"]["input"].as_object() {
                            let message_id = delivered.get("messageId").cloned().unwrap_or_default();
                            update_input(&cb_st, &cb_sess, &message_id, "delivered", None);
                        } else { append(&cb_st, &cb_sess, v); }
                    }
                }
                HostEvent::Permission { id, request } => {
                    let ask = json!({"id": id, "request": serde_json::to_value(request).unwrap_or_default()});
                    cb_sess.lock().unwrap().pending.push(ask.clone());
                    let sid = cb_sess.lock().unwrap().id.clone();
                    cb_st.events.emit("chat.permission", json!({"sessionId": sid, "ask": ask}));
                }
            }),
            &|| sess2.lock().unwrap().stopped,
        );
            {
                let mut s = sess2.lock().unwrap();
                s.pending.clear();
            }
            update_input(
                &st2,
                &sess2,
                &json!(current_input_id),
                if outcome.is_ok() {
                    "delivered"
                } else {
                    "failed"
                },
                None,
            );
            let terminal = match &outcome {
                Ok(o) => json!({"sessionUpdate":"turn_end","stop":o.stop,"idled":o.idled}),
                Err(e) => json!({"sessionUpdate":"turn_end","stop":"error","error":e}),
            };
            append(&st2, &sess2, terminal);
            // The same lock serializes late admission and closing this turn.
            let next = loop {
                let mut session = sess2.lock().unwrap();
                let failed = outcome.as_ref().map_or(true, |o| o.stop != "end_turn");
                if session.stopped || failed {
                    session.state = if session.stopped && session.state == "failed" {
                        "failed"
                    } else if session.stopped {
                        "cancelled"
                    } else if failed {
                        "failed"
                    } else {
                        "idle"
                    }
                    .into();
                    break None;
                }
                if session.inputs.iter().any(|i| i["admitting"] == true) {
                    drop(session);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                if let Some(index) = session
                    .inputs
                    .iter()
                    .position(|i| i["state"] == "queued" && i["deliveryMode"] == "next_turn")
                {
                    let turn_id = format!(
                        "{}:{}",
                        session.id,
                        session.inputs[index]["messageId"]
                            .as_str()
                            .unwrap_or_default()
                    );
                    session.turn_id = Some(turn_id.clone());
                    session.inputs[index]["turnId"] = json!(turn_id);
                    break Some(session.inputs[index].clone());
                }
                session.state = "idle".into();
                session.turn_id = None;
                break None;
            };
            st2.events
                .emit("chat.sessions", json!({"sessions":st2.chat.snapshot()}));
            match next {
                Some(input) => {
                    append(&st2, &sess2, json!({"sessionUpdate":"input","input":input}));
                    current_input_id = input["messageId"].as_str().unwrap_or_default().to_string();
                    turn_text = input["text"].as_str().unwrap_or_default().to_string();
                }
                None => break,
            }
        }
    });
    (StatusCode::ACCEPTED, Json(input)).into_response()
}

fn update_input(
    st: &SharedState,
    sess: &Arc<Mutex<ChatSession>>,
    message_id: &Value,
    state: &str,
    reason: Option<&str>,
) {
    let updated =
        {
            let mut session = sess.lock().unwrap();
            if session.inputs.iter().any(|i| {
                i["messageId"] == *message_id && i["state"] == state && state == "delivered"
            }) {
                return;
            }
            session
                .inputs
                .iter_mut()
                .find(|i| i["messageId"] == *message_id)
                .map(|input| {
                    input["admitting"] = json!(false);
                    if input["state"] != "cancelled"
                        && !(input["state"] == "delivered" && state == "queued")
                    {
                        input["state"] = json!(state);
                    }
                    if let Some(reason) = reason {
                        input["reason"] = json!(reason);
                    }
                    input.clone()
                })
        };
    if let Some(input) = updated {
        append(st, sess, json!({"sessionUpdate":"input","input":input}));
        st.events.emit(
            "chat.input",
            json!({"sessionId":sess.lock().unwrap().id,"input":input}),
        );
    }
}

pub async fn post_input(
    State(st): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    restore(&st).await;
    let Some(sess) = st.chat.sessions.lock().unwrap().get(&id).cloned() else {
        return super::api::err(StatusCode::NOT_FOUND, "unknown conversation");
    };
    let text = body["text"].as_str().unwrap_or_default().trim().to_string();
    let message_id = body["messageId"].as_str().unwrap_or_default().to_string();
    if text.is_empty() || message_id.is_empty() {
        return super::api::err(StatusCode::BAD_REQUEST, "messageId and text are required");
    }
    let idle = sess.lock().unwrap().state != "running";
    if idle {
        return post_prompt(State(st), Path(id), Json(body)).await;
    }
    let (input, handle) = {
        let mut session = sess.lock().unwrap();
        if let Some(existing) = session.inputs.iter().find(|i| i["messageId"] == message_id) {
            return if existing["text"] == text {
                (StatusCode::ACCEPTED, Json(existing.clone())).into_response()
            } else {
                super::api::err(StatusCode::CONFLICT, "messageId has different text")
            };
        }
        if session.state != "running" {
            return super::api::err(StatusCode::CONFLICT, "turn changed; resend this input");
        }
        if text.starts_with('/') {
            return super::api::err(
                StatusCode::CONFLICT,
                "slash commands wait until the current turn ends",
            );
        }
        if body["expectedTurnId"]
            .as_str()
            .is_some_and(|expected| session.turn_id.as_deref() != Some(expected))
        {
            return super::api::err(
                StatusCode::CONFLICT,
                "turn changed; reload the conversation",
            );
        }
        if session.stopped {
            return super::api::err(StatusCode::CONFLICT, "turn is stopping");
        }
        let handle = session.handle.clone().filter(|h| h.steering);
        let input = json!({"messageId":message_id,"text":text,"sequence":session.inputs.len()+1,"state":"queued","admitting":handle.is_some(),"turnId":session.turn_id,"deliveryMode":if handle.is_some(){"steer"}else{"next_turn"}});
        if let Err(error) = crate::acp::sessions::record_update_checked(
            &st.out,
            &id,
            &json!({"sessionUpdate":"input","input":input}),
        ) {
            return super::api::err(StatusCode::INTERNAL_SERVER_ERROR, error);
        }
        session.inputs.push(input.clone());
        (input, handle)
    };
    append(
        &st,
        &sess,
        json!({"sessionUpdate":"user_message","messageId":message_id,"text":text}),
    );
    append(&st, &sess, json!({"sessionUpdate":"input","input":input}));
    if let Some(handle) = handle {
        let st2 = st.clone();
        let sess2 = sess.clone();
        let input2 = input.clone();
        tokio::task::spawn_blocking(move || {
            match handle.steer(&text, input2.clone()) {
                Ok(result) if result["outcome"] == "injected" => {
                    // Delivery is acknowledged separately by the embedded agent.
                    let delivered = sess2
                        .lock()
                        .unwrap()
                        .inputs
                        .iter()
                        .find(|i| i["messageId"] == input2["messageId"])
                        .is_some_and(|i| i["state"] == "delivered");
                    if !delivered {
                        update_input(
                            &st2,
                            &sess2,
                            &input2["messageId"],
                            if handle.input_delivery_events {
                                "queued"
                            } else {
                                "delivered"
                            },
                            None,
                        );
                    }
                }
                Ok(result) if result["outcome"] == "promptRequired" => {
                    {
                        let mut session = sess2.lock().unwrap();
                        if let Some(input) = session
                            .inputs
                            .iter_mut()
                            .find(|i| i["messageId"] == input2["messageId"])
                        {
                            input["deliveryMode"] = json!("next_turn");
                        }
                    }
                    update_input(&st2, &sess2, &input2["messageId"], "queued", None);
                }
                Ok(result) if result["outcome"] == "failed" => update_input(
                    &st2,
                    &sess2,
                    &input2["messageId"],
                    "failed",
                    Some("adapter could not apply the input"),
                ),
                Ok(_) => update_input(
                    &st2,
                    &sess2,
                    &input2["messageId"],
                    "delivery_unknown",
                    Some("adapter returned an unsupported steering outcome"),
                ),
                Err(error) => update_input(
                    &st2,
                    &sess2,
                    &input2["messageId"],
                    "delivery_unknown",
                    Some(&error),
                ),
            }
        });
    }
    (StatusCode::ACCEPTED, Json(input)).into_response()
}

pub async fn cancel(State(st): State<SharedState>, Path(id): Path<String>) -> Response {
    let sess = st.chat.sessions.lock().unwrap().get(&id).cloned();
    match sess {
        Some(s) => {
            let cancelled = {
                let mut session = s.lock().unwrap();
                session.stopped = true;
                session
                    .inputs
                    .iter_mut()
                    .filter_map(|input| {
                        if input["state"] == "queued"
                            || input["state"] == "delivering"
                            || input["state"] == "accepted"
                        {
                            input["state"] = json!("cancelled");
                            Some(input.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            };
            for input in cancelled {
                append(&st, &s, json!({"sessionUpdate":"input","input":input}));
            }
            if let Some(h) = s.lock().unwrap().handle.clone() {
                h.cancel();
            }
            (StatusCode::ACCEPTED, Json(json!({"ok": true}))).into_response()
        }
        None => super::api::err(StatusCode::NOT_FOUND, format!("no chat session {}", id)),
    }
}

// Remove a conversation: an open prompt is cancelled, the agent session closed, and
// the transcript deleted from the session store, so a restart does not bring it
// back. Mirrors docs/frontends/gui.md#chat.
pub async fn delete_session(State(st): State<SharedState>, Path(id): Path<String>) -> Response {
    st.chat.restore(&st.out);
    let Some(sess) = st.chat.sessions.lock().unwrap().remove(&id) else {
        return super::api::err(StatusCode::NOT_FOUND, format!("no chat session {}", id));
    };
    // Closing waits on the agent, so it runs off the async threads.
    if let Some(h) = sess.lock().unwrap().handle.take() {
        tokio::task::spawn_blocking(move || {
            h.cancel();
            h.close();
        });
    }
    crate::acp::sessions::remove(&st.out, &id);
    st.events
        .emit("chat.sessions", json!({"sessions": st.chat.snapshot()}));
    Json(json!({"ok": true})).into_response()
}

// Answer a forwarded permission request: {sessionId, id, optionId?}. A missing
// optionId cancels it.
pub async fn answer_permission(State(st): State<SharedState>, Json(body): Json<Value>) -> Response {
    let sid = body["sessionId"].as_str().unwrap_or_default();
    let ask = body["id"].as_str().unwrap_or_default();
    let option = body["optionId"].as_str().map(|s| s.to_string());
    let sess = st.chat.sessions.lock().unwrap().get(sid).cloned();
    match sess {
        Some(s) => {
            let handle = {
                let mut sl = s.lock().unwrap();
                sl.pending.retain(|p| p["id"] != ask);
                sl.handle.clone()
            };
            match handle {
                Some(h) => {
                    h.answer_permission(ask, option);
                    st.events
                        .emit("chat.sessions", json!({"sessions": st.chat.snapshot()}));
                    (StatusCode::ACCEPTED, Json(json!({"ok": true}))).into_response()
                }
                None => super::api::err(
                    StatusCode::CONFLICT,
                    "no open agent session; prompt the conversation first",
                ),
            }
        }
        None => super::api::err(StatusCode::NOT_FOUND, format!("no chat session {}", sid)),
    }
}

// The commands' GUI implementations. The catalog is shared with the IDE proxy; what
// differs here is only how work starts, because the GUI has a job queue and the
// proxy does not. Mirrors docs/frontends/acp.md#slash-commands.
async fn run_command(st: &SharedState, text: &str) -> Option<String> {
    let (cmd, rest) = crate::acp::commands::split(text, true)?;
    let proj = st.proj();
    let llm = st.llm();
    match cmd {
        "help" => Some(crate::acp::commands::help_text(true)),
        "init" => Some(format!(
            "{} is already a jazyk project.\n\n{}",
            proj.root.display(),
            crate::acp::commands::init_next_steps(&proj, &llm)
        )),
        "config" if rest.is_empty() => Some(crate::acp::commands::config_text(&proj, &llm)),
        "config" => {
            let reply = crate::acp::commands::config_set(&proj, rest);
            st.events.emit("settings.changed", json!({}));
            Some(reply)
        }
        "model" if rest.is_empty() => Some(crate::acp::commands::model_text(&llm)),
        "model" => {
            let reply = crate::acp::commands::model_set(&proj, &llm, rest);
            st.events.emit("settings.changed", json!({}));
            Some(format!(
                "{}\n\nNew jobs use it once settings reload; open conversations keep theirs.",
                reply
            ))
        }
        "agent" if rest.is_empty() => Some(crate::acp::commands::agent_text(&proj)),
        "agent" => {
            let reply = crate::acp::commands::agent_set(&proj, rest);
            st.events.emit("settings.changed", json!({}));
            Some(reply)
        }
        "compile" => {
            let id = st.jobs.submit(st, super::jobs::JobKind::Compile);
            Some(format!("compile queued (job {}); its turns stream below as the build runs", id))
        }
        "generate" => {
            let id = st
                .jobs
                .submit(st, super::jobs::JobKind::Gen { entities: Vec::new(), force: false });
            Some(format!("generation queued (job {})", id))
        }
        "verify" => {
            let id = st.jobs.submit(
                st,
                super::jobs::JobKind::Verify { targets: Vec::new(), test_kind: None, force: false },
            );
            Some(format!("verification queued (job {})", id))
        }
        "questions" => Some(
            crate::answer::questions_summary(&st.out)
                .unwrap_or_else(|| "no standing questions; every open finding is either unprompted or already answered".to_string()),
        ),
        "status" => {
            let s = crate::store::Store::load(&st.out);
            Some(format!(
                "generation {}, verdict {}, {} entity(ies), {} requirement(s), diagnostics {:?}",
                s.status.generation,
                s.status.verdict,
                s.graph.entities.len(),
                s.graph.requirements.len(),
                s.open_diag_counts()
            ))
        }
        "board" => Some(crate::acp::commands::board_text(&proj, &st.out)),
        "preview" => Some(crate::acp::commands::preview_text(&proj, &st.out, rest)),
        "explain" => Some(crate::acp::commands::explain_text(&proj, &st.out, rest)),
        "ripple" => Some(crate::acp::commands::ripple_text(&proj, &st.out, rest)),
        "release" => {
            crate::control::release(&st.proj(), &st.out, None);
            st.events.emit("control.changed", json!({}));
            Some("released: pending compile and generate work is approved".to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod steering_tests {
    use super::*;
    #[test]
    fn restore_keeps_input_identity_and_marks_unfinished_turn_interrupted() {
        let root = tempfile::tempdir().unwrap();
        crate::acp::sessions::open(root.path(), "chat-42", root.path(), "gui");
        let input = json!({"messageId":"second","sequence":2,"text":"update hosting.md","state":"accepted","turnId":"turn-1"});
        crate::acp::sessions::record_update_checked(
            root.path(),
            "chat-42",
            &json!({"sessionUpdate":"input","input":input}),
        )
        .unwrap();
        let manager = ChatManager::default();
        manager.restore(root.path());
        let snapshots = manager.snapshot();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0]["state"], "interrupted");
        assert_eq!(snapshots[0]["inputs"][0]["messageId"], "second");
        assert_eq!(snapshots[0]["inputs"][0]["text"], "update hosting.md");
        assert_eq!(snapshots[0]["inputs"][0]["state"], "delivery_unknown");
        assert_eq!(manager.seq.load(Ordering::Relaxed), 42);
    }
    #[test]
    fn checked_transcript_write_rejects_missing_store() {
        let root = tempfile::tempdir().unwrap();
        assert!(crate::acp::sessions::record_update_checked(
            root.path(),
            "missing",
            &json!({"text":"must survive"})
        )
        .is_err());
    }
}
