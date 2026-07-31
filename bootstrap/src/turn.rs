// The turn harness: one focused LLM session with tools. Wires the model to the tool
// registry through a codec, stages mutations, and hands the finished changeset back to
// the reconciler for commit. Mirrors docs/compiler/turns.md.
use crate::llm::{self, Llm};
use crate::model::WorkItem;
use crate::project::{Limits, Linting};
use crate::store::Store;
use crate::tools::{catalog, toolset, ToolDef, ToolSession, WorkScope};
use serde_json::{json, Value};

// ---- trace ----

#[derive(Clone, Copy, PartialEq)]
pub enum TraceLevel {
    Quiet,
    Normal,
    Verbose,
}

// One structured event per emission. The CLI renders these to stderr; the GUI streams
// them to the browser. Mirrors docs/compiler/turns.md#trace-events.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind")]
pub enum TraceEvent {
    // Where the turn works: the task, its target, the document when it has one, and
    // the dirty sections it must process. The GUI lights those up in place.
    #[serde(rename = "turnStart")]
    #[serde(rename_all = "camelCase")]
    TurnStart {
        label: String,
        task: String,
        target: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        doc: Option<String>,
        sections: Vec<String>,
        dirty: usize,
        stale: usize,
    },
    // `summary` is the condensed line the CLI prints; `full` carries the payload
    // behind it (capped) when condensing cut something, so the GUI expands in place.
    #[serde(rename = "toolCall")]
    ToolCall {
        label: String,
        name: String,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        full: Option<String>,
    },
    #[serde(rename = "toolResult")]
    ToolResult {
        label: String,
        name: String,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        full: Option<String>,
    },
    #[serde(rename = "toolError")]
    ToolError { label: String, rule: String, message: String },
    #[serde(rename = "modelText")]
    ModelText { label: String, text: String },
    // mode: "done" (explicit, with the model's summary), "implicit" (the model went
    // silent with staged work), or "budget" (implicit at the round budget).
    #[serde(rename = "turnDone")]
    TurnDone { label: String, staged: usize, rounds: u32, mode: String, summary: String },
    #[serde(rename = "turnFailed")]
    TurnFailed { label: String, attempt: u32, error: String },
    #[serde(rename = "note")]
    Note { label: String, text: String, verbose: bool },
    // The turn moved to a section: an accepted tool call named one, and it differs
    // from the last. The sequence is the turn's path through the document.
    #[serde(rename = "section")]
    Section { label: String, doc: String, section: String, tool: String },
    // One model call. The request carries the whole outgoing message list, the
    // response the raw assistant message; both are recorded in full in the
    // transcript and elided on the wire (docs/compiler/turns.md#trace-events).
    #[serde(rename = "llmRequest")]
    LlmRequest { label: String, step: String, model: String, messages: Value, tools: Vec<String> },
    #[serde(rename = "llmResponse")]
    LlmResponse { label: String, step: String, ms: u64, tokens: u64, message: Value },
    #[serde(rename = "llmRetry")]
    #[serde(rename_all = "camelCase")]
    LlmRetry { label: String, step: String, attempt: u32, error: String, wait_ms: u64 },
    // A wave of work items is about to run: what is queued, before any turn starts.
    #[serde(rename = "waveStart")]
    WaveStart { wave: u32, task: String, items: Vec<String> },
    // Generation worker events, one entity per bounded task.
    #[serde(rename = "genEntityStart")]
    GenEntityStart { entity: String },
    #[serde(rename = "genEntitySkipped")]
    GenEntitySkipped { entity: String, reason: String },
    #[serde(rename = "genEntityDone")]
    GenEntityDone { entity: String, files: usize },
    // stage: "task" (the task package failed to assemble) or "generate".
    #[serde(rename = "genEntityFailed")]
    GenEntityFailed { entity: String, stage: String, error: String },
    // Verification worker events, one ledger row at a time.
    #[serde(rename = "verifyRowStart")]
    VerifyRowStart { requirement: String, test: String },
    #[serde(rename = "verifyRowDone")]
    VerifyRowDone { requirement: String, verdict: String, run: String, evidence: String },
    #[serde(rename = "verifyRowStale")]
    VerifyRowStale { requirement: String, entity: String, status: String, reason: String },
    #[serde(rename = "verifyRowError")]
    VerifyRowError { requirement: String, message: String },
}

// Render an event exactly as the pre-event trace printed it, so `jazyk compile`
// output is unchanged.
fn render_stderr(ev: &TraceEvent) {
    match ev {
        TraceEvent::TurnStart { label, dirty, stale, .. } => {
            eprintln!("[{}] turn start ({} dirty, {} stale)", label, dirty, stale)
        }
        TraceEvent::ToolCall { label, name, summary, .. } => eprintln!("[{}] → {} {}", label, name, summary),
        TraceEvent::ToolResult { label, summary, .. } => eprintln!("[{}] ← {}", label, summary),
        TraceEvent::ToolError { label, rule, message } => eprintln!("[{}] ✗ {}: {}", label, rule, message),
        TraceEvent::ModelText { label, text } => eprintln!("[{}] · {}", label, llm::truncate(text, 200)),
        TraceEvent::TurnDone { label, staged, rounds, mode, summary } => match mode.as_str() {
            "implicit" => eprintln!("[{}] ✓ implicit done ({} staged, {} rounds)", label, staged, rounds),
            "budget" => eprintln!("[{}] ✓ implicit done at round budget ({} staged)", label, staged),
            _ => eprintln!("[{}] ✓ done ({} staged, {} rounds): {}", label, staged, rounds, summary),
        },
        TraceEvent::TurnFailed { label, attempt, error } => {
            eprintln!("[{}] turn failed (attempt {}): {}", label, attempt, error)
        }
        TraceEvent::Note { label, text, .. } => eprintln!("[{}] {}", label, text),
        // The section path is implicit in the tool rows the default level already
        // prints; naming it again would double every line.
        TraceEvent::Section { .. } => {}
        // Model calls print their arithmetic, never their payload: the verbose context
        // pack note already carries the prompt.
        TraceEvent::LlmRequest { label, step, messages, .. } => {
            eprintln!("[{} {}] → llm ({} messages, {} chars)", label, step, messages.as_array().map(|a| a.len()).unwrap_or(0), messages.to_string().len())
        }
        TraceEvent::LlmResponse { label, step, ms, tokens, .. } => {
            eprintln!("[{} {}] ← llm ({} ms, {} tokens)", label, step, ms, tokens)
        }
        TraceEvent::LlmRetry { label, step, attempt, error, wait_ms } => eprintln!(
            "[{} {}] retrying in {}s (attempt {}): {}",
            label,
            step,
            wait_ms / 1000,
            attempt,
            llm::truncate(error, 120)
        ),
        TraceEvent::WaveStart { wave, task, items } => {
            eprintln!("[wave {}] {} ({} items)", wave, task, items.len())
        }
        // Worker events reach stderr only outside the CLI wrappers (which render them
        // themselves, on the exact historical format); keep these plain.
        TraceEvent::GenEntityStart { entity } => eprintln!("[gen {}] start", entity),
        TraceEvent::GenEntitySkipped { entity, reason } => eprintln!("[gen {}] skipped ({})", entity, reason),
        TraceEvent::GenEntityDone { entity, files } => eprintln!("[gen {}] done ({} file(s))", entity, files),
        TraceEvent::GenEntityFailed { entity, error, .. } => eprintln!("[gen {}] failed: {}", entity, error),
        TraceEvent::VerifyRowStart { requirement, test } => eprintln!("[test {}] start ({})", requirement, test),
        TraceEvent::VerifyRowDone { requirement, verdict, run, .. } => {
            eprintln!("[test {}] {} ({})", requirement, verdict, run)
        }
        TraceEvent::VerifyRowStale { requirement, status, reason, .. } => {
            eprintln!("[test {}] {} ({})", requirement, status, reason)
        }
        TraceEvent::VerifyRowError { requirement, message } => eprintln!("[test {}]{}", requirement, message),
    }
}

