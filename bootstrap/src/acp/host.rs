// The ACP host: jazyk as the client of one downstream agent process. One dedicated
// thread drives the async connection; the rest of jazyk talks to it through a sync
// facade (open a session, prompt, cancel), the same shape run_turn had. Sessions
// multiplex on the one connection. Mirrors docs/frontends/acp.md#roles.
use super::config::ResolvedAgent;
use super::policy::{self, PermissionPolicy};
use agent_client_protocol::schema::v1::{
    CancelNotification, ClientCapabilities, ContentBlock, FileSystemCapabilities,
    InitializeRequest, McpServer, McpServerStdio, NewSessionRequest, PromptRequest,
    ReadTextFileRequest, ReadTextFileResponse, RequestPermissionRequest,
    RequestPermissionResponse, SessionNotification, SessionUpdate, WriteTextFileRequest,
    WriteTextFileResponse,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::util::MatchDispatch;
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, ActiveSession, Agent, Client, ConnectionTo, Dispatch, SessionMessage,
};
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::{FutureExt, StreamExt};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// An MCP server to inject into a session, in jazyk's own terms.
#[derive(Clone, Debug)]
pub struct McpSpec {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

// What a prompt reports while it runs: session updates, and (for Forward-policy
// sessions) permission requests awaiting an answer.
pub enum HostEvent<'a> {
    Update(&'a SessionUpdate),
    Permission { id: String, request: &'a RequestPermissionRequest },
}

pub type OnUpdate = Arc<dyn Fn(&HostEvent) + Send + Sync>;

#[derive(Clone, Debug)]
pub struct PromptOutcome {
    // The protocol stop reason (`end_turn`, `max_turn_requests`, `cancelled`, ...).
    pub stop: String,
    // The idle watchdog fired and cancelled the turn.
    pub idled: bool,
}

enum Cmd {
    Open {
        cwd: PathBuf,
        mcp: Vec<McpSpec>,
        policy: PermissionPolicy,
        reply: std::sync::mpsc::Sender<Result<String, String>>,
    },
    Answer {
        session: String,
        id: String,
        option: Option<String>,
    },
    Prompt {
        session: String,
        text: String,
        on_update: OnUpdate,
        reply: std::sync::mpsc::Sender<Result<PromptOutcome, String>>,
    },
    Cancel {
        session: String,
    },
    Close {
        session: String,
        reply: std::sync::mpsc::Sender<()>,
    },
    Shutdown,
}

enum SessCmd {
    Prompt {
        text: String,
        on_update: OnUpdate,
        reply: std::sync::mpsc::Sender<Result<PromptOutcome, String>>,
    },
    Answer {
        id: String,
        option: Option<String>,
    },
    Close {
        reply: std::sync::mpsc::Sender<()>,
    },
}

pub struct AcpHost {
    cmd_tx: UnboundedSender<Cmd>,
    thread: Option<std::thread::JoinHandle<()>>,
    #[allow(dead_code)]
    pub agent: ResolvedAgent,
}

// A handle to one open session. Cloneable; prompts on the same session queue.
#[derive(Clone)]
pub struct SessionHandle {
    id: String,
    cmd_tx: UnboundedSender<Cmd>,
}

fn err_s(e: agent_client_protocol::Error) -> String {
    format!("{}", e)
}

impl AcpHost {
    // Spawn the agent and complete the initialize handshake. `extra_env` rides on the
    // agent process (the embedded profile gets the resolved LLM settings this way).
    // `root` bounds the file-system methods the agent may call back.
    pub fn start(
        agent: ResolvedAgent,
        root: PathBuf,
        extra_env: Vec<(String, String)>,
    ) -> Result<AcpHost, String> {
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let spawn_agent = agent.clone();
        let thread = std::thread::Builder::new()
            .name("acp-host".into())
            .spawn(move || {
                let mut config = AcpAgentConfig::new(&spawn_agent.command).args(spawn_agent.args.clone());
                for (k, v) in spawn_agent.env.iter().chain(extra_env.iter()) {
                    config = config.env(k, v);
                }
                let transport = AcpAgent::new(config);
                let result = futures::executor::block_on(
                    Client
                        .builder()
                        .name("jazyk")
                        .connect_with(transport, |cx: ConnectionTo<Agent>| {
                            main_loop(cx, cmd_rx, ready_tx.clone(), root)
                        }),
                );
                if let Err(e) = result {
                    // If initialize never completed, start() is still waiting.
                    let _ = ready_tx.send(Err(err_s(e)));
                }
            })
            .map_err(|e| format!("cannot spawn acp host thread: {}", e))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(AcpHost { cmd_tx, thread: Some(thread), agent }),
            Ok(Err(e)) => Err(format!("agent `{}` failed to initialize: {}", agent.name, e)),
            Err(_) => Err(format!("agent `{}` exited during initialize", agent.name)),
        }
    }

    pub fn new_session(
        &self,
        cwd: &std::path::Path,
        mcp: Vec<McpSpec>,
        policy: PermissionPolicy,
    ) -> Result<SessionHandle, String> {
        let (reply, rx) = std::sync::mpsc::channel();
        self.cmd_tx
            .unbounded_send(Cmd::Open { cwd: cwd.to_path_buf(), mcp, policy, reply })
            .map_err(|_| "acp host is gone".to_string())?;
        let id = rx.recv().map_err(|_| "acp host dropped the session request".to_string())??;
        Ok(SessionHandle { id, cmd_tx: self.cmd_tx.clone() })
    }
}

impl Drop for AcpHost {
    fn drop(&mut self) {
        let _ = self.cmd_tx.unbounded_send(Cmd::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl SessionHandle {
    pub fn id(&self) -> &str {
        &self.id
    }

    // Send one prompt and block until the turn ends. Updates stream through
    // `on_update` as they arrive.
    pub fn prompt(&self, text: &str, on_update: OnUpdate) -> Result<PromptOutcome, String> {
        let (reply, rx) = std::sync::mpsc::channel();
        self.cmd_tx
            .unbounded_send(Cmd::Prompt {
                session: self.id.clone(),
                text: text.to_string(),
                on_update,
                reply,
            })
            .map_err(|_| "acp host is gone".to_string())?;
        rx.recv().map_err(|_| "acp host dropped the prompt".to_string())?
    }

    pub fn cancel(&self) {
        let _ = self.cmd_tx.unbounded_send(Cmd::Cancel { session: self.id.clone() });
    }

    // Answer a forwarded permission request. `None` cancels it.
    pub fn answer_permission(&self, id: &str, option: Option<String>) {
        let _ = self.cmd_tx.unbounded_send(Cmd::Answer {
            session: self.id.clone(),
            id: id.to_string(),
            option,
        });
    }

    // Close the session and block until the agent has torn it down (or answered that
    // it cannot). An ephemeral serving's implicit finish lands before this returns.
    pub fn close(&self) {
        let (reply, rx) = std::sync::mpsc::channel();
        if self
            .cmd_tx
            .unbounded_send(Cmd::Close { session: self.id.clone(), reply })
            .is_ok()
        {
            let _ = rx.recv();
        }
    }
}

struct SessionEntry {
    tx: UnboundedSender<SessCmd>,
    cancelled: Arc<AtomicBool>,
}

async fn main_loop(
    cx: ConnectionTo<Agent>,
    mut cmd_rx: UnboundedReceiver<Cmd>,
    ready_tx: std::sync::mpsc::Sender<Result<(), String>>,
    root: PathBuf,
) -> Result<(), agent_client_protocol::Error> {
    // Handshake first: capabilities out, the agent's back. File-system methods are
    // advertised and served against the project tree.
    let init = cx
        .send_request(
            InitializeRequest::new(ProtocolVersion::V1).client_capabilities(
                ClientCapabilities::new().fs(
                    FileSystemCapabilities::new().read_text_file(true).write_text_file(true),
                ),
            ),
        )
        .block_task()
        .await;
    match init {
        Ok(_) => {
            let _ = ready_tx.send(Ok(()));
        }
        Err(e) => {
            let _ = ready_tx.send(Err(err_s(e)));
            return Ok(());
        }
    }

    let mut sessions: HashMap<String, SessionEntry> = HashMap::new();
    while let Some(cmd) = cmd_rx.next().await {
        match cmd {
            Cmd::Open { cwd, mcp, policy, reply } => {
                let servers: Vec<McpServer> = mcp
                    .into_iter()
                    .map(|s| {
                        let mut stdio = McpServerStdio::new(s.name, s.command).args(s.args);
                        stdio = stdio.env(
                            s.env
                                .into_iter()
                                .map(|(k, v)| agent_client_protocol::schema::v1::EnvVariable::new(k, v))
                                .collect::<Vec<_>>(),
                        );
                        McpServer::Stdio(stdio)
                    })
                    .collect();
                let request = NewSessionRequest::new(&cwd).mcp_servers(servers);
                match cx.build_session_from(request).block_task().start_session().await {
                    Ok(active) => {
                        let id: String = active.session_id().to_string();
                        let (tx, rx) = unbounded::<SessCmd>();
                        let cancelled = Arc::new(AtomicBool::new(false));
                        sessions.insert(id.clone(), SessionEntry { tx, cancelled: cancelled.clone() });
                        let task_root = root.clone();
                        let _ = cx.spawn(session_task(active, rx, cancelled, task_root, policy));
                        let _ = reply.send(Ok(id));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(err_s(e)));
                    }
                }
            }
            Cmd::Prompt { session, text, on_update, reply } => match sessions.get(&session) {
                Some(entry) => {
                    entry.cancelled.store(false, Ordering::Relaxed);
                    if entry.tx.unbounded_send(SessCmd::Prompt { text, on_update, reply: reply.clone() }).is_err() {
                        let _ = reply.send(Err("session task is gone".into()));
                    }
                }
                None => {
                    let _ = reply.send(Err(format!("unknown session {}", session)));
                }
            },
            Cmd::Answer { session, id, option } => {
                if let Some(entry) = sessions.get(&session) {
                    let _ = entry.tx.unbounded_send(SessCmd::Answer { id, option });
                }
            }
            Cmd::Cancel { session } => {
                if let Some(entry) = sessions.get(&session) {
                    entry.cancelled.store(true, Ordering::Relaxed);
                }
                let _ = cx.send_notification(CancelNotification::new(session.clone()));
            }
            Cmd::Close { session, reply } => {
                match sessions.remove(&session) {
                    Some(entry) => {
                        if entry.tx.unbounded_send(SessCmd::Close { reply: reply.clone() }).is_err() {
                            let _ = reply.send(());
                        }
                    }
                    None => {
                        let _ = reply.send(());
                    }
                }
            }
            Cmd::Shutdown => break,
        }
    }
    Ok(())
}

// One session's owner: services prompts sequentially, answers the agent's callback
// requests (permissions by policy, file system against the project tree), and applies
// the idle watchdog. Commands arriving mid-turn are handled in place (permission
// answers) or queued behind the turn (prompts, close).
// Mirrors docs/frontends/acp.md#worker-sessions and #permissions.
async fn session_task(
    mut session: ActiveSession<'static, Agent>,
    mut rx: UnboundedReceiver<SessCmd>,
    cancelled: Arc<AtomicBool>,
    root: PathBuf,
    policy: PermissionPolicy,
) -> Result<(), agent_client_protocol::Error> {
    use agent_client_protocol::schema::v1::{
        RequestPermissionOutcome, SelectedPermissionOutcome,
    };
    let idle = super::config::idle_timeout();
    // Forwarded permission requests awaiting the user, keyed by the ask id the
    // HostEvent carried. The Mutex satisfies the Send bound on spawned futures; the
    // task itself never contends on it.
    let pending: std::sync::Mutex<
        HashMap<String, agent_client_protocol::Responder<RequestPermissionResponse>>,
    > = std::sync::Mutex::new(HashMap::new());
    let ask_seq = std::sync::atomic::AtomicU64::new(0);
    let mut queued: std::collections::VecDeque<SessCmd> = Default::default();
    loop {
        let cmd = match queued.pop_front() {
            Some(c) => Some(c),
            None => rx.next().await,
        };
        let Some(cmd) = cmd else { break };
        match cmd {
            SessCmd::Close { reply } => {
                // session/close is capability-gated; an agent without it answers
                // method-not-found, and dropping our handle is all there is to do.
                let sid = session.session_id().clone();
                let _ = session
                    .connection()
                    .send_request(agent_client_protocol::schema::v1::CloseSessionRequest::new(sid))
                    .block_task()
                    .await;
                let _ = reply.send(());
                break;
            }
            // No turn in flight: nothing to answer.
            SessCmd::Answer { .. } => {}
            SessCmd::Prompt { text, on_update, reply } => {
                if let Err(e) = session.send_prompt(text) {
                    let _ = reply.send(Err(err_s(e)));
                    continue;
                }
                let mut idled = false;
                enum Ev {
                    Msg(Result<SessionMessage, agent_client_protocol::Error>),
                    Cmd(Option<SessCmd>),
                    Tick,
                }
                let outcome = loop {
                    let ev = futures::select! {
                        m = session.read_update().fuse() => Ev::Msg(m),
                        c = rx.next() => Ev::Cmd(c),
                        _ = FutureExt::fuse(async_io::Timer::after(idle)) => Ev::Tick,
                    };
                    match ev {
                        Ev::Tick => {
                            if idled {
                                // Cancelled already and still silent: the agent is gone.
                                break Err("agent unresponsive after cancel".to_string());
                            }
                            idled = true;
                            cancelled.store(true, Ordering::Relaxed);
                            let sid = session.session_id().clone();
                            let _ = session
                                .connection()
                                .send_notification(CancelNotification::new(sid));
                        }
                        Ev::Cmd(None) => break Err("acp host is gone".to_string()),
                        Ev::Cmd(Some(SessCmd::Answer { id, option })) => {
                            if let Some(r) = pending.lock().unwrap().remove(&id) {
                                let outcome = match option {
                                    Some(oid) => RequestPermissionOutcome::Selected(
                                        SelectedPermissionOutcome::new(oid),
                                    ),
                                    None => RequestPermissionOutcome::Cancelled,
                                };
                                let _ = r.respond(RequestPermissionResponse::new(outcome));
                            }
                        }
                        // Prompts and closes queue behind the running turn.
                        Ev::Cmd(Some(other)) => queued.push_back(other),
                        Ev::Msg(Err(e)) => break Err(err_s(e)),
                        Ev::Msg(Ok(SessionMessage::StopReason(stop))) => {
                            // The turn is over: unanswered forwarded requests cancel,
                            // per the protocol.
                            for (_, r) in pending.lock().unwrap().drain() {
                                let _ = r.respond(RequestPermissionResponse::new(
                                    RequestPermissionOutcome::Cancelled,
                                ));
                            }
                            let stop = serde_json::to_value(&stop)
                                .ok()
                                .and_then(|v| v.as_str().map(|x| x.to_string()))
                                .unwrap_or_else(|| format!("{:?}", stop));
                            break Ok(PromptOutcome { stop, idled });
                        }
                        Ev::Msg(Ok(SessionMessage::SessionMessage(dispatch))) => {
                            handle_dispatch(HandleArgs {
                                dispatch,
                                on_update: &on_update,
                                cancelled: &cancelled,
                                root: &root,
                                policy,
                                pending: &pending,
                                ask_seq: &ask_seq,
                                session_id: &session.session_id().to_string(),
                            })
                            .await?;
                        }
                        Ev::Msg(Ok(_)) => {}
                    }
                };
                let _ = reply.send(outcome);
            }
        }
    }
    Ok(())
}

struct HandleArgs<'a> {
    dispatch: Dispatch,
    on_update: &'a OnUpdate,
    cancelled: &'a Arc<AtomicBool>,
    root: &'a std::path::Path,
    policy: PermissionPolicy,
    pending: &'a std::sync::Mutex<
        HashMap<String, agent_client_protocol::Responder<RequestPermissionResponse>>,
    >,
    ask_seq: &'a std::sync::atomic::AtomicU64,
    session_id: &'a str,
}

async fn handle_dispatch(a: HandleArgs<'_>) -> Result<(), agent_client_protocol::Error> {
    let on_update = a.on_update.clone();
    let was_cancelled = a.cancelled.load(Ordering::Relaxed);
    let root = a.root;
    MatchDispatch::new(a.dispatch)
        .if_notification(async |n: SessionNotification| {
            on_update(&HostEvent::Update(&n.update));
            Ok(())
        })
        .await
        .if_request(async |req: RequestPermissionRequest, responder| {
            // A cancelled turn answers every pending permission request `cancelled`,
            // per the protocol; otherwise the policy decides: automated sessions by
            // rule, chat sessions by forwarding to the user.
            if was_cancelled {
                return responder.respond(RequestPermissionResponse::new(
                    agent_client_protocol::schema::v1::RequestPermissionOutcome::Cancelled,
                ));
            }
            match a.policy {
                PermissionPolicy::Auto => {
                    let outcome = policy::answer(a.policy, &req);
                    responder.respond(RequestPermissionResponse::new(outcome))
                }
                PermissionPolicy::Forward => {
                    let n = a.ask_seq.fetch_add(1, Ordering::Relaxed) + 1;
                    let id = format!("ask-{}-{}", a.session_id, n);
                    (a.on_update)(&HostEvent::Permission { id: id.clone(), request: &req });
                    a.pending.lock().unwrap().insert(id, responder);
                    Ok(())
                }
            }
        })
        .await
        .if_request(async |req: ReadTextFileRequest, responder| {
            match read_text_file(root, &req) {
                Ok(content) => responder.respond(ReadTextFileResponse::new(content)),
                Err(e) => responder.respond_with_internal_error(e),
            }
        })
        .await
        .if_request(async |req: WriteTextFileRequest, responder| {
            match write_text_file(root, &req) {
                Ok(()) => responder.respond(WriteTextFileResponse::new()),
                Err(e) => responder.respond_with_internal_error(e),
            }
        })
        .await
        .otherwise(|message| async move {
            match message {
                Dispatch::Request(_, responder) => responder.respond_with_error(
                    agent_client_protocol::Error::method_not_found(),
                ),
                Dispatch::Notification(_) | Dispatch::Response(_, _) => Ok(()),
            }
        })
        .await
}

// The file-system methods, bounded to the project root. Reads see the disk (jazyk has
// no unsaved-buffer state of its own here); the write path creates parents.
fn checked_path(root: &std::path::Path, path: &std::path::Path) -> Result<PathBuf, String> {
    let mut normal = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                if !normal.pop() {
                    return Err("path escapes the project root".into());
                }
            }
            std::path::Component::CurDir => {}
            other => normal.push(other.as_os_str()),
        }
    }
    if normal.starts_with(root) {
        Ok(normal)
    } else {
        Err(format!("path {} is outside the project root", path.display()))
    }
}

