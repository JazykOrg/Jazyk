// `jazyk agent`: the embedded ACP agent. A minimal, generic agent over the configured
// OpenAI-compatible endpoint: it connects to whatever MCP servers the session names,
// offers their tools to the model, and streams updates. It knows nothing about jazyk;
// the same session against an external agent carries the same prompt and tools.
// Mirrors docs/frontends/acp.md#the-embedded-agent.
pub mod agent_loop;
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

use agent_loop::{AgentEvent, LoopArgs, Stop};
use mcp_client::{GenericTool, McpServerConn};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

struct SessionState {
    conns: Vec<McpServerConn>,
    tools: Vec<GenericTool>,
    // The running conversation, kept across prompts so a chat session has memory.
    history: Vec<Value>,
    #[allow(dead_code)]
    cwd: std::path::PathBuf,
    // The model this session prompts, which the client may change between turns.
    // Mirrors docs/frontends/acp.md#choosing-a-model.
    model: String,
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
}

type Sessions = Arc<Mutex<HashMap<String, SessionEntry>>>;

// The round cap for one prompt turn. The client's own budgets are the real bound;
// this is the local runaway stop, reported as `max_turn_requests`.
fn max_rounds() -> u32 {
    std::env::var("JAZYK_AGENT_MAX_ROUNDS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(48)
        .max(1)
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
                        InitializeResponse::new(req.protocol_version).agent_capabilities(
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
                        std::process::id(),
                        counter.fetch_add(1, Ordering::Relaxed)
                    );
                    let cwd = req.cwd.clone();
                    let mut conns: Vec<McpServerConn> = Vec::new();
                    for server in &req.mcp_servers {
                        match server {
                            McpServer::Stdio(s) => {
                                let env: Vec<(String, String)> =
                                    s.env.iter().map(|e| (e.name.clone(), e.value.clone())).collect();
                                let command = s.command.to_string_lossy().to_string();
                                match McpServerConn::spawn(&s.name, &command, &s.args, &env, &cwd) {
                                    Ok(c) => conns.push(c),
                                    Err(e) => {
                                        return responder.respond_with_internal_error(format!(
                                            "mcp server `{}`: {}",
                                            s.name, e
                                        ));
                                    }
                                }
                            }
                            _ => {
                                return responder.respond_with_internal_error(
                                    "only stdio MCP servers are supported",
                                );
                            }
                        }
                    }
                    let tools: Vec<GenericTool> = conns.iter().flat_map(|c| c.tools.clone()).collect();
                    let model = new_llm.model.clone();
                    new_sessions.lock().unwrap().insert(
                        id.clone(),
                        SessionEntry {
                            state: Arc::new(Mutex::new(SessionState {
                                conns,
                                tools,
                                history: Vec::new(),
                                cwd,
                                model: model.clone(),
                            })),
                            cancelled: Arc::new(AtomicBool::new(false)),
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
                        let mut state = entry.state.lock().unwrap();
                        // The session's model, which the client may have changed since
                        // the last turn. Mirrors docs/frontends/acp.md#choosing-a-model.
                        let llm = crate::llm::Llm { model: state.model.clone(), ..llm };
                        state.history.push(json!({"role": "user", "content": text}));
                        let notify = |update: SessionUpdate| {
                            let _ = cx.send_notification(SessionNotification::new(sid.clone(), update));
                        };
                        let cancelled = entry.cancelled.clone();
                        let mut total_tokens = 0u64;
                        let label = format!("agent {}", sid);
                        let state = &mut *state;
                        let mut dispatch = |name: &str, args: &Value| -> Result<String, String> {
                            match state.conns.iter_mut().find(|c| c.tools.iter().any(|t| t.name == name)) {
                                Some(c) => c.call(name, args),
                                None => Err(format!("unknown tool `{}`", name)),
                            }
                        };
                        let mut emit = |ev: AgentEvent| match ev {
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
                            AgentEvent::Usage { tokens } => {
                                total_tokens += tokens;
                                // `used`/`size` describe the context window, which an
                                // OpenAI-compatible endpoint does not report; the meta
                                // carries what it does: completion tokens spent.
                                let usage = UsageUpdate::new(0, 0).meta(
                                    serde_json::from_value::<agent_client_protocol::schema::v1::Meta>(json!({
                                        "jazyk": {"completionTokens": tokens, "totalCompletionTokens": total_tokens}
                                    }))
                                    .ok(),
                                );
                                notify(SessionUpdate::UsageUpdate(usage));
                            }
                        };
                        let stop = agent_loop::run_loop(LoopArgs {
                            llm: &llm,
                            history: &mut state.history,
                            tools: &state.tools,
                            dispatch: &mut dispatch,
                            emit: &mut emit,
                            cancelled: &|| cancelled.load(Ordering::Relaxed),
                            max_rounds: max_rounds(),
                            label,
                        });
                        let _ = match stop {
                            Stop::EndTurn => responder.respond(PromptResponse::new(StopReason::EndTurn)),
                            Stop::MaxRounds => {
                                responder.respond(PromptResponse::new(StopReason::MaxTurnRequests))
                            }
                            Stop::Cancelled => responder.respond(PromptResponse::new(StopReason::Cancelled)),
                            Stop::Error(e) => responder.respond_with_internal_error(e),
                        };
                    });
                    Ok(())
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

    // The picker an IDE renders: a select in the model category, carrying the
    // resolved model as its current value, under the id clients persist.
    // Mirrors docs/frontends/acp.md#choosing-a-model.
    #[test]
    fn the_model_option_offers_the_endpoint_and_keeps_its_id() {
        // An endpoint that cannot answer leaves exactly one honest choice.
        let llm = crate::llm::Llm {
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
