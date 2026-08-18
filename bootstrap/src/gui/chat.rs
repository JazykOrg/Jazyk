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

    // The conversations this project already had, read back from the store on first
    // use so a restarted server shows them. They have no live agent behind them
    // until prompted, which opens a fresh session under the same id.
    // Mirrors docs/frontends/acp.md#session-store.
    pub fn restore(&self, out: &std::path::Path) {
        let mut sessions = self.sessions.lock().unwrap();
        if !sessions.is_empty() {
            return;
        }
        for meta in crate::acp::sessions::list(out).into_iter().take(30) {
            let records = crate::acp::sessions::read(out, &meta.id);
            let mut ring: VecDeque<(u64, Value)> = VecDeque::new();
            for (n, r) in records.iter().enumerate() {
                let update = match r["kind"].as_str().unwrap_or("") {
                    "user" => json!({"sessionUpdate": "user_message", "text": r["text"]}),
                    "update" => r["update"].clone(),
                    _ => continue,
                };
                ring.push_back((n as u64, update));
            }
            let next_n = ring.back().map(|(n, _)| n + 1).unwrap_or(0);
            // A number a later append cannot collide with, whatever was trimmed.
            let seq = self.seq.load(Ordering::Relaxed);
            let n = meta.id.strip_prefix("chat-").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            if n > seq {
                self.seq.store(n, Ordering::Relaxed);
            }
            sessions.insert(
                meta.id.clone(),
                Arc::new(Mutex::new(ChatSession {
                    id: meta.id,
                    title: meta.title,
                    state: "idle".into(),
                    handle: None,
                    ring,
                    next_n,
                    pending: Vec::new(),
                })),
            );
        }
    }
}

fn append(st: &SharedState, sess: &Arc<Mutex<ChatSession>>, update: Value) {
    // The transcript outlives the page and the process: the same per-project store
    // the IDE proxy writes. Mirrors docs/frontends/acp.md#session-store.
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
    match update["sessionUpdate"].as_str() {
        Some("user_message") => crate::acp::sessions::record_prompt(
            &st.out,
            &id,
            update["text"].as_str().unwrap_or_default(),
        ),
        _ => crate::acp::sessions::record_update(&st.out, &id, &update),
    }
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
    crate::acp::sessions::open(&st.out, &id, &st.proj().root, "gui");
    st.chat.sessions.lock().unwrap().insert(id.clone(), Arc::new(Mutex::new(session)));
    st.events.emit("chat.sessions", json!({"sessions": st.chat.snapshot()}));
    Json(json!({"id": id})).into_response()
}

pub async fn list_sessions(State(st): State<SharedState>) -> Response {
    st.chat.restore(&st.out);
    // The same catalog the IDE proxy advertises; the GUI is a jazyk client like any
    // other. Mirrors docs/frontends/acp.md#slash-commands.
    let commands: Vec<Value> = crate::acp::commands::available(true)
        .map(|c| json!({"name": format!("/{}", c.name), "description": c.description, "hint": c.hint}))
        .collect();
    Json(json!({"sessions": st.chat.snapshot(), "commands": commands})).into_response()
}

pub async fn get_session(State(st): State<SharedState>, Path(id): Path<String>) -> Response {
    // A reload can ask for one conversation by id without listing first.
    st.chat.restore(&st.out);
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
    // Prompting a conversation restored from the store is how it resumes.
    st.chat.restore(&st.out);
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
        "release" => {
            crate::control::release(&st.proj(), &st.out, None);
            st.events.emit("control.changed", json!({}));
            Some("released: pending compile and generate work is approved".to_string())
        }
        _ => None,
    }
}
