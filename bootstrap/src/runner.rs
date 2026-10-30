// What runs one goal batch as a session. The reconciler and the ledger workers speak
// only this: a name for the trace, the build token, and one call that turns a batch
// into a report. Natively it is an ACP worker session against the configured agent
// (`crate::acp::runner`); in a browser tab it is the loop running in the same wasm
// module (`crate::wasm::runner`). See docs/frontends/wasm.md#sessions.
use crate::model::Goal;
use crate::session::Trace;
use std::collections::BTreeSet;

// What one session left behind: the mutations its commits applied, the goals they
// resolved (from the journal), and whether the batch landed.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionReport {
    pub applied: usize,
    pub rounds: u32,
    // What the turn billed, and the input among it the endpoint served from its prompt
    // cache (docs/compiler/sessions.md#budgets).
    pub tokens: u64,
    pub cached: u64,
    pub failed: Option<String>,
    // Goal ids the batch's journal entries resolved.
    pub resolved: Vec<String>,
    // Why the session stopped short of a plain end of turn (the agent's error text on
    // a refusal, the round budget, a cancel), for a caller that judges its own ledger.
    // Mirrors docs/frontends/acp.md#the-embedded-agent.
    pub stopped: Option<String>,
}

// One session's worth of work: the batch id, its goals, and the scheduler's executor
// hint. The reconciler builds one per board batch; the ledger workers build
// single-goal batches per row.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct BatchRun {
    pub id: String,
    pub goals: Vec<Goal>,
    pub executor: Option<String>,
}

impl BatchRun {
    pub fn single(goal: Goal) -> BatchRun {
        BatchRun {
            id: goal.id.clone(),
            goals: vec![goal],
            executor: None,
        }
    }

    pub(crate) fn goal_ids(&self) -> Vec<String> {
        self.goals.iter().map(|g| g.id.clone()).collect()
    }

    pub(crate) fn kind(&self) -> String {
        self.goals
            .first()
            .map(|g| g.kind.clone())
            .unwrap_or_default()
    }

    pub(crate) fn class(&self) -> String {
        self.goals
            .first()
            .map(|g| g.class.clone())
            .unwrap_or_else(|| "compile".to_string())
    }

    // The serving the batch's goal kinds need: bind and generate work the ledger and
    // the deliverable, verify judges rows, everything else works the graph.
    pub(crate) fn serving_mode(&self) -> &'static str {
        let kinds: BTreeSet<&str> = self.goals.iter().map(|g| g.kind.as_str()).collect();
        if !kinds.is_empty() && kinds.iter().all(|k| matches!(*k, "bind" | "generate")) {
            "generate"
        } else if !kinds.is_empty() && kinds.iter().all(|k| *k == "verify") {
            "verify"
        } else {
            "compile"
        }
    }

    pub(crate) fn is_ledger(&self) -> bool {
        self.serving_mode() != "compile"
    }
}

// The contract every runner satisfies.
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait SessionRunner {
    // Named in the trace line that opens a build.
    fn agent_name(&self) -> String;

    // The control plane's token for the run, carried into every session's serving.
    fn set_build_token(&self, token: Option<String>);

    // One batch, one session. A runner reports what landed; it never decides
    // whether the goals are resolved, which is read from the store.
    async fn run_item(&self, batch: &BatchRun, trace: &Trace) -> SessionReport;

    // One prompt, one reply, no tools and no session: the ledger's one-shots.
    async fn ask(&self, system: &str, user: &str, label: &str, step: &str) -> Result<String, String>;

    async fn ask_traced(
        &self,
        system: &str,
        user: &str,
        label: &str,
        step: &str,
        trace: Option<&Trace>,
    ) -> Result<String, String>;
}

