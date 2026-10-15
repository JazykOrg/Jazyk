// The embedded agent's generic loop: send the history and the session's MCP tools to
// an OpenAI-compatible endpoint, dispatch the calls the model makes, repeat until the
// model stops calling tools. Carries the codec machinery (native tool calls, the text
// fallback, the first-round probe) that used to live in the turn harness; nothing
// here knows anything about jazyk. Mirrors docs/frontends/acp.md#the-embedded-agent.
use crate::llm::{self, Llm};
use serde_json::{json, Value};

// One tool as the model sees it. The name, what it is for, and its JSON Schema.
// Whoever drives the session fills these in: the ACP serving from an MCP handshake,
// the browser backend straight from the tool registry.
#[derive(Clone, Debug)]
pub struct GenericTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

// What the loop reports outward; the serving maps these onto ACP session updates.
pub enum AgentEvent {
    Thought(String),
    Message(String),
    ToolCallStart {
        id: String,
        name: String,
        args: Value,
    },
    ToolCallEnd {
        id: String,
        result: String,
        ok: bool,
    },
    // What the round billed (input and output) and how much of its input the endpoint
    // served from its prompt cache (docs/compiler/sessions.md#budgets).
    Usage {
        tokens: u64,
        cached: u64,
    },
}

pub enum Stop {
    // The model answered without tool calls: the turn is over.
    EndTurn,
    MaxRounds,
    Cancelled,
    Error(String),
}

// The generic system preamble. Deliberately thin: the real instructions arrive as
// prompt content from whoever drives the session.
const SYSTEM: &str = "You are a focused agent. Work with the tools available to you. \
When the work is complete (or no tool applies), reply with a short summary and no tool calls; \
that ends the turn.";

// ---- codecs (moved from the turn harness, generic wording) ----

enum Action {
    Call {
        id: Option<String>,
        name: String,
        args: Value,
    },
    Text(String),
    // A reply that reads as a JSON action but does not parse. Ending the turn on it
    // would treat a dropped brace as a finished answer; the loop repairs instead.
    Malformed(String),
}

trait Codec {
    fn system_suffix(&self, tools: &[GenericTool]) -> String;
    fn tools_param(&self, tools: &[GenericTool]) -> Option<Vec<Value>>;
    fn parse(&self, msg: &Value) -> Vec<Action>;
    fn result_msg(&self, call_id: &Option<String>, name: &str, result: &Value) -> Value;
}

struct NativeCodec;

fn result_text(result: &Value) -> String {
    result.as_str().map(str::to_owned).unwrap_or_else(|| {
        serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string())
    })
}

impl Codec for NativeCodec {
    fn system_suffix(&self, _tools: &[GenericTool]) -> String {
        "\n\nYou may batch several related tool calls into one reply.".to_string()
    }
    fn tools_param(&self, tools: &[GenericTool]) -> Option<Vec<Value>> {
        if tools.is_empty() {
            return None;
        }
        Some(
            tools
                .iter()
                .map(|t| {
                    json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}})
                })
                .collect(),
        )
    }
    fn parse(&self, msg: &Value) -> Vec<Action> {
        let mut out = Vec::new();
        if let Some(text) = msg["content"].as_str() {
            if !text.trim().is_empty() {
                out.push(Action::Text(text.to_string()));
            }
        }
        if let Some(calls) = msg["tool_calls"].as_array() {
            for c in calls {
                let name = c["function"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                // Arguments that do not parse are a cut reply, not an empty call:
                // dispatching `{}` misleads the model and the raw string poisons every
                // later request. Mirrors docs/frontends/acp.md#the-embedded-agent.
                let args = match c["function"]["arguments"].as_str() {
                    Some(s) => match serde_json::from_str::<Value>(s) {
                        Ok(v) => v,
                        Err(e) => {
                            out.push(Action::Malformed(format!(
                                "the arguments of the {} call are not a JSON object ({}; {} chars, likely cut at your output cap)",
                                name, e, s.chars().count()
                            )));
                            continue;
                        }
                    },
                    None => c["function"]["arguments"].clone(),
                };
                out.push(Action::Call {
                    id: c["id"].as_str().map(|s| s.to_string()),
                    name,
                    args,
                });
            }
        }
        out
    }
    // The reply pretty-printed (docs/compiler/tools.md): the model and the person
    // reading the transcript see one field per line.
    fn result_msg(&self, call_id: &Option<String>, name: &str, result: &Value) -> Value {
        let content = result_text(result);
        json!({
            "role": "tool",
            "tool_call_id": call_id.clone().unwrap_or_else(|| name.to_string()),
            "content": content
        })
    }
}

