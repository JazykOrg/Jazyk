// The turn harness: one focused LLM session with tools. Wires the model to the tool
// registry through a codec, stages mutations, and hands the finished changeset back to
// the reconciler for commit. Mirrors docs/compiler/turns.md.
use crate::llm;
use crate::model::{split_section_ref, AnchorProposal, WorkItem};
use crate::project::Linting;
use crate::store::Store;
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
        // Proposals an align-doc turn must decide; zero for every other task.
        #[serde(default, skip_serializing_if = "is_zero")]
        proposals: usize,
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
    ToolError {
        label: String,
        rule: String,
        message: String,
    },
    #[serde(rename = "modelText")]
    ModelText { label: String, text: String },
    // mode: "done" (explicit, with the model's summary), "implicit" (the model went
    // silent with staged work), or "budget" (implicit at the round budget).
    #[serde(rename = "turnDone")]
    TurnDone {
        label: String,
        staged: usize,
        rounds: u32,
        mode: String,
        summary: String,
    },
    #[serde(rename = "turnFailed")]
    TurnFailed {
        label: String,
        attempt: u32,
        error: String,
    },
    #[serde(rename = "note")]
    Note {
        label: String,
        text: String,
        verbose: bool,
    },
    // The turn moved to a section: an accepted tool call named one, and it differs
    // from the last. The sequence is the turn's path through the document.
    #[serde(rename = "section")]
    Section {
        label: String,
        doc: String,
        section: String,
        tool: String,
    },
    // One model call. The request carries the whole outgoing message list, the
    // response the raw assistant message; both are recorded in full in the
    // transcript and elided on the wire (docs/compiler/turns.md#trace-events).
    #[serde(rename = "llmRequest")]
    LlmRequest {
        label: String,
        step: String,
        model: String,
        messages: Value,
        tools: Vec<String>,
    },
    #[serde(rename = "llmResponse")]
    LlmResponse {
        label: String,
        step: String,
        ms: u64,
        tokens: u64,
        message: Value,
    },
    #[serde(rename = "llmRetry")]
    #[serde(rename_all = "camelCase")]
    LlmRetry {
        label: String,
        step: String,
        attempt: u32,
        error: String,
        wait_ms: u64,
    },
    // A wave of work items is about to run: what is queued, before any turn starts.
    #[serde(rename = "waveStart")]
    WaveStart {
        wave: u32,
        task: String,
        items: Vec<String>,
    },
    // Generation worker events, one entity per bounded task.
    #[serde(rename = "genEntityStart")]
    GenEntityStart { entity: String },
    #[serde(rename = "genEntitySkipped")]
    GenEntitySkipped { entity: String, reason: String },
    #[serde(rename = "genEntityDone")]
    GenEntityDone { entity: String, files: usize },
    // stage: "task" (the task package failed to assemble) or "generate".
    #[serde(rename = "genEntityFailed")]
    GenEntityFailed {
        entity: String,
        stage: String,
        error: String,
    },
    // Verification worker events, one ledger row at a time.
    #[serde(rename = "verifyRowStart")]
    VerifyRowStart { requirement: String, test: String },
    #[serde(rename = "verifyRowDone")]
    VerifyRowDone {
        requirement: String,
        verdict: String,
        run: String,
        evidence: String,
    },
    #[serde(rename = "verifyRowStale")]
    VerifyRowStale {
        requirement: String,
        entity: String,
        status: String,
        reason: String,
    },
    #[serde(rename = "verifyRowError")]
    VerifyRowError {
        requirement: String,
        message: String,
    },
}

// Render an event exactly as the pre-event trace printed it, so `jazyk compile`
// output is unchanged.
fn is_zero(n: &usize) -> bool {
    *n == 0
}

