//! Conversation-local routes behind ACP's model picker. Compiler settings stay separate.
use super::*;
use agent_client_protocol::schema::v1::{
    ConfigOptionUpdate, LoadSessionResponse, NewSessionResponse, SessionConfigKind,
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelect,
    SessionConfigSelectOption, SetSessionConfigOptionRequest,
};
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, String>;
#[derive(Default)]
pub(super) struct Routing {
    sessions: Mutex<BTreeMap<String, Conversation>>,
    models: Mutex<BTreeMap<String, String>>,
    transition: Mutex<()>,
}
struct Conversation {
    request: NewSessionRequest,
    choice: String,
    busy: bool,
    continuation: bool,
    transcript: Vec<(String, String)>,
    answer: String,
    auth_opened: bool,
    options: Vec<SessionConfigOption>,
    response: Option<NewSessionResponse>,
    setup_revision: Option<std::time::SystemTime>,
}

// Only used on dedicated threads, never on the SDK's dispatch task.
fn wait<T>(
    future: impl std::future::Future<Output = std::result::Result<T, agent_client_protocol::Error>>,
) -> Result<T> {
    futures::executor::block_on(async {
        match futures::future::select(
            Box::pin(future),
            Box::pin(async_io::Timer::after(std::time::Duration::from_secs(60))),
        )
        .await
        {
            futures::future::Either::Left((result, _)) => result.map_err(|e| e.to_string()),
            futures::future::Either::Right(_) => {
                Err("The agent did not respond within 60 seconds.".into())
            }
        }
    })
}
fn default_choice(st: &ProxyState) -> String {
    let agent = st.agent_name.lock().unwrap().clone();
    if agent != "embedded" {
        return agent;
    }
    crate::chat_provider::selected().unwrap_or_else(|| {
        if st.chat_explicit {
            "endpoint"
        } else {
            "setup"
        }
        .into()
    })
}
pub(super) fn choice(st: &ProxyState, sid: &str) -> String {
    st.chat
        .sessions
        .lock()
        .unwrap()
        .get(sid)
        .map(|s| s.choice.clone())
        .unwrap_or_else(|| "setup".into())
}
fn catalog(st: &ProxyState) -> BTreeMap<String, String> {
    let mut names: BTreeMap<String, String> = [
        ("setup", "[Configure]"),
        ("cloud", "Cloud"),
        ("claude", "Claude Code"),
        ("codex", "Codex"),
        ("opencode", "OpenCode"),
    ]
    .into_iter()
    .map(|(id, name)| (id.into(), name.into()))
    .collect();
    for key in st
        .acp_settings
        .agents
        .keys()
        .chain(crate::project::load_global_acp().agents.keys())
    {
        if key != "embedded" {
            names.entry(key.clone()).or_insert(key.clone());
        }
    }
    if crate::chat_provider::for_source_configured(
        &st.llm.lock().unwrap(),
        "endpoint",
        st.chat_explicit,
    )
    .is_ok()
    {
        names.insert("endpoint".into(), "OpenAI-compatible endpoint".into());
    }
    for (id, url, _, _) in crate::chat_provider::entries()
        .into_iter()
        .filter(|(id, _, _, _)| id.starts_with("endpoint"))
    {
        names.insert(
            id.clone(),
            format!(
                "{} ({url})",
                id.strip_prefix("endpoint:").unwrap_or("OpenAI")
            ),
        );
    }
    names.extend(st.chat.models.lock().unwrap().clone());
    names
}
pub(super) fn options(st: &ProxyState, sid: &str) -> Vec<SessionConfigOption> {
    let current = choice(st, sid);
    let mut names = catalog(st);
    names.entry(current.clone()).or_insert(current.clone());
    let select = SessionConfigOption::new(
        "model",
        "Model",
        SessionConfigKind::Select(SessionConfigSelect::new(
            current,
            std::iter::once(("setup".to_string(), "[Configure]".to_string()))
                .chain(names.into_iter().filter(|(id, _)| id != "setup"))
                .map(|(id, name)| SessionConfigSelectOption::new(id, name))
                .collect::<Vec<_>>(),
        )),
    )
    .category(SessionConfigOptionCategory::Model);
    let mut options = st
        .chat
        .sessions
        .lock()
        .unwrap()
        .get(sid)
        .map(|s| s.options.clone())
        .unwrap_or_default();
    options.retain(|o| {
        o.id.to_string() != "model"
            && o.category.as_ref() != Some(&SessionConfigOptionCategory::Model)
    });
    options.insert(0, select);
    options
}
fn publish(st: &Arc<ProxyState>, sid: &str) {
    if let Some(up) = st.up.get() {
        let sessions = st
            .chat
            .sessions
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for id in sessions {
            let _ = up.send_notification(SessionNotification::new(
                sid_of(&id),
                SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(options(st, &id))),
            ));
        }
        advertise_commands(st, up, sid);
    }
}
fn learn(st: &ProxyState, backend: &str, source: &str, response: &NewSessionResponse) {
    // Config selects can contain grouped options; preserve each reported value verbatim.
    let value = serde_json::to_value(response).unwrap_or_default();
    let mut models = st.chat.models.lock().unwrap();
    let mut add = |id: &str, name: &str| {
        let prefix = if backend == "embedded" {
            source
        } else {
            backend
        };
        models.insert(format!("{prefix}/{id}"), format!("{prefix}/{name}"));
    };
    if let Some(options) = value["configOptions"].as_array() {
        for option in options
            .iter()
            .filter(|o| o["id"] == "model" || o["category"] == "model")
        {
            fn values(option: &serde_json::Value, add: &mut impl FnMut(&str, &str)) {
                if let Some(id) = option["value"].as_str() {
                    add(id, option["name"].as_str().unwrap_or(id));
                }
                if let Some(list) = option["options"].as_array() {
                    for item in list {
                        values(item, add);
                    }
                }
            }
            values(option, &mut add);
        }
    }
}
fn connected(st: &Arc<ProxyState>, name: &str) -> Result<ConnectionTo<Agent>> {
    let up = st.up.get().ok_or("initialize first")?;
    let (_, config) = st.agent_config(Some(name))?;
    let down = ensure_down(st, up, name, config).map_err(|e| e.to_string())?;
    if !st.down_loads.lock().unwrap().contains_key(name) {
        let init = st
            .initialize
            .lock()
            .unwrap()
            .clone()
            .ok_or("initialize first")?;
        let response = wait(down.send_request(init).block_task())?;
        st.down_loads
            .lock()
            .unwrap()
            .insert(name.into(), response.agent_capabilities.load_session);
    }
    Ok(down)
}
fn activate(st: &Arc<ProxyState>, sid: &str, chosen: &str) -> Result<()> {
    let _serial = st.chat.transition.lock().unwrap();
    let chosen = if chosen == "embedded" {
        crate::chat_provider::selected().unwrap_or_else(|| "setup".into())
    } else {
        chosen.into()
    };
    let old_id = st.routes.lock().unwrap().get(sid).cloned();
    let old_route = old_id.map(|id| (st.agent_of(sid), id));
    let request = {
        let sessions = st.chat.sessions.lock().unwrap();
        let s = sessions.get(sid).ok_or("unknown conversation")?;
        s.request.clone()
    };
    let (source, model) = chosen
        .split_once('/')
        .map_or((chosen.as_str(), None), |(a, b)| (a, Some(b)));
    let embedded = source == "cloud" || source.starts_with("endpoint");
    let name = if embedded { "embedded" } else { source };
    if source != "setup" {
        st.agent_config(Some(name))?;
        if let Some(model) = model {
            if !st.chat.models.lock().unwrap().contains_key(&chosen) {
                return Err(format!("Unknown model `{source}/{model}`. Select `{source}` first to discover its models."));
            }
        }
    }
    // Virtual choices are valid before credentials exist. Browser sign-in starts on a prompt.
    let ready = source != "setup"
        && (!embedded
            || crate::chat_provider::for_source_configured(
                &st.llm.lock().unwrap(),
                source,
                st.chat_explicit,
            )
            .is_ok());
    if ready {
        let down = connected(st, name)?;
        let fence = runtime_fence(st);
        let mut next = request.clone().mcp_servers(with_jazyk_serving(
            st,
            request.mcp_servers.clone(),
            &fence,
        ));
        let mut meta = next.meta.take().unwrap_or_default();
        meta.insert("jazyk".into(), json!({"purpose":"chat","source":source}));
        next.meta = Some(meta);
        let response = wait(down.send_request(next).block_task())?;
        learn(st, name, source, &response);
        let downstream = response.session_id.clone();
        let set_result = if let Some(model) = model {
            let id = response
                .config_options
                .as_ref()
                .and_then(|options| {
                    options.iter().find(|o| {
                        o.id.to_string() == "model"
                            || o.category.as_ref() == Some(&SessionConfigOptionCategory::Model)
                    })
                })
                .map(|o| o.id.clone());
            if let Some(id) = id {
                wait(
                    down.send_request(SetSessionConfigOptionRequest::new(
                        downstream.clone(),
                        id,
                        model,
                    ))
                    .block_task(),
                )
                .map(|_| ())
            } else {
                Err(
                    "This agent does not expose model configuration through ACP session options."
                        .into(),
                )
            }
        } else {
            Ok(())
        };
        if let Err(error) = set_result {
            let _ = down
                .send_request(agent_client_protocol::schema::v1::CloseSessionRequest::new(
                    downstream,
                ))
                .on_receiving_result(async |_r| Ok(()));
            return Err(error);
        }
        st.session_agents
            .lock()
            .unwrap()
            .insert(sid.into(), name.into());
        st.add_route(sid, &response.session_id.to_string());
        st.runtime_fences.lock().unwrap().insert(sid.into(), fence);
        let mut sessions = st.chat.sessions.lock().unwrap();
        let session = sessions
            .get_mut(sid)
            .ok_or("conversation closed while switching")?;
        session.options = response.config_options.clone().unwrap_or_default();
        session.response = Some(response);
    } else {
        st.session_agents.lock().unwrap().remove(sid);
        st.routes.lock().unwrap().remove(sid);
        let mut sessions = st.chat.sessions.lock().unwrap();
        let session = sessions
            .get_mut(sid)
            .ok_or("conversation closed while switching")?;
        session.options.clear();
        session.response = None;
    }
    let mut sessions = st.chat.sessions.lock().unwrap();
    let session = sessions.get_mut(sid).unwrap();
    session.choice = chosen.clone();
    session.continuation = !session.transcript.is_empty();
    session.auth_opened = false;
    if chosen == "setup" {
        session.setup_revision = crate::chat_provider::revision();
    }
    drop(sessions);
    if let Err(error) = crate::acp::sessions::append_checked(
        &st.out,
        sid,
        json!({"kind":"selection","choice":chosen}),
    ) {
        eprintln!("jazyk: cannot persist chat selection: {error}");
    }
    // Retired routes are no longer allowed to forward updates, approvals or filesystem requests.
    if let Some((backend, id)) = old_route {
        if st.agent_of(sid) != backend || st.routes.lock().unwrap().get(sid) != Some(&id) {
            if let Some(down) = st.downs.lock().unwrap().get(&backend) {
                let _ = down
                    .send_request(agent_client_protocol::schema::v1::CloseSessionRequest::new(
                        sid_of(&id),
                    ))
                    .on_receiving_result(async |_r| Ok(()));
            }
        }
    }
    Ok(())
}
struct Busy {
    st: Arc<ProxyState>,
    sid: String,
}
impl Drop for Busy {
    fn drop(&mut self) {
        if let Some(s) = self.st.chat.sessions.lock().unwrap().get_mut(&self.sid) {
            s.busy = false;
        }
    }
}
fn reserve(st: &Arc<ProxyState>, sid: &str) -> Result<Busy> {
    let mut sessions = st.chat.sessions.lock().unwrap();
    let s = sessions.get_mut(sid).ok_or("unknown conversation")?;
    if s.busy {
        return Err(
            "Finish or cancel the current turn before switching or sending another message.".into(),
        );
    }
    s.busy = true;
    Ok(Busy {
        st: st.clone(),
        sid: sid.into(),
    })
}
pub(super) fn select(
    st: &Arc<ProxyState>,
    sid: &str,
    value: &str,
) -> Result<Vec<SessionConfigOption>> {
    let _busy = reserve(st, sid)?;
    activate(st, sid, value)?;
    publish(st, sid);
    Ok(options(st, sid))
}
pub(super) fn new(
    st: &Arc<ProxyState>,
    request: NewSessionRequest,
    id: Option<String>,
) -> Result<NewSessionResponse> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sid = id.unwrap_or_else(|| {
        format!(
            "jazyk-chat-{}-{}-{}",
            crate::sys::process_id(),
            crate::sys::now_millis(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    });
    let initial = default_choice(st);
    st.chat.sessions.lock().unwrap().insert(
        sid.clone(),
        Conversation {
            request: request.clone(),
            choice: "setup".into(),
            busy: false,
            continuation: false,
            transcript: Vec::new(),
            answer: String::new(),
            auth_opened: false,
            options: Vec::new(),
            response: None,
            setup_revision: crate::chat_provider::revision(),
        },
    );
    crate::acp::sessions::open(&st.out, &sid, &request.cwd, "embedded");
    if let Err(error) = activate(st, &sid, &initial) {
        st.chat.sessions.lock().unwrap().remove(&sid);
        return Err(error);
    }
    *st.last_session.lock().unwrap() = Some(sid.clone());
    let mut response = st
        .chat
        .sessions
        .lock()
        .unwrap()
        .get(&sid)
        .and_then(|s| s.response.clone())
        .unwrap_or_else(|| NewSessionResponse::new(sid.clone()));
    response.session_id = sid_of(&sid);
    response.config_options = Some(options(st, &sid));
    Ok(response)
}
pub(super) fn load(
    st: &Arc<ProxyState>,
    req: agent_client_protocol::schema::v1::LoadSessionRequest,
) -> Result<LoadSessionResponse> {
    let sid = req.session_id.to_string();
    if st
        .chat
        .sessions
        .lock()
        .unwrap()
        .get(&sid)
        .is_some_and(|s| s.busy)
    {
        return Err("Finish or cancel the current turn before loading this conversation.".into());
    }
    let records = crate::acp::sessions::read(&st.out, &sid);
    let mut request = NewSessionRequest::new(req.cwd).mcp_servers(req.mcp_servers);
    request.meta = req.meta;
    new(st, request, Some(sid.clone()))?;
    let transcript = if records.iter().any(|r| r["kind"] == "chat_turn") {
        records
            .iter()
            .filter(|r| r["kind"] == "chat_turn")
            .flat_map(|r| {
                [
                    ("user".into(), r["user"].as_str().unwrap_or_default().into()),
                    (
                        "assistant".into(),
                        r["assistant"].as_str().unwrap_or_default().into(),
                    ),
                ]
            })
            .collect()
    } else {
        records
            .iter()
            .filter_map(|r| {
                if r["kind"] == "user" {
                    r["text"]
                        .as_str()
                        .filter(|s| !s.starts_with('/'))
                        .map(|text| ("user".into(), text.into()))
                } else if r["kind"] == "update"
                    && r["update"]["sessionUpdate"] == "agent_message_chunk"
                {
                    r["update"]["content"]["text"]
                        .as_str()
                        .map(|text| ("assistant".into(), text.into()))
                } else {
                    None
                }
            })
            .collect()
    };
    {
        let mut sessions = st.chat.sessions.lock().unwrap();
        let s = sessions.get_mut(&sid).unwrap();
        s.transcript = transcript;
        s.continuation = true;
    }
    if let Some(selected) = records.iter().rev().find_map(|r| {
        (r["kind"] == "selection")
            .then(|| r["choice"].as_str())
            .flatten()
    }) {
        if let Some((backend, _)) = selected.split_once('/') {
            select(st, &sid, backend)?;
        }
        select(st, &sid, selected)?;
    }
    if let Some(up) = st.up.get() {
        for update in replay_conversation(&records) {
            let _ = up.send_notification(SessionNotification::new(sid_of(&sid), update));
        }
    }
    if records.is_empty() {
        say(
            st,
            &sid,
            "No recorded conversation history was found; continuing fresh.",
        );
    }
    publish(st, &sid);
    let response = st
        .chat
        .sessions
        .lock()
        .unwrap()
        .get(&sid)
        .and_then(|s| s.response.clone());
    Ok(LoadSessionResponse::new()
        .config_options(options(st, &sid))
        .modes(response.and_then(|r| r.modes)))
}
pub(super) fn record(st: &ProxyState, sid: &str, update: &SessionUpdate) {
    if let SessionUpdate::AgentMessageChunk(chunk) = update {
        if let ContentBlock::Text(text) = &chunk.content {
            if let Some(s) = st.chat.sessions.lock().unwrap().get_mut(sid) {
                s.answer.push_str(&text.text);
            }
        }
    }
}
fn say(st: &ProxyState, sid: &str, text: impl Into<String>) {
    let text = text.into();
    let update = SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from(text)));
    if let Ok(value) = serde_json::to_value(&update) {
        crate::acp::sessions::record_update(&st.out, sid, &value);
    }
    if let Some(up) = st.up.get() {
        let _ = up.send_notification(SessionNotification::new(sid_of(sid), update));
    }
}
pub(super) fn prompt(st: &Arc<ProxyState>, mut req: PromptRequest) -> Result<PromptResponse> {
    let sid = req.session_id.to_string();
    *st.last_session.lock().unwrap() = Some(sid.clone());
    if sid.starts_with("jazyk-run-") {
        say(
            st,
            &sid,
            "This is a read-only record. Start a conversation to talk to an agent.",
        );
        return Ok(PromptResponse::new(StopReason::EndTurn));
    }
    let text = req
        .prompt
        .iter()
        .filter_map(|b| {
            if let ContentBlock::Text(t) = b {
                Some(t.text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim() == "/jazyk" || text.trim().starts_with("/jazyk ") {
        let reply = configure(
            st,
            &sid,
            text.trim()
                .strip_prefix("/jazyk")
                .unwrap_or_default()
                .trim(),
        );
        say(st, &sid, reply);
        return Ok(PromptResponse::new(StopReason::EndTurn));
    }
    if let Some((cmd, args)) =
        crate::acp::commands::split(&text, st.in_project()).filter(|(cmd, _)| {
            !["login", "setup"].contains(cmd) || {
                let source = choice(st, &sid);
                source == "setup" || source == "cloud" || source.starts_with("endpoint")
            }
        })
    {
        if choice(st, &sid) == "setup"
            && !["help", "agent", "model", "login", "setup"].contains(&cmd)
        {
            say(st,&sid,"Setup only configures chat providers. Select Cloud, Claude Code, Codex or an endpoint to work on your project.");
        } else {
            let _busy = if ["model", "agent", "login", "setup"].contains(&cmd) {
                None
            } else {
                Some(reserve(st, &sid)?)
            };
            say(
                st,
                &sid,
                run_command(st, cmd, args, st.up.get().ok_or("initialize first")?, &sid),
            );
        }
        return Ok(PromptResponse::new(StopReason::EndTurn));
    }
    let _busy = reserve(st, &sid)?;
    let selected = choice(st, &sid);
    let source = selected.split('/').next().unwrap_or(&selected);
    if source == "setup" {
        // A completed browser setup can activate its saved provider without restarting the conversation.
        let setup_changed = st
            .chat
            .sessions
            .lock()
            .unwrap()
            .get(&sid)
            .is_some_and(|s| s.setup_revision != crate::chat_provider::revision());
        if let Some(saved) = crate::chat_provider::selected().filter(|_| setup_changed) {
            activate(st, &sid, &saved)?;
            publish(st, &sid);
        } else {
            let lower = text.to_lowercase();
            if lower.contains("cloud") || lower.contains("login") || lower.contains("sign in") {
                activate(st, &sid, "cloud")?;
                publish(st, &sid);
                say(
                    st,
                    &sid,
                    crate::chat_provider::begin(&cwd(st, &sid), "cloud"),
                );
                st.chat
                    .sessions
                    .lock()
                    .unwrap()
                    .get_mut(&sid)
                    .unwrap()
                    .auth_opened = true;
            } else if lower.contains("endpoint") || lower.contains("openai") {
                say(
                    st,
                    &sid,
                    crate::chat_provider::begin(&cwd(st, &sid), "endpoint"),
                );
            } else {
                say(st, &sid, status(st, &sid));
            }
            return Ok(PromptResponse::new(StopReason::EndTurn));
        }
    }
    let selected = choice(st, &sid);
    let source = selected.split('/').next().unwrap_or(&selected);
    if source == "cloud" || source.starts_with("endpoint") {
        let credentials = crate::chat_provider::for_source_configured(
            &st.llm.lock().unwrap(),
            source,
            st.chat_explicit,
        );
        if let Err(message) = credentials {
            let reply = {
                let mut sessions = st.chat.sessions.lock().unwrap();
                let s = sessions.get_mut(&sid).unwrap();
                authentication_reply(&mut s.auth_opened, &message, || {
                    crate::chat_provider::begin(&s.request.cwd, source)
                })
            };
            say(st, &sid, reply);
            return Ok(PromptResponse::new(StopReason::EndTurn));
        }
        if !st.routes.lock().unwrap().contains_key(&sid) {
            activate(st, &sid, &selected)?;
            publish(st, &sid);
        }
    }
    let (transcript, continuation) = {
        let mut sessions = st.chat.sessions.lock().unwrap();
        let s = sessions.get_mut(&sid).unwrap();
        s.answer.clear();
        (s.transcript.clone(), s.continuation)
    };
    if continuation && !transcript.is_empty() {
        let history = transcript
            .iter()
            .map(|(role, text)| format!("{role}: {text}\n"))
            .collect::<String>();
        let recent = history
            .char_indices()
            .rev()
            .nth(65535)
            .map_or(history.as_str(), |(i, _)| &history[i..]);
        let context = crate::payload::fill(
            include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/runtime/acp-continuation.md.mustache"
            )),
            &[("transcript", recent), ("message", text.as_str())],
        );
        req.prompt
            .retain(|block| !matches!(block, ContentBlock::Text(_)));
        req.prompt.insert(0, ContentBlock::from(context));
    }
    if let Some(fence) = st.runtime_fences.lock().unwrap().get(&sid) {
        let _ = std::fs::remove_file(fence);
    }
    let first = crate::acp::sessions::turns(&st.out, &sid) == 0;
    crate::acp::sessions::record_prompt(&st.out, &sid, &text);
    if first {
        if let (Some(up), Some(meta)) = (
            st.up.get(),
            crate::acp::sessions::list(&st.out)
                .into_iter()
                .find(|s| s.id == sid),
        ) {
            let _ = up.send_notification(SessionNotification::new(
                sid_of(&sid),
                SessionUpdate::SessionInfoUpdate(
                    agent_client_protocol::schema::v1::SessionInfoUpdate::new().title(meta.title),
                ),
            ));
        }
    }
    req.session_id = sid_of(&st.route_down(&sid));
    // A human turn can take arbitrarily long; only discovery and transitions have a timeout.
    let result = futures::executor::block_on(
        st.down_of(&sid)
            .map_err(|e| e.to_string())?
            .send_request(req)
            .block_task(),
    )
    .map_err(|e| e.to_string());
    if result
        .as_ref()
        .is_ok_and(|r| r.stop_reason != StopReason::Refusal)
    {
        let mut sessions = st.chat.sessions.lock().unwrap();
        let Some(s) = sessions.get_mut(&sid) else {
            return result;
        };
        let answer = std::mem::take(&mut s.answer);
        let _ = crate::acp::sessions::append_checked(
            &st.out,
            &sid,
            json!({"kind":"chat_turn","user":text,"assistant":answer}),
        );
        s.transcript.push(("user".into(), text));
        s.transcript.push(("assistant".into(), answer));
        s.continuation = false;
    }
    result
}
fn cwd(st: &ProxyState, sid: &str) -> std::path::PathBuf {
    st.chat
        .sessions
        .lock()
        .unwrap()
        .get(sid)
        .map(|s| s.request.cwd.clone())
        .unwrap_or_default()
}
pub(super) fn remove(st: &ProxyState, sid: &str) {
    st.chat.sessions.lock().unwrap().remove(sid);
    st.session_agents.lock().unwrap().remove(sid);
    st.routes.lock().unwrap().remove(sid);
    st.runtime_fences.lock().unwrap().remove(sid);
}

fn authentication_reply(
    opened: &mut bool,
    message: &str,
    launch: impl FnOnce() -> String,
) -> String {
    if *opened {
        format!("{message} Finish setup in your browser, then send your message again.")
    } else {
        *opened = true;
        launch()
    }
}
pub(super) fn backend_options(st: &ProxyState, sid: &str, options: Vec<SessionConfigOption>) {
    let selected = choice(st, sid);
    learn(
        st,
        &st.agent_of(sid),
        selected.split('/').next().unwrap_or(&selected),
        &NewSessionResponse::new(sid.to_string()).config_options(options.clone()),
    );
    if let Some(session) = st.chat.sessions.lock().unwrap().get_mut(sid) {
        session.options = options;
    }
}
pub(super) fn setup(st: &Arc<ProxyState>, sid: &str, source: &str) -> String {
    let selected = if source == "cloud" { "cloud" } else { "setup" };
    if let Err(error) = select(st, sid, selected) {
        return error;
    }
    if let Some(session) = st.chat.sessions.lock().unwrap().get_mut(sid) {
        session.auth_opened = true;
    }
    crate::chat_provider::begin(&cwd(st, sid), source)
}
pub(super) fn help(st: &ProxyState, sid: &str) -> String {
    let setup = choice(st, sid) == "setup";
    let mut text = format!("Current conversation: {}.\n\n", choice(st, sid));
    for command in crate::acp::commands::available(st.in_project())
        .filter(|c| !setup || ["help", "model", "agent", "login", "setup"].contains(&c.name))
    {
        let description = match command.name {
            "model" => "select this conversation's provider/model",
            "agent" => "switch this conversation's agent",
            _ => command.description,
        };
        text.push_str(&format!("/{}: {}\n", command.name, description));
    }
    text
}

pub(super) fn status(st: &ProxyState, sid: &str) -> String {
    let entries = crate::chat_provider::entries();
    let signed = entries
        .iter()
        .any(|(id, _, _, valid)| id == "cloud" && *valid);
    let mut text = format!(
        "Current: {}\nJazyk Cloud: {}\n",
        choice(st, sid),
        if signed {
            "signed in (saved grant)"
        } else {
            "signed out"
        }
    );
    for agent in ["claude", "codex", "opencode"] {
        text.push_str(&format!(
            "{}: {}\n",
            agent,
            if crate::setup::which(agent).is_some() {
                "installed (sign-in managed by agent)"
            } else {
                "not installed"
            }
        ));
    }
    let endpoints = entries
        .into_iter()
        .filter(|(id, _, _, _)| id.starts_with("endpoint"))
        .collect::<Vec<_>>();
    if endpoints.is_empty() {
        text.push_str("OpenAI endpoints: none\n");
    }
    for (id, url, model, _) in endpoints {
        text.push_str(&format!(
            "{}: {} · {}\n",
            id.strip_prefix("endpoint:").unwrap_or("default"),
            url,
            model
        ));
    }
    text.push_str("\n/jazyk login\n/jazyk logout\n/jazyk openai add NAME\n/jazyk openai remove NAME\n/jazyk status");
    text
}
fn configure(st: &Arc<ProxyState>, sid: &str, args: &str) -> String {
    let words = args.split_whitespace().collect::<Vec<_>>();
    match words.as_slice() {
        [] | ["status"] => status(st, sid),
        ["login"] => setup(st, sid, "cloud"),
        ["logout"] => match crate::chat_provider::remove("cloud") {
            Ok(()) => {
                publish(st, sid);
                status(st, sid)
            }
            Err(e) => e,
        },
        ["openai", "add", name]
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') =>
        {
            setup(st, sid, &format!("endpoint:{name}"))
        }
        ["openai", "remove", name] => match crate::chat_provider::remove(&if *name == "default" {
            "endpoint".into()
        } else {
            format!("endpoint:{name}")
        }) {
            Ok(()) => {
                st.chat.models.lock().unwrap().clear();
                publish(st, sid);
                status(st, sid)
            }
            Err(e) => e,
        },
        _ => status(st, sid),
    }
}
pub(super) fn watch(st: &Arc<ProxyState>) {
    let weak = Arc::downgrade(st);
    std::thread::spawn(move || {
        let mut revision = crate::chat_provider::revision();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let Some(st) = weak.upgrade() else {
                break;
            };
            let next = crate::chat_provider::revision();
            if revision == next {
                continue;
            }
            revision = next;
            st.chat.models.lock().unwrap().clear();
            let changed_sid = st.chat.sessions.lock().unwrap().keys().next().cloned();
            if let Some(sid) = changed_sid {
                publish(&st, &sid);
            }
            for (source, _, _, valid) in crate::chat_provider::entries()
                .into_iter()
                .filter(|(id, _, _, valid)| id.starts_with("endpoint") && *valid)
            {
                let _ = valid;
                let llm = crate::chat_provider::for_source_configured(
                    &st.llm.lock().unwrap(),
                    &source,
                    false,
                );
                if let Ok(llm) = llm {
                    let names = llm.list_models();
                    let mut models = st.chat.models.lock().unwrap();
                    for model in names {
                        models.insert(format!("{source}/{model}"), format!("{source}/{model}"));
                    }
                }
            }
            let sid = st.chat.sessions.lock().unwrap().keys().next().cloned();
            if let Some(sid) = sid {
                publish(&st, &sid);
            }
        }
    });
}
#[cfg(test)]
mod tests {
    #[test]
    fn missing_cloud_grant_opens_browser_once_then_waits_for_sign_in() {
        let mut opened = false;
        assert_eq!(
            super::authentication_reply(&mut opened, "Use /login.", || "Browser opened".into()),
            "Browser opened"
        );
        let reply = super::authentication_reply(&mut opened, "Use /login.", || {
            panic!("must not reopen browser")
        });
        assert!(reply.contains("/login") && reply.contains("browser"));
    }
}