struct TextCodec;

// A prose reply that names the action it is about to take instead of taking it.
// Mirrors docs/frontends/acp.md#the-embedded-agent.
fn announces_action(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let tail: String = t.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ");
    let lower = tail.to_lowercase();
    t.ends_with(':')
        || ["i'll ", "i will ", "let me ", "now i ", "next i ", "next, i ", "i'm going to ", "i am going to "]
            .iter()
            .any(|p| lower.contains(p))
}

// An endpoint error about the reply's own JSON: the model's output was cut before a
// tool call closed. Mirrors docs/frontends/acp.md#the-embedded-agent.
// A mid-turn downgrade rewrites the native history into the text codec's form: each
// call becomes its JSON action in the assistant's content and each result a RESULT:
// message, so no native tool_calls go back through the endpoint's tool template.
// Mirrors docs/frontends/acp.md#the-embedded-agent.
fn history_to_text(history: &mut [Value]) {
    for m in history.iter_mut().skip(1) {
        if m["role"] == "tool" {
            let content = m["content"].as_str().unwrap_or("").to_string();
            *m = json!({"role": "user", "content": format!("RESULT: {}", content)});
            continue;
        }
        if m["role"] != "assistant" {
            continue;
        }
        let Some(calls) = m["tool_calls"].as_array().cloned() else {
            continue;
        };
        let mut text = m["content"].as_str().unwrap_or("").to_string();
        for c in calls {
            let name = c["function"]["name"].as_str().unwrap_or("");
            let args: Value = match &c["function"]["arguments"] {
                Value::String(s) => serde_json::from_str(s).unwrap_or(json!({})),
                other => other.clone(),
            };
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&json!({"tool": name, "args": args}).to_string());
        }
        if let Some(obj) = m.as_object_mut() {
            obj.remove("tool_calls");
            obj.insert("content".into(), json!(text));
        }
    }
}

fn reply_unparsed(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("can't find closing")
        || e.contains("cannot find closing")
        || e.contains("unexpected end of json")
        || e.contains("unexpected eof")
        || e.contains("unterminated string")
}

// Balanced top-level `{...}` blocks in reply text, in order, string-aware: the
// text codec's action extractor.
fn balanced_objects(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let start = i;
            let mut depth = 0i32;
            let mut in_str = false;
            let mut esc = false;
            let mut j = i;
            let mut end = None;
            while j < bytes.len() {
                let c = bytes[j];
                if esc {
                    esc = false;
                } else if in_str {
                    match c {
                        b'\\' => esc = true,
                        b'"' => in_str = false,
                        _ => {}
                    }
                } else {
                    match c {
                        b'"' => in_str = true,
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(j);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                j += 1;
            }
            match end {
                Some(e) => {
                    out.push(content[start..=e].to_string());
                    i = e + 1;
                    continue;
                }
                None => break,
            }
        }
        i += 1;
    }
    out
}

