// `jazyk acp`: the IDE-facing proxy. Upstream it serves the protocol on stdio (the
// IDE spawned it as its agent); downstream it drives the configured agent. In
// between: tool injection, doc edit delegation, slash commands, and transparent
// passthrough outside a jazyk project. Mirrors docs/frontends/acp.md#the-ide-proxy.
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, ContentChunk, InitializeRequest, McpServerStdio,
    NewSessionRequest, PromptRequest, PromptResponse, ReadTextFileRequest,
    RequestPermissionRequest, SessionId, SessionNotification, SessionUpdate, StopReason,
    WriteTextFileRequest,
};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo, Stdio};
use serde_json::json;
use std::sync::{Arc, Mutex, OnceLock};

struct ProxyState {
    up: OnceLock<ConnectionTo<Client>>,
    down: OnceLock<ConnectionTo<Agent>>,
    // The IDE's file-system capabilities, learned at initialize.
    client_fs_write: Mutex<bool>,
    // The session the IDE is talking in: where delegated edits and command output go.
    last_session: Mutex<Option<String>>,
    // The delegation socket the injected servings write through.
    sink_path: Mutex<Option<std::path::PathBuf>>,
    // None outside a jazyk project. `/jazyk-init` fills it in without a restart, so
    // it is behind a lock. Mirrors docs/frontends/acp.md#the-ide-proxy.
    project: Mutex<Option<crate::project::Project>>,
    // Which downstream agent this proxy drives, recorded with each conversation.
    agent_name: String,
    // Whether that agent can reopen one of its own sessions, learned at initialize.
    down_loads: Mutex<bool>,
    // A loaded conversation continues on a fresh downstream session when the agent
    // cannot reopen its own: the loaded id routes onto the new one, and back.
    // Mirrors docs/frontends/acp.md#session-store.
    routes: Mutex<std::collections::HashMap<String, String>>,
    // Behind a lock because `/model` retunes it without a restart.
    llm: Mutex<crate::llm::Llm>,
    out: std::path::PathBuf,
}

impl ProxyState {
    fn in_project(&self) -> bool {
        self.project.lock().unwrap().is_some()
    }

    fn add_route(&self, up: &str, down: &str) {
        self.routes
            .lock()
            .unwrap()
            .insert(up.to_string(), down.to_string());
    }

    // The id the downstream agent knows a session by.
    fn route_down(&self, sid: &str) -> String {
        self.routes
            .lock()
            .unwrap()
            .get(sid)
            .cloned()
            .unwrap_or_else(|| sid.to_string())
    }

    // The id the IDE knows a session by.
    fn route_up(&self, sid: &str) -> String {
        self.routes
            .lock()
            .unwrap()
            .iter()
            .find(|(_, d)| d.as_str() == sid)
            .map(|(u, _)| u.clone())
            .unwrap_or_else(|| sid.to_string())
    }

    // `/jazyk-init` in a bare directory: scaffold, then adopt the project without a
    // restart. The commands this proxy runs itself work in the open session; the
    // agent's own jazyk tools are injected at session/new, so they arrive with the
    // next session. Mirrors docs/frontends/acp.md#the-ide-proxy.
    fn init_here(self: &Arc<Self>) -> String {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        if cwd.join("jazyk.toml").exists() {
            return format!("{} already holds a jazyk.toml.", cwd.display());
        }
        if let Err(e) = std::fs::write(cwd.join("jazyk.toml"), crate::cli::INIT_TOML) {
            return format!("jazyk: cannot write jazyk.toml: {}", e);
        }
        let made = match crate::cli::init_scaffold(&cwd) {
            Ok(made) => made,
            Err(e) => return format!("jazyk: cannot scaffold the project: {}", e),
        };
        *self.project.lock().unwrap() = Some(crate::project::Project::load(&cwd));
        start_sink(self);
        watch_runs(self);
        format!(
            "Initialized a jazyk project in {}: jazyk.toml{}{}.\n\
             Describe what you are building in docs/README.md, then run /compile.\n\
             Start a new conversation to give the agent the jazyk tools; the commands work here now.",
            cwd.display(),
            if made.is_empty() { "" } else { ", " },
            made.join(", ")
        )
    }
}

