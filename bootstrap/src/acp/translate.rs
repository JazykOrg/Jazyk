// Session updates → trace events. The runner feeds a worker session's update stream
// through one translator, so the trace, the transcript, and the GUI panels do not
// care which agent ran the turn. Mirrors docs/frontends/acp.md#worker-sessions and
// docs/compiler/sessions.md#trace-events.
use crate::session::{Trace, TraceEvent};
use agent_client_protocol::schema::v1::{SessionUpdate, ToolCallStatus};
use std::collections::HashMap;

// Streamed chunks are tiny; the trace wants readable lines. Chunks accumulate and
// flush on a tool call, at the cap, or at the end of the turn.
const FLUSH_AT: usize = 2_000;

pub struct UpdateTranslator {
    label: String,
    // The batch's document, when it has one: reconcile-section sessions emit Section
    // events so the editor bands follow the work
    // (docs/compiler/sessions.md#trace-events).
    doc: Option<String>,
    at_section: Option<String>,
    thought: String,
    message: String,
    // Tool call titles by id, so an update row can name what finished.
    calls: HashMap<String, (String, serde_json::Value)>,
    // The tokens the agent reported billed (the embedded agent's usage meta), and the
    // input tokens among them its endpoint served from its prompt cache.
    pub tokens: u64,
    pub cached: u64,
}

impl UpdateTranslator {
    pub fn new(label: &str) -> UpdateTranslator {
        // A goal label still drives the Section events: `reconcile-section
        // docs/x.md#/ref` names its document. A batch id label carries none; the
        // runner supplies it through with_doc.
        let doc =
            label.strip_prefix("reconcile-section ").map(
                |t| match crate::model::split_section_ref(t) {
                    Some((d, _)) => d,
                    None => t.to_string(),
                },
            );
        UpdateTranslator {
            label: label.to_string(),
            doc,
            at_section: None,
            thought: String::new(),
            message: String::new(),
            calls: HashMap::new(),
            tokens: 0,
            cached: 0,
        }
    }

    // The batch's document, for sessions labeled by their batch id.
    pub fn with_doc(mut self, doc: Option<String>) -> UpdateTranslator {
        if doc.is_some() {
            self.doc = doc;
        }
        self
    }

    fn flush_into(&mut self, trace: &Trace) {
        self.flush_text(trace, true);
        self.flush_text(trace, false);
    }

    // Emit one buffer as a `modelText` chunk; `thought` picks the reasoning buffer.
    fn flush_text(&mut self, trace: &Trace, thought: bool) {
        let buf = if thought {
            &mut self.thought
        } else {
            &mut self.message
        };
        let text = std::mem::take(buf);
        if !text.is_empty() {
            trace.event(TraceEvent::ModelText {
                summary: false,
                label: self.label.clone(),
                text,
                reply: !thought,
            });
        }
    }

    // Buffer text of one kind. Switching kinds flushes the other buffer first, so
    // chunks keep their order and one chunk never mixes reasoning with a reply.
    fn push_text(&mut self, trace: &Trace, thought: bool, t: &str) {
        self.flush_text(trace, !thought);
        let buf = if thought {
            &mut self.thought
        } else {
            &mut self.message
        };
        buf.push_str(t);
        if buf.len() > FLUSH_AT {
            self.flush_text(trace, thought);
        }
    }

