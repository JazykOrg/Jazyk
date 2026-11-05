//! Model configuration, catalog lookup and owned GDK model transport.
use crate::scoped_state::{AtomicBool, AtomicU64};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
#[cfg(not(target_arch = "wasm32"))]
use std::{sync::OnceLock, time::Duration};

pub fn same_endpoint(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.trim()
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .trim_end_matches('/')
            .to_lowercase()
    };
    norm(a) == norm(b)
}

// Token meter: completion tokens across all calls this run, for status.yaml reporting.
static SPENT_TOKENS: AtomicU64 = AtomicU64::new("SPENT_TOKENS", 0);

pub fn tokens_spent() -> u64 {
    SPENT_TOKENS.load(Ordering::Relaxed)
}

// Tokens an ACP agent reported for work it ran on jazyk's behalf, folded into the
// same meter status.yaml reads.
pub fn add_tokens(n: u64) {
    SPENT_TOKENS.fetch_add(n, Ordering::Relaxed);
}

// Tokens already folded into a persisted status. The meter is process-lifetime, but
// status.yaml is cumulative across builds and processes: a build takes only the delta
// since the last take, so it never clobbers earlier builds' spend.
static FOLDED_TOKENS: AtomicU64 = AtomicU64::new("FOLDED_TOKENS", 0);

pub fn take_tokens_delta() -> u64 {
    let spent = SPENT_TOKENS.load(Ordering::Relaxed);
    let folded = FOLDED_TOKENS.swap(spent, Ordering::Relaxed);
    spent.saturating_sub(folded)
}