pub fn run(opts: &crate::cli::Options) -> i32 {
    // Project discovery from the working directory the IDE launched us in. Outside a
    // project the proxy is a transparent passthrough.
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let in_project = crate::project::find_root(&cwd).is_some();
    let (proj, llm, out) = crate::cli::resolve(&[], opts);
    let agent = match crate::acp::config::resolve_acp(
        None,
        &proj.acp,
        &crate::project::load_global_acp(),
        |n| std::env::var(n).ok(),
    ) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("jazyk acp: {}", e);
            return 2;
        }
    };
    let mut config = AcpAgentConfig::new(&agent.command).args(agent.args.clone());
    for (k, v) in &agent.env {
        config = config.env(k, v);
    }
    if agent.name == crate::acp::config::EMBEDDED {
        config = config
            .env("JAZYK_LLM_BASE_URL", &llm.base_url)
            .env("JAZYK_MODEL", &llm.model);
        if !llm.api_key.is_empty() {
            config = config.env("JAZYK_API_KEY", &llm.api_key);
        }
    }

    let state = Arc::new(ProxyState {
        up: OnceLock::new(),
        down: OnceLock::new(),
        client_fs_write: Mutex::new(false),
        last_session: Mutex::new(None),
        sink_path: Mutex::new(None),
        project: Mutex::new(in_project.then(|| proj.clone())),
        agent_name: agent.name.clone(),
        down_loads: Mutex::new(false),
        routes: Mutex::new(std::collections::HashMap::new()),
        llm: Mutex::new(llm),
        out,
    });
    if state.in_project() {
        start_sink(&state);
    }

    let st_init = state.clone();
    let st_new = state.clone();
    let st_prompt = state.clone();
    let st_list = state.clone();
    let st_load = state.clone();
    let st_config = state.clone();
    let st_mode = state.clone();
    let st_cancel = state.clone();
    if state.in_project() {
        watch_runs(&state);
    }

    let result = futures::executor::block_on(
        Agent
            .builder()
            .name("jazyk-acp")
            .on_receive_request(
                async move |req: InitializeRequest, responder, cx: ConnectionTo<Client>| {
                    let _ = st_init.up.set(cx.clone());
                    *st_init.client_fs_write.lock().unwrap() =
                        req.client_capabilities.fs.write_text_file;
                    let down = ensure_down(&st_init, &cx, config.clone())?;
                    let mirror = st_init.in_project();
                    let st_caps = st_init.clone();
                    down.send_request(req).on_receiving_result(async move |result| {
                        // Inside a project the proxy answers session/list and
                        // session/load itself, mirroring recorded runs; advertise it
                        // whatever the downstream agent supports.
                        // Mirrors docs/frontends/acp.md#mirroring-into-ides.
                        let result = result.map(|mut r| {
                            *st_caps.down_loads.lock().unwrap() = r.agent_capabilities.load_session;
                            if mirror {
                                r.agent_capabilities.load_session = true;
                                r.agent_capabilities.session_capabilities = r
                                    .agent_capabilities
                                    .session_capabilities
                                    .list(agent_client_protocol::schema::v1::SessionListCapabilities::new());
                            }
                            r
                        });
                        responder.respond_with_result(result)
                    })
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |mut req: NewSessionRequest, responder, _cx| {
                    // Inside a jazyk project, the session gains the chat serving; the
                    // dropdown entry stays inert everywhere else.
                    let servers = with_jazyk_serving(&st_new, req.mcp_servers.clone());
                    req = req.mcp_servers(servers);
                    let down = st_new.down.get().cloned().ok_or_else(not_initialized)?;
                    let st = st_new.clone();
                    let cwd = req.cwd.clone();
                    down.send_request(req).on_receiving_result(async move |result| {
                        let sid = result.as_ref().ok().map(|r| r.session_id.to_string());
                        if let Some(sid) = &sid {
                            *st.last_session.lock().unwrap() = Some(sid.clone());
                            // A conversation in a project is recorded from its first
                            // moment, so it can be listed and reopened later.
                            // Mirrors docs/frontends/acp.md#session-store.
                            if st.in_project() {
                                crate::acp::sessions::open(&st.out, sid, &cwd, &st.agent_name);
                            }
                        }
                        responder.respond_with_result(result)?;
                        // Inside a project, the session advertises the jazyk commands;
                        // outside one it advertises the way in, and nothing else.
                        // Mirrors docs/frontends/acp.md#slash-commands.
                        if let (Some(sid), Some(up)) = (sid, st.up.get()) {
                            advertise_commands(&st, up, &sid);
                            // Opening a project with standing questions re-surfaces
                            // them without any request.
                            // Mirrors docs/frontends/acp.md#questions-in-chat.
                            if let Some(q) = st
                                .in_project()
                                .then(|| crate::answer::questions_summary(&st.out))
                                .flatten()
                            {
                                let _ = up.send_notification(SessionNotification::new(
                                    sid_of(&sid),
                                    SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                        ContentBlock::from(q),
                                    )),
                                ));
                            }
                        }
                        Ok(())
                    })
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |req: PromptRequest, responder, cx: ConnectionTo<Client>| {
                    *st_prompt.last_session.lock().unwrap() = Some(req.session_id.to_string());
                    // A mirrored run has no agent behind it; a prompt typed into one
                    // is answered here, never forwarded.
                    // Mirrors docs/frontends/acp.md#mirroring-into-ides.
                    if req.session_id.to_string().starts_with("jazyk-run-") {
                        let _ = cx.send_notification(SessionNotification::new(
                            req.session_id.clone(),
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                ContentBlock::from(
                                    "This session is a read-only mirror of an automated run. \
                                     Start a new session to talk to the agent."
                                        .to_string(),
                                ),
                            )),
                        ));
                        return responder.respond(PromptResponse::new(StopReason::EndTurn));
                    }
                    // Slash commands run the real work here; everything else forwards.
                    // Mirrors docs/frontends/acp.md#slash-commands.
                    {
                        let text: String = req
                            .prompt
                            .iter()
                            .filter_map(|b| match b {
                                ContentBlock::Text(t) => Some(t.text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let in_project = st_prompt.in_project();
                        // Both sides of a conversation are recorded: what was asked
                        // here, what came back at the update choke point below.
                        // Mirrors docs/frontends/acp.md#session-store.
                        if in_project {
                            let sid = req.session_id.to_string();
                            let first = crate::acp::sessions::turns(&st_prompt.out, &sid) == 0;
                            crate::acp::sessions::record_prompt(&st_prompt.out, &sid, &text);
                            // A conversation earns its name from its opening line.
                            // Pushing it means the IDE's history row is labelled the
                            // moment it exists, not on the next listing.
                            // Mirrors docs/frontends/acp.md#session-store.
                            if first {
                                if let Some(title) =
                                    crate::acp::sessions::list(&st_prompt.out)
                                        .into_iter()
                                        .find(|m| m.id == sid)
                                        .map(|m| m.title)
                                {
                                    use agent_client_protocol::schema::v1::SessionInfoUpdate;
                                    let _ = cx.send_notification(SessionNotification::new(
                                        req.session_id.clone(),
                                        SessionUpdate::SessionInfoUpdate(
                                            SessionInfoUpdate::new().title(title),
                                        ),
                                    ));
                                }
                            }
                        }
                        if let Some((cmd, args)) =
                            crate::acp::commands::split(&text, in_project)
                        {
                            let st = st_prompt.clone();
                            let sid = req.session_id.to_string();
                            let up = cx.clone();
                            let args = args.to_string();
                            // A command runs off the dispatch task: a build takes
                            // minutes, and the connection must keep serving.
                            std::thread::spawn(move || {
                                let reply = run_command(&st, cmd, &args, &up, &sid);
                                let _ = up.send_notification(SessionNotification::new(
                                    sid_of(&sid),
                                    SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                        ContentBlock::from(reply),
                                    )),
                                ));
                                let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                            });
                            return Ok(());
                        }
                    }
                    let down = st_prompt.down.get().cloned().ok_or_else(not_initialized)?;
                    let mut req = req;
                    req.session_id = sid_of(&st_prompt.route_down(&req.session_id.to_string()));
                    down.send_request(req).on_receiving_result(async move |result| {
                        responder.respond_with_result(result)
                    })
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: agent_client_protocol::schema::v1::ListSessionsRequest,
                            responder,
                            _cx| {
                    // Conversations first, then the automated runs: a person looking
                    // through history wants what they said before, and the runs are
                    // the machine's own record.
                    // Mirrors docs/frontends/acp.md#session-store.
                    let sessions = st_list
                        .project
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|p| {
                            let mut all = recorded_sessions(&p.root, &st_list.out);
                            all.extend(mirrored_sessions(&p.root, &st_list.out));
                            all
                        })
                        .unwrap_or_default();
                    responder.respond(
                        agent_client_protocol::schema::v1::ListSessionsResponse::new(sessions),
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |req: agent_client_protocol::schema::v1::LoadSessionRequest,
                            responder,
                            cx: ConnectionTo<Client>| {
                    let sid = req.session_id.to_string();
                    if let Some(stem) = sid.strip_prefix("jazyk-run-") {
                        // Replay the recorded run as session updates, then answer:
                        // the protocol's own attach-to-history flow.
                        // Mirrors docs/frontends/acp.md#mirroring-into-ides.
                        for update in replay_transcript(&st_load.out, stem) {
                            let _ = cx.send_notification(SessionNotification::new(
                                sid_of(&sid),
                                update,
                            ));
                        }
                        return responder.respond(
                            agent_client_protocol::schema::v1::LoadSessionResponse::new(),
                        );
                    }
                    *st_load.last_session.lock().unwrap() = Some(sid.clone());
                    let down = st_load.down.get().cloned().ok_or_else(not_initialized)?;
                    // An agent that keeps its own sessions owns the replay: its
                    // history is the real one, and replaying jazyk's copy too would
                    // show the conversation twice. The forwarded load keeps the
                    // jazyk serving, the same way session/new gains it.
                    // Mirrors docs/frontends/acp.md#session-store.
                    if *st_load.down_loads.lock().unwrap() {
                        let mut req = req;
                        let servers = with_jazyk_serving(&st_load, req.mcp_servers.clone());
                        req = req.mcp_servers(servers);
                        let st = st_load.clone();
                        let up = cx.clone();
                        return down.send_request(req).on_receiving_result(async move |result| {
                            let ok = result.is_ok();
                            responder.respond_with_result(result)?;
                            if ok {
                                advertise_commands(&st, &up, &sid);
                            }
                            Ok(())
                        });
                    }
                    // The agent cannot reopen a session, so the load never reaches
                    // it: a fresh downstream session carries the continuation, the
                    // loaded id routes onto it, and the store replays what it holds.
                    // Mirrors docs/frontends/acp.md#session-store.
                    let new_req = agent_client_protocol::schema::v1::NewSessionRequest::new(
                        req.cwd.clone(),
                    )
                    .mcp_servers(with_jazyk_serving(&st_load, req.mcp_servers.clone()));
                    let st = st_load.clone();
                    let up = cx.clone();
                    let cwd = req.cwd.clone();
                    down.send_request(new_req).on_receiving_result(async move |result| {
                        let down_sid = match result {
                            Ok(r) => r.session_id.to_string(),
                            Err(e) => return responder.respond_with_result(Err(e)),
                        };
                        st.add_route(&sid, &down_sid);
                        let records = crate::acp::sessions::read(&st.out, &sid);
                        if st.in_project() {
                            crate::acp::sessions::open(&st.out, &sid, &cwd, &st.agent_name);
                        }
                        let replay = replay_conversation(&records);
                        let empty = replay.is_empty();
                        for update in replay {
                            let _ =
                                up.send_notification(SessionNotification::new(sid_of(&sid), update));
                        }
                        let note = if empty {
                            "(no recorded history for this conversation in this project's \
                             session store; continuing fresh.)"
                        } else {
                            "\n(replayed from this project's session store. This agent does not \
                             restore conversation memory, so it answers from here on without it.)"
                        };
                        let _ = up.send_notification(SessionNotification::new(
                            sid_of(&sid),
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from(
                                note.to_string(),
                            ))),
                        ));
                        // The response is the protocol's end-of-replay signal, so it
                        // goes last. Mirrors docs/frontends/acp.md#session-store.
                        responder.respond(
                            agent_client_protocol::schema::v1::LoadSessionResponse::new(),
                        )?;
                        advertise_commands(&st, &up, &sid);
                        Ok(())
                    })
                },
                agent_client_protocol::on_receive_request!(),
            )
            // Session configuration belongs to the agent behind the proxy: the model
            // picker and the mode picker an IDE shows are its options, so both
            // requests pass straight through.
            // Mirrors docs/frontends/acp.md#choosing-a-model.
            .on_receive_request(
                async move |mut req: agent_client_protocol::schema::v1::SetSessionConfigOptionRequest,
                            responder,
                            _cx| {
                    let down = st_config.down.get().cloned().ok_or_else(not_initialized)?;
                    req.session_id = sid_of(&st_config.route_down(&req.session_id.to_string()));
                    down.send_request(req).on_receiving_result(async move |result| {
                        responder.respond_with_result(result)
                    })
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |mut req: agent_client_protocol::schema::v1::SetSessionModeRequest,
                            responder,
                            _cx| {
                    let down = st_mode.down.get().cloned().ok_or_else(not_initialized)?;
                    req.session_id = sid_of(&st_mode.route_down(&req.session_id.to_string()));
                    down.send_request(req).on_receiving_result(async move |result| {
                        responder.respond_with_result(result)
                    })
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_notification(
                async move |mut n: CancelNotification, _cx| {
                    if let Some(down) = st_cancel.down.get() {
                        n.session_id = sid_of(&st_cancel.route_down(&n.session_id.to_string()));
                        let _ = down.send_notification(n);
                    }
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .connect_to(Stdio::new()),
    );
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("jazyk acp: {}", e);
            1
        }
    }
}

fn sid_of(s: &str) -> SessionId {
    SessionId::new(std::sync::Arc::from(s))
}

// Inside a project, every session the downstream agent opens carries the jazyk chat
// serving, whether it came from session/new or session/load.
// Mirrors docs/frontends/acp.md#session-store.
fn with_jazyk_serving(
    st: &Arc<ProxyState>,
    mut servers: Vec<agent_client_protocol::schema::v1::McpServer>,
) -> Vec<agent_client_protocol::schema::v1::McpServer> {
    if !st.in_project() {
        return servers;
    }
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "jazyk".to_string());
    let mut args = vec![
        "mcp".to_string(),
        "chat".to_string(),
        "--ephemeral".to_string(),
        "--out".to_string(),
        st.out.to_string_lossy().into_owned(),
    ];
    if let Some(sink) = st.sink_path.lock().unwrap().as_ref() {
        args.push("--edit-sink".to_string());
        args.push(sink.to_string_lossy().into_owned());
    }
    servers.push(agent_client_protocol::schema::v1::McpServer::Stdio(
        McpServerStdio::new("jazyk", exe).args(args),
    ));
    servers
}