// The same prompt travels through ACP and the in-process ledger runner. Ledger
// begin_goals returns an acknowledgement; its caller carries the full contract.
pub(crate) fn batch_prompt(project: &crate::project::Project, out: &std::path::Path, batch: &BatchRun) -> String {
    let mut store = crate::store::Store::load(out);
    let (parsed, _) = crate::reconcile::parse_all(project);
    store.sync_docs(&parsed);
    let control = crate::control::Control::load(project, out);
    let (loaded, skills) = crate::session::initial_loaded(&store, &batch.goals);
    let mut pb = crate::session::ProjectBlock::compute(&store, &batch.goals, &control.compile)
        .with_lint(&project.linting);
    pb.batch = batch.id.clone();
    crate::session::session_prompt(&store, &batch.goals, &loaded, &skills, &pb)
}

// What the journal says one session landed between two generations: the mutations
// its entries applied, and the goals they resolved. A runner never reports its own
// success; it reads it here (docs/compiler/graph.md#journal).
pub fn journal_diff(out: &std::path::Path, from: u64, to: u64, goal_ids: &[String]) -> (usize, Vec<String>) {

    let mut applied = 0usize;
    let mut resolved: Vec<String> = Vec::new();
    for g in (from + 1)..=to {
        let path = out.join("journal").join(format!("g{}.yaml", g));
        let Ok(text) = crate::vfs::read_to_string(&path) else {
            continue;
        };
        let Ok(entry) = serde_norway::from_str::<crate::model::JournalEntry>(&text) else {
            continue;
        };
        if entry.kind != "session" || !entry.batch.iter().any(|g| goal_ids.contains(g)) {
            continue;
        }
        applied += entry.mutations.len();
        for r in &entry.resolved_goals {
            if !resolved.contains(&r.goal) {
                resolved.push(r.goal.clone());
            }
        }
    }
    (applied, resolved)
}

// An in-process session's trace: the agent loop's events as the trace events the ACP
// runner's translator emits for a worker session, so a build in a tab reads like a
// native one (docs/frontends/wasm.md#sessions). It also counts the rounds and the
// spend.
pub struct LoopTrace<'a> {
    trace: &'a Trace,
    label: String,
    doc: Option<String>,
    at_section: Option<String>,
    text: String,
    text_flushed: Option<crate::sys::Instant>,
    // The name and arguments of each call in flight, by call id: a `section` event
    // waits for the call to be accepted.
    calls: std::collections::HashMap<String, (String, serde_json::Value)>,
    pub rounds: u32,
    pub tokens: u64,
    pub cached: u64,
}