fn render_stderr(ev: &TraceEvent) {
    match ev {
        TraceEvent::TurnStart {
            label,
            dirty,
            stale,
            proposals,
            ..
        } if *proposals > 0 => {
            eprintln!("[{}] turn start ({} proposal(s))", label, proposals)
        }
        TraceEvent::TurnStart {
            label,
            dirty,
            stale,
            ..
        } => {
            eprintln!("[{}] turn start ({} dirty, {} stale)", label, dirty, stale)
        }
        TraceEvent::ToolCall {
            label,
            name,
            summary,
            ..
        } => eprintln!("[{}] → {} {}", label, name, summary),
        TraceEvent::ToolResult { label, summary, .. } => eprintln!("[{}] ← {}", label, summary),
        TraceEvent::ToolError {
            label,
            rule,
            message,
        } => eprintln!("[{}] ✗ {}: {}", label, rule, message),
        TraceEvent::ModelText { label, text } => {
            eprintln!("[{}] · {}", label, llm::truncate(text, 200))
        }
        TraceEvent::TurnDone {
            label,
            staged,
            rounds,
            mode,
            summary,
        } => match mode.as_str() {
            "implicit" => eprintln!(
                "[{}] ✓ implicit done ({} staged, {} rounds)",
                label, staged, rounds
            ),
            "budget" => eprintln!(
                "[{}] ✓ implicit done at round budget ({} staged)",
                label, staged
            ),
            _ => eprintln!(
                "[{}] ✓ done ({} staged, {} rounds): {}",
                label, staged, rounds, summary
            ),
        },
        TraceEvent::TurnFailed {
            label,
            attempt,
            error,
        } => {
            eprintln!("[{}] turn failed (attempt {}): {}", label, attempt, error)
        }
        TraceEvent::Note { label, text, .. } => eprintln!("[{}] {}", label, text),
        // The section path is implicit in the tool rows the default level already
        // prints; naming it again would double every line.
        TraceEvent::Section { .. } => {}
        // Model calls print their arithmetic, never their payload: the verbose context
        // pack note already carries the prompt.
        TraceEvent::LlmRequest {
            label,
            step,
            messages,
            ..
        } => {
            eprintln!(
                "[{} {}] → llm ({} messages, {} chars)",
                label,
                step,
                messages.as_array().map(|a| a.len()).unwrap_or(0),
                messages.to_string().len()
            )
        }
        TraceEvent::LlmResponse {
            label,
            step,
            ms,
            tokens,
            ..
        } => {
            eprintln!("[{} {}] ← llm ({} ms, {} tokens)", label, step, ms, tokens)
        }
        TraceEvent::LlmRetry {
            label,
            step,
            attempt,
            error,
            wait_ms,
        } => eprintln!(
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
        TraceEvent::GenEntitySkipped { entity, reason } => {
            eprintln!("[gen {}] skipped ({})", entity, reason)
        }
        TraceEvent::GenEntityDone { entity, files } => {
            eprintln!("[gen {}] done ({} file(s))", entity, files)
        }
        TraceEvent::GenEntityFailed { entity, error, .. } => {
            eprintln!("[gen {}] failed: {}", entity, error)
        }
        TraceEvent::VerifyRowStart { requirement, test } => {
            eprintln!("[test {}] start ({})", requirement, test)
        }
        TraceEvent::VerifyRowDone {
            requirement,
            verdict,
            run,
            ..
        } => {
            eprintln!("[test {}] {} ({})", requirement, verdict, run)
        }
        TraceEvent::VerifyRowStale {
            requirement,
            status,
            reason,
            ..
        } => {
            eprintln!("[test {}] {} ({})", requirement, status, reason)
        }
        TraceEvent::VerifyRowError {
            requirement,
            message,
        } => eprintln!("[test {}]{}", requirement, message),
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
        Trace {
            level,
            sink: None,
            cancel: Default::default(),
            transcript: None,
            run: None,
        }
    }
    pub fn to_sink(
        level: TraceLevel,
        sink: std::sync::Arc<dyn Fn(&TraceEvent) + Send + Sync>,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Trace {
        Trace {
            level,
            sink: Some(sink),
            cancel,
            transcript: None,
            run: None,
        }
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
        let compact: String = started
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        let stem = format!("{}-{}-cli{}", compact, kind, std::process::id());
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("{}.jsonl", stem)))
        {
            // The generation at start and finish brackets the run: the journal
            // entries between the two are the run's changesets (gui.md#jobs).
            let meta = json!({"meta": {"id": null, "kind": {"kind": kind}, "startedAt": started, "source": "cli",
                "generation": crate::store::read_generation(out)}});
            let _ = writeln!(file, "{}", meta);
            let _ = file.flush();
            self.transcript = Some(std::sync::Arc::new(std::sync::Mutex::new(Transcript {
                file,
                n: 0,
                out: out.to_path_buf(),
            })));
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
                    TraceEvent::LlmRequest { .. }
                        | TraceEvent::LlmResponse { .. }
                        | TraceEvent::Section { .. }
                );
                if !terse || self.level == TraceLevel::Verbose {
                    render_stderr(&ev);
                }
            }
        }
    }
    pub fn line(&self, prefix: &str, s: &str) {
        self.event(TraceEvent::Note {
            label: prefix.into(),
            text: s.into(),
            verbose: false,
        });
    }
    fn verbose(&self, prefix: &str, s: &str) {
        self.event(TraceEvent::Note {
            label: prefix.into(),
            text: s.into(),
            verbose: true,
        });
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// ---- prompts ----

// The feedback contract, high in every turn's system prompt: the model has a channel
// for jazyk's own defects, and using it is not an excuse to stop working.
// Mirrors docs/compiler/turns.md#message-loop and docs/compiler/tools.md#feedback-tool.
const FEEDBACK_NOTE: &str = include_str!("../../docs/compiler/goals/prompts/feedback-note.md");

// The note rides directly under the role line: the first paragraph says what the turn
// is, the second how to report that the rest of the prompt failed it.
pub(crate) fn with_feedback_note(system: &str) -> String {
    match system.split_once("\n\n") {
        Some((role, rest)) => format!("{}\n\n{}\n\n{}", role.trim_end(), FEEDBACK_NOTE, rest),
        None => format!("{}\n\n{}", system.trim_end(), FEEDBACK_NOTE),
    }
}

const ALIGN_SYSTEM: &str = include_str!("../../docs/compiler/goals/prompts/place-anchors.md");

const RECONCILE_SYSTEM: &str =
    include_str!("../../docs/compiler/goals/prompts/reconcile-section.md");

const REVIEW_REQ_SYSTEM: &str = include_str!("../../docs/compiler/goals/prompts/rejudge-pair.md");

const REVIEW_SYSTEM: &str = include_str!("../../docs/compiler/goals/prompts/review-entity.md");

const GENERATE_SYSTEM: &str = include_str!("../../docs/compiler/goals/prompts/generate.md");

const BIND_SYSTEM: &str = include_str!("../../docs/compiler/goals/prompts/bind.md");

// ---- initial packs ----

// One task's system prompt and work pack, by task type. The single source both
// consumers use: run_turn hands them to the in-process model, begin_compilation ships
// them over MCP as instructions plus package. Mirrors docs/compiler/turns.md#task-types.
pub fn task_prompt(
    store: &Store,
    item: &WorkItem,
    lint: &Linting,
    gen: &crate::gen::GenSettings,
) -> (&'static str, String) {
    let budget = crate::limits::CONTEXT_BUDGET;
    match item.task.as_str() {
        "align-doc" => (ALIGN_SYSTEM, align_pack(store, item, budget)),
        "reconcile-doc" => (RECONCILE_SYSTEM, reconcile_pack(store, item, budget)),
        "review-requirement" => (
            REVIEW_REQ_SYSTEM,
            review_requirement_pack(store, &item.target),
        ),
        "generate-entity" => (GENERATE_SYSTEM, generate_pack(store, &item.target, gen)),
        "bind-requirement" => (BIND_SYSTEM, bind_pack(store, &item.target, gen)),
        _ => (
            REVIEW_SYSTEM,
            review_pack(store, &item.target, budget, lint),
        ),
    }
}

// One line naming a requirement's provenance, whichever kind it carries.
pub(crate) fn provenance_line(r: &crate::model::Requirement) -> String {
    match r.provenance() {
        Some(crate::model::ProvenanceRef::Quote(s)) => {
            format!("{}#{} \"{}\"", s.doc, s.section, s.quote)
        }
        Some(crate::model::ProvenanceRef::Derived { from, reasoning }) => {
            format!("derived from {} ({})", from.join(", "), reasoning)
        }
        Some(crate::model::ProvenanceRef::Decree { author, at, note }) => {
            format!(
                "decreed by {} at {}{}",
                author,
                at,
                note.map(|n| format!(" ({})", n)).unwrap_or_default()
            )
        }
        None => "(no provenance)".to_string(),
    }
}

// The generation turn's pack: the task package rendered for a model. The same fields
// begin_generation serves over MCP. Mirrors docs/compiler/turns.md#generation-turns.
fn generate_pack(store: &Store, target: &str, gs: &crate::gen::GenSettings) -> String {
    let pkg = match crate::gen::task_package(store, target, gs) {
        Ok(p) => p,
        Err(e) => return format!("# Work item: generate {}\n(package error: {})\n", target, e),
    };
    let mut s = format!(
        "# Work item: generate entity {} ({})\n",
        target,
        pkg["name"].as_str().unwrap_or("")
    );
    s.push_str(&format!(
        "deliverable directory: {}\n",
        pkg["deliverable"].as_str().unwrap_or(".")
    ));
    s.push_str(&format!(
        "factHash (pass to record_generation): {}\n",
        pkg["factHash"].as_str().unwrap_or("")
    ));
    if !pkg["medium"].is_null() {
        s.push_str(&format!(
            "medium (already decided; never re-decide): {}\n",
            pkg["medium"]
        ));
    }
    if !pkg["build"].is_null() {
        s.push_str(&format!(
            "recorded build (reuse and extend; never record a second): {}\n",
            pkg["build"]
        ));
    }
    let run_commands = pkg["runCommands"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    if !run_commands.is_empty() {
        s.push_str(&format!(
            "recorded run commands (the established toolchain; reuse it): {}\n",
            run_commands
        ));
    }
    s.push_str(&format!(
        "changed since last generation: {}\n",
        pkg["changed"]
            .as_array()
            .map(|a| a
                .iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", "))
            .unwrap_or_default()
    ));
    s.push_str(&format!(
        "other entities' files (never write to them; `holds` says what is inside): {}\n",
        pkg["generatedFiles"]
    ));
    if pkg["boundTests"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false)
    {
        s.push_str(&format!(
            "bound tests (already written by binding; make the unimplemented ones pass, never rewrite them): {}\n",
            pkg["boundTests"]
        ));
    }
    s.push_str("\n## Requirements (one test row each; testName is the required test name)\n");
    for group in pkg["requirementGroups"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[])
    {
        for r in group.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            s.push_str(&format!(
                "- {} [{}]: {}\n  quote: {}\n",
                r["id"].as_str().unwrap_or(""),
                r["testName"].as_str().unwrap_or(""),
                r["statement"].as_str().unwrap_or(""),
                r["quote"].as_str().unwrap_or("")
            ));
        }
    }
    s.push_str("\n## Context\n");
    s.push_str(pkg["context"].as_str().unwrap_or(""));
    s.push_str("\n## Contract\n");
    s.push_str(pkg["instructions"].as_str().unwrap_or(""));
    s.push('\n');
    s
}