impl Codec for TextCodec {
    fn system_suffix(&self, tools: &[GenericTool]) -> String {
        if tools.is_empty() {
            return String::new();
        }
        let mut s = String::from(
            "\n\nTOOL PROTOCOL: you have no native tool support. To call a tool, reply with EXACTLY ONE JSON object per message, nothing else:\n{\"tool\": \"<name>\", \"args\": { ... }}\nThe result comes back as a message starting with RESULT:. Then reply with your next action. A reply that is not a JSON action ends the turn. Available tools:\n",
        );
        for t in tools {
            s.push_str(&format!(
                "- {}: {} args schema: {}\n",
                t.name, t.description, t.parameters
            ));
        }
        s
    }
    fn tools_param(&self, _tools: &[GenericTool]) -> Option<Vec<Value>> {
        None
    }
    fn parse(&self, msg: &Value) -> Vec<Action> {
        let content = msg["content"].as_str().unwrap_or_default();
        // Every balanced top-level object in the reply, in order: the protocol
        // asks for one action per message, but a model that packs several must
        // see them all executed, never the first with the rest silently dropped
        // (a session that believes 26 goals are marked while one landed spends
        // its rounds re-proposing the other 25).
        let mut calls: Vec<Action> = Vec::new();
        let mut first_err: Option<String> = None;
        for obj in balanced_objects(content) {
            match serde_json::from_str::<Value>(&obj) {
                Ok(v) => {
                    if let Some(name) = v["tool"].as_str() {
                        calls.push(Action::Call {
                            id: None,
                            name: name.to_string(),
                            args: v["args"].clone(),
                        });
                    }
                }
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e.to_string());
                    }
                }
            }
        }
        if !calls.is_empty() {
            return calls;
        }
        if let Some(e) = first_err {
            return vec![Action::Malformed(e)];
        }
        // Content that holds the start of an action but never yielded one is a
        // broken action, not prose: a truncated object must not end the turn as an
        // answer, wherever it sits (a closing sentence once preceded a truncated
        // mark_goal_done and the reply read as prose; the batch parked).
        // Mirrors docs/frontends/acp.md#the-embedded-agent.
        let t = content.trim_start();
        if t.contains('{') && t.contains("\"tool\"") {
            return vec![Action::Malformed(
                "the object is incomplete or unbalanced".to_string(),
            )];
        }
        if content.trim().is_empty() {
            Vec::new()
        } else {
            vec![Action::Text(content.to_string())]
        }
    }
    fn result_msg(&self, _call_id: &Option<String>, _name: &str, result: &Value) -> Value {
        json!({"role": "user", "content": format!("RESULT: {}", result_text(result))})
    }
}

// ---- the loop ----

pub struct LoopArgs<'a> {
    pub llm: &'a Llm,
    // The prompt turn's content, appended to the running history the caller owns.
    pub history: &'a mut Vec<Value>,
    pub tools: &'a [GenericTool],
    pub dispatch: &'a mut dyn FnMut(&str, &Value) -> Result<String, String>,
    pub emit: &'a mut dyn FnMut(AgentEvent),
    pub cancelled: &'a dyn Fn() -> bool,
    pub max_rounds: u32,
    pub label: String,
    // A conversation (a chat turn in a tab): a prose reply after tool calls is the
    // answer and ends the turn, with no finish nudge holding it back.
    // Mirrors docs/frontends/wasm.md#chat.
    pub conversation: bool,
}