    pub fn on_update(&mut self, update: &SessionUpdate, trace: &Trace) {
        match update {
            SessionUpdate::AgentThoughtChunk(c) => {
                if c.meta.as_ref().is_some_and(|meta| {
                    meta.get("jazyk")
                        .is_some_and(|v| v["reasoningSummary"] == true)
                }) {
                    self.flush_into(trace);
                    if let Some(text) = content_text(&c.content) {
                        trace.event(TraceEvent::ModelText {
                            label: self.label.clone(),
                            text: text.into(),
                            reply: false,
                            summary: true,
                        });
                    }
                    return;
                }
                if let Some(t) = content_text(&c.content) {
                    self.push_text(trace, true, t);
                }
            }
            SessionUpdate::AgentMessageChunk(c) => {
                if let Some(t) = content_text(&c.content) {
                    self.push_text(trace, false, t);
                }
            }
            SessionUpdate::ToolCall(call) => {
                self.flush_into(trace);
                let id: &str = call.tool_call_id.0.as_ref();
                let args = call.raw_input.clone().unwrap_or(serde_json::Value::Null);
                self.calls
                    .insert(id.to_string(), (call.title.clone(), args.clone()));
                trace.event(TraceEvent::ToolCall {
                    activity: crate::live_preview::activity(&call.title, &args, None),
                    label: self.label.clone(),
                    name: call.title.clone(),
                    summary: crate::session::condense(&args, 160),
                    full: crate::session::full_payload(&args),
                });
                // An accepted call that names a section says where the turn is.
                if let Some(doc) = self.doc.clone() {
                    if let Some(sec) = crate::session::named_section(&args, &doc) {
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
                    .or_else(|| self.calls.get(id).map(|(name, _)| name.clone()))
                    .unwrap_or_else(|| id.to_string());
                let out = u
                    .fields
                    .raw_output
                    .clone()
                    .unwrap_or(serde_json::Value::Null);
                match u.fields.status {
                    Some(ToolCallStatus::Completed) => trace.event(TraceEvent::ToolResult {
                        activity: crate::live_preview::activity(
                            &name,
                            &self
                                .calls
                                .get(id)
                                .map(|(_, args)| args.clone())
                                .unwrap_or_default(),
                            Some(&crate::live_preview::result_value(&out)),
                        ),
                        label: self.label.clone(),
                        name,
                        summary: crate::session::condense(&out, 160),
                        full: crate::session::full_payload(&out),
                    }),
                    Some(ToolCallStatus::Failed) => {
                        // The rule the reply carries, so a replay extracted from the
                        // trace expects what the harness answered; the tool name
                        // stands in when the reply is not a jazyk refusal.
                        let reply: serde_json::Value = out
                            .as_str()
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or_else(|| out.clone());
                        let rule = reply["error"]["rule"]
                            .as_str()
                            .map(String::from)
                            .unwrap_or(name);
                        trace.event(TraceEvent::ToolError {
                            label: self.label.clone(),
                            rule,
                            message: crate::session::condense(&out, 400),
                        })
                    }
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
                // The embedded agent reports each round's billed tokens and its cached
                // input in the meta; other agents may not report usage at all, and
                // both stay 0.
                if let Ok(v) = serde_json::to_value(&u.meta) {
                    if let Some(t) = v["jazyk"]["tokens"].as_u64() {
                        self.tokens += t;
                    }
                    if let Some(c) = v["jazyk"]["cachedTokens"].as_u64() {
                        self.cached += c;
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

fn content_text(c: &agent_client_protocol::schema::v1::ContentBlock) -> Option<&str> {
    match c {
        agent_client_protocol::schema::v1::ContentBlock::Text(t) => Some(t.text.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::TraceLevel;
    use agent_client_protocol::schema::v1::ContentChunk;
    use std::sync::{Arc, Mutex};

    #[test]
    fn flushing_streamed_text_preserves_whitespace_at_buffer_boundaries() {
        let events: Arc<Mutex<Vec<TraceEvent>>> = Default::default();
        let sink = events.clone();
        let trace = Trace::to_sink(
            TraceLevel::Normal,
            Arc::new(move |event| sink.lock().unwrap().push(event.clone())),
            Default::default(),
        );
        for thought in [true, false] {
            let mut translator = UpdateTranslator::new("batch");
            let chunks = [
                format!("{} ", "a".repeat(FLUSH_AT)),
                " next\n".into(),
                "  ".into(),
            ];
            for chunk in &chunks {
                let content = ContentChunk::new(chunk.clone().into());
                let update = if thought {
                    SessionUpdate::AgentThoughtChunk(content)
                } else {
                    SessionUpdate::AgentMessageChunk(content)
                };
                translator.on_update(&update, &trace);
            }
            translator.finish(&trace);
            let mut events = events.lock().unwrap();
            let text: String = events
                .iter()
                .map(|event| match event {
                    TraceEvent::ModelText { text, reply, .. } if *reply != thought => text.as_str(),
                    other => panic!("expected modelText, got {:?}", other),
                })
                .collect();
            assert_eq!(text, chunks.concat());
            events.clear();
        }
        // Switching between reasoning and reply flushes, keeping the order.
        let mut translator = UpdateTranslator::new("batch");
        for (thought, chunk) in [(true, "think"), (false, "reply"), (true, "more")] {
            let content = ContentChunk::new(chunk.to_string().into());
            translator.on_update(
                &if thought {
                    SessionUpdate::AgentThoughtChunk(content)
                } else {
                    SessionUpdate::AgentMessageChunk(content)
                },
                &trace,
            );
        }
        translator.finish(&trace);
        let chunks: Vec<(String, bool)> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                TraceEvent::ModelText { text, reply, .. } => Some((text.clone(), !*reply)),
                _ => None,
            })
            .collect();
        assert_eq!(
            chunks,
            vec![
                ("think".into(), true),
                ("reply".into(), false),
                ("more".into(), true)
            ]
        );
    }
}