impl<'a> LoopTrace<'a> {
    pub fn new(trace: &'a Trace, batch: &BatchRun) -> LoopTrace<'a> {
        LoopTrace {
            trace,
            label: batch.id.clone(),
            doc: batch_doc(batch),
            at_section: None,
            text: String::new(),
            text_flushed: None,
            calls: Default::default(),
            rounds: 0,
            tokens: 0,
            cached: 0,
        }
    }

    pub fn finish(&mut self) {
        if !self.text.is_empty() {
            self.trace.event(crate::session::TraceEvent::ModelText {
                label: self.label.clone(),
                text: std::mem::take(&mut self.text),
            });
        }
    }

    fn buffer_text(&mut self, text: &str, now: crate::sys::Instant) {
        if text.is_empty() {
            return;
        }
        // Split on UTF-8 boundaries, keeping every byte and bounding each event.
        for ch in text.chars() {
            if self.text.len() + ch.len_utf8() > 2_000 {
                self.finish();
                self.text_flushed = Some(now);
            }
            self.text.push(ch);
        }
        if self.text_flushed.is_none_or(|last| now.duration_since(last).as_millis() >= 250) {
            self.finish();
            self.text_flushed = Some(now);
        }
    }

    pub fn on_event(&mut self, ev: crate::harness::AgentEvent) {
        use crate::harness::AgentEvent;
        use crate::session::TraceEvent;
        if !matches!(&ev, AgentEvent::Message(_) | AgentEvent::Thought(_)) {
            self.finish();
            self.text_flushed = None;
        }
        match ev {
            AgentEvent::Input(_) | AgentEvent::Timing(_) => {}
            AgentEvent::Usage { tokens, cached } => {
                self.rounds += 1;
                self.tokens = self.tokens.saturating_add(tokens);
                self.cached += cached;
            }
            AgentEvent::Message(text) | AgentEvent::Thought(text) => {
                self.buffer_text(&text, crate::sys::Instant::now());
            }
            AgentEvent::ToolCallStart { id, name, args } => {
                self.trace.event(TraceEvent::ToolCall {
                    activity: crate::live_preview::activity(&name, &args, None),
                    label: self.label.clone(),
                    name: name.clone(),
                    summary: crate::session::condense(&args, 160),
                    full: crate::session::full_payload(&args),
                });
                self.calls.insert(id, (name, args));
            }
            AgentEvent::ToolCallEnd { id, result, ok } => {
                let (name, args) = self
                    .calls
                    .remove(&id)
                    .unwrap_or_else(|| (id.clone(), serde_json::Value::Null));
                if !ok {
                    let (rule, message) = crate::session::refusal(&result, &name);
                    self.trace.event(TraceEvent::ToolError {
                        label: self.label.clone(),
                        rule,
                        message,
                    });
                    return;
                }
                let full_out: serde_json::Value = serde_json::from_str(&result)
                    .unwrap_or_else(|_| serde_json::Value::String(result.clone()));
                let out = full_out["content"][0]["text"].as_str()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                    .unwrap_or_else(|| full_out.clone());
                self.trace.event(TraceEvent::ToolResult {
                    activity: crate::live_preview::activity(&name, &args, Some(&out)),
                    label: self.label.clone(),
                    name: name.clone(),
                    summary: crate::session::condense(&out, 160),
                    full: crate::session::full_payload(&full_out),
                });
                // An accepted call that names another section says where the session is.
                let Some(doc) = self.doc.clone() else { return };
                if let Some(sec) = crate::session::named_section(&args, &doc) {
                    if self.at_section.as_deref() != Some(sec.as_str()) {
                        self.at_section = Some(sec.clone());
                        self.trace.event(TraceEvent::Section {
                            label: self.label.clone(),
                            doc,
                            section: sec,
                            tool: name,
                        });
                    }
                }
            }
        }
    }
}

// The document a batch works on: the first goal target naming one.
pub fn batch_doc(batch: &BatchRun) -> Option<String> {
    batch.goals.iter().find_map(|g| {
        let doc = g.target.split('#').next().unwrap_or_default();
        crate::md::is_document_target(doc).then(|| doc.to_string())
    })
}

// The `sessionStart` an in-process session opens with: the goals, the task and
// target, the document, and the sections its reconcile goals name.
pub fn session_start(batch: &BatchRun) -> crate::session::TraceEvent {
    let sections: Vec<String> = batch
        .goals
        .iter()
        .filter(|g| g.kind == "reconcile-section")
        .filter_map(|g| crate::model::split_section_ref(&g.target).map(|(_, s)| s))
        .collect();
    crate::session::TraceEvent::SessionStart {
        label: batch.id.clone(),
        goals: batch.goal_ids(),
        task: batch.kind(),
        target: batch
            .goals
            .first()
            .map(|g| g.target.clone())
            .unwrap_or_default(),
        doc: batch_doc(batch),
        dirty: sections.len(),
        sections,
        stale: 0,
        proposals: 0,
    }
}

// Why a batch that did not land stopped, in words, never a bare "nothing committed"
// (docs/compiler/sessions.md#trace-events).
pub fn unlanded_reason(stop: &crate::harness::Stop, rounds: u32) -> String {
    use crate::harness::Stop;
    match stop {
        Stop::EndTurn => "the model ended its turn with the batch still open".to_string(),
        Stop::MaxRounds => format!("the session ran out of rounds after {}", rounds),
        Stop::Cancelled => "the session was cancelled".to_string(),
        Stop::Suspended => "suspended".to_string(),
        Stop::Error(e) => format!("the model call failed: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{AgentEvent, Stop};
    use crate::session::{TraceEvent, TraceLevel};
    use std::sync::{Arc, Mutex};

    fn capture() -> (Trace, Arc<Mutex<Vec<TraceEvent>>>) {
        let events: Arc<Mutex<Vec<TraceEvent>>> = Default::default();
        let sink = events.clone();
        let trace = Trace::to_sink(
            TraceLevel::Normal,
            Arc::new(move |ev| sink.lock().unwrap().push(ev.clone())),
            Default::default(),
        );
        (trace, events)
    }

    fn batch() -> BatchRun {
        BatchRun {
            id: "b2-1".into(),
            goals: vec![Goal {
                id: "g:reconcile-section:docs/README.md#/hello-world".into(),
                kind: "reconcile-section".into(),
                target: "docs/README.md#/hello-world".into(),
                ..Default::default()
            }],
            executor: None,
        }
    }

    // The in-process loop leaves the trail a native worker session does: the
    // session's start with its document and sections, each call with its result or
    // refusal, the section an accepted call reached, and the model's words
    // (docs/frontends/wasm.md#sessions).
    #[test]
    fn the_loop_trace_says_what_the_session_did() {
        let (trace, events) = capture();
        let b = batch();
        trace.event(session_start(&b));
        let mut lt = LoopTrace::new(&trace, &b);
        lt.on_event(AgentEvent::Usage { tokens: 30, cached: 4 });
        lt.on_event(AgentEvent::Thought("I should read the section first.".into()));
        lt.on_event(AgentEvent::ToolCallStart {
            id: "c1".into(),
            name: "read_section".into(),
            args: serde_json::json!({"section": "docs/README.md#/hello-world"}),
        });
        lt.on_event(AgentEvent::ToolCallEnd {
            id: "c1".into(),
            result: "{\"text\": \"Hello\"}".into(),
            ok: true,
        });
        lt.on_event(AgentEvent::ToolCallStart {
            id: "c2".into(),
            name: "upsert_requirement".into(),
            args: serde_json::json!({"statement": "x"}),
        });
        lt.on_event(AgentEvent::ToolCallEnd {
            id: "c2".into(),
            result: "{\"error\": {\"rule\": \"quote-not-found\", \"message\": \"the quote is not in the section\"}}".into(),
            ok: false,
        });
        lt.on_event(AgentEvent::Message("I cannot find the quote.".into()));
        lt.finish();
        assert_eq!((lt.rounds, lt.tokens, lt.cached), (1, 30, 4));

        let ev = events.lock().unwrap();
        match &ev[0] {
            TraceEvent::SessionStart { label, doc, sections, task, goals, .. } => {
                assert_eq!(label, "b2-1");
                assert_eq!(doc.as_deref(), Some("docs/README.md"));
                assert_eq!(sections, &vec!["/hello-world".to_string()]);
                assert_eq!(task, "reconcile-section");
                assert_eq!(goals.len(), 1);
            }
            other => panic!("expected sessionStart, got {:?}", other),
        }
        assert!(matches!(&ev[1], TraceEvent::ModelText { text, .. } if text.contains("read the section")));
        assert!(matches!(&ev[2], TraceEvent::ToolCall { name, .. } if name == "read_section"));
        assert!(matches!(&ev[3], TraceEvent::ToolResult { name, .. } if name == "read_section"));
        assert!(matches!(&ev[4], TraceEvent::Section { section, doc, .. } if section == "/hello-world" && doc == "docs/README.md"));
        assert!(matches!(&ev[5], TraceEvent::ToolCall { name, .. } if name == "upsert_requirement"));
        match &ev[6] {
            TraceEvent::ToolError { rule, message, label } => {
                assert_eq!(label, "b2-1");
                assert_eq!(rule, "quote-not-found");
                assert_eq!(message, "the quote is not in the section");
            }
            other => panic!("expected toolError, got {:?}", other),
        }
        assert!(matches!(&ev[7], TraceEvent::ModelText { text, .. } if text == "I cannot find the quote."));
        assert_eq!(ev.len(), 8);
    }

    #[test]
    fn tiny_deltas_are_coalesced_without_hiding_progress_or_tool_order() {
        let (trace, events) = capture();
        let b = batch();
        let mut lt = LoopTrace::new(&trace, &b);
        let now = crate::sys::Instant::now();
        lt.buffer_text("First ", now);
        assert_eq!(events.lock().unwrap().len(), 1);
        for _ in 0..5_000 {
            lt.buffer_text("x ", now);
        }
        lt.on_event(AgentEvent::ToolCallStart {
            id: "load".into(),
            name: "load".into(),
            args: serde_json::json!({}),
        });
        let events = events.lock().unwrap();
        assert!(
            events.len() < 10,
            "tiny deltas must not fill the trace allowance"
        );
        let text: String = events
            .iter()
            .filter_map(|event| match event {
                TraceEvent::ModelText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "First ".to_string() + &"x ".repeat(5_000));
        assert!(matches!(events.last(), Some(TraceEvent::ToolCall { .. })));
    }

    #[test]
    fn buffered_text_flushes_at_the_live_interval_and_session_end() {
        let (trace, events) = capture();
        let b = batch();
        let mut lt = LoopTrace::new(&trace, &b);
        let now = crate::sys::Instant::now();
        lt.buffer_text("First ", now);
        lt.buffer_text("next", now + std::time::Duration::from_millis(249));
        assert_eq!(events.lock().unwrap().len(), 1);
        lt.buffer_text(" ", now + std::time::Duration::from_millis(250));
        assert_eq!(events.lock().unwrap().len(), 2);
        lt.buffer_text("last\n", now + std::time::Duration::from_millis(251));
        lt.finish();
        lt.finish();
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[1], TraceEvent::ModelText { text, .. } if text == "next "));
        assert!(matches!(&events[2], TraceEvent::ModelText { text, .. } if text == "last\n"));
    }

    #[test]
    fn streamed_model_text_preserves_whitespace_and_complete_chunks() {
        let (trace, events) = capture();
        let b = batch();
        let mut lt = LoopTrace::new(&trace, &b);
        let chunks = [
            "With",
            " ",
            "pronounciation",
            " written",
            " as:",
            "\n",
            "  ",
            "quote",
            "\n\n",
        ];
        for chunk in chunks {
            lt.on_event(AgentEvent::Thought(chunk.into()));
        }
        let long = format!(" {} ", "é".repeat(2_100));
        lt.on_event(AgentEvent::Message(long.clone()));
        lt.on_event(AgentEvent::Thought(String::new()));
        lt.finish();
        let events = events.lock().unwrap();
        let text: Vec<&str> = events
            .iter()
            .map(|event| match event {
                TraceEvent::ModelText { text, .. } => text.as_str(),
                other => panic!("expected modelText, got {:?}", other),
            })
            .collect();
        assert_eq!(text.concat(), chunks.concat() + &long);
        assert!(text.iter().all(|chunk| chunk.len() <= 2_000));
    }

    // A refusal that is not a jazyk error keeps its text under the tool's name, and
    // a refused call names no section.
    #[test]
    fn a_plain_refusal_names_the_tool() {
        let (trace, events) = capture();
        let b = batch();
        let mut lt = LoopTrace::new(&trace, &b);
        lt.on_event(AgentEvent::ToolCallStart {
            id: "c1".into(),
            name: "load".into(),
            args: serde_json::json!({"section": "/hello-world"}),
        });
        lt.on_event(AgentEvent::ToolCallEnd {
            id: "c1".into(),
            result: "unknown tool".into(),
            ok: false,
        });
        let ev = events.lock().unwrap();
        assert_eq!(ev.len(), 2);
        assert!(matches!(&ev[1], TraceEvent::ToolError { rule, message, .. } if rule == "load" && message == "unknown tool"));
    }

    // The failure reason names the stop; "nothing committed" alone never says why.
    #[test]
    fn an_unlanded_batch_says_why_it_stopped() {
        assert_eq!(
            unlanded_reason(&Stop::EndTurn, 3),
            "the model ended its turn with the batch still open"
        );
        assert_eq!(
            unlanded_reason(&Stop::MaxRounds, 40),
            "the session ran out of rounds after 40"
        );
        assert!(unlanded_reason(&Stop::Error("HTTP 400: maximum context length".into()), 1)
            .contains("maximum context length"));
        for s in [Stop::EndTurn, Stop::MaxRounds, Stop::Cancelled, Stop::Error("x".into())] {
            assert!(!unlanded_reason(&s, 1).contains("nothing committed"));
        }
    }
}
