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
}

pub struct ChatManager {
    host: Mutex<Option<AcpHost>>,
    sessions: Mutex<BTreeMap<String, Arc<Mutex<ChatSession>>>>,
    seq: AtomicU64,
}

impl Default for ChatManager {
    fn default() -> Self {
        ChatManager { host: Mutex::new(None), sessions: Mutex::new(BTreeMap::new()), seq: AtomicU64::new(0) }
    }
}

impl ChatManager {
    // The shared agent process behind every chat session, spawned on first use.
    fn host(&self, st: &SharedState) -> Result<(), String> {
        let mut h = self.host.lock().unwrap();
        if h.is_some() {
            return Ok(());
        }
        let proj = st.proj();
        let agent = config::resolve_acp(None, &proj.acp, &crate::project::load_global_acp(), |n| {
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
                "--out".to_string(),
                st.out.to_string_lossy().into_owned(),
            ],
            env: Vec::new(),
        };
        let h = self.host.lock().unwrap();
        let host = h.as_ref().ok_or("agent host is gone")?;
        let handle = host.new_session(&proj.root, vec![spec], PermissionPolicy::Forward)?;
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
                       "pending": s.pending})
            })
            .collect()
    }
}

fn append(st: &SharedState, sess: &Arc<Mutex<ChatSession>>, update: Value) {
    let (id, n, elided) = {
        let mut s = sess.lock().unwrap();
        let n = s.next_n;
        s.next_n += 1;
        s.ring.push_back((n, update.clone()));
        while s.ring.len() > RING {
            s.ring.pop_front();
        }
        (s.id.clone(), n, super::jobs::elide(&update))
    };
    st.events.emit("chat.update", json!({"sessionId": id, "n": n, "update": elided}));
}

fn set_state(st: &SharedState, sess: &Arc<Mutex<ChatSession>>, state: &str) {
    {
        let mut s = sess.lock().unwrap();
        s.state = state.to_string();
    }
    st.events.emit("chat.sessions", json!({"sessions": st.chat.snapshot()}));
}

// ---- handlers ----

pub async fn post_session(State(st): State<SharedState>) -> Response {
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
    };
    st.chat.sessions.lock().unwrap().insert(id.clone(), Arc::new(Mutex::new(session)));
    st.events.emit("chat.sessions", json!({"sessions": st.chat.snapshot()}));
    Json(json!({"id": id})).into_response()
}

pub async fn list_sessions(State(st): State<SharedState>) -> Response {
    let commands: Vec<Value> =
        COMMANDS.iter().map(|(name, desc)| json!({"name": name, "description": desc})).collect();
    Json(json!({"sessions": st.chat.snapshot(), "commands": commands})).into_response()
}