// The bind turn's pack: the task package rendered for a model. The same fields
// begin_binding serves over MCP. Mirrors docs/consumers/bind.md#the-bind-task.
fn bind_pack(store: &Store, target: &str, gs: &crate::gen::GenSettings) -> String {
    let pkg = match crate::bind::task(store, target, gs) {
        Ok(p) => p,
        Err(e) => return format!("# Work item: bind {}\n(package error: {})\n", target, e),
    };
    let mut s = format!(
        "# Work item: bind requirement {} ({})\n",
        target,
        pkg["reason"].as_str().unwrap_or("")
    );
    s.push_str(&format!(
        "deliverable directory: {}\n",
        pkg["deliverable"].as_str().unwrap_or(".")
    ));
    s.push_str(&format!(
        "statement: {}\n",
        pkg["statement"].as_str().unwrap_or("")
    ));
    s.push_str(&format!("quote: {}\n", pkg["quote"].as_str().unwrap_or("")));
    s.push_str(&format!(
        "suggested test name: {}\n",
        pkg["suggestedTestName"].as_str().unwrap_or("")
    ));
    if !pkg["medium"].is_null() {
        s.push_str(&format!(
            "medium (already decided; never re-decide): {}\n",
            pkg["medium"].as_str().unwrap_or("")
        ));
    }
    if !pkg["build"].is_null() {
        s.push_str(&format!("recorded build: {}\n", pkg["build"]));
    }
    if pkg["testConventions"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false)
    {
        s.push_str(&format!(
            "recorded test conventions (reuse them): {}\n",
            pkg["testConventions"]
        ));
    }
    if pkg["entityFiles"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false)
    {
        s.push_str(&format!(
            "the entity's recorded files (start the search here): {}\n",
            pkg["entityFiles"]
        ));
    }
    s.push_str("\n## Context\n");
    s.push_str(pkg["context"].as_str().unwrap_or(""));
    s.push_str("\n## Contract\n");
    s.push_str(pkg["instructions"].as_str().unwrap_or(""));
    s.push('\n');
    s
}

