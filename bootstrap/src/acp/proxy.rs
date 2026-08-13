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
    project: Option<crate::project::Project>,
    llm: crate::llm::Llm,
    out: std::path::PathBuf,
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
        config = config.env("JAZYK_LLM_BASE_URL", &llm.base_url).env("JAZYK_MODEL", &llm.model);
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
        project: in_project.then(|| proj.clone()),
        llm,
        out,
    });
    if state.project.is_some() {
        start_sink(&state);
    }

    let st_init = state.clone();
    let st_new = state.clone();
    let st_prompt = state.clone();
    let st_list = state.clone();
    let st_load = state.clone();
    let st_cancel = state.clone();
    if state.project.is_some() {
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
                    let mirror = st_init.project.is_some();
                    down.send_request(req).on_receiving_result(async move |result| {
                        // Inside a project the proxy answers session/list and
                        // session/load itself, mirroring recorded runs; advertise it
                        // whatever the downstream agent supports.
                        // Mirrors docs/frontends/acp.md#mirroring-into-ides.
                        let result = result.map(|mut r| {
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
                    if let Some(proj) = &st_new.project {
                        let exe = std::env::current_exe()
                            .ok()
                            .and_then(|p| p.to_str().map(|s| s.to_string()))
                            .unwrap_or_else(|| "jazyk".to_string());
                        let mut args = vec![
                            "mcp".to_string(),
                            "chat".to_string(),
                            "--ephemeral".to_string(),
                            "--out".to_string(),
                            st_new.out.to_string_lossy().into_owned(),
                        ];
                        if let Some(sink) = st_new.sink_path.lock().unwrap().as_ref() {
                            args.push("--edit-sink".to_string());
                            args.push(sink.to_string_lossy().into_owned());
                        }
                        let _ = proj;
                        let mut servers = req.mcp_servers.clone();
                        servers.push(agent_client_protocol::schema::v1::McpServer::Stdio(
                            McpServerStdio::new("jazyk", exe).args(args),
                        ));
                        req = req.mcp_servers(servers);
                    }
                    let down = st_new.down.get().cloned().ok_or_else(not_initialized)?;
                    let st = st_new.clone();
                    down.send_request(req).on_receiving_result(async move |result| {
                        let sid = result.as_ref().ok().map(|r| r.session_id.to_string());
                        if let Some(sid) = &sid {
                            *st.last_session.lock().unwrap() = Some(sid.clone());
                        }
                        responder.respond_with_result(result)?;
                        // Inside a project, the session advertises the jazyk commands.
                        // Mirrors docs/frontends/acp.md#slash-commands.
                        if let (Some(sid), Some(up), true) =
                            (sid, st.up.get(), st.project.is_some())
                        {
                            use agent_client_protocol::schema::v1::{
                                AvailableCommand, AvailableCommandsUpdate,
                            };
                            let commands: Vec<AvailableCommand> = [
                                ("compile", "reconcile the graph with the documents"),
                                ("generate", "bind and generate the deliverable"),
                                ("verify", "run verification over the ledger"),
                                ("status", "summarize the last build"),
                                ("release", "approve pending changes in manual mode"),
                            ]
                            .into_iter()
                            .map(|(n, d)| AvailableCommand::new(n, d))
                            .collect();
                            let _ = up.send_notification(SessionNotification::new(
                                sid_of(&sid),
                                SessionUpdate::AvailableCommandsUpdate(
                                    AvailableCommandsUpdate::new(commands),
                                ),
                            ));
                        }
                        Ok(())
                    })
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |req: PromptRequest, responder, cx: ConnectionTo<Client>| {
                    *st_prompt.last_session.lock().unwrap() = Some(req.session_id.to_string());
                    // Slash commands run the real work here; everything else forwards.
                    // Mirrors docs/frontends/acp.md#slash-commands.
                    if st_prompt.project.is_some() {
                        let text: String = req
                            .prompt
                            .iter()
                            .filter_map(|b| match b {
                                ContentBlock::Text(t) => Some(t.text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let cmd = text.trim().split_whitespace().next().unwrap_or("");
                        if ["/compile", "/generate", "/verify", "/status", "/release"].contains(&cmd) {
                            let st = st_prompt.clone();
                            let sid = req.session_id.to_string();
                            let up = cx.clone();
                            let cmd = cmd.to_string();
                            std::thread::spawn(move || {
                                let reply = run_command(&st, &cmd, &up, &sid);
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
                    let sessions = st_list
                        .project
                        .as_ref()
                        .map(|p| mirrored_sessions(&p.root, &st_list.out))
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
                    let down = st_load.down.get().cloned().ok_or_else(not_initialized)?;
                    down.send_request(req).on_receiving_result(async move |result| {
                        responder.respond_with_result(result)
                    })
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_notification(
                async move |n: CancelNotification, _cx| {
                    if let Some(down) = st_cancel.down.get() {
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
    let up_p = up_n.clone();
    let up_r = up_n.clone();
    let up_w = up_n.clone();
    let backend = Client
        .builder()
        .name("jazyk-acp-down")
        .on_receive_notification(
            async move |n: SessionNotification, _cx| {
                if let Some(up) = &up_n {
                    let _ = up.send_notification(n);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, _cx| {
                let up = up_p.clone().ok_or_else(not_initialized)?;
                up.send_request(req)
                    .on_receiving_result(async move |result| responder.respond_with_result(result))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: ReadTextFileRequest, responder, _cx| {
                let up = up_r.clone().ok_or_else(not_initialized)?;
                up.send_request(req)
                    .on_receiving_result(async move |result| responder.respond_with_result(result))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: WriteTextFileRequest, responder, _cx| {
                let up = up_w.clone().ok_or_else(not_initialized)?;
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
    let path = state.out.join(format!("acp-edit-sink-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = std::os::unix::net::UnixListener::bind(&path) else { return };
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
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else { return };
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
    let sid = st.last_session.lock().unwrap().clone().ok_or("no open session")?;
    let path = v["path"].as_str().ok_or("missing path")?;
    let content = v["content"].as_str().ok_or("missing content")?;
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    up.send_request(WriteTextFileRequest::new(sid_of(&sid), path.to_string(), content.to_string()))
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
fn run_command(st: &Arc<ProxyState>, cmd: &str, up: &ConnectionTo<Client>, sid: &str) -> String {
    let Some(proj) = &st.project else { return "not a jazyk project".into() };
    let say = |text: String| {
        let _ = up.send_notification(SessionNotification::new(
            sid_of(sid),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from(text))),
        ));
    };
    match cmd {
        "/status" => {
            let s = crate::store::Store::load(&st.out);
            format!(
                "generation {}, verdict {}, {} entity(ies), {} requirement(s), diagnostics {:?}",
                s.status.generation,
                s.status.verdict,
                s.graph.entities.len(),
                s.graph.requirements.len(),
                s.open_diag_counts()
            )
        }
        "/release" => {
            crate::control::release(proj, &st.out, None);
            "released: pending compile and generate work is approved".into()
        }
        "/compile" => {
            say("compiling…\n".into());
            let trace = narrated_trace(up.clone(), sid.to_string());
            let report = crate::reconcile::compile(proj, &st.llm, &st.out, &trace);
            format!(
                "\n{} — {} turn(s), {} mutation(s), {} parked, coverage {}%",
                report.verdict, report.turns, report.applied, report.parked, report.coverage_pct
            )
        }
        "/generate" | "/verify" => {
            let runner = match crate::acp::runner::AcpRunner::start(proj, &st.llm, &st.out) {
                Ok(r) => r,
                Err(e) => return format!("agent failed to start: {}", e),
            };
            let trace = narrated_trace(up.clone(), sid.to_string());
            let store = crate::store::Store::load(&st.out);
            let gs = crate::gen::GenSettings::resolve(proj);
            let result = if cmd == "/generate" {
                let _guard = match crate::control::begin_internal_build(proj, &st.out, "generate") {
                    Ok(g) => g,
                    Err(e) => return format!("refused: {}", e),
                };
                runner.set_build_token(Some(format!("internal-{}", std::process::id())));
                let _ = crate::bind::run_all(&store, &runner, &gs, &[], &proj.limits, &proj.linting, &trace);
                crate::gen::run_all(&store, &runner, &gs, &[], false, &proj.limits, &proj.linting, &trace)
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

// Build progress narrated as message chunks: one line per turn lifecycle event.
fn narrated_trace(up: ConnectionTo<Client>, sid: String) -> crate::turn::Trace {
    use crate::turn::TraceEvent;
    let sink: Arc<dyn Fn(&TraceEvent) + Send + Sync> = Arc::new(move |ev| {
        let line = match ev {
            TraceEvent::WaveStart { task, items, .. } => {
                Some(format!("wave: {} ({} item(s))\n", task, items.len()))
            }
            TraceEvent::TurnStart { label, .. } => Some(format!("▶ {}\n", label)),
            TraceEvent::TurnDone { label, staged, .. } => Some(format!("✓ {} ({} staged)\n", label, staged)),
            TraceEvent::TurnFailed { label, error, .. } => Some(format!("✗ {}: {}\n", label, error)),
            TraceEvent::GenEntityDone { entity, files } => Some(format!("✓ gen {} ({} file(s))\n", entity, files)),
            TraceEvent::GenEntityFailed { entity, error, .. } => Some(format!("✗ gen {}: {}\n", entity, error)),
            TraceEvent::VerifyRowDone { requirement, verdict, .. } => {
                Some(format!("{} {}\n", verdict, requirement))
            }
            _ => None,
        };
        if let Some(text) = line {
            let _ = up.send_notification(SessionNotification::new(
                sid_of(&sid),
                SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from(text))),
            ));
        }
    });
    crate::turn::Trace::to_sink(crate::turn::TraceLevel::Normal, sink, Default::default())
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
            let Some(stem) = name.strip_suffix(".jsonl") else { continue };
            let Ok(file) = std::fs::File::open(e.path()) else { continue };
            let mut first = String::new();
            use std::io::BufRead;
            if std::io::BufReader::new(file).read_line(&mut first).is_err() {
                continue;
            }
            let Ok(meta) = serde_json::from_str::<serde_json::Value>(first.trim()) else { continue };
            let kind = meta["meta"]["kind"]["kind"].as_str().unwrap_or("run").to_string();
            let started = meta["meta"]["startedAt"].as_str().unwrap_or_default().to_string();
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
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    let mut updates: Vec<SessionUpdate> = Vec::new();
    let chunk = |t: String| SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from(t)));
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let ev = &v["event"];
        let n = v["n"].as_u64().unwrap_or(0);
        let label = ev["label"].as_str().unwrap_or("");
        match ev["kind"].as_str().unwrap_or("") {
            "turnStart" => updates.push(chunk(format!("▶ {}\n", label))),
            "modelText" => updates.push(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                ContentBlock::from(format!("{}\n", ev["text"].as_str().unwrap_or(""))),
            ))),
            "toolCall" => updates.push(SessionUpdate::ToolCall(
                ToolCall::new(
                    format!("replay-{}", n),
                    format!("{} → {} {}", label, ev["name"].as_str().unwrap_or(""), ev["summary"].as_str().unwrap_or("")),
                )
                .status(ToolCallStatus::Completed),
            )),
            "toolError" => updates.push(SessionUpdate::ToolCall(
                ToolCall::new(
                    format!("replay-{}", n),
                    format!("{} ✗ {}: {}", label, ev["rule"].as_str().unwrap_or(""), ev["message"].as_str().unwrap_or("")),
                )
                .status(ToolCallStatus::Failed),
            )),
            "turnDone" => updates.push(chunk(format!("✓ {}\n", label))),
            "turnFailed" => updates.push(chunk(format!("✗ {}: {}\n", label, ev["error"].as_str().unwrap_or("")))),
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
