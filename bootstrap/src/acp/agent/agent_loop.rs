// The embedded agent's generic loop: send the history and the session's MCP tools to
// an OpenAI-compatible endpoint, dispatch the calls the model makes, repeat until the
// model stops calling tools. Carries the codec machinery (native tool calls, the text
// fallback, the first-round probe) that used to live in the turn harness; nothing
// here knows anything about jazyk. Mirrors docs/frontends/acp.md#the-embedded-agent.
use super::mcp_client::GenericTool;
use crate::llm::{self, Llm};
use serde_json::{json, Value};

// What the loop reports outward; the serving maps these onto ACP session updates.
pub enum AgentEvent {
    Thought(String),
    Message(String),
    ToolCallStart { id: String, name: String, args: Value },
    ToolCallEnd { id: String, result: String, ok: bool },
    Usage { tokens: u64 },
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
    Call { id: Option<String>, name: String, args: Value },
    Text(String),
}

trait Codec {
    fn system_suffix(&self, tools: &[GenericTool]) -> String;
    fn tools_param(&self, tools: &[GenericTool]) -> Option<Vec<Value>>;
    fn parse(&self, msg: &Value) -> Vec<Action>;
    fn result_msg(&self, call_id: &Option<String>, name: &str, result: &Value) -> Value;
}

struct NativeCodec;

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
                let name = c["function"]["name"].as_str().unwrap_or_default().to_string();
                let args = match c["function"]["arguments"].as_str() {
                    Some(s) => serde_json::from_str(s).unwrap_or(json!({})),
                    None => c["function"]["arguments"].clone(),
                };
                out.push(Action::Call { id: c["id"].as_str().map(|s| s.to_string()), name, args });
            }
        }
        out
    }
    fn result_msg(&self, call_id: &Option<String>, name: &str, result: &Value) -> Value {
        json!({
            "role": "tool",
            "tool_call_id": call_id.clone().unwrap_or_else(|| name.to_string()),
            "content": result.to_string()
        })
    }
}

struct TextCodec;

impl Codec for TextCodec {
    fn system_suffix(&self, tools: &[GenericTool]) -> String {
        if tools.is_empty() {
            return String::new();
        }
        let mut s = String::from(
            "\n\nTOOL PROTOCOL: you have no native tool support. To call a tool, reply with EXACTLY ONE JSON object per message, nothing else:\n{\"tool\": \"<name>\", \"args\": { ... }}\nThe result comes back as a message starting with RESULT:. Then reply with your next action. A reply that is not a JSON action ends the turn. Available tools:\n",
        );
        for t in tools {
            s.push_str(&format!("- {}: {} args schema: {}\n", t.name, t.description, t.parameters));
        }
        s
    }
    fn tools_param(&self, _tools: &[GenericTool]) -> Option<Vec<Value>> {
        None
    }
    fn parse(&self, msg: &Value) -> Vec<Action> {
        let content = msg["content"].as_str().unwrap_or_default();
        if let Some(obj) = llm::extract_json_object(content) {
            if let Ok(v) = serde_json::from_str::<Value>(&obj) {
                if let Some(name) = v["tool"].as_str() {
                    return vec![Action::Call { id: None, name: name.to_string(), args: v["args"].clone() }];
                }
            }
        }
        if content.trim().is_empty() {
            Vec::new()
        } else {
            vec![Action::Text(content.to_string())]
        }
    }
    fn result_msg(&self, _call_id: &Option<String>, _name: &str, result: &Value) -> Value {
        json!({"role": "user", "content": format!("RESULT: {}", result)})
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
}

pub fn run_loop(a: LoopArgs) -> Stop {
    // Codec selection is sticky per process, learned by the first-round probe or
    // forced by JAZYK_CODEC.
    let mut mode = llm::tools_mode();
    if mode == 0 {
        if let Ok(env) = std::env::var("JAZYK_CODEC") {
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
    let system = |codec: &dyn Codec, tools: &[GenericTool]| {
        json!({"role": "system", "content": format!("{}{}", SYSTEM, codec.system_suffix(tools))})
    };
    if a.history.first().map(|m| m["role"] == "system").unwrap_or(false) {
        a.history[0] = system(codec.as_ref(), a.tools);
    } else {
        a.history.insert(0, system(codec.as_ref(), a.tools));
    }

    let mut rounds = 0u32;
    // Identical calls repeated verbatim get refused: the answer has not changed, and
    // a looping model should stop paying for the question.
    let mut repeats: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    let mut refusals = 0u32;
    while rounds < a.max_rounds {
        if (a.cancelled)() {
            return Stop::Cancelled;
        }
        rounds += 1;
        let step = format!("r{}", rounds);
        let tools_param = codec.tools_param(a.tools);
        let msg = match a.llm.chat_messages(a.history, tools_param.as_deref(), &a.label, &step) {
            Ok((m, t)) => {
                (a.emit)(AgentEvent::Usage { tokens: t });
                m
            }
            Err(e) if e.starts_with("tools-rejected:") && mode != 2 => {
                llm::set_tools_mode(2);
                mode = 2;
                codec = codec_for(mode);
                a.history[0] = system(codec.as_ref(), a.tools);
                continue;
            }
            Err(e) => return Stop::Error(e),
        };
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
        if mode != 2 && llm::tools_mode() == 0 && actions.iter().any(|x| matches!(x, Action::Call { .. })) {
            llm::set_tools_mode(1);
        }
        a.history.push(msg.clone());
        for field in ["reasoning_content", "reasoning"] {
            if let Some(r) = msg[field].as_str() {
                if !r.trim().is_empty() {
                    (a.emit)(AgentEvent::Thought(r.trim().to_string()));
                }
            }
        }

        // No tool calls: the model is answering, and that ends the turn.
        if !actions.iter().any(|x| matches!(x, Action::Call { .. })) {
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
                    call_n += 1;
                    let call_id = id.clone().unwrap_or_else(|| format!("{}-{}-{}", a.label, rounds, call_n));
                    (a.emit)(AgentEvent::ToolCallStart { id: call_id.clone(), name: name.clone(), args: args.clone() });
                    let key = format!("{}|{}", name, args);
                    let seen = {
                        let c = repeats.entry(key).or_insert(0);
                        *c += 1;
                        *c
                    };
                    if seen >= 3 {
                        refusals += 1;
                        let refusal = json!({"error": {"rule": "repeated-call", "message": format!(
                            "this is call {} to `{}` with identical arguments; the answer has not changed. Act on the answer you already have.",
                            seen, name
                        )}});
                        (a.emit)(AgentEvent::ToolCallEnd { id: call_id, result: refusal.to_string(), ok: false });
                        a.history.push(codec.result_msg(&id, &name, &refusal));
                        if refusals > 8 {
                            return Stop::EndTurn;
                        }
                        continue;
                    }
                    let (mut result, ok) = match (a.dispatch)(&name, &args) {
                        Ok(v) => (v, true),
                        Err(e) => (e, false),
                    };
                    if seen == 2 {
                        result.push_str("\n(repeat: you already made this exact call; this is the same answer. Act on it.)");
                    }
                    (a.emit)(AgentEvent::ToolCallEnd { id: call_id, result: result.clone(), ok });
                    a.history.push(codec.result_msg(&id, &name, &json!(result)));
                }
            }
        }
    }
    Stop::MaxRounds
}