fn reconcile_pack(store: &Store, item: &WorkItem, budget: usize) -> String {
    let mut s = String::new();
    let doc = &item.target;
    s.push_str(&format!("# Work item: reconcile document {}\n", doc));
    if let Some(rec) = store.docs.get(doc) {
        let covered = rec.coverage.len();
        s.push_str(&format!(
            "sections: {} total, {} with coverage\n",
            rec.sections.len(),
            covered
        ));
    }

    // Incoming links the graph already resolved: a parent listed this document as one of
    // its parts, and that list item minted the part's entity. The link is what says which
    // entity this document details. Mirrors docs/compiler/turns.md#incoming-links.
    let mut incoming: Vec<String> = Vec::new();
    let mut subjects: Vec<(String, String)> = Vec::new();
    for (id, e) in &store.graph.entities {
        for m in &e.mentions {
            if &m.doc != doc
                && crate::md::doc_links(&m.quote, &m.doc)
                    .iter()
                    .any(|l| l == doc)
            {
                incoming.push(format!(
                    "- {}#{} \"{}\" introduced {} ({})",
                    m.doc,
                    m.section,
                    crate::llm::truncate(&m.quote, 160),
                    id,
                    e.name
                ));
                subjects.push((id.clone(), e.name.clone()));
            }
        }
    }
    for (id, r) in &store.graph.requirements {
        let Some(src) = r.source.as_ref() else {
            continue;
        };
        if &src.doc != doc
            && crate::md::doc_links(&src.quote, &src.doc)
                .iter()
                .any(|l| l == doc)
        {
            incoming.push(format!(
                "- {}#{} \"{}\" states {} ({})",
                src.doc,
                src.section,
                crate::llm::truncate(&src.quote, 100),
                id,
                crate::llm::truncate(&r.statement, 100)
            ));
        }
    }
    incoming.sort();
    incoming.dedup();
    if !incoming.is_empty() {
        incoming.truncate(12);
        s.push_str("\n## Linked from (what other documents already say this one details)\n");
        s.push_str(&incoming.join("\n"));
        subjects.sort();
        subjects.dedup();
        // The subject question is always answered here, so the turn never guesses
        // what "the system" means. Mirrors docs/compiler/turns.md#incoming-links.
        if subjects.len() == 1 {
            let (id, name) = &subjects[0];
            s.push_str(&format!(
                "\n\nprimarySubject: {} ({}). This document details that entity: read \"the system\", \"this\", and \"it\" as it, reference it from every requirement extracted here, and never mint a second entity for the same concept.\n",
                id, name
            ));
        } else if !subjects.is_empty() {
            let list = subjects
                .iter()
                .map(|(id, name)| format!("{} ({})", id, name))
                .collect::<Vec<_>>()
                .join(", ");
            s.push_str(&format!(
                "\n\ncandidateSubjects: {}. This document details these entities; each statement's own section decides which one it constrains. \"The system\" here means the part being detailed, never the containing application. Do not mint a second entity for any of these concepts.\n",
                list
            ));
        }
    }

    // Known entities: this document's neighborhood first, then the rest of the graph.
    let mut lines: Vec<String> = Vec::new();
    let mut listed: Vec<&String> = Vec::new();
    for (id, e) in &store.graph.entities {
        if e.mentions.iter().any(|m| &m.doc == doc) {
            lines.push(format!(
                "- {} ({}): {}",
                id,
                e.name,
                crate::llm::truncate(e.definition.as_deref().unwrap_or(""), 160)
            ));
            listed.push(id);
        }
    }
    for (id, e) in &store.graph.entities {
        if lines.len() >= 40 {
            lines.push(format!(
                "- (and {} more; use search)",
                store.graph.entities.len() - lines.len() + 1
            ));
            break;
        }
        if !listed.contains(&id) {
            lines.push(format!(
                "- {} ({}): {}",
                id,
                e.name,
                crate::llm::truncate(e.definition.as_deref().unwrap_or(""), 160)
            ));
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
                // An anchor the align turn relocated and flagged still locates; the
                // note says why it is listed all the same.
                let flagged = if store.status.reevaluate.contains(a) {
                    "; relocated by alignment, re-evaluate whether it still holds here"
                } else {
                    ""
                };
                let Some(src) = r.source.as_ref() else {
                    continue;
                };
                s.push_str(&format!(
                    "- {}: {} (in {}#{}; was quoted: \"{}\"{})\n",
                    a,
                    r.statement,
                    src.doc,
                    src.section,
                    crate::llm::truncate(&src.quote, 100),
                    flagged
                ));
            } else if let Some(e) = store.graph.entities.get(a) {
                s.push_str(&format!(
                    "- {} (entity {}): a mention's section changed\n",
                    a, e.name
                ));
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
                s.push_str(&format!(
                    "\n### {}#{} ({}) [coverage: {}]\n",
                    doc, r, sec.title, cov
                ));
                if sec.raw.len() <= per_section {
                    s.push_str(&sec.raw);
                } else {
                    s.push_str(&crate::llm::truncate(&sec.raw, per_section));
                    s.push_str(&format!(
                        "\n(truncated; read_section {}#{} for the rest)",
                        doc, r
                    ));
                }
                s.push('\n');
                // What the section already yielded: an unchanged statement is a no-op,
                // and a coverage claim must see the requirements anchored here before
                // judging the section non-normative.
                let existing: Vec<String> = store
                    .graph
                    .requirements
                    .iter()
                    .filter(|(_, q)| q.anchored_at(doc, r))
                    .map(|(id, q)| format!("- {}: {}", id, q.statement))
                    .collect();
                if !existing.is_empty() {
                    s.push_str(
                        "Already extracted from this section (leave unchanged statements alone):\n",
                    );
                    s.push_str(&existing.join("\n"));
                    s.push('\n');
                }
            }
        }
    }
    s
}

