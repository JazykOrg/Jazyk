// OpenAI-compatible chat client over ureq. What is ours is the provider-behavior
// handling: message-history requests with native tool-calling, a sticky capability
// probe that downgrades to the text codec when the endpoint rejects `tools`, sticky
// temperature and streaming fallbacks, pacing, and retry policy.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

// Codec capability, learned once per run. 0 = unknown, 1 = native tools, 2 = text fallback.
static TOOLS_MODE: AtomicU8 = AtomicU8::new(0);

// Set once a model rejects the temperature parameter (some only allow their default);
// the rest of the run omits it. Providers often wrap the rejection as a bare 400, so the
// first hard 400 with a temperature set triggers the drop-and-retry.
static TEMP_UNSUPPORTED: AtomicBool = AtomicBool::new(false);

// Set once an endpoint answers "stream must be set to true"; the rest of the run uses
// streaming requests and assembles the message from SSE deltas.
static STREAM_REQUIRED: AtomicBool = AtomicBool::new(false);

// Set once a provider rejects reasoning fields echoed back in the message history
// (some 400 when `reasoning_content` appears in an input message). The rest of the run
// strips them from outgoing messages; the turn's own transcript keeps them.
static REASONING_UNSUPPORTED: AtomicBool = AtomicBool::new(false);

// Set once a provider rejects the `max_tokens` completion cap; the rest of the run
// omits it and relies on the stream reader's own cap.
static MAX_TOKENS_UNSUPPORTED: AtomicBool = AtomicBool::new(false);

pub fn tools_mode() -> u8 {
    TOOLS_MODE.load(Ordering::Relaxed)
}

pub fn set_tools_mode(mode: u8) {
    TOOLS_MODE.store(mode, Ordering::Relaxed);
}

// Token meter: completion tokens across all calls this run, for status.yaml reporting.
static SPENT_TOKENS: AtomicU64 = AtomicU64::new(0);

pub fn tokens_spent() -> u64 {
    SPENT_TOKENS.load(Ordering::Relaxed)
}

// Tokens an ACP agent reported for work it ran on jazyk's behalf, folded into the
// same meter status.yaml reads.
pub fn add_tokens(n: u64) {
    SPENT_TOKENS.fetch_add(n, Ordering::Relaxed);
}

