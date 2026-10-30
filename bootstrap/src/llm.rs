// OpenAI-compatible chat client over ureq. What is ours is the provider-behavior
// handling: message-history requests with native tool-calling, a sticky capability
// probe that downgrades to the text codec when the endpoint rejects `tools`, sticky
// temperature and streaming fallbacks, pacing, and retry policy.
use crate::scoped_state::{AtomicBool, AtomicU64, AtomicU8};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

// Codec capability, learned once per run. 0 = unknown, 1 = native tools, 2 = text fallback.
static TOOLS_MODE: AtomicU8 = AtomicU8::new("TOOLS_MODE", 0);

// Set once a model rejects the temperature parameter (some only allow their default);
// the rest of the run omits it. Providers often wrap the rejection as a bare 400, so the
// first hard 400 with a temperature set triggers the drop-and-retry.
static TEMP_UNSUPPORTED: AtomicBool = AtomicBool::new("TEMP_UNSUPPORTED", false);

// Set once an endpoint answers "stream must be set to true"; the rest of the run uses
// streaming requests and assembles the message from SSE deltas.
static STREAM_REQUIRED: AtomicBool = AtomicBool::new("STREAM_REQUIRED", false);

// Set once a provider rejects reasoning fields echoed back in the message history
// (some 400 when `reasoning_content` appears in an input message). The rest of the run
// strips them from outgoing messages; the turn's own transcript keeps them.
static REASONING_UNSUPPORTED: AtomicBool = AtomicBool::new("REASONING_UNSUPPORTED", false);

// Set once a provider rejects the `max_tokens` completion cap; the rest of the run
// omits it and relies on the stream reader's own cap.
static MAX_TOKENS_UNSUPPORTED: AtomicBool = AtomicBool::new("MAX_TOKENS_UNSUPPORTED", false);

pub fn tools_mode() -> u8 {
    TOOLS_MODE.load(Ordering::Relaxed)
}

pub fn set_tools_mode(mode: u8) {
    TOOLS_MODE.store(mode, Ordering::Relaxed);
}

// What one model call cost. `completion` is the tokens it produced (the throughput and
// efficiency figures the benchmark reports); `billed` is what the endpoint bills, its
// input and output tokens together (the session's cost); `cached` is the part of the
// input the endpoint served from its prompt cache, a share of the bill and never a
// deduction from it, 0 when unreported. `measured` says the endpoint reported usage;
// otherwise completion and billed are estimates from characters, a quarter token each.
// Mirrors docs/compiler/sessions.md#budgets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spend {
    pub completion: u64,
    pub billed: u64,
    pub cached: u64,
    pub measured: bool,
}

// The billed tokens a usage object reports: its total, or its input plus output.
fn usage_billed(u: &Value) -> Option<u64> {
    u["total_tokens"].as_u64().or_else(|| {
        let prompt = u["prompt_tokens"].as_u64()?;
        Some(prompt.saturating_add(u["completion_tokens"].as_u64().unwrap_or(0)))
    })
}

// Whether two base URLs name the same endpoint: the same text once a trailing slash
// and a trailing `/v1` are dropped. What decides whether the configured API key rides.
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