// The align pack: the computed section changes, then every pending proposal with its
// old location and wording and its candidates. Mirrors docs/compiler/turns/align-doc.md.
fn align_pack(store: &Store, item: &WorkItem, budget: usize) -> String {
    let doc = &item.target;
    let mut s = String::new();
    s.push_str(&format!("# Work item: align anchors in {}\n", doc));
    let Some(block) = store.status.alignment.iter().find(|b| &b.doc == doc) else {
        s.push_str("\n(no pending proposals; call done)\n");
        return s;
    };
    if !block.changes.is_empty() {
        s.push_str("\n## Section changes (computed)\n");
        for c in &block.changes {
            let sim = c
                .similarity
                .map(|v| format!(" (similarity {}%)", (v * 100.0).round() as u32))
                .unwrap_or_default();
            let line = match c.op.as_str() {
                "added" => format!("- added: {}", c.to.join(", ")),
                "deleted" => format!("- deleted: {}", c.from.join(", ")),
                "edited" => format!("- edited: {}{}", c.to.join(", "), sim),
                op => format!(
                    "- {}: {} → {}{}",
                    op,
                    c.from.join(", "),
                    c.to.join(", "),
                    sim
                ),
            };
            s.push_str(&line);
            s.push('\n');
        }
    }
    let proposals: Vec<&AnchorProposal> = block
        .proposals
        .iter()
        .filter(|p| item.proposals.is_empty() || item.proposals.contains(&p.anchor))
        .collect();
    s.push_str("\n## Proposals (decide every one)\n");
    let per_proposal = budget.saturating_sub(s.len()) / proposals.len().max(1);
    for p in proposals {
        let mut block = String::new();
        let head = match (
            store.graph.requirements.get(&p.anchor),
            store.graph.entities.get(&p.anchor),
        ) {
            (Some(r), _) => format!("\n### {}: {}\n", p.anchor, r.statement),
            (_, Some(e)) => format!("\n### {} (entity {}), mention\n", p.anchor, e.name),
            _ => format!("\n### {}\n", p.anchor),
        };
        block.push_str(&head);
        block.push_str(&format!("was: {} \"{}\"\n", p.from, p.quote));
        if !p.excerpt.is_empty() {
            block.push_str(&indent(&p.excerpt));
        }
        block.push_str("candidates:\n");
        let per_candidate = per_proposal.saturating_sub(block.len()) / p.candidates.len().max(1);
        for (i, c) in p.candidates.iter().enumerate() {
            let title = split_section_ref(&c.section)
                .and_then(|(d, r)| {
                    store
                        .docs
                        .get(&d)
                        .and_then(|rec| rec.sections.get(&r))
                        .map(|sec| sec.title.clone())
                })
                .unwrap_or_default();
            let locates = if c.quote_locates {
                "quote locates: yes".to_string()
            } else {
                format!(
                    "quote locates: no, nearest: \"{}\"",
                    crate::llm::truncate(c.nearest.as_deref().unwrap_or(""), 200)
                )
            };
            block.push_str(&format!(
                "  {}. {} ({}) similarity {}%, {}\n",
                i + 1,
                c.section,
                title,
                (c.similarity * 100.0).round() as u32,
                locates
            ));
            if c.excerpt.len() <= per_candidate {
                block.push_str(&indent(&indent(&c.excerpt)));
            } else {
                block.push_str(&indent(&indent(&crate::llm::truncate(
                    &c.excerpt,
                    per_candidate,
                ))));
                block.push_str(&format!(
                    "     (truncated; read_section {} for the rest)\n",
                    c.section
                ));
            }
        }
        s.push_str(&block);
    }
    s
}