// Mirrors docs/frontends/acp.md#slash-commands.
fn advertise_commands(st: &Arc<ProxyState>, up: &ConnectionTo<Client>, sid: &str) {
    use agent_client_protocol::schema::v1::{AvailableCommand, AvailableCommandsUpdate};
    let commands: Vec<AvailableCommand> = crate::acp::commands::available(st.in_project())
        .map(|c| {
            let cmd = AvailableCommand::new(c.name, c.description);
            match c.hint {
                Some(h) => cmd.input(
                    agent_client_protocol::schema::v1::AvailableCommandInput::Unstructured(
                        agent_client_protocol::schema::v1::UnstructuredCommandInput::new(h),
                    ),
                ),
                None => cmd,
            }
        })
        .collect();
    let _ = up.send_notification(SessionNotification::new(
        sid_of(sid),
        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands)),
    ));
}

fn not_initialized() -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data("initialize first")
}

// Spawn the downstream agent connection once, with handlers that forward its traffic
// to the IDE: updates, permission requests, and file-system calls pass through.
fn ensure_down(
    state: &Arc<ProxyState>,
    cx: &ConnectionTo<Client>,
    config: AcpAgentConfig,
) -> Result<ConnectionTo<Agent>, agent_client_protocol::Error> {
    if let Some(d) = state.down.get() {
        return Ok(d.clone());
    }
    let up_n = state.up.get().cloned();
    let st_rec = state.clone();
    let st_p = state.clone();
    let st_r = state.clone();
    let st_w = state.clone();
    let up_p = up_n.clone();
    let up_r = up_n.clone();
    let up_w = up_n.clone();
    let backend = Client
        .builder()
        .name("jazyk-acp-down")
        .on_receive_notification(
            async move |mut n: SessionNotification, _cx| {
                // Traffic from a routed session carries the id the IDE knows.
                // Mirrors docs/frontends/acp.md#session-store.
                n.session_id = sid_of(&st_rec.route_up(&n.session_id.to_string()));
                // Every update the agent sends passes through here on its way to the
                // IDE, which is where the conversation is recorded.
                // Mirrors docs/frontends/acp.md#session-store.
                if st_rec.in_project() {
                    if let Ok(v) = serde_json::to_value(&n.update) {
                        crate::acp::sessions::record_update(
                            &st_rec.out,
                            &n.session_id.to_string(),
                            &v,
                        );
                    }
                }
                if let Some(up) = &up_n {
                    let _ = up.send_notification(n);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |mut req: RequestPermissionRequest, responder, _cx| {
                let up = up_p.clone().ok_or_else(not_initialized)?;
                req.session_id = sid_of(&st_p.route_up(&req.session_id.to_string()));
                up.send_request(req)
                    .on_receiving_result(async move |result| responder.respond_with_result(result))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |mut req: ReadTextFileRequest, responder, _cx| {
                let up = up_r.clone().ok_or_else(not_initialized)?;
                req.session_id = sid_of(&st_r.route_up(&req.session_id.to_string()));
                up.send_request(req)
                    .on_receiving_result(async move |result| responder.respond_with_result(result))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |mut req: WriteTextFileRequest, responder, _cx| {
                let up = up_w.clone().ok_or_else(not_initialized)?;
                req.session_id = sid_of(&st_w.route_up(&req.session_id.to_string()));
                up.send_request(req)
                    .on_receiving_result(async move |result| responder.respond_with_result(result))
            },
            agent_client_protocol::on_receive_request!(),
        );
    let down = cx.spawn_connection(backend, AcpAgent::new(config))?;
    let _ = state.down.set(down.clone());
    Ok(down)
}

// A delegated edit lands in the IDE's buffer when the client can take it, and on
// disk otherwise. Mirrors docs/frontends/acp.md#doc-edit-delegation.
fn start_sink(state: &Arc<ProxyState>) {
    use std::io::{BufRead, BufReader, Write};
    let path = state
        .out
        .join(format!("acp-edit-sink-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = std::os::unix::net::UnixListener::bind(&path) else {
        return;
    };
    *state.sink_path.lock().unwrap() = Some(path);
    let st = state.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let st = st.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                    return;
                };
                let reply = match delegate_edit(&st, &v) {
                    Ok(()) => json!({"ok": true}),
                    Err(e) => json!({"ok": false, "error": e}),
                };
                let mut w = &stream;
                let _ = writeln!(w, "{}", reply);
            });
        }
    });
}