fn read_text_file(root: &std::path::Path, req: &ReadTextFileRequest) -> Result<String, String> {
    let path = checked_path(root, &req.path)?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let start = req.line.map(|l| l.saturating_sub(1) as usize).unwrap_or(0);
    let take = req.limit.map(|l| l as usize).unwrap_or(usize::MAX);
    if start == 0 && take == usize::MAX {
        return Ok(text);
    }
    Ok(text
        .lines()
        .skip(start)
        .take(take)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn write_text_file(root: &std::path::Path, req: &WriteTextFileRequest) -> Result<(), String> {
    let path = checked_path(root, &req.path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    std::fs::write(&path, &req.content).map_err(|e| format!("write {}: {}", path.display(), e))
}

// Suppress unused warnings for items later phases wire up.
#[allow(dead_code)]
fn _unused(_: &PromptRequest, _: &ContentBlock) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    // A one-shot fake OpenAI endpoint: every POST answers a canned non-streaming
    // completion. Runs until the listener drops.
    fn fake_endpoint(reply_content: &str) -> (String, std::net::TcpListener) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let body = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": reply_content}}],
            "usage": {"completion_tokens": 5}
        })
        .to_string();
        let l2 = listener.try_clone().unwrap();
        std::thread::spawn(move || {
            for stream in l2.incoming() {
                let Ok(mut s) = stream else { break };
                let body = body.clone();
                std::thread::spawn(move || {
                    // Read headers, then the content-length body, then respond.
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let mut content_length = 0usize;
                    let mut header_end = 0usize;
                    loop {
                        let n = match s.read(&mut tmp) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => n,
                        };
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            header_end = pos + 4;
                            let headers = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                            for line in headers.lines() {
                                if let Some(v) = line.strip_prefix("content-length:") {
                                    content_length = v.trim().parse().unwrap_or(0);
                                }
                            }
                            break;
                        }
                    }
                    while buf.len() < header_end + content_length {
                        let n = match s.read(&mut tmp) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => n,
                        };
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = s.write_all(resp.as_bytes());
                });
            }
        });
        (format!("http://{}/v1", addr), listener)
    }

    // The Phase 1 milestone in miniature: spawn the real debug binary as the embedded
    // agent, drive one prompt through the host, and read the streamed reply.
    #[test]
    fn host_runs_a_prompt_through_the_embedded_agent() {
        let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/jazyk");
        if !bin.exists() {
            eprintln!("skipping: {} not built", bin.display());
            return;
        }
        let (url, _listener) = fake_endpoint("hello from the fake endpoint");
        let dir = std::env::temp_dir().join(format!("jazyk-acp-host-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let agent = ResolvedAgent {
            name: "embedded".into(),
            command: bin.to_string_lossy().into_owned(),
            args: vec!["agent".into()],
            env: Vec::new(),
            serve_files: true,
        };
        let extra_env = vec![
            ("JAZYK_LLM_BASE_URL".to_string(), url),
            ("JAZYK_MODEL".to_string(), "fake".to_string()),
            ("JAZYK_MIN_INTERVAL_MS".to_string(), "0".to_string()),
            ("JAZYK_MAX_RETRIES".to_string(), "0".to_string()),
            // The probe treats a prose reply under native as a downgrade signal; with
            // no tools in the session the reply ends the turn either way.
            ("JAZYK_CODEC".to_string(), "native".to_string()),
        ];
        let host = AcpHost::start(agent, dir.clone(), extra_env).expect("host start");
        let session = host
            .new_session(&dir, Vec::new(), PermissionPolicy::Auto)
            .expect("session");
        let got: Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        let sink = got.clone();
        let outcome = session
            .prompt(
                "say hi",
                Arc::new(move |ev: &HostEvent| {
                    if let HostEvent::Update(SessionUpdate::AgentMessageChunk(c)) = ev {
                        if let ContentBlock::Text(t) = &c.content {
                            sink.lock().unwrap().push(t.text.clone());
                        }
                    }
                }),
            )
            .expect("prompt");
        assert_eq!(outcome.stop, "end_turn");
        assert!(!outcome.idled);
        let text = got.lock().unwrap().join("");
        assert!(text.contains("hello from the fake endpoint"), "{}", text);
        std::fs::remove_dir_all(&dir).ok();
    }
}