fn indent(text: &str) -> String {
    let mut out = String::new();
    for l in text.lines() {
        out.push_str("  ");
        out.push_str(l);
        out.push('\n');
    }
    out
}

// The pair-review pack: the changed requirement and its neighbors side by side. The
// neighbor set is recomputed here with the same deterministic function the reconciler
// used to schedule the turn (docs/compiler/compilation.md#waves).
fn review_requirement_pack(store: &Store, rid: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "# Work item: review changed requirement {} against its neighbors\n",
        rid
    ));
    let fmt = |id: &str, r: &crate::model::Requirement| match r.source.as_ref() {
        Some(src) => format!(
            "- {}\n  statement: {}\n  quote: \"{}\"\n  section: {}#{}\n",
            id, r.statement, src.quote, src.doc, src.section
        ),
        None => format!(
            "- {}\n  statement: {}\n  provenance: {}\n",
            id,
            r.statement,
            provenance_line(r)
        ),
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
    let open = open_diagnostics_lines(store, &[rid.to_string()]);
    if !open.is_empty() {
        s.push_str(
            "\n## Open diagnostics naming this requirement (resolve any that no longer hold)\n",
        );
        s.push_str(&open.join("\n"));
        s.push('\n');
    }
    s
}

// Open diagnostics naming any of the given ids, one line each, with subjects that no
// longer exist in the graph marked (deleted): such a diagnostic cannot stand as filed,
// and the turn resolves or refiles it. Mirrors docs/compiler/turns.md#task-types.
fn open_diagnostics_lines(store: &Store, ids: &[String]) -> Vec<String> {
    store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| d.lifecycle == "open" && d.subjects.iter().any(|x| ids.contains(x)))
        .map(|(id, d)| {
            let subjects: Vec<String> = d
                .subjects
                .iter()
                .map(|x| {
                    let resolved = store.resolve_id(x);
                    let is_node = x.starts_with("req:") || x.starts_with("ent:");
                    if !is_node
                        || store.graph.requirements.contains_key(resolved)
                        || store.graph.entities.contains_key(resolved)
                    {
                        x.clone()
                    } else {
                        format!("{} (deleted)", x)
                    }
                })
                .collect();
            format!(
                "- {} ({}, {}) subjects: {}: {}",
                id,
                d.rule,
                d.severity,
                subjects.join(", "),
                d.message
            )
        })
        .collect()
}