fn delegate_edit(st: &Arc<ProxyState>, v: &serde_json::Value) -> Result<(), String> {
    if !*st.client_fs_write.lock().unwrap() {
        return Err("the client has no fs.writeTextFile capability".into());
    }
    let up = st.up.get().ok_or("no upstream connection")?;
    let sid = st
        .last_session
        .lock()
        .unwrap()
        .clone()
        .ok_or("no open session")?;
    let path = v["path"].as_str().ok_or("missing path")?;
    let content = v["content"].as_str().ok_or("missing content")?;
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    up.send_request(WriteTextFileRequest::new(
        sid_of(&sid),
        path.to_string(),
        content.to_string(),
    ))
    .on_receiving_result(async move |result| {
        let _ = tx.send(result.map(|_| ()).map_err(|e| format!("{}", e)));
        Ok(())
    })
    .map_err(|e| format!("{}", e))?;
    rx.recv_timeout(std::time::Duration::from_secs(20))
        .map_err(|_| "the client did not answer the write".to_string())?
}

// The intercepted commands: the real paths, their progress narrated into the open
// turn. Mirrors docs/frontends/acp.md#slash-commands.
fn run_command(
    st: &Arc<ProxyState>,
    cmd: &str,
    args: &str,
    up: &ConnectionTo<Client>,
    sid: &str,
) -> String {
    use crate::acp::commands;
    // The two commands a directory without a project still answers.
    // Mirrors docs/frontends/acp.md#slash-commands.
    let in_project = st.in_project();
    match cmd {
        "help" => return commands::help_text(in_project),
        "init" if !in_project => return st.init_here(),
        _ => {}
    }
    let Some(proj) = st.project.lock().unwrap().clone() else {
        return "not a jazyk project".into();
    };
    let proj = &proj;
    let llm = st.llm.lock().unwrap().clone();
    let say = |text: String| {
        let _ = up.send_notification(SessionNotification::new(
            sid_of(sid),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from(text))),
        ));
    };
    match cmd {
        "init" => format!(
            "{} is already a jazyk project.\n\n{}",
            proj.root.display(),
            commands::init_next_steps(proj, &llm)
        ),
        "config" if args.is_empty() => commands::config_text(proj, &llm),
        "config" => commands::config_set(proj, args),
        "model" if args.is_empty() => commands::model_text(&llm),
        "model" => {
            let mut reply = commands::model_set(proj, &llm, args);
            // The proxy's own builds pick the new model up immediately; the open
            // session gets it through the protocol's config option where the agent
            // takes one. Mirrors docs/frontends/acp.md#choosing-a-model.
            st.llm.lock().unwrap().model = args.to_string();
            if let Some(down) = st.down.get() {
                use agent_client_protocol::schema::v1::{
                    SessionConfigId, SessionConfigValueId, SetSessionConfigOptionRequest,
                };
                let req = SetSessionConfigOptionRequest::new(
                    sid_of(&st.route_down(sid)),
                    SessionConfigId::new(std::sync::Arc::from("model")),
                    SessionConfigValueId::new(std::sync::Arc::from(args)),
                );
                let _ = down
                    .send_request(req)
                    .on_receiving_result(async move |_result| Ok(()));
                reply.push_str(
                    "\n\nBuilds from this proxy use it now. The open session was asked to \
                     switch too; an agent without a `model` option keeps its own.",
                );
            }
            reply
        }
        "agent" if args.is_empty() => commands::agent_text(proj),
        "agent" => commands::agent_set(proj, args),
        "questions" => crate::answer::questions_summary(&st.out).unwrap_or_else(|| {
            "no standing questions; every open finding is either unprompted or already answered"
                .into()
        }),
        "status" => {
            // The verdict with its counts, and the board summary beside it.
            // Mirrors docs/frontends/acp.md#slash-commands.
            let s = crate::store::Store::load(&st.out);
            let board = crate::board::Board::compute(proj, &st.out);
            let c = board.counts();
            format!(
                "generation {}, verdict {}, {} entity(ies), {} requirement(s), diagnostics {:?}\n{}\nboard: {} open, {} ready, {} blocked, {} parked, {} failed",
                s.status.generation,
                s.status.verdict,
                s.graph.entities.len(),
                s.graph.requirements.len(),
                s.open_diag_counts(),
                board.summary_line(),
                c.open,
                c.ready,
                c.blocked,
                c.parked,
                c.failed
            )
        }
        "board" => commands::board_text(proj, &st.out),
        "preview" => commands::preview_text(proj, &st.out, args),
        "explain" => commands::explain_text(proj, &st.out, args),
        "ripple" => commands::ripple_text(proj, &st.out, args),
        "release" => {
            crate::control::release(proj, &st.out, None);
            "released: pending compile and generate work is approved".into()
        }
        "compile" => {
            say("compiling…\n".into());
            let trace = narrated_trace(up.clone(), sid.to_string());
            let report = crate::reconcile::compile(proj, &llm, &st.out, &trace);
            // The verdict carries its counts; the board summary rides beside it.
            let board = crate::board::Board::compute(proj, &st.out);
            format!(
                "\n{}; {} session(s), {} mutation(s), {} parked, coverage {}%\n{}",
                report.verdict,
                report.sessions,
                report.applied,
                report.parked,
                report.coverage_pct,
                board.summary_line()
            )
        }
        "generate" | "verify" => {
            let runner = match crate::acp::runner::AcpRunner::start(proj, &llm, &st.out) {
                Ok(r) => r,
                Err(e) => return format!("agent failed to start: {}", e),
            };
            let trace = narrated_trace(up.clone(), sid.to_string());
            let store = crate::store::Store::load(&st.out);
            let gs = crate::gen::GenSettings::resolve(proj);
            let result = if cmd == "generate" {
                let _guard = match crate::control::begin_internal_build(proj, &st.out, "generate") {
                    Ok(g) => g,
                    Err(e) => return format!("refused: {}", e),
                };
                runner.set_build_token(Some(format!("internal-{}", std::process::id())));
                let _ = crate::bind::run_all(&store, &runner, &gs, &[], &trace);
                crate::gen::run_all(&store, &runner, &gs, &[], false, &trace)
            } else {
                crate::verify::run_all(&store, &runner, &gs, &[], None, false, &trace)
            };
            match result {
                Ok(v) => format!("\ndone: {}", v),
                Err(e) => format!("\nfailed: {}", e),
            }
        }
        _ => "unknown command".into(),
    }
}

