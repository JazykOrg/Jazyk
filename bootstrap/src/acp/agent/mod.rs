// `jazyk agent`: the embedded ACP agent. A minimal, generic agent over the configured
// OpenAI-compatible endpoint: it connects to whatever MCP servers the session names,
// offers their tools to the model, and streams updates. It knows nothing about jazyk;
// the same session against an external agent carries the same prompt and tools.
// Mirrors docs/frontends/acp.md#the-embedded-agent.
pub mod mcp_client;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, CloseSessionRequest, CloseSessionResponse, ContentBlock,
    ContentChunk, InitializeRequest, InitializeResponse, McpServer, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, SessionCapabilities, SessionConfigKind,
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelect,
    SessionConfigSelectOption, SessionNotification, SessionUpdate, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, StopReason, ToolCall, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, UsageUpdate,
};
use agent_client_protocol::{Agent, Stdio};

use crate::harness::{self, AgentEvent, LoopArgs, Stop};
use mcp_client::{GenericTool, McpServerConn};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

struct SessionState {
    conns: Vec<McpServerConn>,
    // Skills `read_skill` reads (docs/frontends/agent-context.md#skills).
    skills: Vec<crate::agent_context::Skill>,
    tools: Vec<GenericTool>,
    // The running conversation, kept across prompts so a chat session has memory.
    history: Vec<Value>,
    #[allow(dead_code)]
    cwd: std::path::PathBuf,
    // The model this session prompts, which the client may change between turns.
    // Mirrors docs/frontends/acp.md#choosing-a-model.
    model: String,
    conversation: bool,
}

// The id the model selector carries. A client persists a user's default under this
// id and replays it on the next session, so it must not drift.
const MODEL_OPTION: &str = "model";

// The models this endpoint offers, as one select option carrying the current choice.
// Mirrors docs/frontends/acp.md#choosing-a-model.
fn model_options(llm: &crate::llm::Llm, current: &str) -> Vec<SessionConfigOption> {
    let names = llm.list_models();
    let options: Vec<SessionConfigSelectOption> = names
        .iter()
        .map(|n| SessionConfigSelectOption::new(n.clone(), n.clone()))
        .collect();
    if options.is_empty() {
        return Vec::new();
    }
    vec![SessionConfigOption::new(
        MODEL_OPTION,
        "Model",
        SessionConfigKind::Select(SessionConfigSelect::new(current.to_string(), options)),
    )
    .description(format!("Served by {}", llm.base_url))
    .category(SessionConfigOptionCategory::Model)]
}

#[derive(Clone)]
struct SessionEntry {
    state: Arc<Mutex<SessionState>>,
    cancelled: Arc<AtomicBool>,
    inbox: Arc<Mutex<(bool, Vec<Value>)>>,
}

type Sessions = Arc<Mutex<HashMap<String, SessionEntry>>>;

fn initial_history<'a>(instructions: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<Value> {
    let mut history = Vec::new();
    for (name, instructions) in instructions {
        harness::push_user_text(
            &mut history,
            &format!("MCP server `{name}` instructions:\n{instructions}"),
        );
    }
    history
}

fn session_tools<'a>(servers: impl IntoIterator<Item = (&'a str, &'a [GenericTool])>)
    -> Result<Vec<GenericTool>, String> {
    let mut names = std::collections::BTreeMap::new();
    let mut tools = Vec::new();
    for (server, definitions) in servers {
        for tool in definitions {
            if let Some(previous) = names.insert(tool.name.as_str(), server) {
                return Err(format!("MCP tool name `{}` is provided by both `{previous}` and `{server}`; injected tool names must be unique", tool.name));
            }
            tools.push(tool.clone());
        }
    }
    Ok(tools)
}

