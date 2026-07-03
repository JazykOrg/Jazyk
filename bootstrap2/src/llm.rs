// Minimal OpenAI-compatible chat client over raw TCP (works against Ollama at /v1).
// Extended for turns: message-history requests with native tool-calling, plus a sticky
// capability probe that downgrades to the text codec when the endpoint rejects `tools`.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
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
}

impl Llm {
    // One turn round: send the full message history, optionally with tool definitions, and
    // return the assistant message object (`content` and, when the model called tools,
    // `tool_calls`). Transport failures retry immediately; a `tools` rejection surfaces as
    // Err so the turn harness can downgrade the codec.
    pub fn chat_messages(&self, messages: &[Value], tools: Option<&[Value]>, label: &str) -> Result<Value, String> {
        let max = max_retries();
        let mut last = String::new();
        let started = std::time::Instant::now();
        if verbose() {
            eprintln!("[jazyk] → {}", label);
        }
        for attempt in 0..=max {
            match self.chat_once(messages, tools) {
                Ok(msg) => {
                    if verbose() {
                        eprintln!("[jazyk] ✓ {} ({} ms)", label, started.elapsed().as_millis());
                    }
                    return Ok(msg);
                }
                Err(e) => {
                    // An endpoint that only serves streaming responses says so; switch
                    // once, sticky for the run, and retry.
                    if e.to_lowercase().contains("stream must be set to true")
                        && !STREAM_REQUIRED.swap(true, Ordering::Relaxed)
                    {
                        eprintln!("[jazyk] endpoint requires streaming; switching to SSE for the rest of the run");
                        continue;
                    }
                    // A model that rejects `temperature` answers 400 (often wrapped by a
                    // proxy). Drop the parameter once, sticky for the run, and retry.
                    let looks_400 = e.contains("400") || e.to_lowercase().contains("temperature");
                    if looks_400 && self.temperature.is_some() && !TEMP_UNSUPPORTED.swap(true, Ordering::Relaxed) {
                        eprintln!("[jazyk] model rejected the request (likely temperature); retrying without it for the rest of the run");
                        continue;
                    }
                    if tools.is_some() && rejects_tools(&e) && !is_transient(&e) {
                        return Err(format!("tools-rejected: {}", e));
                    }
                    last = e;
                    if attempt < max && is_transient(&last) {
                        eprintln!(
                            "[jazyk] {} — transient error, retrying ({}/{}): {}",
                            label,
                            attempt + 1,
                            max,
                            truncate(&last, 120)
                        );
                    } else {
                        break;
                    }
                }
            }
        }
        if verbose() {
            eprintln!("[jazyk] ✗ {} ({} ms): {}", label, started.elapsed().as_millis(), truncate(&last, 120));
        }
        Err(last)
    }

    // Simple one-shot text chat (no history, no tools). Used by small utility paths.
    #[allow(dead_code)]
    pub fn chat(&self, system: &str, user: &str, label: &str) -> Result<String, String> {
        let messages = [json!({"role": "system", "content": system}), json!({"role": "user", "content": user})];
        let msg = self.chat_messages(&messages, None, label)?;
        Ok(msg["content"].as_str().unwrap_or("").to_string())
    }