// Build progress narrated as message chunks: one line per session lifecycle event.
// The machinery of a batch is not news to the person who asked for the build: these
// calls get no row. Mirrors docs/frontends/acp.md#slash-commands.
const LIFECYCLE_TOOLS: &[&str] = &["goals", "begin_goals", "done", "abandon_goals"];

// A row is titled by the decision it carries, not the tool's identifier.
// Mirrors docs/frontends/acp.md#slash-commands.
fn call_title(name: &str, input: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(input).unwrap_or(serde_json::Value::Null);
    let s = |k: &str| args[k].as_str().unwrap_or("").to_string();
    let short = |t: String| {
        if t.chars().count() > 80 {
            format!("{}…", t.chars().take(80).collect::<String>())
        } else {
            t
        }
    };
    match name {
        "search" => format!("search: {}", s("query")),
        "load" => format!("load {}", s("target")),
        "unload" => format!("unload {}", s("target")),
        "expand" => "expand context".into(),
        "graph_status" => "graph status".into(),
        "load_skill" => format!("skill {}", s("name")),
        "read_section" => format!("read {}", s("section")),
        "get_entity" => format!("read entity {}", s("id")),
        "get_view" => format!("read view {}", s("id")),
        "diagnostics" => "read diagnostics".into(),
        "upsert_entity" => format!("entity {}", s("name")),
        "update_entity" => format!("update entity {}", s("id")),
        "delete_entity" => format!("delete entity {}", s("id")),
        "merge_entities" => format!("merge {} into {}", s("absorb"), s("keep")),
        "upsert_requirement" => format!("requirement: {}", short(s("statement"))),
        "update_requirement" => format!("update requirement {}", s("id")),
        "delete_requirement" => format!("delete requirement {}", s("id")),
        "report_diagnostic" => format!("report {} {}", s("severity"), s("rule")),
        "update_diagnostic" => format!("update finding {}", s("id")),
        "resolve_diagnostic" => format!("resolve finding {}", s("id")),
        "set_coverage" => format!("coverage: {} {}", s("section"), s("state")),
        "upsert_view" => format!("view {} {}", s("kind"), s("title")),
        "update_view" => format!("update view {}", s("id")),
        "delete_view" => format!("delete view {}", s("id")),
        // A goal claim is titled by the goal; the justification rides as the row's
        // payload. Mirrors docs/frontends/acp.md#slash-commands.
        "mark_goal_done" => format!("resolved {}", s("goal")),
        "mark_goal_failed" => format!("failed {}", s("goal")),
        "edit_doc_prose" => format!("edit {}", s("doc")),
        _ => name.replace('_', " "),
    }
}

