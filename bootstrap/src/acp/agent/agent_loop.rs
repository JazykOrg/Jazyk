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
    Usage {
        tokens: u64,
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
                let args = match c["function"]["arguments"].as_str() {
                    Some(s) => serde_json::from_str(s).unwrap_or(json!({})),
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
    fn result_msg(&self, call_id: &Option<String>, name: &str, result: &Value) -> Value {
        json!({
            "role": "tool",
            "tool_call_id": call_id.clone().unwrap_or_else(|| name.to_string()),
            "content": result.to_string()
        })
    }
}

struct TextCodec;

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
        // Content that opens like an action but never yielded one is a broken
        // action, not prose: a truncated object must not end the turn as an answer.
        let t = content.trim_start();
        if t.starts_with('{') && t.contains("\"tool\"") {
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
    let mut malformed_streak = 0u32;
    while rounds < a.max_rounds {
        if (a.cancelled)() {
            return Stop::Cancelled;
        }
        rounds += 1;
        let step = format!("r{}", rounds);
        let tools_param = codec.tools_param(a.tools);
        let msg = match a
            .llm
            .chat_messages(a.history, tools_param.as_deref(), &a.label, &step)
        {
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
        if mode != 2
            && llm::tools_mode() == 0
            && actions.iter().any(|x| matches!(x, Action::Call { .. }))
        {
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
            a.history.push(json!({"role": "user", "content": format!(
                "Your JSON action did not parse ({}). Resend the complete action as exactly one JSON object: {{\"tool\": \"<name>\", \"args\": {{ ... }}}}, nothing else.",
                err
            )}));
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
                a.history.push(json!({"role": "user", "content":
                    "Your reply was empty. Reasoning is not shown to anyone and does not count as acting: make the tool call you were about to make, or state your answer as plain message text."}));
                continue;
            }
            if called_any && !nudged {
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
                    let (result, ok) = match (a.dispatch)(&name, &args) {
                        Ok(v) => (v, true),
                        Err(e) => (e, false),
                    };
                    (a.emit)(AgentEvent::ToolCallEnd {
                        id: call_id,
                        result: result.clone(),
                        ok,
                    });
                    a.history.push(codec.result_msg(&id, &name, &json!(result)));
                }
            }
        }
    }
    Stop::MaxRounds
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn text_codec_prose_is_text() {
        let c = TextCodec;
        let msg = json!({"role": "assistant", "content": "The document describes a shop."});
        assert!(matches!(c.parse(&msg)[0], Action::Text(_)));
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