struct ConnectedTools<'a>(&'a mut [McpServerConn], &'a [crate::agent_context::Skill]);
impl harness::ToolHost for ConnectedTools<'_> {
    fn catalog(&mut self) -> harness::LocalFuture<'_, Result<Option<Vec<GenericTool>>, String>> {
        Box::pin(async move {
            for connection in self.0.iter_mut() { connection.refresh_tools()?; }
            let mut tools = session_tools(self.0.iter().map(|c| (c.name.as_str(), c.tools.as_slice())))?;
            if !self.1.is_empty() && !tools.iter().any(|t| t.name == crate::agent_context::READ_SKILL) {
                tools.push(crate::agent_context::read_skill_tool());
            }
            Ok(Some(tools))
        })
    }

    fn call<'a>(&'a mut self, name: &'a str, args: &'a Value) -> harness::LocalFuture<'a, Result<String, String>> {
        Box::pin(async move {
            match self.0.iter_mut().find(|c| c.tools.iter().any(|t| t.name == name)) {
                Some(connection) => connection.call(name, args),
                None if name == crate::agent_context::READ_SKILL && !self.1.is_empty() => {
                    crate::agent_context::read_skill(self.1, args["name"].as_str().unwrap_or_default())
                }
                None => Err(format!("unknown tool `{name}`")),
            }
        })
    }
}

fn connect(name: &str, transport: &crate::agent_context::Transport, cwd: &std::path::Path) -> Result<McpServerConn, String> {
    match transport {
        crate::agent_context::Transport::Stdio { command, args, env } => McpServerConn::spawn(name, command, args, env, cwd),
        crate::agent_context::Transport::Http { url, headers } => McpServerConn::connect_http(name, url, headers),
    }
}

struct Opened {
    conns: Vec<McpServerConn>,
    tools: Vec<GenericTool>,
    history: Vec<Value>,
    skills: Vec<crate::agent_context::Skill>,
}

// A session's servers, tools, and opening context. The client's servers must all
// connect. Outside worker sessions the project's and the user's agent context joins
// them, each discovered part skipped with a warning when it cannot be used.
// Mirrors docs/frontends/acp.md#the-embedded-agent and docs/frontends/agent-context.md.
fn open_session(servers: &[McpServer], cwd: &std::path::Path, context: Option<crate::agent_context::SessionContext>) -> Result<Opened, String> {
    use crate::agent_context::Transport;
    let mut conns: Vec<McpServerConn> = Vec::new();
    let mut client_names = std::collections::BTreeSet::new();
    for server in servers {
        let (name, transport) = match server {
            McpServer::Stdio(s) => (s.name.clone(), Transport::Stdio {
                command: s.command.to_string_lossy().to_string(),
                args: s.args.clone(),
                env: s.env.iter().map(|e| (e.name.clone(), e.value.clone())).collect(),
            }),
            McpServer::Http(h) => (h.name.clone(), Transport::Http {
                url: h.url.clone(),
                headers: h.headers.iter().map(|h| (h.name.clone(), h.value.clone())).collect(),
            }),
            _ => return Err("only stdio and streamable HTTP MCP servers are supported".into()),
        };
        conns.push(connect(&name, &transport, cwd).map_err(|e| format!("mcp server `{name}`: {e}"))?);
        client_names.insert(name);
    }
    let mut tools = session_tools(conns.iter().map(|c| (c.name.as_str(), c.tools.as_slice())))?;
    let mut history = initial_history(conns.iter().filter_map(|c| c.instructions.as_deref().map(|i| (c.name.as_str(), i))));
    let Some(context) = context else {
        return Ok(Opened { conns, tools, history, skills: Vec::new() });
    };
    let mut warnings = context.warnings;
    for server in context.servers {
        if client_names.contains(&server.name) {
            continue;
        }
        let conn = match connect(&server.name, &server.transport, cwd) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(format!("MCP server `{}` ({}) skipped: {e}", server.name, server.source));
                continue;
            }
        };
        let clash: Vec<String> = conn.tools.iter().map(|t| t.name.clone())
            .filter(|n| tools.iter().any(|t| &t.name == n) || n == crate::agent_context::READ_SKILL).collect();
        if !clash.is_empty() {
            warnings.push(format!("MCP server `{}` ({}) skipped: its tool names collide with tools already offered ({})", server.name, server.source, clash.join(", ")));
            continue;
        }
        tools.extend(conn.tools.iter().cloned());
        if let Some(i) = conn.instructions.as_deref() {
            harness::push_user_text(&mut history, &format!("MCP server `{}` instructions:\n{i}", conn.name));
        }
        conns.push(conn);
    }
    let mut skills = context.skills;
    if !skills.is_empty() && tools.iter().any(|t| t.name == crate::agent_context::READ_SKILL) {
        warnings.push("skills skipped: a client MCP server already offers a `read_skill` tool".into());
        skills.clear();
    }
    if !skills.is_empty() {
        tools.push(crate::agent_context::read_skill_tool());
    }
    for text in [context.instructions, crate::agent_context::skills_text(&skills), crate::agent_context::warnings_text(&warnings)].into_iter().flatten() {
        harness::push_user_text(&mut history, &text);
    }
    for w in &warnings {
        eprintln!("jazyk agent: {w}");
    }
    Ok(Opened { conns, tools, history, skills })
}