pub fn run_loop(a: LoopArgs) -> Stop {
    // Codec selection is sticky per process, learned by the first-round probe or
    // forced by JAZYK_CODEC.
    let mut mode = llm::tools_mode();
    if mode == 0 {
        if let Ok(env) = crate::sys::env_var("JAZYK_CODEC") {
            mode = match env.as_str() {
                "text" => 2,
                "native" => 1,
                _ => 0,
            };
            if mode != 0 {
                llm::set_tools_mode(mode);
            }
        }
    }

    // The system message (with the codec's suffix) leads the history exactly once;
    // a codec downgrade rewrites it in place.
    let codec_for = |mode: u8| -> Box<dyn Codec> {
        if mode == 2 {
            Box::new(TextCodec)
        } else {
            Box::new(NativeCodec)
        }
    };
    let mut codec = codec_for(mode);
    let system = |codec: &dyn Codec, tools: &[GenericTool]| json!({"role": "system", "content": format!("{}{}", SYSTEM, codec.system_suffix(tools))});
    if a.history
        .first()
        .map(|m| m["role"] == "system")
        .unwrap_or(false)
    {
        a.history[0] = system(codec.as_ref(), a.tools);
    } else {
        a.history.insert(0, system(codec.as_ref(), a.tools));
    }

    let mut rounds = 0u32;
    let mut called_any = false;
    let mut nudged = false;
    let mut empty_nudges = 0u32;
    let mut cut_nudges = 0u32;
    let mut intent_nudges = 0u32;
    let mut malformed_streak = 0u32;
    let mut failed_calls = 0u32;
    while rounds < a.max_rounds {
        if (a.cancelled)() {
            return Stop::Cancelled;
        }
        rounds += 1;
        let step = format!("r{}", rounds);
        let tools_param = codec.tools_param(a.tools);
        let msg = match a
            .llm
            .chat_messages_cancellable(a.history, tools_param.as_deref(), &a.label, &step, a.cancelled)
        {
            // The cost a session carries is what the endpoint bills, input and output
            // (docs/compiler/sessions.md#budgets).
            Ok((m, t)) => {
                (a.emit)(AgentEvent::Usage { tokens: t.billed, cached: t.cached });
                m
            }
            Err(_) if (a.cancelled)() => return Stop::Cancelled,
            Err(e) if e.starts_with("tools-rejected:") && mode != 2 => {
                llm::set_tools_mode(2);
                mode = 2;
                codec = codec_for(mode);
                a.history[0] = system(codec.as_ref(), a.tools);
                history_to_text(a.history);
                continue;
            }
            // The endpoint could not parse the reply itself (the output cap cut a
            // long tool call): the same request returns the same cut reply, so a
            // nudge toward a smaller call is the repair, twice at most.
            // Mirrors docs/frontends/acp.md#the-embedded-agent.
            Err(e) if reply_unparsed(&e) && cut_nudges < 2 => {
                cut_nudges += 1;
                (a.emit)(AgentEvent::Message(format!(
                    "the endpoint could not parse the last reply ({}); asking for a smaller call",
                    llm::truncate(&e, 160)
                )));
                a.history.push(json!({"role": "user", "content": "Your last reply was cut off before its tool call closed, so nothing landed. Reply with a smaller call: write a long file in parts (write_text_file for the head, then append: true for the rest), each call a few thousand characters at most."}));
                continue;
            }
            Err(e) => return Stop::Error(e),
        };
        if (a.cancelled)() { return Stop::Cancelled; }
        let mut actions = codec.parse(&msg);
        // First-round probe: a prose-only reply under the native codec, from a model
        // whose capability is still unknown, more often means "cannot drive tools"
        // than "done before starting". Downgrade once, sticky for the process.
        if rounds == 1
            && mode != 2
            && llm::tools_mode() == 0
            && !a.tools.is_empty()
            && !actions.iter().any(|x| matches!(x, Action::Call { .. }))
        {
            llm::set_tools_mode(2);
            mode = 2;
            codec = codec_for(mode);
            a.history[0] = system(codec.as_ref(), a.tools);
            continue;
        }
        if mode != 2
            && llm::tools_mode() == 0
            && actions.iter().any(|x| matches!(x, Action::Call { .. }))
        {
            llm::set_tools_mode(1);
        }
        // The finish reason is the loop's to read, never the provider's to see again.
        let finish_reason = msg["finish_reason"].as_str().map(String::from);
        let mut kept = msg.clone();
        if let Some(o) = kept.as_object_mut() {
            o.remove("finish_reason");
        }
        // A cut arguments string never re-enters the history: the endpoint rejects a
        // request carrying it (docs/frontends/acp.md#the-embedded-agent).
        if let Some(calls) = kept["tool_calls"].as_array_mut() {
            for c in calls.iter_mut() {
                if let Some(s) = c["function"]["arguments"].as_str() {
                    if serde_json::from_str::<Value>(s).is_err() {
                        c["function"]["arguments"] = json!("{}");
                    }
                }
            }
        }
        a.history.push(kept);
        for field in ["reasoning_content", "reasoning"] {
            if let Some(r) = msg[field].as_str() {
                if !r.trim().is_empty() {
                    (a.emit)(AgentEvent::Thought(r.trim().to_string()));
                }
            }
        }

        // A broken action gets a repair message naming the parse error, three
        // strikes before the turn fails: a dropped brace is a resend, not an
        // answer. Mirrors docs/frontends/acp.md#the-embedded-agent.
        if let Some(Action::Malformed(err)) =
            actions.iter().find(|x| matches!(x, Action::Malformed(_)))
        {
            malformed_streak += 1;
            if malformed_streak >= 3 {
                return Stop::Error(format!(
                    "the model cannot produce a parseable action ({})",
                    err
                ));
            }
            let repair = if mode == 2 {
                format!(
                    "Your JSON action did not parse ({}). Resend the complete action as exactly one JSON object: {{\"tool\": \"<name>\", \"args\": {{ ... }}}}, nothing else.",
                    err
                )
            } else {
                format!(
                    "Your tool call did not parse ({}). Nothing ran. Resend it complete; if your output cap cut it, make it smaller: write a long file in parts (write_text_file for the head, then append: true for the rest).",
                    err
                )
            };
            a.history.push(json!({"role": "user", "content": repair}));
            continue;
        }
        malformed_streak = 0;

        // No tool calls: the model is answering, and that ends the turn. A model
        // that already worked this turn gets one nudge first: weak models forget
        // they are mid-task more often than they finish silently, and a pure
        // conversational answer (no calls at all) still ends immediately.
        if !actions.iter().any(|x| matches!(x, Action::Call { .. })) {
            // A reply empty of both message and calls while carrying reasoning is a
            // stall, not an answer: reasoning models narrate the action they intend
            // and stop, as if the thinking were visible. Name that, at most twice.
            // Mirrors docs/frontends/acp.md#the-embedded-agent.
            let has_text = actions
                .iter()
                .any(|x| matches!(x, Action::Text(t) if !t.trim().is_empty()));
            let has_reasoning = ["reasoning_content", "reasoning"].iter().any(|f| {
                msg[*f]
                    .as_str()
                    .map(|r| !r.trim().is_empty())
                    .unwrap_or(false)
            });
            if !has_text && has_reasoning && empty_nudges < 2 {
                empty_nudges += 1;
                // The stalled reply keeps its reasoning as its message text: an
                // OpenAI-compatible endpoint drops reasoning fields from input
                // messages, so without this the model re-thinks from nothing and
                // stalls again. Mirrors docs/frontends/acp.md#the-embedded-agent.
                let reasoning = ["reasoning_content", "reasoning"]
                    .iter()
                    .find_map(|f| msg[*f].as_str().map(|r| r.trim().to_string()))
                    .unwrap_or_default();
                if let Some(last) = a.history.last_mut() {
                    let tail: String = reasoning
                        .chars()
                        .rev()
                        .take(2000)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    last["content"] = json!(format!("(my reasoning, not yet acted on) {}", tail));
                }
                let cut = finish_reason.as_deref() == Some("length");
                if cut {
                    // A plan the cap cut gets room once: the next request alone
                    // carries twice the cap (docs/compiler/project-settings.md).
                    llm::boost_next_completion();
                }
                let nudge = if cut {
                    "Your reasoning ran past the completion cap before any call or message. More thinking cannot fit: reply with the tool call alone, no further deliberation, or state your answer in one line."
                } else {
                    "Your reply was empty. Reasoning is not shown to anyone and does not count as acting: make the tool call you were about to make, or state your answer as plain message text."
                };
                a.history.push(json!({"role": "user", "content": nudge}));
                continue;
            }
            // An announced action ("I'll rewrite src/order.rs", "Now I'll record
            // the generation") is not an answer: say so, up to three times, since a
            // local model ended sessions on such a line with the call never made.
            // Mirrors docs/frontends/acp.md#the-embedded-agent.
            let announced = actions
                .iter()
                .any(|x| matches!(x, Action::Text(t) if announces_action(t)));
            if called_any && announced && intent_nudges < 3 {
                intent_nudges += 1;
                a.history.push(json!({"role": "user", "content":
                    "You announced an action but made no tool call, and announcing is not acting: make that call now. If the work is finished, reply with a one-line summary and nothing else."}));
                continue;
            }
            if called_any && !nudged && !a.conversation {
                nudged = true;
                a.history.push(json!({"role": "user", "content":
                    "If the task is not finished, continue with tool calls. If it is finished, reply with a one-line summary and nothing else."}));
                continue;
            }
            for action in actions {
                if let Action::Text(t) = action {
                    if !t.trim().is_empty() {
                        (a.emit)(AgentEvent::Message(t.trim().to_string()));
                    }
                }
            }
            return Stop::EndTurn;
        }

        let mut call_n = 0u32;
        for action in actions.drain(..) {
            match action {
                // Filtered above; a mixed reply's other actions still run.
                Action::Malformed(_) => {}
                Action::Text(t) => {
                    let t = t.trim();
                    if !t.is_empty() {
                        (a.emit)(AgentEvent::Message(t.to_string()));
                    }
                }
                Action::Call { id, name, args } => {
                    if (a.cancelled)() {
                        return Stop::Cancelled;
                    }
                    called_any = true;
                    call_n += 1;
                    let call_id = id
                        .clone()
                        .unwrap_or_else(|| format!("{}-{}-{}", a.label, rounds, call_n));
                    (a.emit)(AgentEvent::ToolCallStart {
                        id: call_id.clone(),
                        name: name.clone(),
                        args: args.clone(),
                    });
                    // The repeated-call guard lives in the tool serving, keyed per
                    // open batch, so every agent gets the same contract
                    // (docs/compiler/sessions.md#repeated-calls).
                    let dispatched = if failed_calls >= crate::limits::AGENT_TOOL_ERROR_LIMIT {
                        Err("Skipped: the turn stopped after five consecutive tool errors.".into())
                    } else {
                        (a.dispatch)(&name, &args)
                    };
                    let (result, ok) = match dispatched {
                        Ok(v) => (v, true),
                        Err(e) => (e, false),
                    };
                    failed_calls = if ok { 0 } else { failed_calls.saturating_add(1) };
                    (a.emit)(AgentEvent::ToolCallEnd {
                        id: call_id,
                        result: result.clone(),
                        ok,
                    });
                    let value = serde_json::from_str(&result).unwrap_or_else(|_| json!(result));
                    a.history.push(codec.result_msg(&id, &name, &value));
                }
            }
        }
        if failed_calls >= crate::limits::AGENT_TOOL_ERROR_LIMIT {
            return Stop::Error("Stopped after five consecutive tool errors; inspect the failed calls before retrying.".into());
        }
    }
    Stop::MaxRounds
}