pub async fn get_session(State(st): State<SharedState>, Path(id): Path<String>) -> Response {
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
                        "pending": s.pending, "updates": updates}))
            .into_response()
        }
        None => (StatusCode::NOT_FOUND, "no such session").into_response(),
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
    let Some(sess) = st.chat.sessions.lock().unwrap().get(&id).cloned() else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    let text = body["text"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        return (StatusCode::BAD_REQUEST, "empty prompt").into_response();
    }
    if sess.lock().unwrap().state == "running" {
        return (StatusCode::CONFLICT, "a turn is already running").into_response();
    }
    // The user's side of the transcript.
    append(&st, &sess, json!({"sessionUpdate": "user_message", "text": text}));

    // Slash commands arrive as prompt text, per the protocol; jazyk matches the
    // prefix and runs the real work instead of prompting the agent.
    // Mirrors docs/frontends/acp.md#slash-commands.
    if let Some(reply) = run_command(&st, &text).await {
        append(&st, &sess, json!({"sessionUpdate": "agent_message", "text": reply}));
        return (StatusCode::ACCEPTED, Json(json!({"ok": true}))).into_response();
    }

    set_state(&st, &sess, "running");
    let st2 = st.clone();
    let sess2 = sess.clone();
    tokio::task::spawn_blocking(move || {
        // The ACP session opens on the first prompt and lives with the chat.
        let handle = {
            let existing = sess2.lock().unwrap().handle.clone();
            match existing {
                Some(h) => h,
                None => match st2.chat.open_acp_session(&st2, &id) {
                    Ok(h) => {
                        sess2.lock().unwrap().handle = Some(h.clone());
                        h
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
        let cb_st = st2.clone();
        let cb_sess = sess2.clone();
        let outcome = handle.prompt(
            &text,
            Arc::new(move |ev: &HostEvent| match ev {
                HostEvent::Update(u) => {
                    if let Ok(v) = serde_json::to_value(u) {
                        append(&cb_st, &cb_sess, v);
                    }
                }
                HostEvent::Permission { id, request } => {
                    let ask = json!({"id": id, "request": serde_json::to_value(request).unwrap_or_default()});
                    cb_sess.lock().unwrap().pending.push(ask.clone());
                    let sid = cb_sess.lock().unwrap().id.clone();
                    cb_st.events.emit("chat.permission", json!({"sessionId": sid, "ask": ask}));
                }
            }),
        );
        {
            let mut s = sess2.lock().unwrap();
            s.pending.clear();
        }
        match outcome {
            Ok(o) => {
                append(&st2, &sess2, json!({"sessionUpdate": "turn_end", "stop": o.stop, "idled": o.idled}));
                set_state(&st2, &sess2, "idle");
            }
            Err(e) => {
                append(&st2, &sess2, json!({"sessionUpdate": "turn_end", "stop": "error", "error": e}));
                set_state(&st2, &sess2, "failed");
            }
        }
    });
    (StatusCode::ACCEPTED, Json(json!({"ok": true}))).into_response()
}

pub async fn cancel(State(st): State<SharedState>, Path(id): Path<String>) -> Response {
    let sess = st.chat.sessions.lock().unwrap().get(&id).cloned();
    match sess {
        Some(s) => {
            if let Some(h) = s.lock().unwrap().handle.clone() {
                h.cancel();
            }
            (StatusCode::ACCEPTED, Json(json!({"ok": true}))).into_response()
        }
        None => (StatusCode::NOT_FOUND, "no such session").into_response(),
    }
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
                    st.events.emit("chat.sessions", json!({"sessions": st.chat.snapshot()}));
                    (StatusCode::ACCEPTED, Json(json!({"ok": true}))).into_response()
                }
                None => (StatusCode::CONFLICT, "no open agent session").into_response(),
            }
        }
        None => (StatusCode::NOT_FOUND, "no such session").into_response(),
    }
}

// The advertised commands and their implementations. Commands run the same paths the
// buttons do; the reply names what happened, and the pane follows the job's own
// stream. Mirrors docs/frontends/acp.md#slash-commands.
pub const COMMANDS: [(&str, &str); 5] = [
    ("/compile", "reconcile the graph with the documents"),
    ("/generate", "bind and generate the deliverable"),
    ("/verify", "run verification over the ledger"),
    ("/status", "summarize the last build"),
    ("/release", "approve pending changes in manual mode"),
];

async fn run_command(st: &SharedState, text: &str) -> Option<String> {
    let (cmd, _rest) = match text.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (text, ""),
    };
    match cmd {
        "/compile" => {
            let id = st.jobs.submit(st, super::jobs::JobKind::Compile);
            Some(format!("compile queued (job {}); its turns stream below as the build runs", id))
        }
        "/generate" => {
            let id = st
                .jobs
                .submit(st, super::jobs::JobKind::Gen { entities: Vec::new(), force: false });
            Some(format!("generation queued (job {})", id))
        }
        "/verify" => {
            let id = st.jobs.submit(
                st,
                super::jobs::JobKind::Verify { targets: Vec::new(), test_kind: None, force: false },
            );
            Some(format!("verification queued (job {})", id))
        }
        "/status" => {
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
        "/release" => {
            crate::control::release(&st.proj(), &st.out, None);
            st.events.emit("control.changed", json!({}));
            Some("released: pending compile and generate work is approved".to_string())
        }
        _ => None,
    }
}
