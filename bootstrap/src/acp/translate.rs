// Session updates → trace events. The runner feeds a worker session's update stream
// through one translator, so the trace, the transcript, and the GUI panels do not
// care which agent ran the turn. Mirrors docs/frontends/acp.md#worker-sessions and
// docs/compiler/turns.md#trace-events.
use crate::turn::{Trace, TraceEvent};
use agent_client_protocol::schema::v1::{SessionUpdate, ToolCallStatus};
use std::collections::HashMap;

// Streamed chunks are tiny; the trace wants readable lines. Chunks accumulate and
// flush on a tool call, at the cap, or at the end of the turn.
const FLUSH_AT: usize = 2_000;

pub struct UpdateTranslator {
    label: String,
    // The turn's document, when the label names one: reconcile turns emit Section
    // events so the editor bands follow the work (docs/compiler/turns.md#trace-events).
    doc: Option<String>,
    at_section: Option<String>,
    thought: String,
    message: String,
    // Tool call titles by id, so an update row can name what finished.
    calls: HashMap<String, String>,
    // Completion tokens the agent reported (the embedded agent's usage meta).
    pub tokens: u64,
}

impl UpdateTranslator {
    pub fn new(label: &str) -> UpdateTranslator {
        let doc = label
            .strip_prefix("reconcile-doc ")
            .map(|d| d.to_string());
        UpdateTranslator {
            label: label.to_string(),
            doc,
            at_section: None,
            thought: String::new(),
            message: String::new(),
            calls: HashMap::new(),
            tokens: 0,
        }
    }

    fn flush_into(&mut self, trace: &Trace) {
        for buf in [&mut self.thought, &mut self.message] {
            let text = buf.trim().to_string();
            buf.clear();
            if !text.is_empty() {
                trace.event(TraceEvent::ModelText { label: self.label.clone(), text });
            }
        }
    }

    pub fn on_update(&mut self, update: &SessionUpdate, trace: &Trace) {
        match update {
            SessionUpdate::AgentThoughtChunk(c) => {
                if let Some(t) = content_text(&c.content) {
                    self.thought.push_str(t);
                    if self.thought.len() > FLUSH_AT {
                        let text = std::mem::take(&mut self.thought);
                        trace.event(TraceEvent::ModelText { label: self.label.clone(), text });
                    }
                }
            }
            SessionUpdate::AgentMessageChunk(c) => {
                if let Some(t) = content_text(&c.content) {
                    self.message.push_str(t);
                    if self.message.len() > FLUSH_AT {
                        let text = std::mem::take(&mut self.message);
                        trace.event(TraceEvent::ModelText { label: self.label.clone(), text });
                    }
                }
            }
            SessionUpdate::ToolCall(call) => {
                self.flush_into(trace);
                let id: &str = call.tool_call_id.0.as_ref();
                self.calls.insert(id.to_string(), call.title.clone());
                let args = call.raw_input.clone().unwrap_or(serde_json::Value::Null);
                trace.event(TraceEvent::ToolCall {
                    label: self.label.clone(),
                    name: call.title.clone(),
                    summary: crate::turn::condense(&args, 160),
                    full: crate::turn::full_payload(&args),
                });
                // An accepted call that names a section says where the turn is.
                if let Some(doc) = self.doc.clone() {
                    if let Some(sec) = named_section(&args, &doc) {
                        if self.at_section.as_deref() != Some(sec.as_str()) {
                            self.at_section = Some(sec.clone());
                            trace.event(TraceEvent::Section {
                                label: self.label.clone(),
                                doc,
                                section: sec,
                                tool: call.title.clone(),
                            });
                        }
                    }
                }
            }
            SessionUpdate::ToolCallUpdate(u) => {
                self.flush_into(trace);
                let id: &str = u.tool_call_id.0.as_ref();
                let name = u
                    .fields
                    .title
                    .clone()
                    .or_else(|| self.calls.get(id).cloned())
                    .unwrap_or_else(|| id.to_string());
                let out = u.fields.raw_output.clone().unwrap_or(serde_json::Value::Null);
                match u.fields.status {
                    Some(ToolCallStatus::Completed) => trace.event(TraceEvent::ToolResult {
                        label: self.label.clone(),
                        name,
                        summary: crate::turn::condense(&out, 160),
                        full: crate::turn::full_payload(&out),
                    }),
                    Some(ToolCallStatus::Failed) => trace.event(TraceEvent::ToolError {
                        label: self.label.clone(),
                        rule: name,
                        message: crate::turn::condense(&out, 400),
                    }),
                    // Pending/in-progress churn stays off the trace; the start row said it.
                    _ => {}
                }
            }
            SessionUpdate::Plan(p) => {
                let lines: Vec<String> = p
                    .entries
                    .iter()
                    .map(|e| format!("[{:?}] {}", e.status, e.content))
                    .collect();
                trace.event(TraceEvent::Note {
                    label: self.label.clone(),
                    text: format!("plan: {}", lines.join("; ")),
                    verbose: false,
                });
            }
            SessionUpdate::UsageUpdate(u) => {
                // The embedded agent reports completion tokens in the meta; other
                // agents may not report usage at all, and tokens stay 0.
                if let Ok(v) = serde_json::to_value(&u.meta) {
                    if let Some(t) = v["jazyk"]["completionTokens"].as_u64() {
                        self.tokens += t;
                    }
                }
            }
            // Mode, command, config, and info updates matter to chat surfaces, not to
            // the build trace.
            _ => {}
        }
    }

    // The end of the turn: whatever is buffered lands.
    pub fn finish(&mut self, trace: &Trace) {
        self.flush_into(trace);
    }
}

// The section a tool call names, when it belongs to this turn's document (moved from
// the retired turn loop).
fn named_section(args: &serde_json::Value, doc: &str) -> Option<String> {
    let raw = args["section"].as_str().or_else(|| args["mention"]["section"].as_str())?;
    match crate::model::split_section_ref(raw) {
        Some((d, sec)) => (d == doc).then_some(sec),
        None => raw.starts_with('/').then(|| raw.to_string()),
    }
}

fn content_text(c: &agent_client_protocol::schema::v1::ContentBlock) -> Option<&str> {
    match c {
        agent_client_protocol::schema::v1::ContentBlock::Text(t) => Some(t.text.as_str()),
        _ => None,
    }
}