// Verbose request logging, enabled by the CLI or the JAZYK_VERBOSE env var.
static VERBOSE: AtomicBool = AtomicBool::new("VERBOSE", false);
static VERBOSE_INIT: AtomicBool = AtomicBool::new("VERBOSE_INIT", false);
pub fn set_verbose(on: bool) {
    VERBOSE.store(on, Ordering::Relaxed);
    VERBOSE_INIT.store(true, Ordering::Relaxed);
}
fn verbose() -> bool {
    if !VERBOSE_INIT.load(Ordering::Relaxed) {
        let on = crate::sys::env_var("JAZYK_VERBOSE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false);
        set_verbose(on);
    }
    VERBOSE.load(Ordering::Relaxed)
}

static MAX_COMPLETION_OVERRIDE: AtomicU64 = AtomicU64::new("MAX_COMPLETION_OVERRIDE", 0);

pub fn set_max_completion_tokens(n: u64) {
    MAX_COMPLETION_OVERRIDE.store(n, Ordering::Relaxed);
}

fn base_completion_tokens() -> u64 {
    let over = MAX_COMPLETION_OVERRIDE.load(Ordering::Relaxed);
    if over > 0 {
        return over.max(256);
    }
    crate::sys::env_var("JAZYK_MAX_COMPLETION_TOKENS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(4096)
        .max(256)
}

pub(crate) fn max_completion_tokens() -> u64 { base_completion_tokens() }

#[cfg(not(target_arch = "wasm32"))]
fn call_timeout() -> Duration {
    let secs = crate::sys::env_var("JAZYK_CALL_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600)
        .max(30);
    Duration::from_secs(secs)
}

#[derive(Clone)]
pub struct Llm {
    pub api: String,
    pub reasoning_summary: Option<String>,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    // Sampling temperature. Defaults to 0, but some models only allow their default;
    // `None` omits the field entirely.
    pub temperature: Option<f64>,
    // Where this client reports what it sends and receives. None keeps the old
    // behavior (notices to stderr, no structured events). A runner attaches its own
    // trace with `with_trace`, so every prompt, reply, and retry reaches the frontend
    // that started the work (docs/compiler/turns.md#trace-events).
    pub trace: Option<crate::session::Trace>,
}

// Per message, in the recorded prompt. Packs are context-budgeted well below this;
// the cap only stops a runaway payload from filling the transcript.
const MESSAGE_CAP: usize = 24_000;

// The outgoing messages as recorded: same shape, long strings cut.
fn recorded(messages: &[Value]) -> Value {
    Value::Array(
        messages
            .iter()
            .map(|m| {
                let mut m = m.clone();
                if let Some(o) = m.as_object_mut() {
                    for k in ["content", "reasoning_content", "reasoning"] {
                        if let Some(s) = o.get(k).and_then(|v| v.as_str()) {
                            if s.len() > MESSAGE_CAP {
                                let cut = format!(
                                    "{} … [{} chars total]",
                                    truncate(s, MESSAGE_CAP),
                                    s.len()
                                );
                                o.insert(k.to_string(), json!(cut));
                            }
                        }
                    }
                }
                m
            })
            .collect(),
    )
}

fn tool_names(tools: Option<&[Value]>) -> Vec<String> {
    tools
        .unwrap_or(&[])
        .iter()
        .filter_map(|t| t["function"]["name"].as_str().map(|s| s.to_string()))
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn native_io_runtime() -> &'static tokio::runtime::Runtime {
    static NETWORK: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    NETWORK.get_or_init(|| tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_all().build().expect("native model and MCP I/O runtime"))
}

#[cfg(not(target_arch = "wasm32"))]
fn forward_model_delta(
    message: &goose_provider_types::conversation::message::Message,
    sink: &crate::harness::ModelDeltaSink,
) {
    use goose_provider_types::conversation::message::MessageContent;
    for content in &message.content {
        match content {
            MessageContent::Text(text) if !text.text.is_empty() => sink("content", &text.text),
            MessageContent::Thinking(thought) if !thought.thinking.is_empty() => sink("reasoning_content", &thought.thinking),
            _ => {}
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn collect_native_stream<S>(stream: S, deltas: Option<crate::harness::ModelDeltaSink>) -> Result<String, String>
where S: futures::Stream<Item = anyhow::Result<String>> + Send + Unpin + 'static {
    use futures::StreamExt;
    use goose_provider_types::{base::collect_stream, errors::ProviderError, formats::openai};
    let decoded = openai::response_to_streaming_message(stream)
        .inspect(move |result| {
            if let (Ok((Some(message), _)), Some(sink)) = (result, &deltas) {
                forward_model_delta(message, sink);
            }
        })
        .map(|result| result.map_err(|e| ProviderError::RequestFailed(e.to_string())));
    let (message, usage) = collect_stream(Box::pin(decoded)).await.map_err(|e| e.to_string())?;
    Ok(json!({"_jazykGdkResponse":{"message":message,"usage":usage}}).to_string())
}

impl Llm {
    /// Send a public-GDK-formatted request. Policy and durable identities live in
    /// the harness and host; this boundary never retries or changes tool codecs.
    pub async fn request_async(
        &self,
        request: crate::harness::ModelRequest,
    ) -> Result<Value, String> {
        let started = crate::sys::Instant::now();
        let request_id = request.attempt_id.clone();
        let purpose = format!("{:?}", request.purpose).to_ascii_lowercase();
        let messages = request.body["messages"].as_array().cloned().unwrap_or_default();
        self.event(crate::session::TraceEvent::LlmRequest {
            label: request_id.clone(), step: purpose.clone(), model: self.model.clone(),
            messages: recorded(&messages), tools: tool_names(request.body["tools"].as_array().map(Vec::as_slice)),
        });
        #[cfg(target_arch = "wasm32")]
        let response = { crate::wasm::llm_bridge::chat_async(&request.body.to_string()).await? };
        #[cfg(not(target_arch = "wasm32"))]
        let response = if crate::vfs::is_scoped() {
            let mut body = request.body;
            body["_jazyk"] = json!({"requestId":request.attempt_id,"purpose":request.purpose});
            crate::hosted::transport_async(&request.attempt_id, &body.to_string(), request.deltas).await?
        } else {
            use futures::StreamExt;
            // Only owned HTTP work runs on this process-shared executor. The
            // harness and every MCP/VFS callback stay on their owning thread.
            let network = native_io_runtime();
            let base_url = self.base_url.clone();
            let api_key = self.api_key.clone();
            let timeout = call_timeout();
            let api = request.body["_jazykApi"].as_str().unwrap_or(&self.api).to_owned();
            let summary = request.body.get("_jazykReasoningSummary").map(|v| v.as_str().map(String::from)).unwrap_or_else(|| self.reasoning_summary.clone());
            tokio_util::task::AbortOnDropHandle::new(network
                .spawn(async move {
                    use eventsource_stream::Eventsource;
                    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
                    let client = CLIENT.get_or_init(reqwest::Client::new);
                    let binding = format!("{}:{}", base_url.trim_end_matches('/'), request.body["model"].as_str().unwrap_or_default());
                    let mut body = crate::model_api::request(&request.body, &api, summary.as_deref(), &binding)?;
                    body["stream"] = json!(true);
                    if api == "chat_completions" { body["stream_options"] = json!({"include_usage":true}); }
                    let mut call = client
                        .post(format!(
                            "{}/{}",
                            base_url.trim_end_matches('/'), if api == "responses" { "responses" } else { "chat/completions" }
                        ))
                        .timeout(timeout)
                        .json(&body);
                    if !api_key.is_empty() {
                        call = call.bearer_auth(&api_key);
                    }
                    let response = call
                        .send()
                        .await
                        .map_err(|e| format!("model transport: {e}"))?;
                    let status = response.status();
                    let streaming = response.headers().get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("text/event-stream"));
                    if status.is_success() && streaming {
                        let mut received = 0usize;
                        let bytes = response.bytes_stream().map(move |chunk| {
                            let chunk = chunk.map_err(anyhow::Error::from)?;
                            received = received.saturating_add(chunk.len());
                            if received > 16 * 1024 * 1024 { anyhow::bail!("model response exceeds 16 MiB"); }
                            Ok::<_, anyhow::Error>(chunk)
                        });
                        let events = bytes.eventsource().map(|event| event
                            .map(|event| format!("data: {}", event.data)).map_err(anyhow::Error::from));
                        if api == "responses" {
                            let mut events = Box::pin(events);
                            let mut collector = crate::model_api::ResponseStream::default();
                            while let Some(event) = events.next().await {
                                let frame = event.map_err(|e| e.to_string())?;
                                let data = frame.strip_prefix("data: ").unwrap_or(&frame);
                                if data == "[DONE]" { continue; }
                                let value = serde_json::from_str(data).map_err(|e| format!("Invalid Responses event: {e}"))?;
                                if let Some((field, text)) = collector.event(&value)? { if let Some(sink) = &request.deltas { sink(field, &text); } }
                            }
                            return Ok(collector.finish(&binding)?.to_string());
                        }
                        return collect_native_stream(Box::pin(events), request.deltas).await;
                    }
                    let mut stream = response.bytes_stream();
                    let mut bytes = Vec::new();
                    while let Some(chunk) = stream.next().await {
                        let chunk = chunk.map_err(|e| format!("model response: {e}"))?;
                        if bytes.len().saturating_add(chunk.len()) > 16 * 1024 * 1024 {
                            return Err("model response exceeds 16 MiB".into());
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    let body = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                    if !status.is_success() {
                        return Err(format!("HTTP {status}: {}", truncate(&body, 1000)));
                    }
                    if api == "responses" {
                        let value = serde_json::from_str(&body).map_err(|e| format!("Invalid Responses reply: {e}"))?;
                        return Ok(crate::model_api::completion(&value, &binding)?.to_string());
                    }
                    Ok::<_, String>(body)
                }))
                .await
                .map_err(|e| format!("model network task: {e}"))??
        };
        let response: Value = serde_json::from_str(&response).map_err(|e| format!("Invalid model response: {e}"))?;
        self.event(crate::session::TraceEvent::LlmResponse {
            label: request_id, step: purpose, ms: started.elapsed().as_millis() as u64,
            tokens: response["usage"]["completion_tokens"].as_u64()
                .or_else(|| response["_jazykGdkResponse"]["usage"]["usage"]["output_tokens"].as_u64()).unwrap_or(0),
            message: response.get("_jazykGdkResponse").map(|v| v["message"].clone())
                .unwrap_or_else(|| response["choices"][0]["message"].clone()),
        });
        Ok(response)
    }

    // A client that reports into this trace. Cheap: the trace is a handle.
    pub fn with_trace(&self, trace: &crate::session::Trace) -> Llm {
        Llm {
            trace: Some(trace.clone()),
            ..self.clone()
        }
    }

    // What the endpoint says it can serve, for a client that offers the user a
    // choice. An endpoint that does not answer `/models` (or answers something
    // unexpected) is not an error: the configured model is the only one on offer,
    // which is exactly the state of the world before asking.
    // Mirrors docs/frontends/acp.md#choosing-a-model.
    #[cfg(target_arch = "wasm32")]
    pub fn list_models(&self) -> Vec<String> {
        // The page holds the model; there is no endpoint to enumerate.
        if self.model.is_empty() {
            Vec::new()
        } else {
            vec![self.model.clone()]
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn list_models(&self) -> Vec<String> {
        if crate::vfs::is_scoped() {
            return vec![self.model.clone()];
        }
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(10))
            .build();
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let mut req = agent.get(&url);
        if !self.api_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.api_key));
        }
        let mut names: Vec<String> = req
            .call()
            .ok()
            .and_then(|r| r.into_string().ok())
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v["data"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
            .collect();
        names.sort();
        names.dedup();
        if !self.model.is_empty() && !names.contains(&self.model) {
            names.insert(0, self.model.clone());
        }
        names
    }

    fn event(&self, event: crate::session::TraceEvent) {
        if let Some(trace) = &self.trace { trace.event(event); }
        else if verbose() { eprintln!("[jazyk] {}", serde_json::to_string(&event).unwrap_or_default()); }
    }

    // Utility inference uses the same GDK pipeline with no advertised tools.
    #[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
    pub async fn chat(
        &self,
        system: &str,
        user: &str,
        label: &str,
        step: &str,
    ) -> Result<String, String> {
        let mut history = vec![json!({"role":"user","content":user})];
        let mut output = String::new();
        let cancelled = || self.trace.as_ref().is_some_and(|trace| trace.is_cancelled());
        let mut emit = |event| {
            if let crate::harness::AgentEvent::Message(text) = event { output.push_str(&text); }
        };
        let mut memory = crate::harness::MemoryRecovery::default();
        #[cfg(not(target_arch = "wasm32"))]
        let mut durable = if crate::vfs::is_scoped() {
            let key = format!("utility:{}", crate::model::hash_hex(&json!([label, step, system, user]).to_string()));
            Some(crate::session_recovery::McpRecovery::utility(key)?)
        } else { None };
        #[cfg(not(target_arch = "wasm32"))]
        let recovery: &mut dyn crate::harness::Recovery = match durable.as_mut() {
            Some(recovery) => recovery,
            None => &mut memory,
        };
        #[cfg(target_arch = "wasm32")]
        let recovery: &mut dyn crate::harness::Recovery = &mut memory;
        let mut host = crate::harness::provider::DefaultModelHost::new(self);
        let stop = crate::harness::run_with_host(crate::harness::LoopArgs {
            llm: self, history: &mut history, tools: &[],
            dispatch: &mut |_, _| Err("No tools are available in this inference".into()),
            emit: &mut emit, cancelled: &cancelled, max_rounds: 1,
            label: format!("{label} {step}"), conversation: true, inputs: None, input_close: None,
        }, system, &mut host, recovery).await;
        if output.is_empty() {
            if let Some(message) = history.iter().rev().find(|message| message["role"] == "assistant") {
                output = message["content"].as_str().unwrap_or_default().to_string();
            }
        }
        match stop {
            crate::harness::Stop::EndTurn => Ok(output),
            crate::harness::Stop::Error(error) => Err(error),
            crate::harness::Stop::Cancelled => Err("execution cancelled".into()),
            crate::harness::Stop::Suspended => Err("suspended".into()),
            crate::harness::Stop::MaxRounds => Err("utility inference reached its request limit".into()),
        }
    }

}

pub fn extract_json_object(s: &str) -> Option<String> {
    let mut s = s.to_string();
    while let (Some(a), Some(b)) = (s.find("<think>"), s.find("</think>")) {
        if a < b {
            s.replace_range(a..b + "</think>".len(), "");
        } else {
            break;
        }
    }
    let bytes = s.as_bytes();
    let start = s.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for i in start..bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(s[start..=i].to_string());
                    }
                }
                _ => {}
            }
        }
    }
    None
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n).collect();
    out.push('…');
    out
}


#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