// The persisted transcript behind a CLI build: the same JSON-lines file a GUI job
// writes under <out>/trace/. Mirrors docs/frontends/cli.md#jazyk-compile.
struct Transcript {
    file: std::fs::File,
    n: u64,
    out: std::path::PathBuf,
}

#[derive(Clone)]
pub struct Trace {
    pub level: TraceLevel,
    // None renders to stderr. A sink receives the structured events instead.
    sink: Option<std::sync::Arc<dyn Fn(&TraceEvent) + Send + Sync>>,
    // Best-effort cancellation, checked between waves, entities, and rows. It rides
    // in the trace because the trace is already threaded through every runner.
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    transcript: Option<std::sync::Arc<std::sync::Mutex<Transcript>>>,
    // The run's transcript name under <out>/trace, when the run leaves one. Carried so
    // a record made mid-run (a feedback entry) can name the run it came from.
    run: Option<String>,
}

impl Trace {
    pub fn stderr(level: TraceLevel) -> Trace {
        Trace { level, sink: None, cancel: Default::default(), transcript: None, run: None }
    }
    pub fn to_sink(
        level: TraceLevel,
        sink: std::sync::Arc<dyn Fn(&TraceEvent) + Send + Sync>,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Trace {
        Trace { level, sink: Some(sink), cancel, transcript: None, run: None }
    }
    // The transcript name of the run this trace belongs to. A GUI job names its own
    // (it writes the file itself); a CLI build names the one with_transcript opened.
    pub fn with_run(mut self, stem: &str) -> Trace {
        self.run = Some(stem.to_string());
        self
    }
    pub fn run(&self) -> Option<String> {
        self.run.clone()
    }
    // Persist this trace as a transcript under <out>/trace/, the same format the GUI
    // job runner writes, so a CLI build shows up in the Build view.
    pub fn with_transcript(mut self, out: &std::path::Path, kind: &str) -> Trace {
        use std::io::Write;
        let dir = out.join("trace");
        std::fs::create_dir_all(&dir).ok();
        let started = crate::verify::now_iso();
        let compact: String = started.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let stem = format!("{}-{}-cli{}", compact, kind, std::process::id());
        if let Ok(mut file) =
            std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{}.jsonl", stem)))
        {
            // The generation at start and finish brackets the run: the journal
            // entries between the two are the run's changesets (gui.md#jobs).
            let meta = json!({"meta": {"id": null, "kind": {"kind": kind}, "startedAt": started, "source": "cli",
                "generation": crate::store::read_generation(out)}});
            let _ = writeln!(file, "{}", meta);
            let _ = file.flush();
            self.transcript =
                Some(std::sync::Arc::new(std::sync::Mutex::new(Transcript { file, n: 0, out: out.to_path_buf() })));
            self.run = Some(stem);
        }
        self
    }
    pub fn finish_transcript(&self, state: &str, result: &Value) {
        use std::io::Write;
        if let Some(t) = &self.transcript {
            let mut t = t.lock().unwrap();
            let line = json!({"outcome": {"state": state, "result": result, "finishedAt": crate::verify::now_iso(),
                "generation": crate::store::read_generation(&t.out)}});
            let _ = writeln!(t.file, "{}", line);
            let _ = t.file.flush();
        }
    }
    pub fn event(&self, ev: TraceEvent) {
        // The transcript records what a Normal-level trace shows, independent of the
        // terminal level: --quiet still leaves a full transcript.
        if let Some(t) = &self.transcript {
            if !matches!(&ev, TraceEvent::Note { verbose: true, .. }) {
                use std::io::Write;
                let mut t = t.lock().unwrap();
                t.n += 1;
                let line = json!({"n": t.n, "event": &ev});
                let _ = writeln!(t.file, "{}", line);
                let _ = t.file.flush();
            }
        }
        let keep = match self.level {
            TraceLevel::Quiet => false,
            TraceLevel::Normal => !matches!(&ev, TraceEvent::Note { verbose: true, .. }),
            TraceLevel::Verbose => true,
        };
        if !keep {
            return;
        }
        match &self.sink {
            // A sink is a structured reader (the GUI); it gets everything the level
            // kept, payloads included.
            Some(s) => s(&ev),
            // The terminal is a different audience: model calls and section moves are
            // noise beside the tool rows, so they render only at Verbose.
            None => {
                let terse = matches!(
                    &ev,
                    TraceEvent::LlmRequest { .. } | TraceEvent::LlmResponse { .. } | TraceEvent::Section { .. }
                );
                if !terse || self.level == TraceLevel::Verbose {
                    render_stderr(&ev);
                }
            }
        }
    }
    pub fn line(&self, prefix: &str, s: &str) {
        self.event(TraceEvent::Note { label: prefix.into(), text: s.into(), verbose: false });
    }
    fn verbose(&self, prefix: &str, s: &str) {
        self.event(TraceEvent::Note { label: prefix.into(), text: s.into(), verbose: true });
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// ---- codecs ----

enum Action {
    Call { id: Option<String>, name: String, args: Value },
    Text(String),
}

trait Codec {
    // The extra system-prompt section this codec needs (tool docs for the text codec).
    fn system_suffix(&self, tools: &[&ToolDef]) -> String;
    fn tools_param(&self, tools: &[&ToolDef]) -> Option<Vec<Value>>;
    fn parse(&self, msg: &Value) -> Vec<Action>;
    // The message that carries a tool result back to the model.
    fn result_msg(&self, call_id: &Option<String>, name: &str, result: &Value) -> Value;
    // The corrective message when a reply contained no usable action.
    fn nudge(&self) -> Value;
}

struct NativeCodec;

impl Codec for NativeCodec {
    // Pacing is the codec's to give: native batches, text goes one action per reply.
    // The shared system prompt stays codec-neutral. Mirrors docs/compiler/turns.md#codecs.
    fn system_suffix(&self, _tools: &[&ToolDef]) -> String {
        "\n\nBatch ALL tool calls for one section into a single reply: the searches, the upserts, and its coverage mark together.".to_string()
    }
    fn tools_param(&self, tools: &[&ToolDef]) -> Option<Vec<Value>> {
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
    fn nudge(&self) -> Value {
        json!({"role": "user", "content": "Reply by calling a tool. When the work is complete, call done."})
    }
}

struct TextCodec;

impl Codec for TextCodec {
    fn system_suffix(&self, tools: &[&ToolDef]) -> String {
        let mut s = String::from(
            "\n\nTOOL PROTOCOL: you have no native tool support. Reply with EXACTLY ONE JSON object per message, nothing else:\n{\"tool\": \"<name>\", \"args\": { ... }}\nThe result comes back as a message starting with RESULT:. Then reply with your next action. Available tools:\n",
        );
        for t in tools {
            s.push_str(&format!("- {}: {} args schema: {}\n", t.name, t.description, t.parameters));
        }
        s
    }
    fn tools_param(&self, _tools: &[&ToolDef]) -> Option<Vec<Value>> {
        None
    }
    fn parse(&self, msg: &Value) -> Vec<Action> {
        let content = msg["content"].as_str().unwrap_or_default();
        if let Some(obj) = llm::extract_json_object(content) {
            if let Ok(v) = serde_json::from_str::<Value>(&obj) {
                if let Some(name) = v["tool"].as_str() {
                    return vec![Action::Call {
                        id: None,
                        name: name.to_string(),
                        args: v["args"].clone(),
                    }];
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
    fn nudge(&self) -> Value {
        json!({"role": "user", "content": "Reply with exactly one JSON action object: {\"tool\": \"<name>\", \"args\": {...}}. When the work is complete, use the done tool."})
    }
}

// ---- prompts ----

// The feedback contract, high in every turn's system prompt: the model has a channel
// for jazyk's own defects, and using it is not an excuse to stop working.
// Mirrors docs/compiler/turns.md#message-loop and docs/compiler/tools.md#feedback-tool.
const FEEDBACK_NOTE: &str = "If anything here is ambiguous, wrong, or confusing (these instructions, a tool, its arguments, or an error message), call report_feedback saying what blocked you, then continue with your best judgment. It reaches jazyk's developers, not this project's authors, and never changes the graph. Problems in the documents themselves are not feedback.";

// The note rides directly under the role line: the first paragraph says what the turn
// is, the second how to report that the rest of the prompt failed it.
fn with_feedback_note(system: &str) -> String {
    match system.split_once("\n\n") {
        Some((role, rest)) => format!("{}\n\n{}\n\n{}", role, FEEDBACK_NOTE, rest),
        None => format!("{}\n\n{}", system, FEEDBACK_NOTE),
    }
}

const RECONCILE_SYSTEM: &str = r#"You are the compilation turn of jazyk, a natural language compiler. Your job: bring the semantic graph in line with one document's changed sections, by calling tools.

The graph holds entities (domain concepts), EARS requirements attached to entities, and a coverage mark per section.

The documents do not necessarily describe software. The SUBJECT is whatever the documents are about and its parts: a service, a slide deck, a book, a course, a schematic, a contract. Read "the system" in any EARS pattern as that subject. An artifact that does nothing still has obligations: it must contain, show, and look like what the documents say.

Work section by section, finishing one before starting the next. For ONE section:
1. Apply this test to every sentence: does it say what the subject or one of its parts IS, DOES, CONTAINS, SHOWS, USES, ALLOWS, REQUIRES, or LIMITS? If yes, it is a requirement. Documentation rarely says 'shall'; rephrase the sentence into an EARS shall statement and keep the source sentence verbatim as the quote. Statements of composition and technology choice pass the test: "The gateway is a REST service built with Go" yields TWO requirements ("The gateway shall be a REST service.", "The gateway shall be built with Go."), one atomic fact each, both quoting that same sentence. Never put two facts in one ears statement. Access and permission rules pass the test too: "All management operations can be performed by Admins only." IS a requirement ("The user management system shall allow only Admins to perform management operations."), not background. Content, appearance, and material facts pass the test too, and are the most commonly missed kind: what an artifact says, shows, or contains is an obligation on it ("This slide shows a headline title `Jazyk`" yields "The Introduction slide shall show a headline title `Jazyk`."), and a stated value, color, font, size, or measurement is an obligation on the thing it describes ("The primary color is #248555" yields "The slides shall use #248555 as the primary color."). A stated fact is never "just a fact": the document states it because the result must match it.
2. A fenced code block giving pseudo code, steps, or an algorithm is a claim about the system, step by step. Extract one requirement per step that states behavior, quoting that step's own line verbatim. A branch is its own obligation: "If stripped line is empty string, continue to next line" is an unwanted-behavior requirement ("If a stripped line is empty, then the system shall skip it."). A variable local to the steps (a loop counter, an accumulator array) is requirement detail, never an entity. A block is an illustration only when it shows sample data or a payload format. The section is covered only when EACH behavioral step has a requirement; extracting one step and skipping the rest is a dishonest coverage claim.
3. A test case with concrete input and expected output is an event-driven obligation on the system under test: "When given the input lines `321`, `654`, `453`, the sort utility shall output `321`, `453`, `654`." Quote the case's lead-in line; the concrete values ride in the ears text. Reference the entity of the system under test, never the test file or the suite. A test-case section is NEVER non-normative.
4. A sentence ending in a colon followed by a list is a claim about EACH item. The lead-in sentence alone states nothing; never record it as a requirement by itself. Record one requirement per list item, quoting that item's own bullet line verbatim. An item naming an actor, a component, a sub-system, or a stored field also introduces that entity. An item that is a link still counts: under "The sub-systems are:", the item "[User Management](./user.md)" states that the parent includes the User Management sub-system; record that requirement with entities for both and an edge.
5. For every entity a requirement mentions: call search first. Reuse an existing entity when it means the same concept, even under another name: "backend", "backend system", and "the Warehouse backend" are ONE entity. When you reuse under a different wording, record that wording with update_entity add_aliases. Create with upsert_entity only when search finds nothing that means the same thing. Tools take ids (ent:...), never display names.
6. Tag each requirement with the entity the statement is about (its own grammatical subject) AND every other entity the statement names: "The user account shall have a username" references both the account and the username field; that reference is what ties the field into the graph. Only entities count here: a named operation (createUser) or technology (React) is requirement detail, never an entity. Never substitute a broader system for a named part ("The inventory system manages products" is about the inventory system, not the application containing it). One sentence introduces at most one entity for its subject: "This software is a warehouse management system" defines ONE entity, not two. A pronoun subject (This, It) resolves to the system the document already introduced: "This is a script written in javascript" is an obligation on that existing entity, never a new Script entity minted from the predicate noun.
7. Record each requirement with upsert_requirement. The quote is copied character for character from the section body shown to you; for a bulleted item, quote that single bullet line exactly as it appears. Never paraphrase, merge, or reflow a quote.
8. Then set_coverage for the section, exactly once, after its extraction: covered when you recorded (or the pack shows the section already yielded) a requirement sourced from it. non-normative is the EXCEPTION, allowed only when NO sentence passed the test: navigation pages that only link elsewhere, glossaries defining outside-world terms, changelogs, roadmap wish lists. If any sentence is about the subject, extract from it instead. These three reasons for non-normative are always wrong and will be rejected: "it states a fact, not a requirement", "it describes content or appearance, not behavior", "it is not a requirement on the system".

Then repeat for the next dirty section. Stale anchors are a contract: for each one, if the document still states the fact, re-record it with upsert_requirement (the same statement with a fresh verbatim quote updates it in place); if the fact is gone, delete_requirement. done is rejected while a stale anchor is untouched. When every dirty section has its coverage mark, call done with a one-line summary. If done is rejected, repair exactly what the error names, then call done again.

Rules:
- Entities are the subject's own parts, actors, and domain objects: a component, a type, a field, a user role, a stored record, a product, a slide, a chapter. Never file paths, CLI flags, markdown terms, or generic phrases. The document itself (a glossary, a roadmap, an overview) is not an entity.
- Technologies, languages, and third-party tools named in a statement (React, Go, PostgreSQL) belong in the ears text, NOT as entities. "The gateway shall be built with Go" references the entity gateway only.
- A sentence whose only content is WHERE something is written is navigation, not an obligation: "The slides themselves are defined under [Slides](./slides.md)" and "This document describes how X works" say nothing the result must satisfy. Skip them. The test is whether the sentence constrains the result or only the documentation of it. A list item that names a part IS a fact about the result ("the sub-systems are: [User Management](./user.md)"), and the difference is that the item names a part, while the navigation sentence names a file.
- Extract only obligations the source itself states; never invent facts the text does not carry. But grammar does not matter: a plain declarative sentence about the subject is an obligation, and a sentence naming what something is built with, composed of, made to look like, or responsible for is a requirement, not background.
- The gateway sentences in these instructions are illustrations, not content. Extract only from the section bodies shown in the work pack; a quote that is not in the document will be rejected.
- When a requirement ties two entities structurally, declare the pair in edges with a relationship type. A sub-system list is the common case: "the sub-systems are: X, Y" ties each sub-system to its parent.
- Prefer attaching detail to a requirement over minting a new entity; mint a sub-entity only when statements are about it directly.
- When the pack has a "Linked from" section, another document already listed this one as one of its parts and minted the entity for it. That entity is what this document is about: reference it from the requirements you extract here, and never mint a second entity for the same concept under the document's own heading. E.g. if the pack says a parent's item "[Introduction](./slide-intro.md)" introduced ent:introduction, then every statement in this document is an obligation on ent:introduction.
- Never set scope on an entity unless the documents explicitly name a bounded context. An invented scope splits one concept into two.
- The ears text may rephrase the statement into EARS form, but the quote must stay a verbatim copy of the source sentence.
- A tool error names what was wrong and how to repair the call; fix it and continue.
- Staging nothing is a correct outcome. If the graph already reflects the sections (the pack lists what each section already yielded), set coverage and finish. Prefer a no-op over cosmetic rewording of existing definitions or statements; stability of the graph across builds matters more than polish. Stage only what the document supports."#;

const REVIEW_REQ_SYSTEM: &str = r#"You are the pair-review turn of jazyk, a natural language compiler. Your job: judge ONE changed requirement against each of its neighbor statements, by calling tools.

The pack shows the changed requirement and its neighbors, each with its statement (ears), verbatim source quote, and source section. The neighbors were selected deterministically because they overlap this requirement; judge every one of them.

For EACH neighbor give exactly one verdict:
- duplicate: the same obligation reworded. When both quote the same document, delete the worse-sourced one with delete_requirement (keep the one whose quote states the obligation directly). When they quote different documents, the redundancy is intentional: report_diagnostic rule duplicate-requirement, severity info, subjects both ids, message saying both are kept.
- contradiction: the two cannot both hold, in their statements or in their source quotes (opposite defaults, opposite behavior for the same condition, incompatible values). report_diagnostic rule contradiction, subjects both ids, message quoting the conflicting claims. Severity error when no reading lets both hold, warning otherwise.
- consistent: both can hold and they state different facts. No action, no diagnostic.

Ground each verdict in the quotes as much as the ears statements: the quote is the document's own text. If the changed requirement's ears no longer says what its quote says, first repair the ears with update_requirement, then judge the pairs against the repaired statement.

Then:
- If an open diagnostic listed in the pack no longer holds, resolve it with resolve_diagnostic.
- Call done with a one-line summary naming the verdict per neighbor.

Rules:
- Judge only the pairs shown. Use read_section or get_entity only when a quote alone cannot settle a verdict.
- A duplicate is the same obligation, not the same topic. Two statements about the same flag that impose different behavior contradict, they do not duplicate.
- If every pair is consistent, call done immediately with no mutations."#;

const REVIEW_SYSTEM: &str = r#"You are the review turn of jazyk, a natural language compiler. Your job: judge one entity whose facts changed, by calling tools.

Work in this order:
1. Read the entity and its requirements (gathered across all documents) in the pack below.
2. If the definition no longer matches the requirements as a whole, refresh it with update_entity.
3. Judge every lookalike candidate listed below. A name variant ("backend" vs "backend system"), a synonym, or the same thing at different detail is the SAME concept: merge with merge_entities (keep the better-established id) and say why. Merging is the expected outcome for lookalikes; keeping both is the exception and needs a reason. The absorbed name survives as an alias and its requirements follow automatically.
4. Judge each requirement listed under "Statements naming this entity without referencing it": when the statement is about this entity, add the entity to the requirement with update_requirement, passing ONLY id and entities (the full list, including the ones already there). Never pass section or quote on such a call: those two re-anchor the provenance to a different sentence in the document, they are not the ears statement, and a call that only adds a reference must leave them out. A missing reference is what strands an entity unreachable.
5. Delete duplicate requirements: when two requirements on this entity state the same fact (the same obligation reworded), keep the better-sourced one and delete_requirement the other, saying why. A lead-in sentence's requirement duplicated by its list item's requirement is the common case; keep the item's.
6. Report real problems with report_diagnostic: rule contradiction for requirements that cannot all hold, duplicate-entity for two entities that are one concept, ambiguity for a statement open to more than one reading, missing-link for a concept the documents rely on but never define.
7. If requirements tie this entity to another structurally but declare no edges, add them with update_requirement, passing ONLY id and edges (with a relationship type). Again, no section and no quote.
8. If an open diagnostic shown in the pack no longer holds, resolve it with resolve_diagnostic.
9. Call done with a one-line summary.

Rules:
- Documentation is loose by design. Flag only findings the document author can act on. Do not demand formal-spec completeness (persistence details, versioning, exhaustive cases).
- Severity: error only when two statements cannot both hold; warning for real but repairable issues; info for observations.
- If everything is coherent, call done immediately with no mutations."#;

// ---- initial packs ----

fn reconcile_pack(store: &Store, item: &WorkItem, budget: usize) -> String {
    let mut s = String::new();
    let doc = &item.target;
    s.push_str(&format!("# Work item: reconcile document {}\n", doc));
    if let Some(rec) = store.docs.get(doc) {
        let covered = rec.coverage.len();
        s.push_str(&format!("sections: {} total, {} with coverage\n", rec.sections.len(), covered));
    }

    // Incoming links the graph already resolved: a parent listed this document as one of
    // its parts, and that list item minted the part's entity. The link is what says which
    // entity this document details. Mirrors docs/compiler/turns.md#incoming-links.
    let mut incoming: Vec<String> = Vec::new();
    for (id, e) in &store.graph.entities {
        for m in &e.mentions {
            if &m.doc != doc && crate::md::doc_links(&m.quote, &m.doc).iter().any(|l| l == doc) {
                incoming.push(format!(
                    "- {}#{} \"{}\" introduced {} ({})",
                    m.doc,
                    m.section,
                    crate::llm::truncate(&m.quote, 100),
                    id,
                    e.name
                ));
            }
        }
    }
    for (id, r) in &store.graph.requirements {
        if &r.source.doc != doc && crate::md::doc_links(&r.source.quote, &r.source.doc).iter().any(|l| l == doc) {
            incoming.push(format!(
                "- {}#{} \"{}\" states {} ({})",
                r.source.doc,
                r.source.section,
                crate::llm::truncate(&r.source.quote, 100),
                id,
                crate::llm::truncate(&r.ears, 100)
            ));
        }
    }
    incoming.sort();
    incoming.dedup();
    if !incoming.is_empty() {
        incoming.truncate(12);
        s.push_str("\n## Linked from (what other documents already say this one details)\n");
        s.push_str(&incoming.join("\n"));
        s.push_str("\n\nThis document details what those statements introduced. Its requirements reference those entities; do not mint a second entity for the same concept.\n");
    }

    // Known entities: this document's neighborhood first, then the rest of the graph.
    let mut lines: Vec<String> = Vec::new();
    let mut listed: Vec<&String> = Vec::new();
    for (id, e) in &store.graph.entities {
        if e.mentions.iter().any(|m| &m.doc == doc) {
            lines.push(format!("- {} ({}): {}", id, e.name, crate::llm::truncate(e.definition.as_deref().unwrap_or(""), 80)));
            listed.push(id);
        }
    }
    for (id, e) in &store.graph.entities {
        if lines.len() >= 40 {
            lines.push(format!("- (and {} more; use search)", store.graph.entities.len() - lines.len() + 1));
            break;
        }
        if !listed.contains(&id) {
            lines.push(format!("- {} ({}): {}", id, e.name, crate::llm::truncate(e.definition.as_deref().unwrap_or(""), 80)));
        }
    }
    if !lines.is_empty() {
        s.push_str("\n## Known entities (search before creating new ones)\n");
        s.push_str(&lines.join("\n"));
        s.push('\n');
    }

    if !item.stale_anchors.is_empty() {
        s.push_str("\n## Stale anchors (their source text changed or vanished; re-anchor, update, or delete)\n");
        for a in &item.stale_anchors {
            if let Some(r) = store.graph.requirements.get(a) {
                s.push_str(&format!("- {}: {} (was quoted: \"{}\")\n", a, r.ears, crate::llm::truncate(&r.source.quote, 100)));
            } else if let Some(e) = store.graph.entities.get(a) {
                s.push_str(&format!("- {} (entity {}): a mention's section changed\n", a, e.name));
            }
        }
    }

    s.push_str("\n## Dirty sections\n");
    let per_section = budget.saturating_sub(s.len()) / item.dirty_sections.len().max(1);
    if let Some(rec) = store.docs.get(doc) {
        for r in &item.dirty_sections {
            if let Some(sec) = rec.sections.get(r) {
                let cov = rec
                    .coverage
                    .get(r)
                    .map(|c| c.state.clone())
                    .unwrap_or_else(|| "unprocessed".to_string());
                s.push_str(&format!("\n### {}#{} ({}) [coverage: {}]\n", doc, r, sec.title, cov));
                if sec.raw.len() <= per_section {
                    s.push_str(&sec.raw);
                } else {
                    s.push_str(&crate::llm::truncate(&sec.raw, per_section));
                    s.push_str(&format!("\n(truncated; read_section {}#{} for the rest)", doc, r));
                }
                s.push('\n');
                // What the section already yielded: an unchanged statement is a no-op,
                // and a coverage claim must see the requirements anchored here before
                // judging the section non-normative.
                let existing: Vec<String> = store
                    .graph
                    .requirements
                    .iter()
                    .filter(|(_, q)| &q.source.doc == doc && &q.source.section == r)
                    .map(|(id, q)| format!("- {}: {}", id, q.ears))
                    .collect();
                if !existing.is_empty() {
                    s.push_str("Already extracted from this section (leave unchanged statements alone):\n");
                    s.push_str(&existing.join("\n"));
                    s.push('\n');
                }
            }
        }
    }
    s
}

// The pair-review pack: the changed requirement and its neighbors side by side. The
// neighbor set is recomputed here with the same deterministic function the reconciler
// used to schedule the turn (docs/compiler/reconciler.md#waves).
fn review_requirement_pack(store: &Store, rid: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!("# Work item: review changed requirement {} against its neighbors\n", rid));
    let fmt = |id: &str, r: &crate::model::Requirement| {
        format!(
            "- {}\n  ears: {}\n  quote: \"{}\"\n  section: {}#{}\n",
            id,
            r.ears,
            r.source.quote,
            r.source.doc,
            r.source.section
        )
    };
    if let Some(r) = store.graph.requirements.get(rid) {
        s.push_str("\n## The changed requirement\n");
        s.push_str(&fmt(rid, r));
    }
    let neighbors = store.pair_review_neighbors(rid);
    if !neighbors.is_empty() {
        s.push_str("\n## Neighbors (one verdict each: duplicate, contradiction, or consistent)\n");
        for n in &neighbors {
            if let Some(r) = store.graph.requirements.get(n) {
                s.push_str(&fmt(n, r));
            }
        }
    }
    let open: Vec<String> = store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| d.lifecycle == "open" && d.subjects.iter().any(|x| x == rid))
        .map(|(id, d)| format!("- {} ({}, {}): {}", id, d.rule, d.severity, d.message))
        .collect();
    if !open.is_empty() {
        s.push_str("\n## Open diagnostics naming this requirement (resolve any that no longer hold)\n");
        s.push_str(&open.join("\n"));
        s.push('\n');
    }
    s
}

fn review_pack(store: &Store, entity_id: &str, budget: usize, lint: &Linting) -> String {
    let mut s = String::new();
    s.push_str(&format!("# Work item: review entity {}\n\n", entity_id));
    match crate::context::assemble(
        store,
        entity_id,
        &crate::context::Focus { parents: 1, mentions: 1, requirements: 2 },
        budget.saturating_sub(1200),
    ) {
        Ok(pack) => s.push_str(&pack.pack),
        Err(e) => s.push_str(&format!("(context error: {})\n", e)),
    }
    // Lookalike candidates: token-overlap hits on the entity's name, excluding itself.
    if let Some(e) = store.graph.entities.get(entity_id) {
        let hits = store.search(&e.name);
        let others: Vec<String> = hits
            .iter()
            .filter(|(id, _, _)| id != entity_id)
            .map(|(id, name, def)| format!("- {} ({}): {}", id, name, crate::llm::truncate(def, 80)))
            .collect();
        if !others.is_empty() {
            s.push_str("\n## Lookalike candidates (merge only if truly the same concept)\n");
            s.push_str(&others.join("\n"));
            s.push('\n');
        }
        // Missing-reference candidates: requirements whose statement names this entity
        // (word-bounded name or alias) but whose entities list omits it. A missing
        // reference is what strands an entity cluster unreachable from the roots.
        let contains_word = |hay: &str, word: &str| -> bool {
            let hay = hay.to_lowercase();
            let word = word.to_lowercase();
            let mut start = 0;
            while let Some(pos) = hay[start..].find(&word) {
                let (b, e) = (start + pos, start + pos + word.len());
                let ok_before = b == 0 || !hay.as_bytes()[b - 1].is_ascii_alphanumeric();
                let ok_after = e >= hay.len() || !hay.as_bytes()[e].is_ascii_alphanumeric();
                if ok_before && ok_after {
                    return true;
                }
                start = e;
            }
            false
        };
        let names: Vec<&String> = std::iter::once(&e.name).chain(e.aliases.iter()).collect();
        let unreferenced: Vec<String> = store
            .graph
            .requirements
            .iter()
            .filter(|(_, r)| !r.entities.iter().any(|x| store.resolve_id(x) == entity_id))
            .filter(|(_, r)| names.iter().any(|n| contains_word(&r.ears, n)))
            .take(6)
            .map(|(rid, r)| format!("- {}: {}", rid, r.ears))
            .collect();
        if !unreferenced.is_empty() {
            s.push_str("\n## Statements naming this entity without referencing it (add the reference if the statement is about it)\n");
            s.push_str(&unreferenced.join("\n"));
            s.push('\n');
        }
    }
    // Project lint rules run in review turns; violations use report_diagnostic rule `lint`.
    if !lint.warnings.is_empty() || !lint.errors.is_empty() {
        s.push_str("\n## Project lint rules\nReport a violation with report_diagnostic, rule `lint`, and the severity listed.\n");
        for w in &lint.warnings {
            s.push_str(&format!("- (warning) {}\n", w));
        }
        for e in &lint.errors {
            s.push_str(&format!("- (error) {}\n", e));
        }
    }
    s
}

// ---- the loop ----

pub struct TurnOutput {
    pub session: ToolSession,
    pub rounds: u32,
    // Completion tokens the turn spent, journaled per changeset
    // (docs/compiler/graph.md#journal).
    pub tokens: u64,
    pub failed: Option<String>,
}

fn condense(v: &Value, n: usize) -> String {
    llm::truncate(&v.to_string(), n)
}

// The section a tool call names, when it belongs to this turn's document. Tools carry
// it as `section` (set_coverage, upsert_requirement, read_section) or under a mention
// (upsert_entity); either form may be qualified with the document.
fn named_section(args: &Value, doc: &str) -> Option<String> {
    let raw = args["section"].as_str().or_else(|| args["mention"]["section"].as_str())?;
    match crate::model::split_section_ref(raw) {
        Some((d, sec)) => (d == doc).then_some(sec),
        None => raw.starts_with('/').then(|| raw.to_string()),
    }
}

// The payload behind a condensed line, only when condensing cut something, capped so
// a huge context pack cannot flood the trace file.
fn full_payload(v: &Value) -> Option<String> {
    let s = v.to_string();
    if s.len() <= 160 {
        None
    } else {
        Some(llm::truncate(&s, 8_000).to_string())
    }
}

pub fn run_turn(llm: &Llm, snapshot: Store, item: &WorkItem, limits: &Limits, lint: &Linting, gen: &crate::gen::GenSettings, trace: &Trace) -> TurnOutput {
    let prefix = format!("{} {}", item.task, item.target);
    let scope = match item.task.as_str() {
        "reconcile-doc" => WorkScope {
            task: item.task.clone(),
            doc: Some(item.target.clone()),
            target_sections: item.dirty_sections.clone(),
            stale_anchors: item.stale_anchors.clone(),
        },
        _ => WorkScope { task: item.task.clone(), doc: None, target_sections: Vec::new(), stale_anchors: Vec::new() },
    };
    let (system, pack) = match item.task.as_str() {
        "reconcile-doc" => (RECONCILE_SYSTEM, reconcile_pack(&snapshot, item, limits.context_budget)),
        "review-requirement" => (REVIEW_REQ_SYSTEM, review_requirement_pack(&snapshot, &item.target)),
        _ => (REVIEW_SYSTEM, review_pack(&snapshot, &item.target, limits.context_budget, lint)),
    };
    let names = toolset(&item.task);
    let all_defs = catalog();
    let defs: Vec<&ToolDef> = all_defs.iter().filter(|t| names.contains(&t.name)).collect();
    let mut session = ToolSession::new(snapshot, scope, limits.turn_mutations, limits.context_budget);
    session.gen = gen.clone();
    // References a feedback entry records: which turn, which model, which run.
    session.caller = crate::feedback::Caller {
        source: "turn".into(),
        target: item.target.clone(),
        model: llm.model.clone(),
        run: trace.run(),
        ..Default::default()
    };

    trace.event(TraceEvent::TurnStart {
        label: prefix.clone(),
        task: item.task.clone(),
        target: item.target.clone(),
        doc: (item.task == "reconcile-doc").then(|| item.target.clone()),
        sections: item.dirty_sections.clone(),
        dirty: item.dirty_sections.len(),
        stale: item.stale_anchors.len(),
    });
    trace.verbose(&prefix, &format!("--- context pack ---\n{}\n--- end pack ---", pack));

    // Every prompt and reply this turn sends reports under the turn's label.
    let llm = llm.with_trace(trace);
    let llm = &llm;

    // Codec selection with a first-round probe: native unless the run already learned otherwise.
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

    // Accumulates across codec downgrades: a probe round costs tokens too.
    let mut tokens = 0u64;
    // Where the turn is in its document, for the frontends that show it in place.
    let turn_doc: Option<String> = (item.task == "reconcile-doc").then(|| item.target.clone());
    let mut at_section: Option<String> = None;
    'codec: loop {
        let codec: Box<dyn Codec> = if mode == 2 { Box::new(TextCodec) } else { Box::new(NativeCodec) };
        session.caller.codec = if mode == 2 { "text".into() } else { "native".into() };
        let mut messages = vec![
            json!({"role": "system", "content": format!("{}{}", with_feedback_note(system), codec.system_suffix(&defs))}),
            json!({"role": "user", "content": pack.clone()}),
        ];
        let tools_param = codec.tools_param(&defs);
        let mut invalid_streak = 0u32;
        let mut rounds = 0u32;
        // Identical calls seen this turn, keyed by tool plus canonical arguments. A model
        // that re-asks a question it already asked is looping, not working; the harness
        // says so instead of letting it burn the budget.
        // Mirrors docs/compiler/turns.md#repeated-calls.
        let mut repeats: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
        // The round budget scales with extraction density: a one-action-per-reply model
        // needs a round per mutation, so a dense work item gets at least 8 rounds per
        // dirty section. Mirrors docs/compiler/turns.md#budgets.
        let round_budget = limits.turn_rounds.max(item.dirty_sections.len() as u32 * 8);

        while rounds < round_budget {
            rounds += 1;
            // The label groups the turn, the step names the round inside it.
            let step = format!("r{}", rounds);
            let msg = match llm.chat_messages(&messages, tools_param.as_deref(), &prefix, &step) {
                Ok((m, t)) => {
                    tokens += t;
                    m
                }
                Err(e) if e.starts_with("tools-rejected:") && mode != 2 => {
                    trace.line(&prefix, "endpoint rejected native tools; downgrading to the text codec for this run");
                    llm::set_tools_mode(2);
                    mode = 2;
                    continue 'codec;
                }
                Err(e) => {
                    return TurnOutput { session, rounds, tokens, failed: Some(e) };
                }
            };
            let actions = codec.parse(&msg);
            // First-round probe: a prose-only reply under the native codec means the model
            // does not drive tools natively; downgrade once, sticky for the run.
            if rounds == 1
                && mode != 2
                && llm::tools_mode() == 0
                && !actions.iter().any(|a| matches!(a, Action::Call { .. }))
            {
                trace.line(&prefix, "model answered prose without tool calls; downgrading to the text codec for this run");
                llm::set_tools_mode(2);
                mode = 2;
                continue 'codec;
            }
            if mode != 2 && llm::tools_mode() == 0 && actions.iter().any(|a| matches!(a, Action::Call { .. })) {
                llm::set_tools_mode(1);
                mode = 1;
            }
            messages.push(msg.clone());

            // Reasoning carried in a dedicated field is model text too; content prose
            // reaches the trace through the codec's Action::Text.
            for field in ["reasoning_content", "reasoning"] {
                if let Some(r) = msg[field].as_str() {
                    if !r.trim().is_empty() {
                        trace.event(TraceEvent::ModelText { label: prefix.clone(), text: r.trim().to_string() });
                    }
                }
            }

            if !actions.iter().any(|a| matches!(a, Action::Call { .. })) {
                invalid_streak += 1;
                if invalid_streak >= 3 {
                    // Implicit done: a model that goes silent with staged work is treated
                    // as having called done; the same commit gates apply.
                    if session.finish_implicit("(implicit: the model stopped calling tools)") {
                        trace.event(TraceEvent::TurnDone {
                            label: prefix.clone(),
                            staged: session.staged.len(),
                            rounds,
                            mode: "implicit".into(),
                            summary: String::new(),
                        });
                        return TurnOutput { session, rounds, tokens, failed: None };
                    }
                    return TurnOutput {
                        session,
                        rounds,
                        tokens,
                        failed: Some("three consecutive replies without a usable tool call".into()),
                    };
                }
                messages.push(codec.nudge());
                continue;
            }

            let mut errored = false;
            for action in actions {
                match action {
                    Action::Text(t) => {
                        let t = t.trim();
                        if !t.is_empty() {
                            trace.event(TraceEvent::ModelText { label: prefix.clone(), text: t.to_string() });
                        }
                    }
                    Action::Call { id, name, args } => {
                        trace.event(TraceEvent::ToolCall {
                            label: prefix.clone(),
                            name: name.clone(),
                            summary: condense(&args, 160),
                            full: full_payload(&args),
                        });
                        trace.verbose(&prefix, &format!("full args: {}", args));
                        // Repeat guard: the same call with the same arguments has the same
                        // answer. The second one is warned, the third is refused, so a turn
                        // spends its rounds on the document instead of on a stuck question.
                        let seen = if name == "done" {
                            0
                        } else {
                            let key = format!("{}|{}", name, args);
                            let c = repeats.entry(key).or_insert(0);
                            *c += 1;
                            *c
                        };
                        if seen >= 3 {
                            errored = true;
                            let e = crate::tools::ToolError::new(
                                "repeated-call",
                                format!(
                                    "this is call {} to `{}` with identical arguments in this turn, and the answer has not changed. Stop calling it. Act on the answer you already have: record what the section states, mark its coverage, and move to the next section.",
                                    seen, name
                                ),
                            );
                            trace.event(TraceEvent::ToolError {
                                label: prefix.clone(),
                                rule: e.rule.clone(),
                                message: e.message.clone(),
                            });
                            messages.push(codec.result_msg(&id, &name, &e.to_value()));
                            continue;
                        }
                        let result = match session.dispatch(&name, &args) {
                            Ok(mut v) => {
                                if seen == 2 {
                                    if let Some(o) = v.as_object_mut() {
                                        o.insert(
                                            "repeat".into(),
                                            json!(format!(
                                                "you already made this exact `{}` call in this turn; this is the same answer. Do not call it again, act on it.",
                                                name
                                            )),
                                        );
                                    }
                                }
                                let v = v;
                                trace.event(TraceEvent::ToolResult {
                                    label: prefix.clone(),
                                    name: name.clone(),
                                    summary: condense(&v, 160),
                                    full: full_payload(&v),
                                });
                                trace.verbose(&prefix, &format!("full result: {}", v));
                                // An accepted call that names a section says where the
                                // turn is; a rejected one names nothing real.
                                if let Some(doc) = &turn_doc {
                                    if let Some(sec) = named_section(&args, doc) {
                                        if at_section.as_deref() != Some(sec.as_str()) {
                                            at_section = Some(sec.clone());
                                            trace.event(TraceEvent::Section {
                                                label: prefix.clone(),
                                                doc: doc.clone(),
                                                section: sec,
                                                tool: name.clone(),
                                            });
                                        }
                                    }
                                }
                                v
                            }
                            Err(e) => {
                                errored = true;
                                trace.event(TraceEvent::ToolError {
                                    label: prefix.clone(),
                                    rule: e.rule.clone(),
                                    message: e.message.clone(),
                                });
                                e.to_value()
                            }
                        };
                        messages.push(codec.result_msg(&id, &name, &result));
                        if session.done.is_some() {
                            trace.event(TraceEvent::TurnDone {
                                label: prefix.clone(),
                                staged: session.staged.len(),
                                rounds,
                                mode: "done".into(),
                                summary: session.done.clone().unwrap_or_default(),
                            });
                            return TurnOutput { session, rounds, tokens, failed: None };
                        }
                    }
                }
            }
            if errored {
                invalid_streak += 1;
                if invalid_streak >= 3 {
                    return TurnOutput {
                        session,
                        rounds,
                        tokens,
                        failed: Some("three consecutive rounds with rejected tool calls".into()),
                    };
                }
            } else {
                invalid_streak = 0;
            }
        }
        // Same implicit-done rule at the round budget: commit valid staged work.
        if session.finish_implicit("(implicit: round budget exhausted)") {
            trace.event(TraceEvent::TurnDone {
                label: prefix.clone(),
                staged: session.staged.len(),
                rounds: round_budget,
                mode: "budget".into(),
                summary: String::new(),
            });
            return TurnOutput { session, rounds: round_budget, tokens, failed: None };
        }
        return TurnOutput {
            session,
            rounds: round_budget,
            tokens,
            failed: Some(format!("round budget ({}) exhausted without done", round_budget)),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_note_rides_under_the_role_line_of_every_prompt() {
        for system in [RECONCILE_SYSTEM, REVIEW_REQ_SYSTEM, REVIEW_SYSTEM] {
            let s = with_feedback_note(system);
            let paras: Vec<&str> = s.split("\n\n").collect();
            assert!(paras[0].starts_with("You are the"), "role line stays first");
            assert_eq!(paras[1], FEEDBACK_NOTE, "the note is the second paragraph");
            assert!(s.contains("report_feedback"));
            // Nothing of the original prompt is lost to the insertion.
            assert!(s.ends_with(system.split_once("\n\n").unwrap().1));
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
    fn section_events_follow_this_document_only() {
        let doc = "docs/main.md";
        assert_eq!(named_section(&json!({"section": "/shop/cart"}), doc).as_deref(), Some("/shop/cart"));
        assert_eq!(
            named_section(&json!({"section": "docs/main.md#/shop"}), doc).as_deref(),
            Some("/shop")
        );
        // Another document's section says nothing about where this turn is.
        assert_eq!(named_section(&json!({"section": "docs/other.md#/shop"}), doc), None);
        // An entity mention carries it one level down.
        assert_eq!(
            named_section(&json!({"mention": {"section": "/shop", "quote": "x"}}), doc).as_deref(),
            Some("/shop")
        );
        assert_eq!(named_section(&json!({"query": "cart"}), doc), None);
    }

    #[test]
    fn text_codec_prose_is_text() {
        let c = TextCodec;
        let msg = json!({"role": "assistant", "content": "The document describes a shop."});
        let actions = c.parse(&msg);
        assert!(matches!(actions[0], Action::Text(_)));
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