// Verbose request logging, enabled by the CLI or the JAZYK_VERBOSE env var.
static VERBOSE: AtomicBool = AtomicBool::new(false);
static VERBOSE_INIT: AtomicBool = AtomicBool::new(false);
pub fn set_verbose(on: bool) {
    VERBOSE.store(on, Ordering::Relaxed);
    VERBOSE_INIT.store(true, Ordering::Relaxed);
}
fn verbose() -> bool {
    if !VERBOSE_INIT.load(Ordering::Relaxed) {
        let on = std::env::var("JAZYK_VERBOSE").map(|v| !v.is_empty() && v != "0").unwrap_or(false);
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
        let n = std::env::var("JAZYK_MAX_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(6)
            .max(1);
        Semaphore { permits: Mutex::new(n), cv: Condvar::new() }
    })
}
// Minimum gap between request starts, so tight failure loops cannot hammer an endpoint.
// Tunable with JAZYK_MIN_INTERVAL_MS; default 500.
static LAST_REQUEST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
fn pace() {
    let min_ms = std::env::var("JAZYK_MIN_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(500);
    if min_ms == 0 {
        return;
    }
    let wait = {
        let mut last = LAST_REQUEST.lock().unwrap();
        let now = std::time::Instant::now();
        let wait = match *last {
            Some(t) => Duration::from_millis(min_ms).saturating_sub(now.duration_since(t)),
            None => Duration::ZERO,
        };
        *last = Some(now + wait);
        wait
    };
    if !wait.is_zero() {
        std::thread::sleep(wait);
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
    std::env::var("JAZYK_MAX_RETRIES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(2)
}

// Completion cap for one response, sent as `max_tokens` and enforced again on streamed
// content. This is the loop detector: a small model stuck repeating tokens is bounded
// by the cap, not by its context window. Tunable with JAZYK_MAX_COMPLETION_TOKENS;
// default 4096. See docs/compiler/project-settings.md#environment-tuning.
fn max_completion_tokens() -> u64 {
    std::env::var("JAZYK_MAX_COMPLETION_TOKENS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(4096)
        .max(256)
}

// Wall clock for one whole LLM call. JAZYK_READ_TIMEOUT waits for the next byte; this
// bounds a call whose bytes keep arriving. Tunable with JAZYK_CALL_TIMEOUT; default 600.
fn call_timeout() -> Duration {
    let secs = std::env::var("JAZYK_CALL_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600)
        .max(30);
    Duration::from_secs(secs)
}

// Whether an error looks transient (worth retrying) versus a hard client error.
fn is_transient(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("502")
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
    pub trace: Option<crate::turn::Trace>,
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
                                let cut = format!("{} … [{} chars total]", truncate(s, MESSAGE_CAP), s.len());
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

impl Llm {
    // A client that reports into this trace. Cheap: the trace is a handle.
    pub fn with_trace(&self, trace: &crate::turn::Trace) -> Llm {
        Llm { trace: Some(trace.clone()), ..self.clone() }
    }

    // A provider-behavior notice (a sticky fallback, a rate-limit wait). It belongs to
    // the work that triggered it, so it goes to the trace when there is one.
    fn note(&self, label: &str, text: &str) {
        match &self.trace {
            Some(t) => t.line(label, text),
            None => eprintln!("[jazyk] {}", text),
        }
    }

    fn event(&self, ev: crate::turn::TraceEvent) {
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
    ) -> Result<(Value, u64), String> {
        let max = max_retries();
        let mut last = String::new();
        let started = std::time::Instant::now();
        if verbose() {
            eprintln!("[jazyk] → {} {}", label, step);
        }
        // The prompt as sent, recorded once for the whole call: retries resend it
        // unchanged, and a retry says so on its own row.
        self.event(crate::turn::TraceEvent::LlmRequest {
            label: label.to_string(),
            step: step.to_string(),
            model: self.model.clone(),
            messages: recorded(messages),
            tools: tool_names(tools),
        });
        let mut try_stream = false;
        for attempt in 0..=max {
            let streaming = STREAM_REQUIRED.load(Ordering::Relaxed) || try_stream;
            match self.chat_once(messages, tools, streaming) {
                Ok((msg, tokens)) => {
                    // A streaming probe that succeeds after a non-streaming failure
                    // sticks for the run.
                    if try_stream && !STREAM_REQUIRED.swap(true, Ordering::Relaxed) {
                        self.note(label, "streaming retry succeeded; using SSE for the rest of the run");
                    }
                    if verbose() {
                        eprintln!("[jazyk] ✓ {} {} ({} ms)", label, step, started.elapsed().as_millis());
                    }
                    self.event(crate::turn::TraceEvent::LlmResponse {
                        label: label.to_string(),
                        step: step.to_string(),
                        ms: started.elapsed().as_millis() as u64,
                        tokens,
                        message: msg.clone(),
                    });
                    return Ok((msg, tokens));
                }
                Err(e) => {
                    // An endpoint that only serves streaming responses says so; switch
                    // once, sticky for the run, and retry.
                    if e.to_lowercase().contains("stream must be set to true")
                        && !STREAM_REQUIRED.swap(true, Ordering::Relaxed)
                    {
                        self.note(label, "endpoint requires streaming; switching to SSE for the rest of the run");
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
                    let looks_400 = e.contains("400") || e.to_lowercase().contains("temperature");
                    if looks_400 && self.temperature.is_some() && !TEMP_UNSUPPORTED.swap(true, Ordering::Relaxed) {
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
                        let wait = if last.to_lowercase().contains("rate limit") { 20 } else { 5 };
                        let ev = crate::turn::TraceEvent::LlmRetry {
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
                        std::thread::sleep(Duration::from_secs(wait));
                    } else {
                        break;
                    }
                }
            }
        }
        if verbose() {
            eprintln!("[jazyk] ✗ {} {} ({} ms): {}", label, step, started.elapsed().as_millis(), truncate(&last, 120));
        }
        // The caller reports the failure itself (a failed turn, a failed entity); this
        // only keeps the structured record complete for a reader of the transcript.
        if let Some(t) = &self.trace {
            t.line(label, &format!("llm call failed after {} ms: {}", started.elapsed().as_millis(), truncate(&last, 400)));
        }
        Err(last)
    }

    // Simple one-shot text chat (no history, no tools). Used by small utility paths.
    #[allow(dead_code)]
    pub fn chat(&self, system: &str, user: &str, label: &str, step: &str) -> Result<String, String> {
        let messages = [json!({"role": "system", "content": system}), json!({"role": "user", "content": user})];
        let (msg, _tokens) = self.chat_messages(&messages, None, label, step)?;
        Ok(msg["content"].as_str().unwrap_or("").to_string())
    }

    fn chat_once(&self, messages: &[Value], tools: Option<&[Value]>, streaming: bool) -> Result<(Value, u64), String> {
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

        // Bound concurrent requests across all worker threads, and pace request starts.
        let _permit = acquire();
        pace();

        // Bounded reads keep a stalled endpoint from holding a turn open indefinitely.
        let read_timeout = std::env::var("JAZYK_READ_TIMEOUT")
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
        let resp = match req.send_string(&body) {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                return Err(format!("http error: HTTP {} :: {}", code, truncate(&text, 300)));
            }
            Err(ureq::Error::Transport(t)) => {
                return Err(format!("transport: {}", t));
            }
        };

        if streaming {
            return read_stream_message(BufReader::new(resp.into_reader()), max_completion_tokens(), call_timeout());
        }

        // The same wall clock the stream path enforces: per-read timeouts bound the
        // gap between bytes, this bounds the whole body, so no call outlives
        // JAZYK_CALL_TIMEOUT by more than one read.
        let deadline = std::time::Instant::now() + call_timeout();
        let mut reader = resp.into_reader().take(64 * 1024 * 1024);
        let mut bytes: Vec<u8> = Vec::new();
        let mut buf = [0u8; 65536];
        loop {
            if std::time::Instant::now() > deadline {
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
        let v: Value = serde_json::from_str(&resp_body)
            .map_err(|e| format!("response json: {} :: {}", e, truncate(&resp_body, 300)))?;
        let msg = v["choices"][0]["message"].clone();
        if msg.is_null() {
            return Err(format!("no message in response :: {}", truncate(&resp_body, 300)));
        }
        let tokens = v["usage"]["completion_tokens"]
            .as_u64()
            .unwrap_or_else(|| (msg["content"].as_str().unwrap_or("").chars().count() as u64).div_ceil(4));
        SPENT_TOKENS.fetch_add(tokens, Ordering::Relaxed);
        Ok((msg, tokens))
    }
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
fn read_stream_message<R: BufRead>(reader: R, cap_tokens: u64, deadline: Duration) -> Result<(Value, u64), String> {
    struct TcAcc {
        id: String,
        name: String,
        args: String,
    }
    let mut content = String::new();
    let mut reasoning_content = String::new();
    let mut reasoning = String::new();
    let mut tcs: Vec<TcAcc> = Vec::new();
    let mut usage_tokens: Option<u64> = None;
    let started = std::time::Instant::now();
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
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                break;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
            if let Some(u) = v["usage"]["completion_tokens"].as_u64() {
                usage_tokens = Some(u);
            }
            let delta = &v["choices"][0]["delta"];
            if let Some(c) = delta["content"].as_str() {
                content.push_str(c);
            }
            if let Some(r) = delta["reasoning_content"].as_str() {
                reasoning_content.push_str(r);
            }
            if let Some(r) = delta["reasoning"].as_str() {
                reasoning.push_str(r);
            }
            if let Some(calls) = delta["tool_calls"].as_array() {
                for tc in calls {
                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    while tcs.len() <= idx {
                        tcs.push(TcAcc { id: String::new(), name: String::new(), args: String::new() });
                    }
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

    if content.is_empty() && tcs.is_empty() && reasoning_content.is_empty() && reasoning.is_empty() {
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
    let mut msg = json!({"role": "assistant", "content": content});
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
    Ok((msg, tokens))
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
    use super::*;

    fn sse(chunks: &[&str]) -> String {
        chunks.iter().map(|c| format!("data: {}\n\n", c)).collect::<String>() + "data: [DONE]\n\n"
    }

    #[test]
    fn stream_keeps_reasoning_deltas() {
        let body = sse(&[
            r#"{"choices":[{"delta":{"reasoning_content":"the section "}}]}"#,
            r#"{"choices":[{"delta":{"reasoning_content":"states a fact"}}]}"#,
            r#"{"choices":[{"delta":{"content":"Recording it."}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"done","arguments":"{}"}}]}}]}"#,
        ]);
        let (msg, _tokens) = read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
        assert_eq!(msg["reasoning_content"], "the section states a fact");
        assert_eq!(msg["content"], "Recording it.");
        assert_eq!(msg["tool_calls"][0]["function"]["name"], "done");
    }

    #[test]
    fn stream_keeps_reasoning_field_variant() {
        let body = sse(&[r#"{"choices":[{"delta":{"reasoning":"thinking"}}]}"#]);
        let (msg, _tokens) = read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
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

    #[test]
    fn stream_without_reasoning_adds_no_field() {
        let body = sse(&[r#"{"choices":[{"delta":{"content":"plain"}}]}"#]);
        let (msg, _tokens) = read_stream_message(body.as_bytes(), 4096, Duration::from_secs(600)).unwrap();
        assert_eq!(msg["content"], "plain");
        assert!(msg.get("reasoning_content").is_none());
        assert!(msg.get("reasoning").is_none());
    }
}