// When the result settles what happened, the completed row says so: an upsert is
// retitled `added` or `updated` with the id the store minted.
// Mirrors docs/frontends/acp.md#slash-commands.
fn result_title(name: &str, output: &str) -> Option<String> {
    let mut v: serde_json::Value = serde_json::from_str(output).ok()?;
    // MCP results arrive as a JSON string holding JSON: unwrap the inner document.
    if let serde_json::Value::String(inner) = &v {
        v = serde_json::from_str(inner).ok()?;
    }
    let id = v["id"].as_str()?;
    let verb = match v["created"].as_bool() {
        Some(true) => "added",
        Some(false) => "updated",
        None => return None,
    };
    let kind = match name {
        "upsert_entity" | "update_entity" => "entity",
        "upsert_requirement" | "update_requirement" => "requirement",
        _ => return None,
    };
    Some(format!("{} {} {}", verb, kind, id))
}

// A command's build streams into the open turn at full fidelity: boundaries as
// message text, worker reasoning as thought chunks, and each graph tool call as a
// tool_call row with its result. Ids are namespaced per worker so parallel turns
// never collide. Mirrors docs/frontends/acp.md#slash-commands.
fn narrated_trace(up: ConnectionTo<Client>, sid: String) -> crate::session::Trace {
    use crate::session::TraceEvent;
    use agent_client_protocol::schema::v1::{
        ToolCall, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    };
    // Per worker label: calls made so far, the id of the one still open, and the
    // summary the suppressed `done` call carried (the runner's SessionDone has none;
    // the model's own words fill the closing line).
    #[derive(Default)]
    struct WorkerRow {
        calls: u64,
        open: Option<String>,
        done_summary: Option<String>,
    }
    let open: Mutex<std::collections::HashMap<String, WorkerRow>> =
        Mutex::new(std::collections::HashMap::new());
    let send = move |update: SessionUpdate| {
        let _ = up.send_notification(SessionNotification::new(sid_of(&sid), update));
    };
    let sink: Arc<dyn Fn(&TraceEvent) + Send + Sync> = Arc::new(move |ev| {
        let text_update = |text: String| {
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from(text)))
        };
        match ev {
            TraceEvent::Board {
                goals,
                kinds,
                blocked,
                ..
            } => {
                // The board summary the build prints first.
                // Mirrors docs/frontends/cli.md#jazyk-compile.
                let per_kind: Vec<String> =
                    kinds.iter().map(|(k, n)| format!("{} {}", n, k)).collect();
                let mut line = format!("compile: {} goals", goals);
                if !per_kind.is_empty() {
                    line.push_str(&format!(" ({})", per_kind.join(", ")));
                }
                if *blocked > 0 {
                    line.push_str(&format!(", {} blocked", blocked));
                }
                line.push('\n');
                send(text_update(line));
            }
            TraceEvent::BatchStart {
                label,
                class,
                goals,
                ..
            } => {
                send(text_update(format!(
                    "batch {}: {} ({} goal(s))\n",
                    label,
                    class,
                    goals.len()
                )));
            }
            TraceEvent::GcBurst {
                goal_kind,
                target,
                count,
                limit,
                ..
            } => {
                send(text_update(format!(
                    "gc burst: {} {} ({} > {})\n",
                    goal_kind, target, count, limit
                )));
            }
            TraceEvent::Goal {
                goal,
                event,
                justification,
                reason,
                ..
            } => {
                let tail = justification
                    .as_ref()
                    .or(reason.as_ref())
                    .map(|t| format!(": {}", t))
                    .unwrap_or_default();
                send(text_update(format!("{} {}{}\n", event, goal, tail)));
            }
            TraceEvent::SessionStart { label, .. } => {
                open.lock()
                    .unwrap()
                    .entry(label.clone())
                    .or_default()
                    .done_summary = None;
                send(text_update(format!("▶ {}\n", label)));
            }
            TraceEvent::SessionDone {
                label,
                staged,
                summary,
                ..
            } => {
                // The model's own account of the turn: the answer to "done doing
                // what?". Mirrors docs/frontends/acp.md#slash-commands.
                let said = if summary.trim().is_empty() {
                    open.lock()
                        .unwrap()
                        .get_mut(label)
                        .and_then(|r| r.done_summary.take())
                        .unwrap_or_default()
                } else {
                    summary.trim().to_string()
                };
                let tail = if said.is_empty() {
                    String::new()
                } else {
                    format!(": {}", said)
                };
                send(text_update(format!(
                    "✓ {} ({} staged){}\n",
                    label, staged, tail
                )));
            }
            TraceEvent::SessionFailed { label, error, .. } => {
                send(text_update(format!("✗ {}: {}\n", label, error)));
            }
            TraceEvent::GenEntityDone { entity, files } => {
                send(text_update(format!(
                    "✓ gen {} ({} file(s))\n",
                    entity, files
                )));
            }
            TraceEvent::GenEntityFailed { entity, error, .. } => {
                send(text_update(format!("✗ gen {}: {}\n", entity, error)));
            }
            TraceEvent::VerifyRowDone {
                requirement,
                verdict,
                ..
            } => {
                send(text_update(format!("{} {}\n", verdict, requirement)));
            }
            TraceEvent::ModelText { text, .. } => {
                send(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                    ContentBlock::from(format!("{}\n", text)),
                )));
            }
            // Jazyk's own narration is commentary, not the model thinking: it
            // renders as message text, so a thought section is never empty.
            // Mirrors docs/frontends/acp.md#slash-commands.
            TraceEvent::Note { text, verbose, .. } if !verbose => {
                send(text_update(format!("{}\n", text)));
            }
            TraceEvent::ToolCall {
                label,
                name,
                summary,
                full,
            } => {
                let input = full.clone().unwrap_or_else(|| summary.clone());
                if LIFECYCLE_TOOLS.contains(&name.as_str()) {
                    if name == "done" {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&input) {
                            if let Some(s) = v["summary"].as_str().filter(|s| !s.is_empty()) {
                                open.lock()
                                    .unwrap()
                                    .entry(label.clone())
                                    .or_default()
                                    .done_summary = Some(s.to_string());
                            }
                        }
                    }
                    return;
                }
                let id = {
                    let mut map = open.lock().unwrap();
                    let entry = map.entry(label.clone()).or_default();
                    entry.calls += 1;
                    let id = format!("jazyk:{}:{}", label, entry.calls);
                    entry.open = Some(id.clone());
                    id
                };
                send(SessionUpdate::ToolCall(
                    ToolCall::new(id, call_title(name, &input))
                        .status(ToolCallStatus::InProgress)
                        .raw_input(serde_json::Value::String(input)),
                ));
            }
            TraceEvent::ToolResult {
                label,
                name,
                summary,
                full,
            } => {
                if LIFECYCLE_TOOLS.contains(&name.as_str()) {
                    return;
                }
                if let Some(id) = open
                    .lock()
                    .unwrap()
                    .get_mut(label)
                    .and_then(|e| e.open.take())
                {
                    let output = full.clone().unwrap_or_else(|| summary.clone());
                    let title = result_title(name, &output);
                    send(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                        id,
                        ToolCallUpdateFields::new()
                            .status(ToolCallStatus::Completed)
                            .title(title)
                            .raw_output(serde_json::Value::String(output)),
                    )));
                }
            }
            TraceEvent::ToolError {
                label,
                rule,
                message,
            } => {
                if let Some(id) = open
                    .lock()
                    .unwrap()
                    .get_mut(label)
                    .and_then(|e| e.open.take())
                {
                    send(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                        id,
                        ToolCallUpdateFields::new()
                            .status(ToolCallStatus::Failed)
                            .title(rule.clone())
                            .raw_output(serde_json::Value::String(message.clone())),
                    )));
                }
            }
            _ => {}
        }
    });
    crate::session::Trace::to_sink(crate::session::TraceLevel::Normal, sink, Default::default())
}