#[cfg(test)]
mod tests {

    // The reader serving's first call (docs/frontends/wasm.md#chat): the system
    // preamble with the text codec's tool block, then the instructions, must fit a
    // small model's window with room for the turns. 8k tokens is about 32k
    // characters; the bound here leaves the rest for the conversation.
    #[test]
    fn reader_prompt_fits_a_small_window() {
        let dir = std::env::temp_dir().join(format!("jazyk-reader-prompt-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write({ let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path }, "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n").unwrap();
        let project = crate::project::Project::load(&dir);
        let out = project.out.clone();
        let server = crate::mcp::McpServer::with_bridge(
            project,
            out,
            vec!["graph".to_string()],
            false,
            crate::mcp::BridgeFlags {
                ephemeral: true,
                reader: true,
                ..Default::default()
            },
        );
        let init = server.handle("initialize", &json!({"protocolVersion": "2024-11-05"})).unwrap();
        let instructions = init["instructions"].as_str().unwrap().to_string();
        let list = server.handle("tools/list", &json!({})).unwrap();
        let tools: Vec<GenericTool> = list["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| GenericTool {
                name: t["name"].as_str().unwrap_or_default().to_string(),
                description: t["description"].as_str().unwrap_or_default().to_string(),
                parameters: t["inputSchema"].clone(),
            })
            .collect();
        let system = format!("{}{}", SYSTEM, TextCodec.system_suffix(&tools));
        let chars = system.chars().count() + instructions.chars().count();
        eprintln!("reader first-call prompt: {} chars (system {}, instructions {})", chars, system.chars().count(), instructions.chars().count());
        assert!(chars < 16_000, "the reader prompt is {} chars", chars);
    }

    // A mid-turn downgrade leaves no native tool_calls or tool-role messages behind.
    // Mirrors docs/frontends/acp.md#the-embedded-agent.
    #[test]
    fn a_downgrade_rewrites_the_native_history_as_text() {
        let mut h = vec![
            json!({"role": "system", "content": "s"}),
            json!({"role": "user", "content": "u"}),
            json!({"role": "assistant", "content": "Reading.", "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "read_section", "arguments": "{\"ref\":\"a.md#/a\"}"}}]}),
            json!({"role": "tool", "tool_call_id": "c1", "content": "{\"title\":\"A\"}"}),
        ];
        history_to_text(&mut h);
        assert!(h[2].get("tool_calls").is_none(), "{}", h[2]);
        assert_eq!(h[2]["content"], "Reading.\n{\"tool\":\"read_section\",\"args\":{\"ref\":\"a.md#/a\"}}");
        assert_eq!(h[3]["role"], "user");
        assert_eq!(h[3]["content"], "RESULT: {\"title\":\"A\"}");
    }

    use super::*;

    #[test]
    fn consecutive_tool_errors_bound_a_batch_without_dangling_results() {
        use std::io::{BufRead, BufReader, Read, Write};
        let _scope = crate::scoped_state::Scope::enter();
        crate::llm::set_tools_mode(1);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let provider = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" { break; }
                if let Some((key, value)) = line.split_once(':') {
                    if key.eq_ignore_ascii_case("content-length") { length = value.trim().parse().unwrap(); }
                }
            }
            reader.read_exact(&mut vec![0; length]).unwrap();
            let calls: Vec<_> = (0..12).map(|i| json!({"id":format!("c{i}"),"type":"function","function":{"name":"fixture","arguments":json!({"index":i}).to_string()}})).collect();
            let body = json!({"choices":[{"message":{"role":"assistant","content":"","tool_calls":calls},"finish_reason":"tool_calls"}],"usage":{"total_tokens":1}}).to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let llm = Llm {base_url:format!("http://{address}"),model:"fixture".into(),api_key:String::new(),temperature:None,trace:None};
        let mut history = vec![json!({"role":"user","content":"test"})];
        let mut dispatched = 0;
        let stop = run_loop(LoopArgs {
            llm: &llm, history: &mut history, tools: &[],
            dispatch: &mut |_, args| { dispatched += 1; if args["index"] == 2 { Ok("{}".into()) } else { Err("bad argument".into()) } },
            emit: &mut |_| {}, cancelled: &|| false, max_rounds: 1,
            label: "error-budget".into(), conversation: false,
        });
        provider.join().unwrap();
        assert!(matches!(stop, Stop::Error(_)));
        assert_eq!(dispatched, 8, "a successful call resets the failure count");
        let results: Vec<_> = history.iter().filter(|m| m["role"] == "tool").collect();
        assert_eq!(results.len(), 12);
        for (i, result) in results.iter().enumerate() { assert_eq!(result["tool_call_id"], format!("c{i}")); }
        assert!(results[11]["content"].as_str().unwrap().contains("Skipped"));
    }

    #[test]
    fn tool_results_have_one_json_layer_and_preserve_plain_text() {
        let object = json!({"tasks": [{"id": "receipt"}], "revision": "abc"});
        for codec in [&NativeCodec as &dyn Codec, &TextCodec as &dyn Codec] {
            let message = codec.result_msg(&Some("call-1".into()), "task_context", &object);
            let content = message["content"].as_str().unwrap().trim_start_matches("RESULT: ");
            assert_eq!(serde_json::from_str::<Value>(content).unwrap(), object);
            let message = codec.result_msg(&None, "log", &json!("line one\nline two"));
            assert!(message["content"].as_str().unwrap().ends_with("line one\nline two"));
        }
    }

    #[test]
    fn text_codec_parses_single_action() {
        let c = TextCodec;
        let msg = json!({"role": "assistant", "content": "I will search first.\n{\"tool\": \"search\", \"args\": {\"query\": \"cart\"}}"});
        let actions = c.parse(&msg);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Call { name, args, .. } => {
                assert_eq!(name, "search");
                assert_eq!(args["query"], "cart");
            }
            _ => panic!("expected a call"),
        }
    }

    #[test]
    fn text_codec_executes_every_action_object() {
        let c = TextCodec;
        let msg = json!({"role": "assistant", "content": "Marking both.\n{\"tool\": \"mark_goal_done\", \"args\": {\"goal\": \"g:a\"}}\n{\"tool\": \"mark_goal_done\", \"args\": {\"goal\": \"g:b\"}}"});
        let actions = c.parse(&msg);
        assert_eq!(actions.len(), 2);
        for (a, g) in actions.iter().zip(["g:a", "g:b"]) {
            match a {
                Action::Call { name, args, .. } => {
                    assert_eq!(name, "mark_goal_done");
                    assert_eq!(args["goal"], g);
                }
                _ => panic!("expected calls"),
            }
        }
    }

    #[test]
    fn text_codec_truncated_action_after_prose_is_malformed() {
        let c = TextCodec;
        let msg = json!({"role": "assistant", "content": "Now I'll mark the goal done.\n\n{\"tool\": \"mark_goal_done\", \"args\": {\"goal\": \"g:a\", \"evidence\": {\"curate-view\": []}}"});
        assert!(matches!(c.parse(&msg)[0], Action::Malformed(_)));
    }

    #[test]
    fn text_codec_prose_is_text() {
        let c = TextCodec;
        let msg = json!({"role": "assistant", "content": "The document describes a shop."});
        assert!(matches!(c.parse(&msg)[0], Action::Text(_)));
    }

    #[test]
    fn announced_actions_are_recognized() {
        assert!(announces_action("All tests pass. Now I'll record the generation with the manifest."));
        assert!(announces_action("Let me fix src/order.rs first:"));
        assert!(!announces_action("Generation of ent:stock is complete and recorded."));
    }

    #[test]
    fn native_codec_cut_arguments_are_malformed() {
        let c = NativeCodec;
        let msg = json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{"id": "c1", "function": {"name": "write_text_file", "arguments": "{\"path\": \"src/a.rs\", \"content\": \"fn main() {"}}]
        });
        let actions = c.parse(&msg);
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], Action::Malformed(e) if e.contains("write_text_file")));
    }

    #[test]
    fn native_codec_parses_tool_calls() {
        let c = NativeCodec;
        let msg = json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{"id": "c1", "function": {"name": "done", "arguments": "{\"summary\": \"ok\"}"}}]
        });
        let actions = c.parse(&msg);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Call { id, name, args } => {
                assert_eq!(id.as_deref(), Some("c1"));
                assert_eq!(name, "done");
                assert_eq!(args["summary"], "ok");
            }
            _ => panic!("expected a call"),
        }
    }
}