pub fn run() -> i32 {
    // The endpoint is machine configuration (env, project [llm], global config), the
    // same ladder every jazyk command uses. Reading it is not jazyk knowledge; the
    // agent stays generic.
    let (_proj, llm, _out) = crate::cli::resolve(&[], &crate::cli::Options::default());
    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
    let counter = Arc::new(AtomicU64::new(0));

    let init_sessions = sessions.clone();
    let new_sessions = sessions.clone();
    let prompt_sessions = sessions.clone();
    let config_sessions = sessions.clone();
    let close_sessions = sessions.clone();
    let steering_sessions = sessions.clone();
    let cancel_sessions = sessions;
    let new_llm = llm.clone();
    let config_llm = llm.clone();

    let result = futures::executor::block_on(
        Agent
            .builder()
            .name("jazyk-embedded")
            .on_receive_request(
                async move |req: InitializeRequest, responder, _cx| {
                    let _ = &init_sessions;
                    responder.respond(
                        InitializeResponse::new(req.protocol_version).meta(serde_json::from_value::<agent_client_protocol::schema::v1::Meta>(json!({"steering":{"supported":true,"idleBehavior":"promptRequired","inputDeliveryEvents":true}})).ok()).agent_capabilities(
                            // session/close matters: closing a session tears its MCP
                            // servers down, and an ephemeral jazyk serving runs its
                            // implicit finish on that EOF.
                            AgentCapabilities::new().session_capabilities(
                                SessionCapabilities::new().close(
                                    agent_client_protocol::schema::v1::SessionCloseCapabilities::new(),
                                ),
                            ),
                        ),
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |req: NewSessionRequest, responder, _cx| {
                    let id = format!(
                        "sess-{}-{}",
                        crate::sys::process_id(),
                        counter.fetch_add(1, Ordering::Relaxed)
                    );
                    let cwd = req.cwd.clone();
                    let purpose = req.meta.as_ref().and_then(|m| m.get("jazyk")).and_then(|m| m["purpose"].as_str().map(str::to_owned));
                    // Worker sessions stay hermetic: the batch prompt and the serving only.
                    let context = (purpose.as_deref() != Some("worker")).then(|| crate::agent_context::native(&cwd));
                    let Opened { conns, tools, history, skills } = match open_session(&req.mcp_servers, &cwd, context) {
                        Ok(opened) => opened,
                        Err(error) => return responder.respond_with_internal_error(error),
                    };
                    let model = new_llm.model.clone();
                    new_sessions.lock().unwrap().insert(
                        id.clone(),
                        SessionEntry {
                            state: Arc::new(Mutex::new(SessionState {
                                conns,
                                skills,
                                tools,
                                history,
                                cwd,
                                model: model.clone(),
                                conversation: req.meta.as_ref().and_then(|m| m.get("jazyk")).is_some_and(|m| m["purpose"] == "chat"),
                            })),
                            cancelled: Arc::new(AtomicBool::new(false)),
                            inbox: Arc::new(Mutex::new((false, Vec::new()))),
                        },
                    );
                    let options = model_options(&new_llm, &model);
                    let mut resp = NewSessionResponse::new(id);
                    if !options.is_empty() {
                        resp = resp.config_options(options);
                    }
                    responder.respond(resp)
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |req: PromptRequest, responder, cx| {
                    let sid = req.session_id.clone();
                    let entry = match prompt_sessions.lock().unwrap().get(sid.0.as_ref()).cloned() {
                        Some(e) => e,
                        None => {
                            return responder
                                .respond_with_internal_error(format!("unknown session {}", sid));
                        }
                    };
                    {
                        let mut inbox = entry.inbox.lock().unwrap();
                        if inbox.0 { return responder.respond_with_internal_error("a prompt is already active"); }
                        inbox.0 = true;
                    }
                    entry.cancelled.store(false, Ordering::Relaxed);
                    let text: String = req
                        .prompt
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let llm = llm.clone();
                    // The blocking loop (HTTP calls, subprocess dispatch) runs on its
                    // own thread; updates are queued straight onto the connection.
                    std::thread::spawn(move || {
                        let notify = |update: SessionUpdate| {
                            let _ = cx.send_notification(SessionNotification::new(sid.clone(), update));
                        };
                        // A panic anywhere in the turn answers the prompt as a refusal
                        // carrying the panic text, never a dropped responder the client
                        // waits on until the idle timeout. A poisoned session lock is
                        // taken anyway: what it guards is the history.
                        // Mirrors docs/frontends/acp.md#the-embedded-agent.
                        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut state = entry.state.lock().unwrap_or_else(|p| p.into_inner());
                        // The session's model, which the client may have changed since
                        // the last turn. Mirrors docs/frontends/acp.md#choosing-a-model.
                        let llm = crate::llm::Llm { model: state.model.clone(), ..llm };
                        harness::push_user_text(&mut state.history,&text);
                        let cancelled = entry.cancelled.clone();
                        let mut total_tokens = 0u64;
                        let label = format!("agent {}", sid);
                        let state = &mut *state;
                        let mut tools = ConnectedTools(&mut state.conns, &state.skills);
                        let mut model = harness::provider::DefaultModelHost::new(&llm);
                        let mut recovery = harness::MemoryRecovery;
                        let mut dispatch = |_: &str, _: &Value| Err("tool host unavailable".into());
                        let mut emit = |ev: AgentEvent| match ev {
                            AgentEvent::Input(input) => {
                                notify(SessionUpdate::UserMessageChunk(ContentChunk::new(ContentBlock::from(String::new())).meta(serde_json::from_value::<agent_client_protocol::schema::v1::Meta>(json!({"jazyk":{"input":input,"state":"delivered"}})).ok())));
                            }
                            AgentEvent::Timing(timing) => {
                                notify(SessionUpdate::UsageUpdate(UsageUpdate::new(0,0).meta(serde_json::from_value::<agent_client_protocol::schema::v1::Meta>(json!({"jazyk":{"timing":timing}})).ok())));
                            }
                            AgentEvent::ReasoningSummary(s) => notify(SessionUpdate::AgentThoughtChunk(
                                ContentChunk::new(ContentBlock::from(s)).meta(serde_json::from_value::<agent_client_protocol::schema::v1::Meta>(json!({"jazyk":{"reasoningSummary":true}})).ok()),
                            )),
                            AgentEvent::Thought(s) => notify(SessionUpdate::AgentThoughtChunk(
                                ContentChunk::new(ContentBlock::from(s)),
                            )),
                            AgentEvent::Message(s) => notify(SessionUpdate::AgentMessageChunk(
                                ContentChunk::new(ContentBlock::from(s)),
                            )),
                            AgentEvent::ToolCallStart { id, name, args } => {
                                let call = ToolCall::new(id, name.clone())
                                    .status(ToolCallStatus::InProgress)
                                    .raw_input(args);
                                notify(SessionUpdate::ToolCall(call));
                            }
                            AgentEvent::ToolCallEnd { id, result, ok } => {
                                let fields = ToolCallUpdateFields::new()
                                    .status(if ok { ToolCallStatus::Completed } else { ToolCallStatus::Failed })
                                    .raw_output(json!(result));
                                notify(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(id, fields)));
                            }
                            AgentEvent::Usage { tokens, cached } => {
                                total_tokens += tokens;
                                // `used`/`size` describe the context window, which an
                                // OpenAI-compatible endpoint does not report; the meta
                                // carries what it does: the tokens the round billed,
                                // input and output, and the input it served from its
                                // prompt cache (docs/compiler/sessions.md#budgets).
                                let usage = UsageUpdate::new(0, 0).meta(
                                    serde_json::from_value::<agent_client_protocol::schema::v1::Meta>(json!({
                                        "jazyk": {"tokens": tokens, "cachedTokens": cached, "totalTokens": total_tokens}
                                    }))
                                    .ok(),
                                );
                                notify(SessionUpdate::UsageUpdate(usage));
                            }
                        };
                        let mut inputs = || {
                            let mut inbox = entry.inbox.lock().unwrap();
                            if cancelled.load(Ordering::Relaxed) { inbox.1.clear(); }
                            std::mem::take(&mut inbox.1)
                        };
                        let mut input_close = || {
                            let mut inbox = entry.inbox.lock().unwrap();
                            if !inbox.1.is_empty() { return false; }
                            inbox.0 = false;
                            true
                        };
                        harness::run_with_hosts(LoopArgs {
                            llm: &llm,
                            history: &mut state.history,
                            tools: &state.tools,
                            dispatch: &mut dispatch,
                            emit: &mut emit,
                            cancelled: &|| cancelled.load(Ordering::Relaxed),
                            max_rounds: crate::limits::agent_max_rounds(),
                            label,
                            conversation: state.conversation,
                            inputs: Some(&mut inputs),
                            input_close: Some(&mut input_close),
                        }, "", &mut model, &mut tools, &mut recovery)
                        }));
                        let stop = match outcome {
                            Ok(stop) => stop,
                            Err(p) => Stop::Error(format!(
                                "the agent panicked: {}",
                                crate::sys::panic_text(p.as_ref())
                            )),
                        };
                        // Closing admission and draining use the same queue lock.
                        { let mut inbox = entry.inbox.lock().unwrap(); inbox.0 = false; inbox.1.clear(); }
                        let _ = match stop {
                            Stop::EndTurn => responder.respond(PromptResponse::new(StopReason::EndTurn)),
                            Stop::MaxRounds => {
                                responder.respond(PromptResponse::new(StopReason::MaxTurnRequests))
                            }
                            Stop::Cancelled => responder.respond(PromptResponse::new(StopReason::Cancelled)),
                            Stop::Suspended => {
                                notify(SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from("This native ACP session does not support suspension recovery."))));
                                responder.respond(PromptResponse::new(StopReason::Refusal))
                            },
                            // A loop failure answers as a refusal with the error in a
                            // message chunk, never as a protocol error: the client
                            // library treats an error response to session/prompt as
                            // fatal to the whole connection, so one rate-limited call
                            // would take down the host and every later session with it.
                            // Mirrors docs/frontends/acp.md#the-embedded-agent.
                            Stop::Error(e) => {
                                notify(SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                    ContentBlock::from(format!("agent error: {}", e)),
                                )));
                                responder.respond(PromptResponse::new(StopReason::Refusal))
                            }
                        };
                    });
                    Ok(())
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |req: super::steering::SteeringRequest, responder, _cx| {
                    let Some(entry) = steering_sessions.lock().unwrap().get(&req.session_id).cloned() else {
                        return responder.respond_with_internal_error("unknown session");
                    };
                    if req.meta.as_ref().and_then(|m|m.get("steering")).and_then(|m|m.get("idleBehavior")).is_some_and(|value|value != "promptRequired") {
                        return responder.respond_with_internal_error("unsupported idleBehavior; use promptRequired");
                    }
                    let mut texts = Vec::new();
                    for block in req.prompt {
                        match block {
                            ContentBlock::Text(t) => texts.push(t.text),
                            _ => return responder.respond_with_internal_error("steering supports text only"),
                        }
                    }
                    let text = texts.join("\n");
                    if text.trim().is_empty() { return responder.respond_with_internal_error("text is required"); }
                    let mut inbox = entry.inbox.lock().unwrap();
                    if !inbox.0 || entry.cancelled.load(Ordering::Relaxed) {
                        return responder.respond(json!({"outcome":"promptRequired","reason":"noRunningTurn"}));
                    }
                    let mut input = req.meta.as_ref().and_then(|m| m.get("jazyk")).cloned().unwrap_or_else(|| json!({}));
                    input["text"] = json!(text);
                    inbox.1.push(input);
                    responder.respond(json!({"outcome":"injected"}))
                },
                agent_client_protocol::on_receive_request!(),
            )
            // The client picked a model. It takes effect on the next prompt, and the
            // answer restates the whole option set, per the protocol.
            // Mirrors docs/frontends/acp.md#choosing-a-model.
            .on_receive_request(
                async move |req: SetSessionConfigOptionRequest, responder, _cx| {
                    let Some(entry) =
                        config_sessions.lock().unwrap().get(req.session_id.0.as_ref()).cloned()
                    else {
                        return responder.respond_with_internal_error("unknown session");
                    };
                    if req.config_id.0.as_ref() != MODEL_OPTION {
                        return responder
                            .respond_with_internal_error(format!("unknown option `{}`", req.config_id));
                    }
                    let Some(value) = req.value.as_value_id() else {
                        return responder.respond_with_internal_error("the model is named by value id");
                    };
                    let chosen = value.0.to_string();
                    let current = {
                        let mut state = entry.state.lock().unwrap();
                        state.model = chosen;
                        state.model.clone()
                    };
                    responder.respond(SetSessionConfigOptionResponse::new(model_options(
                        &config_llm,
                        &current,
                    )))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |req: CloseSessionRequest, responder, _cx| {
                    let entry = close_sessions.lock().unwrap().remove(req.session_id.0.as_ref());
                    match entry {
                        Some(e) => {
                            e.cancelled.store(true, Ordering::Relaxed);
                            // Dropping the state closes each MCP server's stdin and
                            // waits for it to exit, so an ephemeral jazyk serving has
                            // run its implicit finish before this response goes out.
                            // The wait blocks, so it runs off the dispatch task.
                            std::thread::spawn(move || {
                                drop(e);
                                let _ = responder.respond(CloseSessionResponse::new());
                            });
                            Ok(())
                        }
                        None => responder.respond(CloseSessionResponse::new()),
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_notification(
                async move |n: CancelNotification, _cx| {
                    if let Some(e) = cancel_sessions.lock().unwrap().get(n.session_id.0.as_ref()) {
                        e.cancelled.store(true, Ordering::Relaxed);
                        e.inbox.lock().unwrap().1.clear();
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
            eprintln!("jazyk agent: {}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_injected_mcp_tool_names_refuse_the_session() {
        let tool = GenericTool { name:"read_file".into(),description:"Read a file".into(),parameters:json!({"type":"object"}) };
        let first = vec![tool.clone()];
        let second = vec![tool];
        let error = session_tools([("project",first.as_slice()),("other",second.as_slice())]).unwrap_err();
        assert!(error.contains("read_file") && error.contains("project") && error.contains("other"));
        assert_eq!(session_tools([("project",first.as_slice())]).unwrap().len(),1);
    }

    fn fixture(tool: &str, instructions: &str) -> crate::agent_context::Transport {
        let script = format!(r#"import json,sys
for line in sys.stdin:
 m=json.loads(line)
 if 'id' not in m: continue
 if m['method']=='initialize': r={{'protocolVersion':m['params']['protocolVersion'],'capabilities':{{'tools':{{}}}},'serverInfo':{{'name':'f','version':'1'}},'instructions':'{instructions}'}}
 elif m['method']=='tools/list': r={{'tools':[{{'name':'{tool}','inputSchema':{{'type':'object'}}}}]}}
 else: r={{'content':[{{'type':'text','text':'ok'}}]}}
 print(json.dumps({{'jsonrpc':'2.0','id':m['id'],'result':r}}),flush=True)
"#);
        crate::agent_context::Transport::Stdio { command: "python3".into(), args: vec!["-c".into(), script], env: vec![] }
    }

    fn discovered(name: &str, transport: crate::agent_context::Transport) -> crate::agent_context::Server {
        crate::agent_context::Server { name: name.into(), source: ".mcp.json".into(), transport, project: true, hash: String::new() }
    }

    // Client servers come first and must connect; discovered ones join unless their
    // name is taken, they fail, or their tools collide. Worker sessions get none.
    // Mirrors docs/frontends/agent-context.md#mcp-servers.
    #[test]
    fn sessions_add_discovered_context_without_letting_it_break_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let crate::agent_context::Transport::Stdio { command, args, .. } = fixture("graph_status", "Client jazyk.") else { unreachable!() };
        let client = vec![McpServer::Stdio(agent_client_protocol::schema::v1::McpServerStdio::new("jazyk", command).args(args))];
        let skill_dir = dir.path().join(".agents/skills/s");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "---\nname: s\ndescription: d\n---\nDo it.").unwrap();
        let (skills, _) = crate::agent_context::skills(dir.path(), None);
        let context = crate::agent_context::SessionContext {
            instructions: Some("PROJECT INSTRUCTIONS: be brief".into()),
            servers: vec![
                discovered("jazyk", fixture("other", "Shadow.")),
                discovered("extra", fixture("extra_tool", "Extra server.")),
                discovered("broken", crate::agent_context::Transport::Stdio { command: "/nonexistent/mcp".into(), args: vec![], env: vec![] }),
                discovered("clash", fixture("graph_status", "Clash.")),
            ],
            skills,
            warnings: vec!["project skills were fine".into()],
        };
        let opened = open_session(&client, dir.path(), Some(context)).unwrap();
        let names: Vec<&str> = opened.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["graph_status", "extra_tool", "read_skill"]);
        let text = opened.history[0]["content"].as_str().unwrap();
        for part in ["Client jazyk.", "Extra server.", "PROJECT INSTRUCTIONS: be brief", "- s: d", "`broken` (.mcp.json) skipped", "`clash` (.mcp.json) skipped", "graph_status"] {
            assert!(text.contains(part), "{part}: {text}");
        }
        assert!(!text.contains("Shadow."), "a discovered jazyk never replaces the client's");
        let mut opened = opened;
        let mut host = ConnectedTools(&mut opened.conns, &opened.skills);
        let reply = futures::executor::block_on(harness::ToolHost::call(&mut host, "read_skill", &json!({"name": "s"}))).unwrap();
        assert!(reply.contains("Do it."));
        // A worker session reads nothing beyond its client servers.
        let worker = open_session(&client, dir.path(), None).unwrap();
        assert_eq!(worker.tools.len(), 1);
        assert_eq!(worker.history[0]["content"], "MCP server `jazyk` instructions:\nClient jazyk.");
    }

    #[test]
    fn initial_mcp_instructions_and_prompt_form_one_user_message() {
        let mut history = initial_history([
            ("project", "Read the project."),
            ("task", "Inspect released tasks."),
        ]);
        harness::push_user_text(&mut history, "Explain hosting.md.");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["role"], "user");
        assert_eq!(history[0]["content"],"MCP server `project` instructions:\nRead the project.\n\nMCP server `task` instructions:\nInspect released tasks.\n\nExplain hosting.md.");
        let call = json!({"role":"assistant","content":null,"tool_calls":[{"id":"read-1","type":"function","function":{"name":"read_document","arguments":"{}"}}]});
        let result = json!({"role":"tool","tool_call_id":"read-1","content":"current contents"});
        history.push(call.clone());
        history.push(result.clone());
        harness::push_user_text(&mut history, "Keep its existing heading.");
        assert_eq!(history.len(), 4);
        assert_eq!(history[1], call);
        assert_eq!(history[2], result);
        assert_eq!(
            history[3],
            json!({"role":"user","content":"Keep its existing heading."})
        );
    }

    // The picker an IDE renders: a select in the model category, carrying the
    // resolved model as its current value, under the id clients persist.
    // Mirrors docs/frontends/acp.md#choosing-a-model.
    #[test]
    fn the_model_option_offers_the_endpoint_and_keeps_its_id() {
        // An endpoint that cannot answer leaves exactly one honest choice.
        let llm = crate::llm::Llm { api: "chat_completions".into(), reasoning_summary: None,
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "some-model".into(),
            api_key: String::new(),
            temperature: None,
            trace: None,
        };
        let options = model_options(&llm, &llm.model);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].id.0.as_ref(), MODEL_OPTION);
        assert_eq!(
            options[0].category,
            Some(SessionConfigOptionCategory::Model)
        );
        let SessionConfigKind::Select(select) = &options[0].kind else {
            panic!("the model option is a select");
        };
        assert_eq!(select.current_value.0.as_ref(), "some-model");
        let json = serde_json::to_value(&options[0]).unwrap();
        assert_eq!(json["type"], "select");
        assert_eq!(json["options"][0]["value"], "some-model");

        // A model with no name at all offers nothing rather than an empty picker.
        let bare = crate::llm::Llm {
            model: String::new(),
            ..llm
        };
        assert!(model_options(&bare, "").is_empty());
    }
}