// Recorded runs as read-only sessions: one per transcript under <out>/trace, newest
// first. Mirrors docs/frontends/acp.md#mirroring-into-ides.
fn mirrored_sessions(
    root: &std::path::Path,
    out: &std::path::Path,
) -> Vec<agent_client_protocol::schema::v1::SessionInfo> {
    use agent_client_protocol::schema::v1::SessionInfo;
    let mut entries: Vec<(String, String, String)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(out.join("trace")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".jsonl") else {
                continue;
            };
            let Ok(file) = std::fs::File::open(e.path()) else {
                continue;
            };
            let mut first = String::new();
            use std::io::BufRead;
            if std::io::BufReader::new(file).read_line(&mut first).is_err() {
                continue;
            }
            let Ok(meta) = serde_json::from_str::<serde_json::Value>(first.trim()) else {
                continue;
            };
            let kind = meta["meta"]["kind"]["kind"]
                .as_str()
                .unwrap_or("run")
                .to_string();
            let started = meta["meta"]["startedAt"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            entries.push((stem.to_string(), kind, started));
        }
    }
    entries.sort_by(|a, b| b.2.cmp(&a.2));
    entries.truncate(20);
    entries
        .into_iter()
        .map(|(stem, kind, started)| {
            SessionInfo::new(sid_of(&format!("jazyk-run-{}", stem)), root)
                .title(format!("{} {}", kind, started))
        })
        .collect()
}