fn review_pack(store: &Store, entity_id: &str, budget: usize, lint: &Linting) -> String {
    let mut s = String::new();
    s.push_str(&format!("# Work item: review entity {}\n\n", entity_id));
    match crate::context::assemble(
        store,
        entity_id,
        &crate::context::Focus {
            parents: 1,
            mentions: 1,
            requirements: 2,
        },
        budget.saturating_sub(1200),
    ) {
        Ok(pack) => s.push_str(&pack.pack),
        Err(e) => s.push_str(&format!("(context error: {})\n", e)),
    }
    // Lookalike candidates: token-overlap hits on the entity's name, excluding itself.
    // Partitioned so a child concept never reads as a merge suggestion: a candidate
    // whose name extends this one (or vice versa) with extra words is usually a
    // field, part, or role, not an alias.
    if let Some(e) = store.graph.entities.get(entity_id) {
        let tokens = |n: &str| -> std::collections::BTreeSet<String> {
            n.to_lowercase()
                .split_whitespace()
                .map(|t| t.to_string())
                .collect()
        };
        let mine = tokens(&e.name);
        let hits = store.search(&e.name);
        let (mut aliases, mut related): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        for (id, name, def) in hits.iter().filter(|(id, _, _)| id != entity_id) {
            let theirs = tokens(name);
            let line = format!("- {} ({}): {}", id, name, crate::llm::truncate(def, 160));
            let extension =
                (theirs.is_superset(&mine) || mine.is_superset(&theirs)) && theirs != mine;
            if extension {
                related.push(line);
            } else {
                aliases.push(line);
            }
        }
        if !aliases.is_empty() {
            s.push_str("\n## Name-similar candidates (a shared word proves nothing; merge only when they are one concept)\n");
            s.push_str(&aliases.join("\n"));
            s.push('\n');
        }
        if !related.is_empty() {
            s.push_str("\n## Related but separate candidates (a field, part, or child concept; merge only with explicit evidence they are one concept)\n");
            s.push_str(&related.join("\n"));
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
        // Matching runs on prose only: a name inside a code span (`orderly report`)
        // is a command token, not a domain reference.
        let strip_code = |s: &str| -> String {
            let mut out = String::with_capacity(s.len());
            let mut in_code = false;
            for ch in s.chars() {
                if ch == '`' {
                    in_code = !in_code;
                    out.push(' ');
                } else {
                    out.push(if in_code { ' ' } else { ch });
                }
            }
            out
        };
        let names: Vec<&String> = std::iter::once(&e.name).chain(e.aliases.iter()).collect();
        let unreferenced: Vec<String> = store
            .graph
            .requirements
            .iter()
            .filter(|(_, r)| !r.entities.iter().any(|x| store.resolve_id(x) == entity_id))
            .filter(|(_, r)| {
                let prose = strip_code(&r.statement);
                if !names.iter().any(|n| contains_word(&prose, n)) {
                    return false;
                }
                // A match that is only part of a referenced compound name
                // ("Catalog" inside "Catalog category") is that other entity's
                // reference, not a missing one here.
                !r.entities
                    .iter()
                    .filter_map(|x| store.graph.entities.get(store.resolve_id(x)))
                    .any(|other| {
                        std::iter::once(&other.name)
                            .chain(other.aliases.iter())
                            .any(|on| {
                                names.iter().any(|n| {
                                    !on.eq_ignore_ascii_case(n)
                                        && contains_word(on, n)
                                        && contains_word(&prose, on)
                                })
                            })
                    })
            })
            .take(6)
            .map(|(rid, r)| format!("- {}: {}", rid, r.statement))
            .collect();
        if !unreferenced.is_empty() {
            s.push_str("\n## Statements naming this entity without referencing it (add the reference if the statement is about it)\n");
            s.push_str(&unreferenced.join("\n"));
            s.push_str("\nThese candidates are word matches, not judgments: a name appearing in the text does not make the statement about this entity, and leaving a candidate alone is a correct outcome.\n");
        }
    }
    // Open diagnostics naming this entity's requirements: the entity review is the
    // net for findings the pairwise wave cannot see, so it must see what is filed.
    // (Diagnostics naming the entity itself already ride in the context pack.)
    {
        let ids: Vec<String> = store.requirements_referencing(entity_id);
        let open = open_diagnostics_lines(store, &ids);
        if !open.is_empty() {
            s.push_str("\n## Open diagnostics on this entity's statements (resolve any that no longer hold)\n");
            s.push_str(&open.join("\n"));
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

pub fn condense(v: &Value, n: usize) -> String {
    llm::truncate(&v.to_string(), n)
}

// The payload behind a condensed line, only when condensing cut something, capped so
// a huge context pack cannot flood the trace file.
pub fn full_payload(v: &Value) -> Option<String> {
    let s = v.to_string();
    if s.len() <= 160 {
        None
    } else {
        Some(llm::truncate(&s, 8_000).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_note_rides_under_the_first_paragraph_of_every_contract() {
        for system in [
            ALIGN_SYSTEM,
            RECONCILE_SYSTEM,
            REVIEW_REQ_SYSTEM,
            REVIEW_SYSTEM,
            GENERATE_SYSTEM,
            BIND_SYSTEM,
        ] {
            assert!(
                system.starts_with("This goal"),
                "a contract paragraph names its goal first: {}",
                system
            );
            let s = with_feedback_note(system);
            let paras: Vec<&str> = s.split("\n\n").collect();
            assert!(
                paras[0].starts_with("This goal"),
                "the goal paragraph stays first"
            );
            assert_eq!(paras[1], FEEDBACK_NOTE, "the note is the second paragraph");
            assert!(s.contains("report_feedback"));
            // Nothing of the original prompt is lost to the insertion.
            let rest = system
                .split_once("\n\n")
                .map(|(_, r)| r)
                .unwrap_or(FEEDBACK_NOTE);
            assert!(s.ends_with(rest));
        }
    }
}