// The cached input tokens a usage object reports: OpenAI's
// `prompt_tokens_details.cached_tokens`, or an Anthropic-compatible endpoint's
// `cache_read_input_tokens`. 0 when neither is present.
fn usage_cached(u: &Value) -> u64 {
    u["prompt_tokens_details"]["cached_tokens"]
        .as_u64()
        .or_else(|| u["cache_read_input_tokens"].as_u64())
        .unwrap_or(0)
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

// Global cap on concurrent in-flight LLM requests, so parallel turns do not overwhelm the
// backend (a local Ollama serializes work and 502s under heavy fan-out). Tunable with
// JAZYK_MAX_CONCURRENCY; default 6.
struct Semaphore {
    permits: Mutex<usize>,
    cv: Condvar,
}
static SEM: OnceLock<Semaphore> = OnceLock::new();
fn semaphore() -> &'static Semaphore {
    SEM.get_or_init(|| {
        let n = crate::sys::env_var("JAZYK_MAX_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(6)
            .max(1);
        Semaphore {
            permits: Mutex::new(n),
            cv: Condvar::new(),
        }
    })
}
// Minimum gap between request starts, so tight failure loops cannot hammer an endpoint.
// Tunable with JAZYK_MIN_INTERVAL_MS; default 500.
static LAST_REQUEST: Mutex<Option<crate::sys::Instant>> = Mutex::new(None);
fn pace() {
    let min_ms = crate::sys::env_var("JAZYK_MIN_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(500);
    if min_ms == 0 {
        return;
    }
    let wait = {
        let mut last = LAST_REQUEST.lock().unwrap();
        let now = crate::sys::Instant::now();
        let wait = match *last {
            Some(t) => Duration::from_millis(min_ms).saturating_sub(now.duration_since(t)),
            None => Duration::ZERO,
        };
        *last = Some(now + wait);
        wait
    };
    if !wait.is_zero() {
        crate::sys::sleep(wait);
    }
}

struct Permit;
fn acquire() -> Permit {
    let s = semaphore();
    let mut p = s.permits.lock().unwrap();
    while *p == 0 {
        p = s.cv.wait(p).unwrap();
    }
    *p -= 1;
    Permit
}
impl Drop for Permit {
    fn drop(&mut self) {
        let s = semaphore();
        let mut p = s.permits.lock().unwrap();
        *p += 1;
        s.cv.notify_one();
    }
}

// Number of retries (in addition to the first attempt) for failed LLM calls. Tunable with
// JAZYK_MAX_RETRIES; default 2.
fn max_retries() -> usize {
    crate::sys::env_var("JAZYK_MAX_RETRIES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(2)
}

// Completion cap for one response, sent as `max_tokens` and enforced again on streamed
// content. This is the loop detector: a small model stuck repeating tokens is bounded
// by the cap, not by its context window. Tunable with JAZYK_MAX_COMPLETION_TOKENS;
// default 4096. See docs/compiler/project-settings.md#environment-tuning.
// A cap set by the host, taking the place of the environment where there is none
// (the browser backend sets it at boot: docs/frontends/wasm.md#the-model). Zero
// means unset.
static MAX_COMPLETION_OVERRIDE: AtomicU64 = AtomicU64::new("MAX_COMPLETION_OVERRIDE", 0);

pub fn set_max_completion_tokens(n: u64) {
    MAX_COMPLETION_OVERRIDE.store(n, Ordering::Relaxed);
}

// Set by the agent loop after a capped reply with no call or missing required
// arguments: the next request alone carries twice the cap. Mirrors
// docs/compiler/project-settings.md#environment-tuning.
static BOOST_NEXT: AtomicBool = AtomicBool::new("BOOST_NEXT", false);

pub fn boost_next_completion() {
    BOOST_NEXT.store(true, Ordering::Relaxed);
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

pub(crate) fn max_completion_tokens() -> u64 {
    let base = base_completion_tokens();
    if BOOST_NEXT.load(Ordering::Relaxed) {
        base * 2
    } else {
        base
    }
}

// Wall clock for one whole LLM call. JAZYK_READ_TIMEOUT waits for the next byte; this
// bounds a call whose bytes keep arriving. Tunable with JAZYK_CALL_TIMEOUT; default 600.
fn call_timeout() -> Duration {
    let secs = crate::sys::env_var("JAZYK_CALL_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600)
        .max(30);
    Duration::from_secs(secs)
}

// Whether an error looks transient (worth retrying) versus a hard client error.
fn is_transient(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("http 429")
        || e.contains("rate limit")
        || e.contains("too many requests")
        || e.contains("overloaded")
        || e.contains("502")
        || e.contains("503")
        || e.contains("504")
        || e.contains("bad gateway")
        || e.contains("service unavailable")
        || e.contains("gateway timeout")
        || e.contains("request failed")
        || e.contains("timed out")
        || e.contains("connect ")
        || e.contains("read:")
        || e.contains("write:")
        || e.contains("no http body")
        || e.contains("transport")
        || e.contains("network")
        || e.contains("io error")
}

// Whether an error indicates the endpoint or model rejects the `tools` parameter.
fn rejects_tools(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("tools") || e.contains("tool_choice") || e.contains("function")
}

#[derive(Clone)]
pub struct Llm {
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
            crate::hosted::transport_async(&request.attempt_id, &body.to_string(), None).await?
        } else {
            use futures::StreamExt;
            // Only owned HTTP work runs on this process-shared executor. The
            // harness and every MCP/VFS callback stay on their owning thread.
            let network = native_io_runtime();
            let base_url = self.base_url.clone();
            let api_key = self.api_key.clone();
            let timeout = call_timeout();
            tokio_util::task::AbortOnDropHandle::new(network
                .spawn(async move {
                    use eventsource_stream::Eventsource;
                    use goose_provider_types::{base::collect_stream, errors::ProviderError, formats::openai};
                    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
                    let client = CLIENT.get_or_init(reqwest::Client::new);
                    let mut body = request.body;
                    body["stream"] = json!(true);
                    body["stream_options"] = json!({"include_usage":true});
                    let mut call = client
                        .post(format!(
                            "{}/chat/completions",
                            base_url.trim_end_matches('/')
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
                        let decoded = openai::response_to_streaming_message(Box::pin(events))
                            .map(|result| result.map_err(|e| ProviderError::RequestFailed(e.to_string())));
                        let (message, usage) = collect_stream(Box::pin(decoded)).await.map_err(|e| e.to_string())?;
                        return Ok(json!({"_jazykGdkResponse":{"message":message,"usage":usage}}).to_string());
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

    // A provider-behavior notice (a sticky fallback, a rate-limit wait). It belongs to
    // the work that triggered it, so it goes to the trace when there is one.
    fn note(&self, label: &str, text: &str) {
        match &self.trace {
            Some(t) => t.line(label, text),
            None => eprintln!("[jazyk] {}", text),
        }
    }

    fn event(&self, ev: crate::session::TraceEvent) {
        if let Some(t) = &self.trace {
            t.event(ev);
        }
    }

    // One turn round: send the full message history, optionally with tool definitions, and
    // return the assistant message object (`content` and, when the model called tools,
    // `tool_calls`) plus the completion tokens the call spent. Transport failures retry
    // immediately; a `tools` rejection surfaces as Err so the turn harness can downgrade
    // the codec.
    pub fn chat_messages(
        &self,
        messages: &[Value],
        tools: Option<&[Value]>,
        label: &str,
        step: &str,
    ) -> Result<(Value, Spend), String> {
        self.chat_messages_cancellable(messages, tools, label, step, &|| {
            self.trace.as_ref().is_some_and(|t| t.is_cancelled())
        })
    }

    pub fn chat_messages_cancellable(
        &self,
        messages: &[Value],
        tools: Option<&[Value]>,
        label: &str,
        step: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(Value, Spend), String> {
        self.chat_messages_deltas(messages, tools, label, step, cancelled, None)
    }

    pub fn chat_messages_deltas(
        &self,
        messages: &[Value],
        tools: Option<&[Value]>,
        label: &str,
        step: &str,
        cancelled: &dyn Fn() -> bool,
        mut delta: Option<&mut dyn FnMut(&str, &str)>,
    ) -> Result<(Value, Spend), String> {
        // A boosted cap serves this one call, retries included, whatever its outcome.
        struct ClearBoost;
        impl Drop for ClearBoost {
            fn drop(&mut self) {
                BOOST_NEXT.store(false, Ordering::Relaxed);
            }
        }
        let _clear = ClearBoost;
        let max = max_retries();
        let mut last = String::new();
        let started = crate::sys::Instant::now();
        if verbose() {
            eprintln!("[jazyk] → {} {}", label, step);
        }
        // The prompt as sent, recorded once for the whole call: retries resend it
        // unchanged, and a retry says so on its own row.
        self.event(crate::session::TraceEvent::LlmRequest {
            label: label.to_string(),
            step: step.to_string(),
            model: self.model.clone(),
            messages: recorded(messages),
            tools: tool_names(tools),
        });
        let mut try_stream = false;
        let mut prefer_stream = delta.is_some();
        for attempt in 0..=max {
            if cancelled() {
                return Err("execution cancelled".into());
            }
            let streaming = STREAM_REQUIRED.load(Ordering::Relaxed) || try_stream || prefer_stream;
            let mut visible = false;
            let mut forward = |field: &str, text: &str| {
                visible |= !text.is_empty();
                if let Some(callback) = delta.as_mut() {
                    callback(field, text);
                }
            };
            let outcome = self.chat_once(messages, tools, streaming, cancelled, &mut forward);
            match outcome {
                Ok((msg, tokens)) => {
                    if cancelled() {
                        return Err("execution cancelled".into());
                    }
                    // A streaming probe that succeeds after a non-streaming failure
                    // sticks for the run.
                    if try_stream && !STREAM_REQUIRED.swap(true, Ordering::Relaxed) {
                        self.note(
                            label,
                            "streaming retry succeeded; using SSE for the rest of the run",
                        );
                    }
                    if verbose() {
                        eprintln!(
                            "[jazyk] ✓ {} {} ({} ms)",
                            label,
                            step,
                            started.elapsed().as_millis()
                        );
                    }
                    self.event(crate::session::TraceEvent::LlmResponse {
                        label: label.to_string(),
                        step: step.to_string(),
                        ms: started.elapsed().as_millis() as u64,
                        tokens: tokens.completion,
                        message: msg.clone(),
                    });
                    return Ok((msg, tokens));
                }
                Err(e) => {
                    if visible {
                        return Err(format!("Model attempt interrupted after partial output; retry requires a new attempt: {e}"));
                    }
                    if cancelled() {
                        return Err("execution cancelled".into());
                    }
                    if prefer_stream && e.contains("400") && e.to_lowercase().contains("stream") {
                        prefer_stream = false;
                        self.note(
                            label,
                            "endpoint rejected streaming; falling back to complete responses",
                        );
                        continue;
                    }
                    // An endpoint that only serves streaming responses says so; switch
                    // once, sticky for the run, and retry.
                    if e.to_lowercase().contains("stream must be set to true")
                        && !STREAM_REQUIRED.swap(true, Ordering::Relaxed)
                    {
                        self.note(
                            label,
                            "endpoint requires streaming; switching to SSE for the rest of the run",
                        );
                        continue;
                    }
                    // A provider that rejects reasoning fields echoed back in the
                    // history names them in the 400. Strip them once, sticky for the
                    // run, and retry. Checked before the temperature fallback so the
                    // named rejection is not misread as a temperature one.
                    if e.contains("400")
                        && e.to_lowercase().contains("reasoning")
                        && !REASONING_UNSUPPORTED.swap(true, Ordering::Relaxed)
                    {
                        self.note(label, "provider rejected reasoning fields in the history; stripping them from requests for the rest of the run");
                        continue;
                    }
                    // A provider that rejects `max_tokens` names it. Drop the cap once,
                    // sticky for the run, and retry; the stream reader still bounds the
                    // completion. Checked before the temperature fallback so the named
                    // rejection is not misread as a temperature one.
                    if e.contains("400")
                        && e.to_lowercase().contains("max_tokens")
                        && !MAX_TOKENS_UNSUPPORTED.swap(true, Ordering::Relaxed)
                    {
                        self.note(label, "provider rejected max_tokens; omitting the completion cap for the rest of the run");
                        continue;
                    }
                    // A model that rejects `temperature` answers 400 (often wrapped by a
                    // proxy). Drop the parameter once, sticky for the run, and retry.
                    let looks_400 =
                        e.contains("HTTP 400") || e.to_lowercase().contains("temperature");
                    if looks_400
                        && self.temperature.is_some()
                        && !TEMP_UNSUPPORTED.swap(true, Ordering::Relaxed)
                    {
                        self.note(label, "model rejected the request (likely temperature); retrying without it for the rest of the run");
                        continue;
                    }
                    // Ollama renders tool schemas through the model's chat template
                    // and parses tool-call replies as XML; either side failing
                    // surfaces as a 500 with a Go XML error ("expected element
                    // type ...", "XML syntax error ..."). Retries rarely recover
                    // (the template, not the network, is at fault), so treat it
                    // as tools rejection wearing a transient status code.
                    if tools.is_some()
                        && ((rejects_tools(&e) && !is_transient(&e))
                            || e.contains("expected element type")
                            || e.contains("XML syntax error"))
                    {
                        return Err(format!("tools-rejected: {}", e));
                    }
                    last = e;
                    if attempt < max && is_transient(&last) {
                        // Some proxies fail relaying a non-streaming response (e.g. tool
                        // calls through a router); probe the retry over SSE. Sticky only
                        // if the streaming attempt succeeds; a failed probe reverts.
                        if !streaming {
                            try_stream = true;
                        } else if try_stream {
                            try_stream = false;
                        }
                        // A rate limit is not a hiccup: pause before retrying instead of
                        // hammering the window shut.
                        let wait = if last.to_lowercase().contains("rate limit") {
                            20
                        } else {
                            5
                        };
                        let ev = crate::session::TraceEvent::LlmRetry {
                            label: label.to_string(),
                            step: step.to_string(),
                            attempt: attempt as u32 + 1,
                            error: truncate(&last, 400),
                            wait_ms: wait * 1000,
                        };
                        match &self.trace {
                            Some(t) => t.event(ev),
                            None => eprintln!(
                                "[jazyk] {} {} - retrying in {}s ({}/{}): {}",
                                label,
                                step,
                                wait,
                                attempt + 1,
                                max,
                                truncate(&last, 120)
                            ),
                        }
                        for _ in 0..wait * 20 {
                            if cancelled() {
                                return Err("execution cancelled".into());
                            }
                            crate::sys::sleep(Duration::from_millis(50));
                        }
                    } else {
                        break;
                    }
                }
            }
        }
        if verbose() {
            eprintln!(
                "[jazyk] ✗ {} {} ({} ms): {}",
                label,
                step,
                started.elapsed().as_millis(),
                truncate(&last, 120)
            );
        }
        // The caller reports the failure itself (a failed turn, a failed entity); this
        // only keeps the structured record complete for a reader of the transcript.
        if let Some(t) = &self.trace {
            t.line(
                label,
                &format!(
                    "llm call failed after {} ms: {}",
                    started.elapsed().as_millis(),
                    truncate(&last, 400)
                ),
            );
        }
        Err(last)
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
        let stop = crate::harness::run_loop_with_instructions(crate::harness::LoopArgs {
            llm: self, history: &mut history, tools: &[],
            dispatch: &mut |_, _| Err("No tools are available in this inference".into()),
            emit: &mut emit, cancelled: &cancelled, max_rounds: 1,
            label: format!("{label} {step}"), conversation: true, inputs: None, input_close: None,
        }, system).await;
        match stop {
            crate::harness::Stop::EndTurn => Ok(output),
            crate::harness::Stop::Error(error) => Err(error),
            crate::harness::Stop::Cancelled => Err("execution cancelled".into()),
            crate::harness::Stop::Suspended => Err("execution suspended".into()),
            crate::harness::Stop::MaxRounds => Err("utility inference reached its request limit".into()),
        }
    }

    fn chat_once(
        &self,
        messages: &[Value],
        tools: Option<&[Value]>,
        streaming: bool,
        cancelled: &dyn Fn() -> bool,
        delta: &mut dyn FnMut(&str, &str),
    ) -> Result<(Value, Spend), String> {
        // History goes out as-is, reasoning fields included, so a reasoning model keeps
        // its chain across rounds. Stripped only under the sticky rejection fallback.
        let outgoing: Vec<Value> = if REASONING_UNSUPPORTED.load(Ordering::Relaxed) {
            messages
                .iter()
                .map(|m| {
                    let mut m = m.clone();
                    if let Some(o) = m.as_object_mut() {
                        o.remove("reasoning_content");
                        o.remove("reasoning");
                    }
                    m
                })
                .collect()
        } else {
            messages.to_vec()
        };
        let mut payload = json!({
            "model": self.model,
            "stream": streaming,
            "messages": outgoing,
        });
        if streaming {
            payload["stream_options"] = json!({"include_usage": true});
        }
        if let Some(t) = self.temperature {
            if !TEMP_UNSUPPORTED.load(Ordering::Relaxed) {
                payload["temperature"] = json!(t);
            }
        }
        if !MAX_TOKENS_UNSUPPORTED.load(Ordering::Relaxed) {
            payload["max_tokens"] = json!(max_completion_tokens());
        }
        if let Some(tools) = tools {
            payload["tools"] = json!(tools);
        }
        let body = payload.to_string();

        // One transport call: the bytes of a reply, or the assembled message when the
        // endpoint streamed it. Native speaks HTTP; the browser backend speaks to the
        // in-page model (docs/frontends/wasm.md#the-model).
        // A reply without usage is billed by estimate: what went out and what came back.
        let sent_estimate = (body.chars().count() as u64).div_ceil(4);
        settle(
            self.transport_cancellable(&body, streaming, cancelled, delta)?,
            sent_estimate,
        )
    }

    // Native HTTP waits hold only an owned request, never the session or its tools.
    // A late response is dropped when the caller stops waiting. Hosted transports
    // enforce cancellation themselves and must retain their thread-local scope.
    fn transport_cancellable(
        &self,
        body: &str,
        streaming: bool,
        cancelled: &dyn Fn() -> bool,
        delta: &mut dyn FnMut(&str, &str),
    ) -> Result<Transported, String> {
        if cancelled() {
            return Err("execution cancelled".into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !crate::vfs::is_scoped() {
            let client = self.clone();
            let body = body.to_owned();
            let (tx, rx) = std::sync::mpsc::channel();
            let (delta_tx, delta_rx) = std::sync::mpsc::channel::<(String, String)>();
            std::thread::spawn(move || {
                let _ = tx.send(client.transport(&body, streaming, &mut |field, text| {
                    let _ = delta_tx.send((field.into(), text.into()));
                }));
            });
            let flush = |delta: &mut dyn FnMut(&str, &str)| {
                let mut field = String::new();
                let mut text = String::new();
                for (next_field, next_text) in delta_rx.try_iter() {
                    if !field.is_empty() && field != next_field {
                        delta(&field, &text);
                        text.clear();
                    }
                    field = next_field;
                    text.push_str(&next_text);
                }
                if !text.is_empty() {
                    delta(&field, &text);
                }
            };
            loop {
                flush(delta);
                if cancelled() {
                    return Err("execution cancelled".into());
                }
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(result) => {
                        flush(delta);
                        return result;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => (),
                    Err(_) => return Err("model transport stopped".into()),
                }
            }
        }
        self.transport(body, streaming, delta)
    }

    // The HTTP transport. Everything above it is protocol, shared by both targets.
    #[cfg(not(target_arch = "wasm32"))]
    fn transport(
        &self,
        body: &str,
        streaming: bool,
        delta: &mut dyn FnMut(&str, &str),
    ) -> Result<Transported, String> {
        if crate::vfs::is_scoped() {
            return crate::hosted::transport_deltas(body, delta).map(Transported::Body);
        }
        // Bound concurrent requests across all worker threads, and pace request starts.
        let _permit = acquire();
        pace();

        // Bounded reads keep a stalled endpoint from holding a turn open indefinitely.
        let read_timeout = crate::sys::env_var("JAZYK_READ_TIMEOUT")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(300)
            .max(10);
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(read_timeout))
            .timeout_write(Duration::from_secs(60))
            .build();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut req = agent.post(&url).set("Content-Type", "application/json");
        if !self.api_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.api_key));
        }
        let resp = match req.send_string(body) {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                return Err(format!(
                    "http error: HTTP {} :: {}",
                    code,
                    truncate(&text, 300)
                ));
            }
            Err(ureq::Error::Transport(t)) => {
                return Err(format!("transport: {}", t));
            }
        };

        if streaming
            && !resp
                .header("content-type")
                .unwrap_or_default()
                .contains("application/json")
        {
            // The same byte cap as the whole-body path: a proxy that never ends a
            // line cannot grow the buffer without bound.
            return read_stream_message_deltas(
                BufReader::new(resp.into_reader().take(64 * 1024 * 1024)),
                max_completion_tokens(),
                call_timeout(),
                delta,
            )
            .map(Transported::Message);
        }

        // The same wall clock the stream path enforces: per-read timeouts bound the
        // gap between bytes, this bounds the whole body, so no call outlives
        // JAZYK_CALL_TIMEOUT by more than one read.
        let deadline = crate::sys::Instant::now() + call_timeout();
        let mut reader = resp.into_reader().take(64 * 1024 * 1024);
        let mut bytes: Vec<u8> = Vec::new();
        let mut buf = [0u8; 65536];
        loop {
            if crate::sys::Instant::now() > deadline {
                return Err(format!(
                    "call timeout: no complete response within {}s (JAZYK_CALL_TIMEOUT)",
                    call_timeout().as_secs()
                ));
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => bytes.extend_from_slice(&buf[..n]),
                Err(e) => return Err(format!("read: {}", e)),
            }
        }
        let resp_body = String::from_utf8_lossy(&bytes).to_string();
        Ok(Transported::Body(resp_body))
    }

    // The browser transport. The page holds the model, so a call hands the same
    // OpenAI-shaped request out to the host and blocks until the host hands a reply
    // back. Streaming has no transport to stream over: the host answers once.
    #[cfg(target_arch = "wasm32")]
    fn transport(
        &self,
        body: &str,
        _streaming: bool,
        _delta: &mut dyn FnMut(&str, &str),
    ) -> Result<Transported, String> {
        crate::wasm::llm_bridge::chat_deltas(body, _delta).map(Transported::Body)
    }
}

// What a transport hands back: the bytes of a reply, or, when the endpoint streamed,
// the message already assembled from the chunks.
enum Transported {
    Body(String),
    Message((Value, Spend)),
}

// The message and its spend from what the transport handed back. A streamed message
// arrives with its spend; unmeasured, it still owes what went out. A body is parsed.
// `sent_estimate` is the request's size in tokens, a quarter token per character.
fn settle(t: Transported, sent_estimate: u64) -> Result<(Value, Spend), String> {
    match t {
        Transported::Message((m, mut spend)) => {
            if !spend.measured {
                spend.billed = spend.billed.saturating_add(sent_estimate);
            }
            Ok((m, spend))
        }
        Transported::Body(b) => parse_completion(&b, sent_estimate),
    }
}

// A non-streaming chat completion body: its first choice's message with the finish
// reason riding on it, and its spend. With a usage object the spend is measured
// (billed is its total, or prompt plus completion); without one the completion is
// estimated from what came back and billed adds what went out.
// Mirrors docs/compiler/sessions.md#budgets.
fn parse_completion(body: &str, sent_estimate: u64) -> Result<(Value, Spend), String> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| format!("response json: {} :: {}", e, truncate(body, 300)))?;
    let mut msg = v["choices"][0]["message"].clone();
    if msg.is_null() {
        return Err(format!("no message in response :: {}", truncate(body, 300)));
    }
    // The finish reason rides on the message for the loop to read (a `length`
    // finish is a reply the completion cap cut); the loop strips it before the
    // message joins the history. Mirrors docs/frontends/acp.md#the-embedded-agent.
    if let Some(f) = v["choices"][0]["finish_reason"].as_str() {
        msg["finish_reason"] = Value::String(f.to_string());
    }
    let received = (msg["content"].as_str().unwrap_or("").chars().count()
        + msg["tool_calls"].to_string().chars().count() * usize::from(msg["tool_calls"].is_array()))
        as u64;
    let completion = v["usage"]["completion_tokens"]
        .as_u64()
        .unwrap_or_else(|| received.div_ceil(4));
    let spend = match usage_billed(&v["usage"]) {
        Some(billed) => Spend {
            completion,
            billed,
            cached: usage_cached(&v["usage"]),
            measured: true,
        },
        None => Spend {
            completion,
            billed: sent_estimate + completion,
            cached: 0,
            measured: false,
        },
    };
    SPENT_TOKENS.fetch_add(completion, Ordering::Relaxed);
    Ok((msg, spend))
}

// Read a streamed (SSE) chat completion and assemble the assistant message: content
// deltas concatenate; reasoning deltas (`reasoning_content` or `reasoning`) concatenate
// into the same field they arrived in, so the assembled message carries what a
// non-streaming reply would; tool-call deltas accumulate per index (id and name arrive
// first, arguments append across chunks). Non-`data:` lines (blanks, comments) are
// skipped. Two runaway bounds: accumulated content is capped (a server ignoring
// `max_tokens` streams a looping model forever otherwise), and the whole call has a
// wall-clock deadline (per-read timeouts reset on every chunk, so a live stream never
// trips them).
#[cfg(test)]
fn read_stream_message<R: BufRead>(
    reader: R,
    cap_tokens: u64,
    deadline: Duration,
) -> Result<(Value, Spend), String> {
    read_stream_message_deltas(reader, cap_tokens, deadline, &mut |_, _| {})
}

fn read_stream_message_deltas<R: BufRead>(
    reader: R,
    cap_tokens: u64,
    deadline: Duration,
    delta_out: &mut dyn FnMut(&str, &str),
) -> Result<(Value, Spend), String> {
    struct TcAcc {
        id: String,
        name: String,
        args: String,
    }
    let mut content = String::new();
    let mut reasoning_content = String::new();
    let mut reasoning = String::new();
    let mut tcs: Vec<TcAcc> = Vec::new();
    // The provider's call index → the accumulator it currently feeds.
    let mut slot: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let mut usage_tokens: Option<u64> = None;
    let mut usage_billed_tokens: Option<u64> = None;
    let mut usage_cached_tokens = 0u64;
    let mut finish_reason: Option<String> = None;
    let started = crate::sys::Instant::now();
    // Chars per token runs 3 to 4; the slack keeps an honest near-cap reply alive while
    // still bounding a server that ignores `max_tokens`.
    let cap_chars = (cap_tokens as usize).saturating_mul(6);

    for line in reader.lines() {
        let line = line.map_err(|e| format!("read: {}", e))?;
        if started.elapsed() > deadline {
            return Err(format!(
                "runaway completion: call exceeded {}s wall clock (JAZYK_CALL_TIMEOUT)",
                deadline.as_secs()
            ));
        }
        let streamed = content.len()
            + reasoning_content.len()
            + reasoning.len()
            + tcs.iter().map(|t| t.args.len()).sum::<usize>();
        if streamed > cap_chars {
            return Err(format!(
                "runaway completion: streamed past the {} token cap (JAZYK_MAX_COMPLETION_TOKENS); the model is likely looping",
                cap_tokens
            ));
        }
        {
            let line = line.trim_end_matches('\r').to_string();
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                break;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if let Some(u) = v["usage"]["completion_tokens"].as_u64() {
                usage_tokens = Some(u);
            }
            if let Some(b) = usage_billed(&v["usage"]) {
                usage_billed_tokens = Some(b);
                usage_cached_tokens = usage_cached(&v["usage"]);
            }
            if let Some(f) = v["choices"][0]["finish_reason"].as_str() {
                finish_reason = Some(f.to_string());
            }
            let delta = &v["choices"][0]["delta"];
            if let Some(c) = delta["content"].as_str() {
                content.push_str(c);
                delta_out("content", c);
            }
            if let Some(r) = delta["reasoning_content"].as_str() {
                reasoning_content.push_str(r);
                delta_out("reasoning_content", r);
            }
            if let Some(r) = delta["reasoning"].as_str() {
                reasoning.push_str(r);
                delta_out("reasoning", r);
            }
            if let Some(calls) = delta["tool_calls"].as_array() {
                for tc in calls {
                    let provider_idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    let new_id = tc["id"].as_str().filter(|s| !s.is_empty());
                    // A delta carrying a new call id starts a new call even when its
                    // index repeats: some endpoints stream each parallel call whole
                    // under index 0. Mirrors docs/compiler/project-settings.md#llm.
                    let idx = match slot.get(&provider_idx).copied() {
                        Some(i)
                            if new_id
                                .is_some_and(|id| !tcs[i].id.is_empty() && tcs[i].id != id) =>
                        {
                            tcs.push(TcAcc {
                                id: String::new(),
                                name: String::new(),
                                args: String::new(),
                            });
                            tcs.len() - 1
                        }
                        Some(i) => i,
                        None => {
                            tcs.push(TcAcc {
                                id: String::new(),
                                name: String::new(),
                                args: String::new(),
                            });
                            tcs.len() - 1
                        }
                    };
                    slot.insert(provider_idx, idx);
                    if let Some(id) = tc["id"].as_str() {
                        tcs[idx].id = id.to_string();
                    }
                    if let Some(n) = tc["function"]["name"].as_str() {
                        tcs[idx].name = n.to_string();
                    }
                    if let Some(a) = tc["function"]["arguments"].as_str() {
                        tcs[idx].args.push_str(a);
                    }
                }
            }
        }
    }

    if content.is_empty() && tcs.is_empty() && reasoning_content.is_empty() && reasoning.is_empty()
    {
        return Err("empty stream response".to_string());
    }
    let tokens = usage_tokens.unwrap_or_else(|| {
        ((content.chars().count()
            + reasoning_content.chars().count()
            + reasoning.chars().count()
            + tcs.iter().map(|t| t.args.chars().count()).sum::<usize>()) as u64)
            .div_ceil(4)
    });
    SPENT_TOKENS.fetch_add(tokens, Ordering::Relaxed);
    // Billed is the usage's total; without one the caller adds what went out.
    let spend = match usage_billed_tokens {
        Some(billed) => Spend {
            completion: tokens,
            billed,
            cached: usage_cached_tokens,
            measured: true,
        },
        None => Spend {
            completion: tokens,
            billed: tokens,
            cached: 0,
            measured: false,
        },
    };
    let mut msg = json!({"role": "assistant", "content": content});
    if let Some(f) = finish_reason {
        msg["finish_reason"] = json!(f);
    }
    if !reasoning_content.is_empty() {
        msg["reasoning_content"] = json!(reasoning_content);
    }
    if !reasoning.is_empty() {
        msg["reasoning"] = json!(reasoning);
    }
    if !tcs.is_empty() {
        msg["tool_calls"] = json!(tcs
            .iter()
            .map(|t| {
                json!({"id": t.id, "type": "function", "function": {"name": t.name, "arguments": t.args}})
            })
            .collect::<Vec<_>>());
    }
    Ok((msg, spend))
}

// Extract the first balanced JSON object from possibly noisy model output. The text codec
// parses actions with this.
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

#[cfg(test)]
mod tests {

    // A boost doubles the cap until chat_messages' guard clears it.
    // Mirrors docs/compiler/project-settings.md#environment-tuning.
    #[test]
    fn a_boost_doubles_one_call_and_clears() {
        let base = base_completion_tokens();
        boost_next_completion();
        assert_eq!(max_completion_tokens(), base * 2);
        BOOST_NEXT.store(false, Ordering::Relaxed);
        assert_eq!(max_completion_tokens(), base);
    }

    use super::*;

    fn sse(chunks: &[&str]) -> String {
        chunks
            .iter()
            .map(|c| format!("data: {}\n\n", c))
            .collect::<String>()
            + "data: [DONE]\n\n"
    }

    #[test]
    fn stream_delta_callback_keeps_channels_separate_and_complete_history() {
        let body = sse(&[
            r#"{"choices":[{"delta":{"reasoning_content":"Consider "}}]}"#,
            r#"{"choices":[{"delta":{"content":"Hello "}}]}"#,
            r#"{"choices":[{"delta":{"content":"there"},"finish_reason":"stop"}]}"#,
        ]);
        let mut deltas = Vec::new();
        let (message, _) = read_stream_message_deltas(
            body.as_bytes(),
            4096,
            Duration::from_secs(5),
            &mut |field, text| deltas.push((field.to_string(), text.to_string())),
        )
        .unwrap();
        assert_eq!(
            deltas,
            vec![
                ("reasoning_content".into(), "Consider ".into()),
                ("content".into(), "Hello ".into()),
                ("content".into(), "there".into())
            ]
        );
        assert_eq!(message["content"], "Hello there");
        assert_eq!(message["reasoning_content"], "Consider ");
    }

    #[test]
    fn stream_keeps_reasoning_deltas() {
        let body = sse(&[
            r#"{"choices":[{"delta":{"reasoning_content":"the section "}}]}"#,
            r#"{"choices":[{"delta":{"reasoning_content":"states a fact"}}]}"#,
            r#"{"choices":[{"delta":{"content":"Recording it."}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"done","arguments":"{}"}}]}}]}"#,
        ]);
        let (msg, _tokens) =
            read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
        assert_eq!(msg["reasoning_content"], "the section states a fact");
        assert_eq!(msg["content"], "Recording it.");
        assert_eq!(msg["tool_calls"][0]["function"]["name"], "done");
    }

    // Parallel calls streamed whole under one index stay separate calls, while a
    // call split across deltas under its index still joins.
    // Mirrors docs/compiler/project-settings.md#llm.
    #[test]
    fn stream_separates_parallel_calls_that_share_an_index() {
        let body = sse(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"read_text_file","arguments":"{\"path\":\"a.md\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_b","function":{"name":"read_text_file","arguments":"{\"path\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"b.md\"}"}}]}}]}"#,
        ]);
        let (msg, _tokens) =
            read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
        let calls = msg["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2, "{}", msg);
        assert_eq!(calls[0]["function"]["arguments"], r#"{"path":"a.md"}"#);
        assert_eq!(calls[1]["id"], "call_b");
        assert_eq!(calls[1]["function"]["arguments"], r#"{"path":"b.md"}"#);
    }

    #[test]
    fn stream_keeps_reasoning_field_variant() {
        let body = sse(&[r#"{"choices":[{"delta":{"reasoning":"thinking"}}]}"#]);
        let (msg, _tokens) =
            read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
        assert_eq!(msg["reasoning"], "thinking");
        assert!(msg.get("reasoning_content").is_none());
    }

    #[test]
    fn stream_past_cap_is_a_runaway() {
        // A looping model streams the same phrase forever; the reader stops at the cap.
        let chunk = r#"{"choices":[{"delta":{"content":"same words again and again "}}]}"#;
        let body = sse(&vec![chunk; 400]);
        let err = read_stream_message(body.as_bytes(), 256, Duration::from_secs(600)).unwrap_err();
        assert!(err.contains("runaway completion"), "{}", err);
    }

    // A session's cost is what the endpoint bills: input and output, read from the
    // usage the stream ends with; without usage, an estimate the caller completes.
    // Mirrors docs/compiler/sessions.md#budgets.
    #[test]
    fn stream_spend_bills_input_and_output() {
        let body = sse(&[
            r#"{"choices":[{"delta":{"content":"ok"}}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":9749,"completion_tokens":31,"total_tokens":9780}}"#,
        ]);
        let (_msg, spend) =
            read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
        assert_eq!(
            spend,
            Spend {
                completion: 31,
                billed: 9780,
                cached: 0,
                measured: true
            }
        );
        let body = sse(&[
            r#"{"choices":[{"delta":{"content":"ok"}}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":5}}"#,
        ]);
        let (_msg, spend) =
            read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
        assert_eq!(spend.billed, 105);
        let body = sse(&[r#"{"choices":[{"delta":{"content":"eight ch"}}]}"#]);
        let (_msg, spend) =
            read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
        assert!(!spend.measured);
        assert_eq!(spend.completion, 2);
    }

    // A non-streaming reply with a usage total is billed that total, measured, and
    // the finish reason rides on the message. Mirrors docs/compiler/sessions.md#budgets.
    #[test]
    fn nonstream_usage_total_is_billed_measured() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":100,"completion_tokens":7,"total_tokens":107}}"#;
        let (msg, spend) = parse_completion(body, 50).unwrap();
        assert_eq!(msg["content"], "ok");
        assert_eq!(msg["finish_reason"], "stop");
        assert_eq!(
            spend,
            Spend {
                completion: 7,
                billed: 107,
                cached: 0,
                measured: true
            }
        );
        // No total: prompt plus completion.
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"prompt_tokens":100,"completion_tokens":7}}"#;
        let (_msg, spend) = parse_completion(body, 50).unwrap();
        assert_eq!(
            spend,
            Spend {
                completion: 7,
                billed: 107,
                cached: 0,
                measured: true
            }
        );
    }

    // Without usage the completion is a quarter token per character of what came
    // back (content and the tool calls' text) and billed adds the request estimate.
    #[test]
    fn nonstream_without_usage_bills_sent_plus_received() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"eight ch"}}]}"#;
        let (_msg, spend) = parse_completion(body, 50).unwrap();
        assert_eq!(
            spend,
            Spend {
                completion: 2,
                billed: 52,
                cached: 0,
                measured: false
            }
        );
        // Tool calls count by their serialized text; a null content counts nothing.
        let body = r#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"done","arguments":"{}"}}]}}]}"#;
        let (msg, spend) = parse_completion(body, 50).unwrap();
        let text = msg["tool_calls"].to_string().chars().count() as u64;
        assert_eq!(
            spend,
            Spend {
                completion: text.div_ceil(4),
                billed: 50 + text.div_ceil(4),
                cached: 0,
                measured: false
            }
        );
        // A body without a message is an error naming the body.
        let err = parse_completion(r#"{"choices":[]}"#, 50).unwrap_err();
        assert!(err.starts_with("no message in response"), "{}", err);
        assert!(parse_completion("not json", 50)
            .unwrap_err()
            .starts_with("response json"));
    }

    // A usage object naming only completion tokens bills nothing on its own: the
    // completion is taken as reported, the input stays an estimate, and the spend
    // is unmeasured.
    #[test]
    fn usage_without_prompt_tokens_is_unmeasured() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"completion_tokens":7}}"#;
        let (_msg, spend) = parse_completion(body, 50).unwrap();
        assert_eq!(
            spend,
            Spend {
                completion: 7,
                billed: 57,
                cached: 0,
                measured: false
            }
        );
        assert_eq!(usage_billed(&json!({"completion_tokens": 7})), None);
        assert_eq!(usage_billed(&json!({"prompt_tokens": 3})), Some(3));
    }

    // Cached input rides beside the bill under either provider's name: OpenAI's
    // `prompt_tokens_details.cached_tokens` or Anthropic's `cache_read_input_tokens`,
    // read on a body and on a stream alike, 0 when neither is reported
    // (docs/compiler/sessions.md#budgets).
    #[test]
    fn cached_input_tokens_are_read_under_both_names_on_both_paths() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"prompt_tokens":100,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":64}}}"#;
        let (_msg, spend) = parse_completion(body, 50).unwrap();
        assert_eq!(
            spend,
            Spend {
                completion: 7,
                billed: 107,
                cached: 64,
                measured: true
            }
        );
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"prompt_tokens":100,"completion_tokens":7,"cache_read_input_tokens":80}}"#;
        let (_msg, spend) = parse_completion(body, 50).unwrap();
        assert_eq!(spend.cached, 80);
        assert_eq!(usage_cached(&json!({"prompt_tokens": 3})), 0);
        // The stream's closing usage chunk carries it the same way.
        let body = sse(&[
            r#"{"choices":[{"delta":{"content":"hi"}}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":9749,"completion_tokens":31,"total_tokens":9780,"prompt_tokens_details":{"cached_tokens":9000}}}"#,
        ]);
        let (_msg, spend) =
            read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
        assert_eq!(
            spend,
            Spend {
                completion: 31,
                billed: 9780,
                cached: 9000,
                measured: true
            }
        );
        let body = sse(&[
            r#"{"choices":[{"delta":{"content":"hi"}}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":5,"cache_read_input_tokens":90}}"#,
        ]);
        let (_msg, spend) =
            read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
        assert_eq!((spend.billed, spend.cached), (105, 90));
    }

    // A streamed message the transport already assembled keeps a measured spend as
    // is; an unmeasured one adds the request estimate to what the stream counted.
    #[test]
    fn a_streamed_unmeasured_message_adds_sent_estimate() {
        let msg = json!({"role": "assistant", "content": "ok"});
        let (m, spend) = settle(
            Transported::Message((
                msg.clone(),
                Spend {
                    completion: 2,
                    billed: 2,
                    cached: 0,
                    measured: false,
                },
            )),
            50,
        )
        .unwrap();
        assert_eq!(m, msg);
        assert_eq!(
            spend,
            Spend {
                completion: 2,
                billed: 52,
                cached: 0,
                measured: false
            }
        );
        let (_m, spend) = settle(
            Transported::Message((
                msg,
                Spend {
                    completion: 31,
                    billed: 9780,
                    cached: 0,
                    measured: true,
                },
            )),
            50,
        )
        .unwrap();
        assert_eq!(
            spend,
            Spend {
                completion: 31,
                billed: 9780,
                cached: 0,
                measured: true
            }
        );
    }

    #[test]
    fn stream_without_reasoning_adds_no_field() {
        let body = sse(&[r#"{"choices":[{"delta":{"content":"plain"}}]}"#]);
        let (msg, _tokens) =
            read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
        assert_eq!(msg["content"], "plain");
        assert!(msg.get("reasoning_content").is_none());
        assert!(msg.get("reasoning").is_none());
    }
    #[test]
    fn stop_releases_a_waiting_model_request_before_the_provider_replies() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, wait) = std::sync::mpsc::channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = cancel.clone();
        let provider = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 8192];
            stream.read(&mut request).unwrap();
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            wait.recv().unwrap();
            let body = r#"{"choices":[{"message":{"role":"assistant","content":"late"}}]}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        });
        let llm = Llm {
            base_url: format!("http://{address}"),
            model: "test".into(),
            api_key: String::new(),
            temperature: None,
            trace: None,
        };
        let started = std::time::Instant::now();
        let result = llm.chat_messages_cancellable(
            &[json!({"role":"user","content":"test"})],
            None,
            "stop-test",
            "r1",
            &|| cancel.load(std::sync::atomic::Ordering::SeqCst),
        );
        assert_eq!(result.unwrap_err(), "execution cancelled");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        release.send(()).unwrap();
        provider.join().unwrap();
    }
}