// One transcript replayed as session updates, capped to the recent tail.
fn replay_transcript(out: &std::path::Path, stem: &str) -> Vec<SessionUpdate> {
    use agent_client_protocol::schema::v1::{ToolCall, ToolCallStatus};
    let path = out.join("trace").join(format!("{}.jsonl", stem));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut updates: Vec<SessionUpdate> = Vec::new();
    let chunk =
        |t: String| SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from(t)));
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let ev = &v["event"];
        let n = v["n"].as_u64().unwrap_or(0);
        let label = ev["label"].as_str().unwrap_or("");
        match ev["kind"].as_str().unwrap_or("") {
            "sessionStart" => updates.push(chunk(format!("▶ {}\n", label))),
            "batchStart" => updates.push(chunk(format!(
                "batch {}: {} ({} goal(s))\n",
                label,
                ev["class"].as_str().unwrap_or(""),
                ev["goals"].as_array().map(|a| a.len()).unwrap_or(0)
            ))),
            "board" => updates.push(chunk(format!(
                "compile: {} goals, {} blocked\n",
                ev["goals"].as_u64().unwrap_or(0),
                ev["blocked"].as_u64().unwrap_or(0)
            ))),
            "goal" => {
                let tail = ev["justification"]
                    .as_str()
                    .or(ev["reason"].as_str())
                    .map(|t| format!(": {}", t))
                    .unwrap_or_default();
                updates.push(chunk(format!(
                    "{} {}{}\n",
                    ev["event"].as_str().unwrap_or(""),
                    ev["goal"].as_str().unwrap_or(""),
                    tail
                )));
            }
            "gcBurst" => updates.push(chunk(format!(
                "gc burst: {} {} ({} > {})\n",
                ev["goalKind"].as_str().unwrap_or(""),
                ev["target"].as_str().unwrap_or(""),
                ev["count"].as_u64().unwrap_or(0),
                ev["limit"].as_u64().unwrap_or(0)
            ))),
            "modelText" => updates.push(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                ContentBlock::from(format!("{}\n", ev["text"].as_str().unwrap_or(""))),
            ))),
            "toolCall" => updates.push(SessionUpdate::ToolCall(
                ToolCall::new(
                    format!("replay-{}", n),
                    format!(
                        "{} → {} {}",
                        label,
                        ev["name"].as_str().unwrap_or(""),
                        ev["summary"].as_str().unwrap_or("")
                    ),
                )
                .status(ToolCallStatus::Completed),
            )),
            "toolError" => updates.push(SessionUpdate::ToolCall(
                ToolCall::new(
                    format!("replay-{}", n),
                    format!(
                        "{} ✗ {}: {}",
                        label,
                        ev["rule"].as_str().unwrap_or(""),
                        ev["message"].as_str().unwrap_or("")
                    ),
                )
                .status(ToolCallStatus::Failed),
            )),
            "sessionDone" => updates.push(chunk(format!("✓ {}\n", label))),
            "sessionFailed" => updates.push(chunk(format!(
                "✗ {}: {}\n",
                label,
                ev["error"].as_str().unwrap_or("")
            ))),
            _ => {}
        }
    }
    if updates.len() > 400 {
        let cut = updates.len() - 400;
        updates.drain(..cut);
    }
    updates
}

// Nudge capable clients when the run list changes. The notification is
// underscore-namespaced, so a client that does not know it ignores it, per the
// protocol's extensibility rules. Mirrors docs/frontends/acp.md#mirroring-into-ides.
fn watch_runs(state: &Arc<ProxyState>) {
    let st = state.clone();
    std::thread::spawn(move || {
        let dir = st.out.join("trace");
        let mut last: usize = std::fs::read_dir(&dir).map(|r| r.count()).unwrap_or(0);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let now = std::fs::read_dir(&dir).map(|r| r.count()).unwrap_or(0);
            if now != last {
                last = now;
                if let Some(up) = st.up.get() {
                    if let Ok(msg) = agent_client_protocol::UntypedMessage::new(
                        "_jazyk/session_list_changed",
                        &json!({}),
                    ) {
                        let _ = up.send_notification(msg);
                    }
                }
            }
        }
    });
}

// The project's recorded conversations, as sessions an IDE can reopen.
// Mirrors docs/frontends/acp.md#session-store.
fn recorded_sessions(
    root: &std::path::Path,
    out: &std::path::Path,
) -> Vec<agent_client_protocol::schema::v1::SessionInfo> {
    use agent_client_protocol::schema::v1::SessionInfo;
    crate::acp::sessions::list(out)
        .into_iter()
        .take(30)
        // The timestamp is what a history picker renders as "5m ago"; without it a
        // row cannot be placed in time. Mirrors docs/frontends/acp.md#session-store.
        .map(|m| {
            SessionInfo::new(sid_of(&m.id), root)
                .title(m.title)
                .updated_at(m.updated_at)
        })
        .collect()
}

// A recorded conversation replayed as updates: what the person asked, and the
// agent's own updates exactly as they were sent.
// Mirrors docs/frontends/acp.md#session-store.
fn replay_conversation(records: &[serde_json::Value]) -> Vec<SessionUpdate> {
    let mut updates = Vec::new();
    for r in records {
        match r["kind"].as_str().unwrap_or("") {
            "user" => {
                let text = r["text"].as_str().unwrap_or_default().to_string();
                updates.push(SessionUpdate::UserMessageChunk(ContentChunk::new(
                    ContentBlock::from(text),
                )));
            }
            "update" => {
                // A recorded update is replayed as itself. One jazyk does not
                // recognize is skipped rather than guessed at.
                if let Ok(u) = serde_json::from_value::<SessionUpdate>(r["update"].clone()) {
                    updates.push(u);
                }
            }
            _ => {}
        }
    }
    if updates.len() > 400 {
        let cut = updates.len() - 400;
        updates.drain(..cut);
    }
    updates
}