    fn chat_once(&self, messages: &[Value], tools: Option<&[Value]>) -> Result<Value, String> {
        let streaming = STREAM_REQUIRED.load(Ordering::Relaxed);
        let mut payload = json!({
            "model": self.model,
            "stream": streaming,
            "messages": messages,
        });
        if streaming {
            payload["stream_options"] = json!({"include_usage": true});
        }
        if let Some(t) = self.temperature {
            if !TEMP_UNSUPPORTED.load(Ordering::Relaxed) {
                payload["temperature"] = json!(t);
            }
        }
        if let Some(tools) = tools {
            payload["tools"] = json!(tools);
        }
        let body = payload.to_string();

        // Bound concurrent requests across all worker threads.
        let _permit = acquire();

        let (host, port, base_path) = parse_url(&self.base_url)?;
        let path = format!("{}/chat/completions", base_path);
        let addr = format!("{}:{}", host, port);
        let mut conn = TcpStream::connect(&addr).map_err(|e| format!("connect {}: {}", addr, e))?;
        // Bounded reads keep a stalled endpoint from holding a turn open indefinitely.
        let read_timeout = std::env::var("JAZYK_READ_TIMEOUT")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(300)
            .max(10);
        conn.set_read_timeout(Some(Duration::from_secs(read_timeout))).ok();
        conn.set_write_timeout(Some(Duration::from_secs(60))).ok();
        let auth = if self.api_key.is_empty() {
            String::new()
        } else {
            format!("Authorization: Bearer {}\r\n", self.api_key)
        };
        let req = format!(
            "POST {} HTTP/1.0\r\nHost: {}\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            path,
            host,
            auth,
            body.as_bytes().len(),
            body
        );
        conn.write_all(req.as_bytes()).map_err(|e| format!("write: {}", e))?;

        if streaming {
            return read_stream_message(&mut conn);
        }

        let mut buf = Vec::new();
        conn.read_to_end(&mut buf).map_err(|e| format!("read: {}", e))?;
        let text = String::from_utf8_lossy(&buf).to_string();
        let sep = text.find("\r\n\r\n").ok_or("no http body separator")?;
        let head = &text[..sep];
        let resp_body = &text[sep + 4..];
        let ok = head.lines().next().map(|l| l.contains(" 200")).unwrap_or(false);
        if !ok {
            return Err(format!(
                "http error: {} :: {}",
                head.lines().next().unwrap_or(""),
                truncate(resp_body, 300)
            ));
        }
        let v: Value = serde_json::from_str(resp_body)
            .map_err(|e| format!("response json: {} :: {}", e, truncate(resp_body, 300)))?;
        let msg = v["choices"][0]["message"].clone();
        if msg.is_null() {
            return Err(format!("no message in response :: {}", truncate(resp_body, 300)));
        }
        let tokens = v["usage"]["completion_tokens"]
            .as_u64()
            .unwrap_or_else(|| (msg["content"].as_str().unwrap_or("").chars().count() as u64).div_ceil(4));
        SPENT_TOKENS.fetch_add(tokens, Ordering::Relaxed);
        Ok(msg)
    }
}

// Read a streamed (SSE) chat completion and assemble the assistant message: content
// deltas concatenate; tool-call deltas accumulate per index (id and name arrive first,
// arguments append across chunks). Non-`data:` lines (chunk sizes, blanks) are skipped.
fn read_stream_message(conn: &mut TcpStream) -> Result<Value, String> {
    struct TcAcc {
        id: String,
        name: String,
        args: String,
    }
    let mut raw: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    let mut headers_done = false;
    let mut pending = String::new();
    let mut content = String::new();
    let mut tcs: Vec<TcAcc> = Vec::new();
    let mut usage_tokens: Option<u64> = None;
    let mut done = false;

    loop {
        let n = match conn.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(format!("read: {}", e)),
        };
        if !headers_done {
            raw.extend_from_slice(&buf[..n]);
            let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
            let head = String::from_utf8_lossy(&raw[..pos]).to_string();
            let head_line = head.lines().next().unwrap_or("").to_string();
            if !head_line.contains(" 200") {
                // Drain and surface the body so the fallback logic sees the error text.
                let mut rest = raw[pos + 4..].to_vec();
                while let Ok(m) = conn.read(&mut buf) {
                    if m == 0 {
                        break;
                    }
                    rest.extend_from_slice(&buf[..m]);
                }
                let body = String::from_utf8_lossy(&rest);
                return Err(format!("http error: {} :: {}", head_line, truncate(&body, 300)));
            }
            headers_done = true;
            pending.push_str(&String::from_utf8_lossy(&raw[pos + 4..]));
        } else {
            pending.push_str(&String::from_utf8_lossy(&buf[..n]));
        }

        while let Some(nl) = pending.find('\n') {
            let line = pending[..nl].trim_end_matches('\r').to_string();
            pending.drain(..nl + 1);
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                done = true;
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
        if done {
            break;
        }
    }

    if content.is_empty() && tcs.is_empty() {
        return Err("empty stream response".to_string());
    }
    let tokens = usage_tokens.unwrap_or_else(|| {
        ((content.chars().count() + tcs.iter().map(|t| t.args.chars().count()).sum::<usize>()) as u64).div_ceil(4)
    });
    SPENT_TOKENS.fetch_add(tokens, Ordering::Relaxed);
    let mut msg = json!({"role": "assistant", "content": content});
    if !tcs.is_empty() {
        msg["tool_calls"] = json!(tcs
            .iter()
            .map(|t| {
                json!({"id": t.id, "type": "function", "function": {"name": t.name, "arguments": t.args}})
            })
            .collect::<Vec<_>>());
    }
    Ok(msg)
}

fn parse_url(u: &str) -> Result<(String, u16, String), String> {
    let rest = u.strip_prefix("http://").or_else(|| u.strip_prefix("https://")).unwrap_or(u);
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (host, port) = match hostport.find(':') {
        Some(i) => (hostport[..i].to_string(), hostport[i + 1..].parse::<u16>().unwrap_or(80)),
        None => (hostport.to_string(), 80),
    };
    Ok((host, port, path.trim_end_matches('/').to_string()))
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
