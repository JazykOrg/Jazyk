// The session harness: trace events, the transcript, the skill payloads, and the
// assembled session prompt for one goal batch. One agent, many goal kinds; variety
// lives in the kinds, which are data. Mirrors docs/compiler/sessions.md.
use crate::context::LoadedSet;
use crate::goals;
use crate::llm;
use crate::model::Goal;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::BTreeSet;

// ---- trace ----

#[derive(Clone, Copy, PartialEq)]
pub enum TraceLevel {
    Quiet,
    Normal,
    Verbose,
}

// One structured event per emission. The CLI renders these to stderr; the GUI streams
// them to the browser. Mirrors docs/compiler/sessions.md#trace-events.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind")]
pub enum TraceEvent {
    // A session starts on a batch: the goals it carries, the task and target the
    // serving claims, the document when it has one, and the sections it must process.
    // The GUI lights those up in place.
    #[serde(rename = "sessionStart")]
    #[serde(rename_all = "camelCase")]
    SessionStart {
        label: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        goals: Vec<String>,
        task: String,
        target: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        doc: Option<String>,
        sections: Vec<String>,
        dirty: usize,
        stale: usize,
        // Proposals a place-anchors session must decide; zero for every other kind.
        #[serde(default, skip_serializing_if = "is_zero")]
        proposals: usize,
    },
    // The scheduler formed a batch: its class and tier, the goals with their kinds
    // and targets, the resolved executor.
    #[serde(rename = "batchStart")]
    BatchStart {
        label: String,
        class: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tier: Option<u8>,
        goals: Vec<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        executor: Option<String>,
    },
    // A goal changed state: opened (with its cause), resolved (with its
    // justification), failed or parked (with the reason).
    #[serde(rename = "goal")]
    Goal {
        label: String,
        goal: String,
        event: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cause: Option<crate::model::Cause>,
        #[serde(skip_serializing_if = "Option::is_none")]
        justification: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    // A GC burst starts on a settled cone: the kind, the target, and the count
    // against the limit that opened it.
    #[serde(rename = "gcBurst")]
    GcBurst {
        label: String,
        #[serde(rename = "goalKind")]
        goal_kind: String,
        target: String,
        count: u64,
        limit: u64,
        detail: String,
    },
    // The board summary a build prints first: the goal count, the count per kind,
    // and the blocked count.
    #[serde(rename = "board")]
    Board {
        label: String,
        goals: usize,
        kinds: Vec<(String, usize)>,
        blocked: usize,
    },
    #[serde(rename = "graphPreview")]
    GraphPreview {
        #[serde(flatten)]
        preview: crate::live_preview::Preview,
    },
    // `summary` is the condensed line the CLI prints; `full` carries the payload
    // behind it when condensing cut something, so the GUI expands in place.
    #[serde(rename = "toolCall")]
    ToolCall {
        label: String,
        activity: String,
        name: String,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        full: Option<String>,
    },
    #[serde(rename = "toolResult")]
    ToolResult {
        label: String,
        activity: String,
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
    // `reply` marks the model's reply; a reasoning chunk omits it
    // (docs/compiler/sessions.md#trace-events).
    #[serde(rename = "modelText")]
    ModelText {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        summary: bool,
        label: String,
        text: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reply: bool,
    },
    // mode: "done" (explicit, with the model's summary), "implicit" (the model went
    // silent with staged work), or "budget" (implicit at the round budget).
    #[serde(rename = "sessionDone")]
    SessionDone {
        label: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        goals: Vec<String>,
        staged: usize,
        rounds: u32,
        mode: String,
        summary: String,
    },
    #[serde(rename = "sessionFailed")]
    SessionFailed {
        label: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        goals: Vec<String>,
        attempt: u32,
        error: String,
    },
    // The attempt ended with nothing landed and no failure of its own: the build was
    // cancelled, a ledger batch returned for its ledger to judge, or the runner left
    // without a verdict. One of the three terminal kinds closes every attempt
    // (docs/compiler/sessions.md#trace-events).
    #[serde(rename = "sessionEnded")]
    SessionEnded {
        label: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        goals: Vec<String>,
        reason: String,
    },
    #[serde(rename = "note")]
    Note {
        label: String,
        text: String,
        verbose: bool,
    },
    // The session moved to a section: an accepted tool call named one, and it differs
    // from the last. The sequence is the session's path through the document.
    #[serde(rename = "section")]
    Section {
        label: String,
        doc: String,
        section: String,
        tool: String,
    },
    // One model call. The request carries the outgoing messages after the `prefix`
    // it repeats from the run's previous request, the response the raw assistant
    // message; both are recorded in full in the transcript and elided on the wire
    // (docs/compiler/sessions.md#trace-events).
    #[serde(rename = "llmRequest")]
    LlmRequest {
        label: String,
        step: String,
        model: String,
        prefix: usize,
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

fn is_zero(n: &usize) -> bool {
    *n == 0
}

// Render an event exactly as the trace has always printed it, so `jazyk compile`
// output is unchanged.
fn render_stderr(ev: &TraceEvent) {
    match ev {
        TraceEvent::Board {
            goals,
            kinds,
            blocked,
            ..
        } => {
            let per_kind: Vec<String> = kinds.iter().map(|(k, n)| format!("{} {}", n, k)).collect();
            let mut s = format!("compile: {} goals", goals);
            if !per_kind.is_empty() {
                s.push_str(&format!(" ({})", per_kind.join(", ")));
            }
            if *blocked > 0 {
                s.push_str(&format!(", {} blocked", blocked));
            }
            eprintln!("{}", s)
        }
        TraceEvent::GcBurst {
            goal_kind,
            target,
            count,
            limit,
            detail,
            ..
        } => {
            // A limit goal prints its crossing; a judgment goal has no threshold to
            // cross, so its line carries the evidence count instead.
            if *limit > 0 {
                eprintln!("gc burst: {} {} ({} > {})", goal_kind, target, count, limit)
            } else {
                eprintln!("gc burst: {} {} ({} {})", goal_kind, target, count, detail)
            }
        }
        TraceEvent::BatchStart {
            label,
            class,
            tier,
            goals,
            ..
        } => eprintln!(
            "[{}] batch: {} {}({} goal(s))",
            label,
            class,
            tier.map(|t| format!("tier {} ", t)).unwrap_or_default(),
            goals.len()
        ),
        TraceEvent::Goal {
            goal,
            event,
            cause,
            justification,
            reason,
            ..
        } => {
            let tail = match (cause, justification, reason) {
                (Some(c), _, _) => format!("  (g{} via {})", c.generation, c.via),
                (_, Some(j), _) => format!("  {}", j),
                (_, _, Some(r)) => format!("  {}", r),
                _ => String::new(),
            };
            eprintln!("{:<8} {}{}", event, goal, tail)
        }
        TraceEvent::SessionStart {
            label, proposals, ..
        } if *proposals > 0 => {
            eprintln!("[{}] session start ({} proposal(s))", label, proposals)
        }
        TraceEvent::SessionStart {
            label,
            dirty,
            stale,
            ..
        } => {
            eprintln!(
                "[{}] session start ({} dirty, {} stale)",
                label, dirty, stale
            )
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
        TraceEvent::ModelText { label, text, .. } => {
            eprintln!("[{}] · {}", label, llm::truncate(text, 200))
        }
        TraceEvent::SessionDone {
            label,
            staged,
            rounds,
            mode,
            summary,
            ..
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
        TraceEvent::SessionFailed {
            label,
            attempt,
            error,
            ..
        } => {
            eprintln!(
                "[{}] session failed (attempt {}): {}",
                label, attempt, error
            )
        }
        TraceEvent::SessionEnded { label, reason, .. } => {
            eprintln!("[{}] session ended: {}", label, reason)
        }
        TraceEvent::Note { label, text, .. } => eprintln!("[{}] {}", label, text),
        // The section path is implicit in the tool rows the default level already
        // prints; naming it again would double every line.
        TraceEvent::Section { .. } | TraceEvent::GraphPreview { .. } => {}
        // Model calls print their arithmetic, never their payload: the verbose context
        // pack note already carries the prompt.
        TraceEvent::LlmRequest {
            label,
            step,
            prefix,
            messages,
            ..
        } => {
            eprintln!(
                "[{} {}] → llm ({} messages, {} new chars)",
                label,
                step,
                prefix + messages.as_array().map(|a| a.len()).unwrap_or(0),
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

// A receiver of the structured trace events.
pub type TraceSink = std::sync::Arc<dyn Fn(&TraceEvent) + Send + Sync>;

#[derive(Clone)]
pub struct Trace {
    pub level: TraceLevel,
    // None renders to stderr. A sink receives the structured events instead.
    sink: Option<TraceSink>,
    // Cancellation reaches model waits, session runners, and pipeline boundaries.
    // It rides in the trace because every runner already carries the trace.
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    transcript: Option<std::sync::Arc<std::sync::Mutex<Transcript>>>,
    // The run's transcript name under <out>/trace, when the run leaves one. Carried so
    // a record made mid-run (a feedback entry) can name the run it came from.
    run: Option<String>,
    // Per session label, whether its latest attempt is still open: `sessionStart`
    // opens it, the first terminal event closes it, and a second terminal event for a
    // closed attempt is dropped. Shared across clones so every runner sees one
    // ledger of attempts. Mirrors docs/compiler/sessions.md#trace-events.
    attempts: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, bool>>>,
    // Per model run, the digests of the messages its latest request carried, so the
    // next request records only what changed. Shared across clones.
    requests: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, Vec<String>>>>,
}

impl Trace {
    pub fn stderr(level: TraceLevel) -> Trace {
        Trace {
            level,
            sink: None,
            cancel: Default::default(),
            transcript: None,
            run: None,
            attempts: Default::default(),
            requests: Default::default(),
        }
    }
    pub fn to_sink(
        level: TraceLevel,
        sink: TraceSink,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Trace {
        Trace {
            level,
            sink: Some(sink),
            cancel,
            transcript: None,
            run: None,
            attempts: Default::default(),
            requests: Default::default(),
        }
    }
    // The attempt ledger behind the terminal-event invariant: an attempt opens on
    // `sessionStart` and closes on its first terminal event; a terminal event for an
    // attempt already closed is a duplicate and is dropped. A label this trace never
    // saw start passes (a serving's own trace reports its finish without a start).
    // Returns false when the event must be dropped.
    // Mirrors docs/compiler/sessions.md#trace-events.
    fn admit(&self, ev: &TraceEvent) -> bool {
        let (label, terminal) = match ev {
            TraceEvent::SessionStart { label, .. } => (label, false),
            TraceEvent::SessionDone { label, .. }
            | TraceEvent::SessionFailed { label, .. }
            | TraceEvent::SessionEnded { label, .. } => (label, true),
            _ => return true,
        };
        let mut attempts = self.attempts.lock().unwrap();
        if !terminal {
            attempts.insert(label.clone(), true);
            return true;
        }
        match attempts.get_mut(label) {
            Some(open) if !*open => false,
            Some(open) => {
                *open = false;
                true
            }
            None => true,
        }
    }
    // How many leading messages repeat the run's previous request; remembers this
    // request for the next. Mirrors docs/compiler/sessions.md#trace-events.
    pub fn request_prefix(&self, run: &str, messages: &[Value]) -> usize {
        let digests: Vec<String> = messages
            .iter()
            .map(|m| crate::model::hash_hex(&m.to_string()))
            .collect();
        let mut requests = self.requests.lock().unwrap_or_else(|p| p.into_inner());
        let previous = requests.get(run).map(Vec::as_slice).unwrap_or_default();
        let prefix = previous
            .iter()
            .zip(&digests)
            .take_while(|(a, b)| a == b)
            .count();
        requests.insert(run.to_string(), digests);
        prefix
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
    pub fn with_transcript(self, out: &std::path::Path, kind: &str) -> Trace {
        self.with_transcript_goals(out, kind, &[])
    }
    // The batch-scoped form: the meta line names the batch's goal ids beside the
    // store version. Mirrors docs/compiler/sessions.md#trace-events.
    pub fn with_transcript_goals(
        mut self,
        out: &std::path::Path,
        kind: &str,
        goals: &[String],
    ) -> Trace {
        if crate::vfs::is_scoped() {
            return self;
        }
        use std::io::Write;
        let dir = out.join("trace");
        crate::vfs::create_dir_all(&dir).ok();
        let started = crate::verify::now_iso();
        let compact: String = started
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        let stem = format!("{}-{}-cli{}", compact, kind, crate::sys::process_id());
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("{}.jsonl", stem)))
        {
            // The generation at start and finish brackets the run: the journal
            // entries between the two are the run's changesets (gui.md#jobs).
            let mut meta = json!({"meta": {"id": null, "kind": {"kind": kind}, "startedAt": started, "source": "cli",
                "generation": crate::store::read_generation(out),
                "storeVersion": crate::store::STORE_VERSION}});
            if !goals.is_empty() {
                meta["meta"]["goals"] = json!(goals);
            }
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
        if !self.admit(&ev) {
            return;
        }
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
        // Quiet keeps the board summary and the burst lines: the build's outline,
        // never its rows (docs/frontends/cli.md#jazyk-compile).
        let keep = match self.level {
            TraceLevel::Quiet => {
                matches!(&ev, TraceEvent::Board { .. } | TraceEvent::GcBurst { .. })
            }
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
    // The flag that cancels this trace's run, for a caller that stops it from
    // another thread.
    pub fn cancel_flag(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.cancel.clone()
    }
    pub fn is_cancelled(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        if crate::wasm::llm_bridge::cancel_requested() {
            return true;
        }
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// ---- payloads ----

// The fixed session contract; every kind-specific instruction arrives as data inside
// the assembled prompt. Mirrors docs/compiler/sessions.md#the-prompt.
const AGENT_CONTRACT: &str = include_str!("../../docs/compiler/goals/prompts/agent-contract.md");

// The feedback contract, filled into the contract's `{feedback}` slot.
const FEEDBACK_NOTE: &str = include_str!("../../docs/compiler/goals/prompts/feedback-note.md");

// The per-kind gate lines and the prompt's block framing.
// Mirrors docs/compiler/sessions.md#payload-files.
pub(crate) const GATES: crate::payload::Fragments =
    crate::payload::Fragments(include_str!("../../docs/compiler/goals/prompts/gates.md"));
pub(crate) const BLOCKS: crate::payload::Fragments = crate::payload::Fragments(include_str!(
    "../../docs/compiler/goals/prompts/session-blocks.md"
));

// The protocol line; {target} is the batch id.
const WORKER_PROTOCOL: &str = include_str!("../../docs/compiler/goals/prompts/worker-protocol.md");
// A session run in process with its batch already open: no claim, no begin_goals.
// Mirrors docs/compiler/sessions.md#the-prompt.
const OPEN_BATCH_PROTOCOL: &str =
    include_str!("../../docs/compiler/goals/prompts/open-batch-protocol.md");
// The protocol line for a prompt returned by begin_goals itself: the batch is already
// claimed when the agent reads it. Mirrors docs/compiler/sessions.md#the-prompt.
const SERVING_PROTOCOL: &str =
    include_str!("../../docs/compiler/goals/prompts/serving-protocol.md");

// Which side of the claim the prompt is read from: ahead of it (an ACP session prompt,
// a preview) or after it (the instructions a standalone MCP serving returns).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protocol {
    Worker,
    Serving,
    OpenBatch,
}

// The agent contract with the feedback note in its `{feedback}` slot: the first
// paragraph says what the session is, the second how to report that the rest of the
// prompt failed it. Mirrors docs/compiler/sessions.md#the-prompt.
pub(crate) fn agent_contract() -> String {
    crate::payload::fill(AGENT_CONTRACT, &[("feedback", FEEDBACK_NOTE.trim_end())])
}

// The skill payloads, embedded at compile time. Mirrors docs/compiler/sessions.md#skills.
pub const SKILLS: [(&str, &str); 8] = [
    (
        "extraction",
        include_str!("../../docs/compiler/skills/extraction.md"),
    ),
    ("edges", include_str!("../../docs/compiler/skills/edges.md")),
    (
        "judgment",
        include_str!("../../docs/compiler/skills/judgment.md"),
    ),
    (
        "criteria",
        include_str!("../../docs/compiler/skills/criteria.md"),
    ),
    (
        "flow-views",
        include_str!("../../docs/compiler/skills/flow-views.md"),
    ),
    (
        "structural-views",
        include_str!("../../docs/compiler/skills/structural-views.md"),
    ),
    (
        "abstraction",
        include_str!("../../docs/compiler/skills/abstraction.md"),
    ),
    (
        "conformance",
        include_str!("../../docs/compiler/skills/conformance.md"),
    ),
];

pub fn skill_payload(name: &str) -> Option<&'static str> {
    SKILLS.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
}

// The skill a loaded node auto-activates: a section brings extraction, a flow view
// flow-views, a structural view structural-views. Mirrors docs/compiler/sessions.md#skills.
pub fn skill_for_target(store: &Store, target: &str) -> Option<&'static str> {
    if crate::model::split_section_ref(target).is_some() || store.docs.contains_key(target) {
        return Some("extraction");
    }
    if let Some(v) = store.graph.views.get(target) {
        return Some(if goals::is_flow_kind(&v.kind) {
            "flow-views"
        } else {
            "structural-views"
        });
    }
    None
}

// ---- skills state ----

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SkillEntry {
    pub name: String,
    pub active: bool,
    pub chars: usize,
}

// What one activation did. Rendered carries the payload exactly once.
pub enum SkillLoad {
    Rendered(&'static str),
    Active,
    Reactivated,
    CapReached,
}

// The skills rendered this session, active or inactive, capped. Rendered text stays
// in the conversation, so the cap counts every rendering and an inactive skill keeps
// its chars. Mirrors docs/compiler/sessions.md#skills.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SkillState {
    pub rendered: Vec<SkillEntry>,
    pub cap: usize,
    // Goal-kind skills: active from the first round, never deactivated while the
    // session runs.
    pinned: BTreeSet<String>,
}

impl Default for SkillState {
    fn default() -> Self {
        SkillState::new()
    }
}

impl SkillState {
    pub fn new() -> SkillState {
        SkillState {
            rendered: Vec::new(),
            cap: crate::limits::SKILLS_PER_SESSION,
            pinned: BTreeSet::new(),
        }
    }

    // Whether a goal kind's skill governs this session. A batch works under its
    // kinds' skills and reads other nodes as reference; only a serving auto-loads.
    // Mirrors docs/compiler/sessions.md#skills.
    pub fn has_pinned(&self) -> bool {
        !self.pinned.is_empty()
    }

    // A goal kind's skill: rendered in the prompt from the first round, pinned active.
    pub fn pin(&mut self, name: &str) {
        if skill_payload(name).is_none() {
            return;
        }
        self.pinned.insert(name.to_string());
        if !self.rendered.iter().any(|e| e.name == name) && self.rendered.len() < self.cap {
            self.rendered.push(SkillEntry {
                name: name.to_string(),
                active: true,
                chars: skill_payload(name).map(crate::context::chars).unwrap_or(0),
            });
        }
    }

    // Bring a skill in: renders once, reactivates without re-rendering, refuses past
    // the cap. An unknown name errors with the index.
    pub fn activate(&mut self, name: &str) -> Result<SkillLoad, String> {
        let Some(payload) = skill_payload(name) else {
            return Err(format!(
                "unknown skill `{}`; the skills: {}",
                name,
                SKILLS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        };
        if let Some(e) = self.rendered.iter_mut().find(|e| e.name == name) {
            if e.active {
                return Ok(SkillLoad::Active);
            }
            e.active = true;
            return Ok(SkillLoad::Reactivated);
        }
        if self.rendered.len() >= self.cap {
            return Ok(SkillLoad::CapReached);
        }
        self.rendered.push(SkillEntry {
            name: name.to_string(),
            active: true,
            chars: crate::context::chars(payload),
        });
        Ok(SkillLoad::Rendered(payload))
    }

    // Unloading the last node of a kind marks its skill inactive; the text already in
    // context stands and keeps its chars and its cap slot.
    pub fn deactivate(&mut self, name: &str) {
        if self.pinned.contains(name) {
            return;
        }
        if let Some(e) = self.rendered.iter_mut().find(|e| e.name == name) {
            e.active = false;
        }
    }

    // A skill shipped as a pointer was never rendered: drop the entry so load_skill
    // renders it. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    pub fn forget(&mut self, name: &str) {
        self.rendered.retain(|e| e.name != name);
    }

    pub fn is_active(&self, name: &str) -> bool {
        self.rendered.iter().any(|e| e.name == name && e.active)
    }

    pub fn is_rendered(&self, name: &str) -> bool {
        self.rendered.iter().any(|e| e.name == name)
    }

    pub fn rendered_chars(&self) -> usize {
        self.rendered.iter().map(|e| e.chars).sum()
    }

    pub fn rendered_names(&self) -> Vec<String> {
        self.rendered.iter().map(|e| e.name.clone()).collect()
    }

    // The active payloads, prompt order.
    pub fn active_payloads(&self) -> Vec<(String, &'static str)> {
        self.rendered
            .iter()
            .filter(|e| e.active)
            .filter_map(|e| skill_payload(&e.name).map(|p| (e.name.clone(), p)))
            .collect()
    }

    // The index line under ## Loaded: active skills with their size, rendered but
    // inactive ones marked, the rest loadable.
    pub fn index_line(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for e in &self.rendered {
            if e.active {
                parts.push(format!(
                    "{} (active, {:.1}k)",
                    e.name,
                    e.chars as f64 / 1000.0
                ));
            } else {
                parts.push(format!("{} (inactive)", e.name));
            }
        }
        let loadable: Vec<&str> = SKILLS
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| !self.rendered.iter().any(|e| e.name == *n))
            .collect();
        let mut line = format!("Skills: {}", parts.join("; "));
        if !loadable.is_empty() {
            if self.rendered.len() >= self.cap {
                line.push_str(&format!("; {} (cap reached)", loadable.join(", ")));
            } else {
                line.push_str(&format!("; {} (load_skill)", loadable.join(", ")));
            }
        }
        line
    }
}

// ---- the assembled prompt ----

// The ## Project block's data: the build identity, the mode, the counts.
// Mirrors docs/compiler/sessions.md#the-prompt.
pub struct ProjectBlock {
    pub generation: u64,
    pub mode: String,
    pub diagnostics: std::collections::BTreeMap<String, u64>,
    pub here: usize,
    pub elsewhere: usize,
    pub blocked: usize,
    // Open ratification proposals: derived facts awaiting a human, not defects.
    pub ratification: usize,
    // The batch id: names the session and the protocol line's {target}.
    pub batch: String,
    // The project's lint rules with their severities, so a `lint` diagnostic names a
    // rule the model has read. Mirrors docs/compiler/sessions.md#the-prompt.
    pub lint: crate::project::Linting,
}

impl ProjectBlock {
    // Derives every goal kind once. A caller holding a derived board passes its goals
    // to `from_goals` instead of deriving the board a second time.
    pub fn compute(store: &Store, batch: &[Goal], mode: &str) -> ProjectBlock {
        let gs = crate::gen::GenSettings::from_out(&store.out);
        let derived: Vec<Goal> = goals::REGISTRY
            .iter()
            .flat_map(|k| k.derive_goals(store, &gs))
            .collect();
        ProjectBlock::from_goals(store, &derived, batch, mode)
    }

    // The block counted from goals already derived (a board's).
    pub fn from_goals(store: &Store, derived: &[Goal], batch: &[Goal], mode: &str) -> ProjectBlock {
        let blocked = derived
            .iter()
            .filter(|g| goals::blocked_on_human(&g.kind))
            .count();
        let total = derived.len() - blocked;
        let here = batch.len();
        ProjectBlock {
            generation: store.status.generation,
            mode: mode.to_string(),
            diagnostics: store.open_diag_counts(),
            ratification: store
                .graph
                .diagnostics
                .values()
                .filter(|d| d.lifecycle == "open" && d.rule == "ratification-pending")
                .count(),
            here,
            elsewhere: total.saturating_sub(here),
            blocked,
            batch: format!("b{}-0", store.status.generation),
            lint: Default::default(),
        }
    }

    // The project's lint rules ride on the block. Mirrors
    // docs/compiler/sessions.md#the-prompt.
    pub fn with_lint(mut self, lint: &crate::project::Linting) -> ProjectBlock {
        self.lint = lint.clone();
        self
    }

    fn render(&self) -> String {
        let line = |name: &str, vars: &[(&str, &str)]| BLOCKS.fill(name, vars) + "\n";
        let mut s = line("project", &[]);
        s.push_str(&line(
            "project-generation",
            &[
                ("generation", &self.generation.to_string()),
                ("mode", &self.mode),
            ],
        ));
        // The judgment findings on the graph; the checks' findings arrive with the
        // build's tail. Mirrors docs/compiler/sessions.md#the-prompt.
        let ratification = self.ratification.to_string();
        if self.diagnostics.is_empty() {
            s.push_str(&line("project-diagnostics-none", &[]));
        } else {
            let parts: Vec<String> = self
                .diagnostics
                .iter()
                .map(|(sev, n)| {
                    BLOCKS.fill(
                        "project-diagnostic-count",
                        &[("count", &n.to_string()), ("severity", sev)],
                    )
                })
                .collect();
            let note = if self.ratification > 0 {
                BLOCKS.fill(
                    "project-diagnostics-ratification",
                    &[("count", &ratification)],
                )
            } else {
                String::new()
            };
            s.push_str(&line(
                "project-diagnostics",
                &[("counts", &parts.join(", ")), ("ratification", &note)],
            ));
        }
        let note = if self.ratification > 0 {
            BLOCKS.fill("project-board-ratification", &[("count", &ratification)])
        } else {
            String::new()
        };
        s.push_str(&line(
            "project-board",
            &[
                ("here", &self.here.to_string()),
                ("elsewhere", &self.elsewhere.to_string()),
                ("blocked", &self.blocked.to_string()),
                ("ratification", &note),
            ],
        ));
        let rules: Vec<String> = self
            .lint
            .warnings
            .iter()
            .map(|r| ("warning", r))
            .chain(self.lint.errors.iter().map(|r| ("error", r)))
            .enumerate()
            .map(|(i, (severity, rule))| {
                BLOCKS.fill(
                    "project-lint-rule",
                    &[
                        ("index", &(i + 1).to_string()),
                        ("severity", severity),
                        ("rule", rule),
                    ],
                )
            })
            .collect();
        if !rules.is_empty() {
            s.push_str(&line("project-lint", &[("rules", &rules.join("; "))]));
        }
        s
    }
}

// The gate in one line, per kind, from gates.md. The kind's page under
// docs/compiler/goals/ states the full gate; this is the line the goal block carries.
pub fn gate_line(kind: &str) -> &'static str {
    GATES
        .sections()
        .into_iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, line)| line)
        .unwrap_or_else(|| GATES.get("unknown"))
}

// The change in one line: the condensed change payload plus the cause.
pub fn change_line(goal: &Goal) -> String {
    let mut s = if goal.change.is_null() {
        "(no change payload)".to_string()
    } else {
        llm::truncate(&change_words(&goal.change), 240).to_string()
    };
    if let Some(c) = &goal.cause {
        s.push_str(&format!(" (g{} via {})", c.generation, c.via));
    }
    s
}

// A change record as `field: value` pairs, one line, never raw JSON: `dirty: doc
// docs/orders.md, section /orders; unprocessed`, `revised: req:orders-6; fields:
// statement, transition`. Nested objects flatten to their own pairs, lists join with
// commas, a bare true is the field name alone, and false or empty fields are dropped.
// Mirrors docs/compiler/sessions.md#the-prompt.
pub fn change_words(v: &Value) -> String {
    fn scalar(v: &Value) -> Option<String> {
        match v {
            Value::Null => None,
            Value::Bool(false) => None,
            Value::Bool(true) => Some(String::new()),
            Value::String(s) if s.is_empty() => None,
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Array(a) if a.is_empty() => None,
            Value::Array(a) => {
                let parts: Vec<String> = a.iter().filter_map(scalar).collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join(", "))
                }
            }
            Value::Object(o) if o.is_empty() => None,
            Value::Object(o) => {
                let parts: Vec<String> = o
                    .iter()
                    .filter_map(|(k, v)| scalar(v).map(|s| pair(k, &s)))
                    .collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join(", "))
                }
            }
        }
    }
    fn pair(k: &str, v: &str) -> String {
        if v.is_empty() {
            k.to_string()
        } else {
            format!("{} {}", k, v)
        }
    }
    match v {
        Value::Object(o) => {
            let parts: Vec<String> = o
                .iter()
                .filter_map(|(k, v)| {
                    let s = scalar(v)?;
                    Some(if s.is_empty() {
                        k.to_string()
                    } else {
                        format!("{}: {}", k, s)
                    })
                })
                .collect();
            if parts.is_empty() {
                "(no change payload)".to_string()
            } else {
                parts.join("; ")
            }
        }
        other => scalar(other).unwrap_or_else(|| "(no change payload)".to_string()),
    }
}

// Assemble the session prompt for a batch, in the fixed order: the agent contract
// (feedback note as its second paragraph), the active skills, the project block, one
// block per goal, the loaded status block, the protocol line.
// Mirrors docs/compiler/sessions.md#the-prompt.
pub fn session_prompt(
    store: &Store,
    batch: &[Goal],
    loaded: &LoadedSet,
    skills: &SkillState,
    project_block: &ProjectBlock,
) -> String {
    session_prompt_elided(
        store,
        batch,
        loaded,
        skills,
        project_block,
        true,
        &BTreeSet::new(),
        Protocol::Worker,
    )
}

// The elided form the MCP serving uses after a serving's first batch: the agent
// contract and the skills already delivered ship as one-line references instead of
// their payloads; the project, goals, and loaded blocks always ship whole.
// Mirrors docs/frontends/mcp.md#compilation-over-mcp.
pub fn session_prompt_elided(
    _store: &Store,
    batch: &[Goal],
    loaded: &LoadedSet,
    skills: &SkillState,
    project_block: &ProjectBlock,
    include_contract: bool,
    delivered_skills: &BTreeSet<String>,
    protocol: Protocol,
) -> String {
    let mut s = if include_contract {
        agent_contract()
    } else {
        BLOCKS.get("contract-elided").to_string() + "\n"
    };
    if !s.ends_with('\n') {
        s.push('\n');
    }
    for (name, payload) in skills.active_payloads() {
        if delivered_skills.contains(&name) {
            s.push('\n');
            s.push_str(&BLOCKS.fill("skill-delivered", &[("name", &name)]));
            s.push('\n');
            continue;
        }
        s.push('\n');
        s.push_str(&BLOCKS.fill("skill-active", &[("name", &name)]));
        s.push('\n');
        s.push_str(payload);
        if !s.ends_with('\n') {
            s.push('\n');
        }
    }
    s.push('\n');
    s.push_str(&project_block.render());
    if batch
        .iter()
        .any(|g| matches!(g.kind.as_str(), "bind" | "generate" | "verify"))
    {
        s.push('\n');
        s.push_str(BLOCKS.get("task-execution"));
        s.push('\n');
        s.push_str(crate::task_runtime::PROMPT);
    }
    s.push('\n');
    s.push_str(BLOCKS.get("goals"));
    s.push('\n');
    let item = |name: &str, vars: &[(&str, &str)]| format!("  {}\n", BLOCKS.fill(name, vars));
    // The contract paragraph renders once per kind, under the first goal of that kind;
    // later goals of the kind point at it. Mirrors docs/compiler/sessions.md#the-prompt.
    let mut contracted: BTreeSet<String> = BTreeSet::new();
    // A hint repeated verbatim from an earlier goal renders once.
    let mut hinted: BTreeSet<String> = BTreeSet::new();
    for g in batch {
        s.push_str(&BLOCKS.fill(
            "goal",
            &[
                ("id", &g.id),
                (
                    "necessity",
                    if g.mandatory { "mandatory" } else { "optional" },
                ),
            ],
        ));
        s.push('\n');
        if let Some(k) = goals::kind(&g.kind) {
            let paragraph = k.prompt().trim();
            if !paragraph.is_empty() {
                if contracted.insert(g.kind.clone()) {
                    for line in paragraph.lines() {
                        s.push_str("  ");
                        s.push_str(line);
                        s.push('\n');
                    }
                } else {
                    s.push_str(&item("goal-contract-pointer", &[("kind", &g.kind)]));
                }
            }
        }
        s.push_str(&item("goal-change", &[("change", &change_line(g))]));
        s.push_str(&item("goal-gate", &[("gate", gate_line(&g.kind))]));
        let fresh: Vec<&String> = g
            .hints
            .iter()
            .filter(|h| hinted.insert((*h).clone()))
            .collect();
        if !fresh.is_empty() {
            let line: Vec<&str> = fresh.iter().map(|h| h.as_str()).collect();
            s.push_str(&item("goal-hints", &[("hints", &line.join("; "))]));
        }
    }
    s.push('\n');
    let pinned: BTreeSet<String> = batch.iter().map(|g| g.target.clone()).collect();
    s.push_str(&loaded.render_status(&skills.index_line(), skills.rendered_chars(), &pinned));
    // The items themselves follow the status block: the set is read, not only listed.
    // Mirrors docs/compiler/sessions.md#the-prompt.
    if !loaded.initial_pack.is_empty() {
        s.push('\n');
        s.push_str(BLOCKS.get("loaded-items"));
        s.push('\n');
        for t in &loaded.initial_pack {
            // An item's own `## id` header nests under the Loaded block.
            for line in t.trim_end().lines() {
                if let Some(rest) = line.strip_prefix("## ") {
                    s.push_str("### ");
                    s.push_str(rest);
                } else {
                    s.push_str(line);
                }
                s.push('\n');
            }
        }
    }
    s.push('\n');
    let line = match protocol {
        Protocol::Worker => WORKER_PROTOCOL,
        Protocol::Serving => SERVING_PROTOCOL,
        Protocol::OpenBatch => OPEN_BATCH_PROTOCOL,
    };
    s.push_str(&crate::payload::fill(
        line,
        &[("target", &project_block.batch)],
    ));
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

// The initially loaded set for a batch: the registry's pack computes it from the
// goals' hints; the skills of the batch's goal kinds activate from the first round.
// Mirrors docs/compiler/context.md#the-loaded-set.
pub fn initial_loaded(store: &Store, batch: &[Goal]) -> (LoadedSet, SkillState) {
    let mut loaded = LoadedSet::new(crate::limits::CONTEXT_BUDGET);
    let mut skills = SkillState::new();
    for g in batch {
        for s in goals::skills_for(&g.kind, store, &g.target) {
            skills.pin(s);
        }
    }
    let mut kinds_seen: Vec<&str> = Vec::new();
    for g in batch {
        if kinds_seen.contains(&g.kind.as_str()) {
            continue;
        }
        kinds_seen.push(&g.kind);
        let Some(k) = goals::kind(&g.kind) else {
            continue;
        };
        let of_kind: Vec<Goal> = batch.iter().filter(|x| x.kind == g.kind).cloned().collect();
        for line in k.pack(store, &of_kind).lines() {
            let full = line.contains(" full");
            let Some(target) = line
                .trim_start_matches("- ")
                .split_whitespace()
                .find(|t| resolves(store, t))
            else {
                continue;
            };
            if loaded.contains(target) {
                continue;
            }
            // A goal's own target is primary, and so is each side of a pair
            // target: a judgment cannot run on stubs of its own subjects.
            let primary = batch.iter().any(|g| {
                target == format!("alignment:{}", g.target)
                    || g.target == target
                    || g.target.split('~').any(|part| part == target)
            });
            if full && (primary || !loaded.over_high_water(0)) {
                let text = loaded.load(store, target, 1).ok();
                // The mark is a ceiling, not a suggestion: a supporting item that
                // lands the set past it downgrades to a stub, and the model loads
                // it back deliberately when it earns the budget. A goal's own
                // target stays full: the goal cannot be worked without its subject.
                if !primary && loaded.used() > loaded.high_water {
                    loaded.unload(target);
                    stub_or_defer(&mut loaded, store, target);
                } else if let Some(t) = text {
                    // What stays loaded in full is read in the prompt, not only listed.
                    loaded.initial_pack.push(t);
                }
            } else if primary {
                loaded.load_stub(store, target);
            } else {
                stub_or_defer(&mut loaded, store, target);
            }
        }
    }
    (loaded, skills)
}

// A supporting item enters as a stub while the stub fits the budget; past it the item
// is named on the status's Not loaded line instead. Mirrors
// docs/compiler/context.md#the-loaded-set.
fn stub_or_defer(loaded: &mut LoadedSet, store: &Store, target: &str) {
    loaded.load_stub(store, target);
    if loaded.used() > loaded.budget {
        loaded.unload(target);
        loaded.deferred.push(target.to_string());
    }
}

// A pack line's loadable target: a node, a document, a section, or `scope:<scope>`,
// the top level of a scope (docs/compiler/concepts/levels.md#the-scope-root).
fn resolves(store: &Store, target: &str) -> bool {
    let id = store.resolve_id(target);
    if let Some(doc) = id.strip_prefix("alignment:") {
        return store.status.alignment.iter().any(|block| block.doc == doc);
    }
    if let Some(scope) = crate::board::scope_target(id) {
        return !crate::board::scope_root(store, scope).is_empty();
    }
    store.graph.entities.contains_key(id)
        || store.graph.requirements.contains_key(id)
        || store.graph.views.contains_key(id)
        || store.graph.diagnostics.contains_key(id)
        || store.docs.contains_key(id)
        || crate::model::split_section_ref(id)
            .map(|(d, s)| {
                store
                    .docs
                    .get(&d)
                    .map(|r| r.sections.contains_key(&s))
                    .unwrap_or(false)
            })
            .unwrap_or(false)
}

// Render the prompt a batch would receive, exactly as the model would see it: the
// same code, no LLM call, nothing written. `jazyk preview` and the GUI use it.
// Mirrors docs/compiler/sessions.md#preview.
pub fn preview(store: &Store, batch: &[Goal]) -> String {
    preview_with(store, batch, &Default::default())
}

// The same preview with the project's lint rules on the project block.
pub fn preview_with(store: &Store, batch: &[Goal], lint: &crate::project::Linting) -> String {
    let (loaded, skills) = initial_loaded(store, batch);
    let pb = ProjectBlock::compute(store, batch, "auto").with_lint(lint);
    session_prompt(store, batch, &loaded, &skills, &pb)
}

// The prompt of a session run in process with its batch already open (a benchmark
// case, a snippet, a replay): the same prompt with the open-batch protocol line.
// Mirrors docs/compiler/sessions.md#the-prompt.
pub fn open_batch_prompt(store: &Store, batch: &[Goal], lint: &crate::project::Linting) -> String {
    let (loaded, skills) = initial_loaded(store, batch);
    let pb = ProjectBlock::compute(store, batch, "auto").with_lint(lint);
    let prompt = session_prompt_elided(
        store,
        batch,
        &loaded,
        &skills,
        &pb,
        true,
        &BTreeSet::new(),
        Protocol::OpenBatch,
    );
    with_opening_contract(prompt, batch)
}

// A batch holding a generate goal opens with the one generation contract after the
// assembled prompt, whichever session form runs it (an owned worker, a benchmark
// case, a snippet, a replay); its begin_generation replies then omit it.
// Mirrors docs/compiler/goals/generate.md#what-the-model-sees.
pub fn with_opening_contract(mut prompt: String, batch: &[Goal]) -> String {
    if batch.iter().any(|goal| goal.kind == "generate") {
        prompt.push_str("\n\n");
        prompt.push_str(&crate::gen::contract());
    }
    prompt
}

// One line naming a requirement's provenance, whichever kind it carries.
pub fn provenance_line(r: &crate::model::Requirement) -> String {
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

pub fn condense(v: &Value, n: usize) -> String {
    llm::truncate(&v.to_string(), n)
}

// Keep the original serialized value behind a condensed line. Wire elision and
// host event admission limits belong to the transport, not the retained JSON.
pub fn full_payload(v: &Value) -> Option<String> {
    let s = v.to_string();
    if s.len() <= 160 {
        None
    } else {
        Some(s)
    }
}

// The section a tool call names, when it belongs to the session's document. Every
// runner emits a `section` event from it (docs/compiler/sessions.md#trace-events).
pub fn named_section(args: &Value, doc: &str) -> Option<String> {
    let raw = args["section"]
        .as_str()
        .or_else(|| args["mention"]["section"].as_str())?;
    match crate::model::split_section_ref(raw) {
        Some((d, sec)) => (d == doc).then_some(sec),
        None => raw.starts_with('/').then(|| raw.to_string()),
    }
}

// The rule and message a refused tool call carries: a jazyk refusal is
// `{"error": {"rule", "message"}}`; anything else is its own message under
// `fallback` (docs/compiler/tools.md).
pub fn refusal(text: &str, fallback: &str) -> (String, String) {
    let parsed: Value =
        serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()));
    let rule = parsed["error"]["rule"]
        .as_str()
        .unwrap_or(fallback)
        .to_string();
    let message = parsed["error"]["message"]
        .as_str()
        .unwrap_or(text)
        .to_string();
    (rule, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use std::collections::BTreeMap;

    fn fixture() -> Store {
        let mut s = Store::default();
        let text = "# Shop\nintro\n\n## Cart\nThe Shopping Cart holds items.\n";
        s.docs.insert(
            "shop.md".into(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
        s.graph.entities.insert(
            "ent:shopping-cart".into(),
            Entity {
                name: "Shopping Cart".into(),
                definition: Some("holds items".into()),
                mentions: vec![SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop/cart".into(),
                    quote: "The Shopping Cart holds items.".into(),
                }],
                ..Default::default()
            },
        );
        s
    }

    fn goal() -> Goal {
        Goal {
            id: "g:reconcile-section:shop.md#/shop/cart".into(),
            kind: "reconcile-section".into(),
            class: "compile".into(),
            mandatory: true,
            target: "shop.md#/shop/cart".into(),
            unit: "section".into(),
            change: serde_json::json!({"unprocessed": true}),
            cause: Some(Cause {
                generation: 3,
                mutation: 0,
                via: "section".into(),
            }),
            state: GoalState::Open,
            hints: vec!["load shop.md#/shop/cart".into(), "skill extraction".into()],
        }
    }

    #[test]
    fn alignment_evidence_is_in_the_opening_prompt_and_reloadable() {
        let mut store = fixture();
        store.status.alignment.push(DocAlignment {
            doc: "shop.md".into(),
            changes: vec![SectionOp {
                op: "moved".into(),
                from: vec!["shop.md#/shop/old-cart".into()],
                to: vec!["shop.md#/shop/cart".into()],
                similarity: Some(0.9),
            }],
            proposals: vec![AnchorProposal {
                anchor: "ent:cart".into(),
                from: "shop.md#/shop/old-cart".into(),
                quote: "The Shopping Cart holds items.".into(),
                excerpt: "Previous cart context.".into(),
                candidates: vec![AnchorCandidate {
                    section: "shop.md#/shop/cart".into(),
                    similarity: 0.9,
                    quote_locates: true,
                    nearest: None,
                    excerpt: "The Shopping Cart holds items.".into(),
                }],
            }],
        });
        let mut goal = goal();
        goal.id = "g:place-anchors:shop.md".into();
        goal.kind = "place-anchors".into();
        goal.target = "shop.md".into();
        let (mut loaded, _) = initial_loaded(&store, std::slice::from_ref(&goal));
        assert!(loaded.contains("alignment:shop.md"));
        let prompt = preview(&store, &[goal]);
        for evidence in [
            "Previous cart context.",
            "old-cart",
            "quoteLocates",
            "0.9",
            "The Shopping Cart holds items.",
        ] {
            assert!(prompt.contains(evidence), "Missing {evidence}: {prompt}");
        }
        loaded.unload("alignment:shop.md");
        let reloaded = loaded.load(&store, "alignment:shop.md", 1).unwrap();
        assert!(reloaded.contains("Previous cart context."));

        let mut tight = LoadedSet::new(500);
        let first = tight.load(&store, "alignment:shop.md", 1).unwrap();
        let handle = tight.items[0].handles[0].handle.clone();
        assert!(first.contains(&handle));
        let rest = tight.expand(&store, &handle).unwrap();
        assert!(rest.contains("Expansion of alignment:shop.md (body)"));
        assert!(tight.load(&store, "alignment:missing.md", 1).is_err());
    }

    // The initially loaded set obeys the high-water ceiling on the normal path.
    // Mirrors docs/compiler/context.md#policy.
    // Mirrors docs/compiler/sessions.md#the-prompt: the project block carries the
    // lint rules with their severities, and nothing when the project declares none.
    #[test]
    fn project_block_carries_the_lint_rules() {
        let s = Store::default();
        let bare = ProjectBlock::compute(&s, &[], "auto").render();
        assert!(!bare.contains("- lint:"), "{}", bare);
        let lint = crate::project::Linting {
            warnings: vec!["An em dash appears in prose.".into()],
            errors: vec!["A TODO marker appears in prose.".into()],
        };
        let with = ProjectBlock::compute(&s, &[], "auto")
            .with_lint(&lint)
            .render();
        assert!(
            with.contains(
                "- lint: [1] warning: An em dash appears in prose.; [2] error: A TODO marker appears in prose.\n"
            ),
            "{}",
            with
        );
        assert!(preview_with(&s, &[], &lint).contains("- lint: [1] warning: An em dash"));
    }

    // A board's goals give the same counts as a fresh derivation.
    #[test]
    fn project_block_counts_from_a_board_without_deriving_again() {
        let s = fixture();
        let gs = crate::gen::GenSettings::from_out(&s.out);
        let derived: Vec<Goal> = goals::REGISTRY
            .iter()
            .flat_map(|k| k.derive_goals(&s, &gs))
            .collect();
        assert!(!derived.is_empty(), "the fixture opens goals");
        let batch = &derived[..1];
        let fresh = ProjectBlock::compute(&s, batch, "auto");
        let counted = ProjectBlock::from_goals(&s, &derived, batch, "auto");
        assert_eq!(fresh.render(), counted.render());
        assert_eq!(
            counted.elsewhere + counted.here + counted.blocked,
            derived.len()
        );
    }

    // Goal kinds pin their skills into a fresh session before anything else renders,
    // so the cap can never drop a pinned skill while every skill fits under it.
    // Mirrors docs/compiler/sessions.md#skills.
    #[test]
    fn every_pinned_skill_fits_under_the_session_cap() {
        assert!(SKILLS.len() <= crate::limits::SKILLS_PER_SESSION);
        let mut all = SkillState::new();
        for (name, _) in SKILLS {
            all.pin(name);
        }
        assert!(SKILLS.iter().all(|(name, _)| all.is_active(name)));
    }

    #[test]
    fn initial_loaded_stays_under_the_high_water_mark() {
        let s = fixture();
        let g = goal();
        let (loaded, _) = initial_loaded(&s, std::slice::from_ref(&g));
        assert!(
            loaded.used() <= loaded.high_water,
            "{} > {}",
            loaded.used(),
            loaded.high_water
        );
    }

    // A criteria batch listing more requirements than the budget holds opens within
    // it: past the budget an item is named on the Not loaded line, never stubbed.
    // Mirrors docs/compiler/context.md#the-loaded-set.
    #[test]
    fn initial_loaded_defers_what_the_budget_cannot_hold() {
        let mut s = fixture();
        let long = "Ines Marlow keeps the lantern lit for the harbor crew until dawn on the night of the storm, and the whole quay sees it from the water. ".repeat(12);
        let mut ids = Vec::new();
        for i in 0..160 {
            let id = format!("req:big-{}", i);
            s.graph.requirements.insert(
                id.clone(),
                crate::model::Requirement {
                    statement: format!("{} ({})", long, i),
                    entities: vec!["ent:order".into()],
                    ..Default::default()
                },
            );
            ids.push(id);
        }
        let g = Goal {
            id: "g:derive-criteria:ent:order".into(),
            kind: "derive-criteria".into(),
            class: "compile".into(),
            mandatory: true,
            target: "ent:order".into(),
            unit: "entity".into(),
            change: serde_json::json!({"none": ids, "revised": []}),
            cause: None,
            state: GoalState::Open,
            hints: vec![],
        };
        let (loaded, _) = initial_loaded(&s, std::slice::from_ref(&g));
        assert!(
            loaded.used() <= loaded.budget,
            "{} > {}",
            loaded.used(),
            loaded.budget
        );
        assert!(!loaded.deferred.is_empty());
        assert!(
            loaded
                .render_condensed(0)
                .contains("Not loaded (over the budget"),
            "{}",
            loaded.render_condensed(0)
        );
    }

    // A pack line naming `scope:<scope>` resolves and loads the scope's top level as
    // stubs, full because it is the goal's own target.
    // Mirrors docs/compiler/concepts/levels.md#the-scope-root.
    #[test]
    fn initial_loaded_packs_a_scope_root() {
        let s = fixture();
        assert!(resolves(&s, "scope:public"));
        assert!(!resolves(&s, "scope:nope"));
        let g = Goal {
            id: "g:abstract-entity:scope:public".into(),
            kind: "abstract-entity".into(),
            class: "gc".into(),
            mandatory: false,
            target: "scope:public".into(),
            unit: "entity".into(),
            change: serde_json::json!({"fan_out": 1, "limit": {"soft": 9, "hard": 15}, "candidates": []}),
            cause: None,
            state: GoalState::Open,
            hints: vec!["load scope:public".into(), "skill abstraction".into()],
        };
        let (loaded, _) = initial_loaded(&s, std::slice::from_ref(&g));
        assert!(
            loaded.contains("scope:public"),
            "{:?}",
            loaded.render_condensed(0)
        );
        let status = loaded.render_condensed(0);
        assert!(
            status.contains("top level: 1 entities as stubs"),
            "{}",
            status
        );
        // The prompt's loaded block names the root's line, pinned as the goal's target.
        let prompt = preview(&s, std::slice::from_ref(&g));
        assert!(
            prompt.contains("- scope:public   top level: 1 entities as stubs"),
            "{}",
            prompt
        );
        assert!(
            !prompt.contains("Consider unloading: scope:public"),
            "{}",
            prompt
        );
    }

    // The prompt assembles in the fixed order, with the feedback note as the second
    // paragraph and the protocol line naming the batch.
    // Mirrors docs/compiler/sessions.md#the-prompt.
    #[test]
    fn prompt_assembles_in_order_with_payloads_placed() {
        let s = fixture();
        let g = goal();
        let prompt = preview(&s, std::slice::from_ref(&g));
        assert!(prompt.starts_with("You are one session of jazyk"));
        let paras: Vec<&str> = prompt.split("\n\n").collect();
        assert_eq!(paras[1], FEEDBACK_NOTE, "the note is the second paragraph");
        // The agent contract itself names the block headings mid-sentence, so the
        // blocks are located by their heading lines.
        let project = prompt.find("\n## Project\n").expect("project block");
        let goals_at = prompt.find("\n## Goals\n").expect("goals block");
        let loaded = prompt.find("\n## Loaded (").expect("loaded block");
        let proto = prompt.rfind("PROTOCOL").expect("protocol line");
        assert!(project < goals_at && goals_at < loaded && loaded < proto);
        assert!(prompt.contains("- [g:reconcile-section:shop.md#/shop/cart] mandatory"));
        assert!(prompt.contains("Change: "));
        assert!(prompt.contains(&format!("Gate: {}", gate_line("reconcile-section"))));
        assert!(prompt.contains("Hints: load shop.md#/shop/cart; skill extraction"));
        // The batch's goal kind activates its skill from the first round.
        assert!(prompt.contains("[skill: extraction (active)]"));
        // The protocol line names the batch id, never the raw placeholder.
        assert!(!prompt.contains("{target}"));
        assert!(prompt.contains("b0-0"));
    }

    #[test]
    fn every_contract_paragraph_names_its_goal_first() {
        for k in goals::REGISTRY.iter() {
            if goals::blocked_on_human(k.kind()) {
                continue;
            }
            assert!(
                k.prompt().starts_with("This goal"),
                "{} names its goal first",
                k.kind()
            );
        }
        let s = agent_contract();
        assert!(s.contains("report_feedback"));
        let rest = AGENT_CONTRACT
            .split_once("{feedback}")
            .map(|(_, r)| r)
            .unwrap();
        assert!(s.ends_with(rest), "nothing of the contract is lost");
    }

    #[test]
    fn declare_edges_pins_only_unchanged_edge_and_stability_rules() {
        let store = fixture();
        let mut edges = goal();
        edges.kind = "declare-edges".into();
        edges.target = "req:cart-items".into();
        let (_, skills) = initial_loaded(&store, &[edges]);
        assert!(skills.has_pinned());
        assert_eq!(
            skills
                .rendered
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["edges"]
        );
        let extraction = skill_payload("extraction").unwrap();
        let edges = skill_payload("edges").unwrap();
        let edge_rules = extraction
            .split_once("Edges. ")
            .unwrap()
            .1
            .split_once("Transitions. ")
            .unwrap()
            .0;
        let stability = extraction
            .split_once("Stability. ")
            .unwrap()
            .1
            .split_once("Rounds. ")
            .unwrap()
            .0;
        assert_eq!(
            edges,
            format!("Edges. {edge_rules}Stability. {}\n", stability.trim_end())
        );
        assert!(edges.len() < extraction.len() / 4);
        assert_eq!(
            goals::skills_for("reconcile-section", &store, ""),
            vec!["extraction"]
        );
    }

    // Auto-load once, the cap, and the inactive marking.
    // Mirrors docs/compiler/sessions.md#skills.
    #[test]
    fn skill_state_renders_once_caps_and_goes_inactive() {
        let mut sk = SkillState::new();
        sk.cap = 4; // Exercise a deliberately small fixture cap, independent of the production default.
        assert!(matches!(
            sk.activate("extraction"),
            Ok(SkillLoad::Rendered(_))
        ));
        assert!(matches!(sk.activate("extraction"), Ok(SkillLoad::Active)));
        sk.deactivate("extraction");
        assert!(!sk.is_active("extraction"));
        assert!(sk.index_line().contains("extraction (inactive)"));
        // Rendered text keeps its chars and its cap slot.
        assert!(sk.rendered_chars() > 0);
        assert!(matches!(
            sk.activate("extraction"),
            Ok(SkillLoad::Reactivated)
        ));
        for name in ["judgment", "flow-views", "structural-views"] {
            assert!(matches!(sk.activate(name), Ok(SkillLoad::Rendered(_))));
        }
        assert!(matches!(
            sk.activate("abstraction"),
            Ok(SkillLoad::CapReached)
        ));
        assert!(sk.index_line().contains("cap reached"));
        assert!(sk.activate("no-such-skill").is_err());
        // A pinned goal-kind skill never deactivates.
        let mut pinned = SkillState::new();
        pinned.pin("judgment");
        pinned.deactivate("judgment");
        assert!(pinned.is_active("judgment"));
    }
}
