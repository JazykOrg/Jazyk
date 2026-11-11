//! GUI-owned, immutable agent selections. Discovery never starts an agent.
use super::{api::err, state::SharedState};
use crate::{
    acp::config::{self, ResolvedAgent},
    llm::Llm,
};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct Choices {
    selections: Mutex<BTreeMap<String, Arc<Selection>>>,
    challenges: Mutex<BTreeMap<String, (String, std::time::Instant)>>,
}
pub struct Selection {
    pub id: String,
    pub agent: ResolvedAgent,
    pub llm: Llm,
    pub source: String,
    bridge: Mutex<BTreeMap<String, Pending>>,
}
struct Pending {
    body: Value,
    reply: Option<tokio::sync::oneshot::Sender<Result<Value, String>>>,
}
impl Selection {
    pub fn summary(&self) -> Value {
        json!({"id":self.id,"agent":self.agent.name,"source":self.source,"model":self.llm.model,"execution":"local"})
    }
}
impl Choices {
    pub fn get(&self, id: &str) -> Option<Arc<Selection>> {
        self.selections.lock().unwrap().get(id).cloned()
    }
}
fn profile(st: &SharedState, name: Option<&str>) -> Result<ResolvedAgent, String> {
    config::resolve_acp(
        name,
        &st.proj().acp,
        &crate::project::load_global_acp(),
        |n| std::env::var(n).ok(),
    )
}
fn agent_row(agent: ResolvedAgent) -> Value {
    let executable = match agent.name.as_str() {
        "embedded" => None,
        "codex" if agent.command == "npx" => Some("codex"),
        "claude" if agent.command == "npx" => Some("claude"),
        _ => Some(agent.command.as_str()),
    };
    let installed = executable.is_none_or(|cmd| crate::setup::which(cmd).is_some());
    let adapter = agent.command.is_empty() || crate::setup::which(&agent.command).is_some();
    json!({"id":agent.name,"installed":installed,"available":installed && adapter,"status":if !installed {"not-installed"} else if !adapter {"needs-adapter"} else if agent.name == "embedded" {"ready"} else {"login-unknown"},"reason":if !installed {"Install the coding agent on this computer."} else if !adapter {"Install the adapter launcher on this computer."} else {""},"login":"Managed by the coding agent; checked when it starts."})
}
pub async fn list(State(st): State<SharedState>) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        let mut names: Vec<String> = config::BUILTIN_AGENTS.iter().map(|a| a.0.to_string()).collect();
        names.extend(st.proj().acp.agents.keys().cloned());
        names.extend(crate::project::load_global_acp().agents.keys().cloned());
        names.sort(); names.dedup();
        let rows: Vec<Value> = names.iter().filter_map(|name| profile(&st, Some(name)).ok()).map(agent_row).collect();
        let current = st.agent_selection.as_ref().map(|s| s.summary()).unwrap_or_else(|| json!({"agent":profile(&st,None).map(|a|a.name).unwrap_or_default(),"source":"configured","execution":"local","model":st.llm().model}));
        json!({"agents":rows,"selection":current,"selectionProtocol":1})
    }).await;
    match result {
        Ok(v) => Json(v).into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "Agent discovery failed."),
    }
}
fn endpoint(body: &Value, mut llm: Llm) -> Result<Llm, String> {
    let url = body["baseUrl"]
        .as_str()
        .ok_or("Endpoint URL is required.")?;
    let parsed = reqwest::Url::parse(url).map_err(|_| "Invalid endpoint URL.")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Use an HTTP endpoint without URL credentials, query or fragment.".into());
    }
    let model = body["model"]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() < 256)
        .ok_or("Model is required.")?;
    let api = body["api"].as_str().unwrap_or("chat_completions");
    if !matches!(api, "chat_completions" | "responses") {
        return Err("Unsupported model API.".into());
    }
    llm.base_url = url.trim_end_matches('/').into();
    llm.model = model.into();
    llm.api = api.into();
    llm.api_key = body["apiKey"].as_str().unwrap_or_default().into();
    llm.reasoning_summary = body["reasoningSummary"].as_str().map(str::to_string);
    Ok(llm)
}
fn cloud_origin(body: &Value) -> Result<String, String> {
    let url = reqwest::Url::parse(
        body["cloudOrigin"]
            .as_str()
            .ok_or("Cloud origin is required.")?,
    )
    .map_err(|_| "Invalid Cloud origin.")?;
    if (url.scheme() != "https"
        && !(url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Invalid Cloud origin.".into());
    }
    Ok(url.origin().ascii_serialization())
}
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Cloud client")
}
pub async fn challenge(State(st): State<SharedState>) -> Response {
    let id = super::state::mint_token().unwrap();
    let proof = super::state::mint_token().unwrap();
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(proof.as_bytes()));
    let mut challenges = st.agent_choices.challenges.lock().unwrap();
    challenges.retain(|_, (_, at)| at.elapsed().as_secs() < 120);
    if challenges.len() >= 32 {
        return err(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many pending authorizations.",
        );
    }
    challenges.insert(id.clone(), (proof, std::time::Instant::now()));
    Json(json!({"id":id,"challenge":hash})).into_response()
}
pub async fn select(
    State(st): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let result: Result<Value, String> = async {
        let agent = profile(&st, body["agent"].as_str().or(Some("embedded")))?;
        if agent_row(agent.clone())["available"] != true {
            return Err(
                "This coding agent is not installed or its launcher is unavailable.".into(),
            );
        }
        let source = body["source"].as_str().unwrap_or("configured");
        if agent.name != "embedded" && source != "configured" {
            return Err("External agents use their own model configuration.".into());
        }
        let id = super::state::mint_token()?;
        let mut llm = st.llm();
        match source {
            "configured" => {}
            "endpoint" => llm = endpoint(&body, llm)?,
            "cloud" => {
                let origin = cloud_origin(&body)?;
                let challenge_id = body["challengeId"]
                    .as_str()
                    .ok_or("Cloud authorization is required.")?;
                let (proof, at) = st
                    .agent_choices
                    .challenges
                    .lock()
                    .unwrap()
                    .remove(challenge_id)
                    .ok_or("Authorization expired. Choose Cloud again.")?;
                if at.elapsed().as_secs() >= 120 {
                    return Err("Authorization expired. Choose Cloud again.".into());
                }
                let response = client()
                    .post(format!("{origin}/api/inference/exchange"))
                    .json(&json!({"proof":proof}))
                    .send()
                    .await
                    .map_err(|_| "Cloud authorization could not be reached.")?;
                if !response.status().is_success() {
                    return Err(
                        "Cloud authorization was refused. Sign in and authorize again.".into(),
                    );
                }
                let grant: Value = response
                    .json()
                    .await
                    .map_err(|_| "Invalid Cloud authorization response.")?;
                llm.api_key = grant["token"]
                    .as_str()
                    .ok_or("Invalid Cloud credential.")?
                    .into();
                llm.base_url = format!("{origin}/api/inference/v1");
                llm.model = "cloud".into();
                llm.api = "chat_completions".into();
                llm.reasoning_summary = None;
            }
            "browser" => {
                let port = headers
                    .get(header::HOST)
                    .and_then(|h| h.to_str().ok())
                    .and_then(|h| h.parse::<axum::http::uri::Authority>().ok())
                    .and_then(|h| h.port_u16())
                    .ok_or("Local listener port is required.")?;
                llm.base_url = format!("http://127.0.0.1:{port}/model-bridge/{id}");
                llm.api_key = super::state::mint_token()?;
                llm.model = "browser".into();
                llm.api = "chat_completions".into();
                llm.reasoning_summary = None;
            }
            _ => return Err("Unknown model source.".into()),
        }
        let selected = Arc::new(Selection {
            id: id.clone(),
            agent,
            llm,
            source: source.into(),
            bridge: Mutex::new(BTreeMap::new()),
        });
        let summary = selected.summary();
        let mut choices = st.agent_choices.selections.lock().unwrap();
        if choices.len() >= 128 {
            return Err("Too many selections. Restart the local connector to clear them.".into());
        }
        choices.insert(id, selected);
        Ok(summary)
    }
    .await;
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}
pub async fn bridge_request(
    State(st): State<SharedState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let Some(selection) = st.agent_choices.get(&id).filter(|s| s.source == "browser") else {
        return err(
            StatusCode::NOT_FOUND,
            "Unknown browser inference selection.",
        );
    };
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    if !super::state::token_matches(bearer, &selection.llm.api_key) {
        return err(StatusCode::UNAUTHORIZED, "Invalid bridge token.");
    }
    let request_id = super::state::mint_token().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    {
        let mut pending = selection.bridge.lock().unwrap();
        pending.retain(|_, p| p.reply.as_ref().is_some_and(|s| !s.is_closed()));
        if pending.len() >= 8 {
            return err(StatusCode::TOO_MANY_REQUESTS, "Browser inference is busy.");
        }
        pending.insert(
            request_id.clone(),
            Pending {
                body,
                reply: Some(tx),
            },
        );
    }
    let reply = tokio::time::timeout(std::time::Duration::from_secs(120), rx).await;
    selection.bridge.lock().unwrap().remove(&request_id);
    match reply {
        Ok(Ok(Ok(v))) => Json(v).into_response(),
        Ok(Ok(Err(e))) => err(StatusCode::BAD_GATEWAY, e),
        _ => err(
            StatusCode::GATEWAY_TIMEOUT,
            "Browser inference disconnected or timed out.",
        ),
    }
}
pub async fn bridge_poll(State(st): State<SharedState>, Path(id): Path<String>) -> Response {
    let Some(selection) = st.agent_choices.get(&id) else {
        return err(StatusCode::NOT_FOUND, "Unknown selection.");
    };
    let mut pending = selection.bridge.lock().unwrap();
    pending.retain(|_, p| p.reply.as_ref().is_some_and(|s| !s.is_closed()));
    Json(json!({"requests":pending.iter().map(|(id,p)|json!({"id":id,"body":p.body})).collect::<Vec<_>>()})).into_response()
}
pub async fn bridge_reply(
    State(st): State<SharedState>,
    Path((id, request)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let Some(selection) = st.agent_choices.get(&id) else {
        return err(StatusCode::NOT_FOUND, "Unknown selection.");
    };
    let pending = selection.bridge.lock().unwrap().remove(&request);
    match pending.and_then(|p| p.reply) {
        Some(tx) => {
            let reply = if body["error"].is_string() {
                Err("Browser inference failed or disconnected.".into())
            } else {
                Ok(body["response"].clone())
            };
            let _ = tx.send(reply);
            StatusCode::NO_CONTENT.into_response()
        }
        None => err(StatusCode::GONE, "Inference request is no longer active."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn selections_are_private_snapshots_and_do_not_coalesce_jobs() {
        let st = super::super::server::tests::state(None, "https://jazyk.dev");
        let request = |model: &str| json!({"agent":"embedded","source":"endpoint","baseUrl":"http://127.0.0.1:3625/v1","model":model,"apiKey":"private-fixture-key"});
        let response = select(State(st.clone()), HeaderMap::new(), Json(request("first"))).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("private-fixture-key"));
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let first = st.agent_choices.get(body["id"].as_str().unwrap()).unwrap();
        let mut first_state = st.scoped(None).unwrap();
        Arc::get_mut(&mut first_state).unwrap().agent_selection = Some(first.clone());
        assert_eq!(first_state.llm().model, "first");
        assert_eq!(
            config::resolve_acp(
                None,
                &first_state.proj().acp,
                &Default::default(),
                |_| Some("invalid-env-agent".into())
            )
            .unwrap()
            .name,
            "embedded"
        );
        let job = st
            .jobs
            .submit(&first_state, super::super::jobs::JobKind::Compile);
        let second = select(State(st.clone()), HeaderMap::new(), Json(request("second"))).await;
        let bytes = axum::body::to_bytes(second.into_body(), 4096)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let mut second_state = st.scoped(None).unwrap();
        Arc::get_mut(&mut second_state).unwrap().agent_selection =
            st.agent_choices.get(body["id"].as_str().unwrap());
        assert_ne!(
            job,
            st.jobs
                .submit(&second_state, super::super::jobs::JobKind::Compile)
        );
        assert_eq!(first_state.llm().model, "first");
        assert_eq!(st.jobs.get(job).unwrap()["agentSelection"]["id"], first.id);
    }
    #[tokio::test]
    async fn unknown_selection_is_rejected_instead_of_using_defaults() {
        use tower::ServiceExt;
        let st = super::super::server::tests::state(None, "https://jazyk.dev");
        let response = super::super::server::router(st)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/capabilities")
                    .header("x-jazyk-agent-selection", "expired")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
    #[test]
    fn endpoint_rejects_credentials_and_does_not_reuse_a_key() {
        let llm = Llm {
            api: "chat_completions".into(),
            base_url: "http://localhost/v1".into(),
            model: "old".into(),
            api_key: "old-key".into(),
            reasoning_summary: None,
            temperature: None,
            trace: None,
        };
        assert!(endpoint(
            &json!({"baseUrl":"https://key@host/v1","model":"x"}),
            llm.clone()
        )
        .is_err());
        assert!(endpoint(
            &json!({"baseUrl":"https://host/v1?key=x","model":"x"}),
            llm.clone()
        )
        .is_err());
        let next = endpoint(&json!({"baseUrl":"https://host/v1","model":"x"}), llm).unwrap();
        assert!(next.api_key.is_empty());
    }
    #[test]
    fn cloud_origin_is_bounded() {
        for url in [
            "https://user@api.jazyk.dev",
            "http://evil.example",
            "https://api.jazyk.dev/path",
        ] {
            assert!(cloud_origin(&json!({"cloudOrigin":url})).is_err());
        }
        assert_eq!(
            cloud_origin(&json!({"cloudOrigin":"https://api.jazyk.dev"})).unwrap(),
            "https://api.jazyk.dev"
        );
    }
}

// Preparation runs only after the human chooses a local agent. It populates npm's
// pinned adapter cache without opening a model session or altering agent logins.
pub async fn setup(State(st): State<SharedState>, Json(body): Json<Value>) -> Response {
    let Some(name) = body["agent"].as_str() else {
        return err(StatusCode::BAD_REQUEST, "Agent is required.");
    };
    let resolved = match profile(&st, Some(name)) {
        Ok(a) => a,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    let Some(builtin) = config::builtin_profile(name) else {
        return Json(json!({"ready":true})).into_response();
    };
    if resolved != builtin || !matches!(name, "codex" | "claude") {
        return Json(json!({"ready":true})).into_response();
    }
    let Some(package) = builtin
        .args
        .iter()
        .find(|arg| arg.starts_with("@agentclientprotocol/"))
    else {
        return err(StatusCode::BAD_REQUEST, "Adapter package is unavailable.");
    };
    let mut command = tokio::process::Command::new("npm");
    command
        .args([
            "exec",
            "--yes",
            &format!("--package={package}"),
            "--",
            "node",
            "-e",
            "",
        ])
        .current_dir(&st.root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    match tokio::time::timeout(std::time::Duration::from_secs(120),command.status()).await {
        Ok(Ok(status)) if status.success()=>Json(json!({"ready":true,"login":"Managed by the coding agent."})).into_response(),
        _=>err(StatusCode::BAD_GATEWAY,"Could not prepare the pinned adapter. Check npm and the agent installation, then retry.")
    }
}
