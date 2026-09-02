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
    // One model call. The request carries the whole outgoing message list, the
    // response the raw assistant message; both are recorded in full in the
    // transcript and elided on the wire (docs/compiler/sessions.md#trace-events).
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
        TraceEvent::ModelText { label, text } => {
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
    // Best-effort cancellation, checked between batches, entities, and rows. It rides
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
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// ---- payloads ----

// The fixed session contract; every kind-specific instruction arrives as data inside
// the assembled prompt. Mirrors docs/compiler/sessions.md#the-prompt.
const AGENT_CONTRACT: &str = include_str!("../../docs/compiler/goals/prompts/agent-contract.md");

// The feedback contract, spliced in as the contract's second paragraph.
const FEEDBACK_NOTE: &str = include_str!("../../docs/compiler/goals/prompts/feedback-note.md");

// The protocol line; {target} is the batch id.
const WORKER_PROTOCOL: &str = include_str!("../../docs/compiler/goals/prompts/worker-protocol.md");

// The note rides directly under the first paragraph: the first says what the session
// is, the second how to report that the rest of the prompt failed it.
pub(crate) fn with_feedback_note(system: &str) -> String {
    match system.split_once("\n\n") {
        Some((role, rest)) => format!("{}\n\n{}\n\n{}", role.trim_end(), FEEDBACK_NOTE, rest),
        None => format!("{}\n\n{}", system.trim_end(), FEEDBACK_NOTE),
    }
}

// The skill payloads, embedded at compile time. Mirrors docs/compiler/sessions.md#skills.
pub const SKILLS: [(&str, &str); 6] = [
    (
        "extraction",
        include_str!("../../docs/compiler/skills/extraction.md"),
    ),
    (
        "judgment",
        include_str!("../../docs/compiler/skills/judgment.md"),
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

#[derive(Clone, Debug)]
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
                chars: skill_payload(name).map(|p| p.len()).unwrap_or(0),
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
            chars: payload.len(),
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
    // The batch id: names the session and the protocol line's {target}.
    pub batch: String,
}

impl ProjectBlock {
    pub fn compute(store: &Store, batch: &[Goal], mode: &str) -> ProjectBlock {
        let gs = crate::gen::GenSettings::from_out(&store.out);
        let mut total = 0usize;
        let mut blocked = 0usize;
        for k in goals::REGISTRY.iter() {
            let derived = k.derive_goals(store, &gs).len();
            if goals::blocked_on_human(k.kind()) {
                blocked += derived;
            } else {
                total += derived;
            }
        }
        let here = batch.len();
        ProjectBlock {
            generation: store.status.generation,
            mode: mode.to_string(),
            diagnostics: store.open_diag_counts(),
            here,
            elsewhere: total.saturating_sub(here),
            blocked,
            batch: format!("b{}-0", store.status.generation),
        }
    }

    fn render(&self) -> String {
        let mut s = String::from("## Project\n");
        s.push_str(&format!(
            "- generation {}, {} mode\n",
            self.generation, self.mode
        ));
        if self.diagnostics.is_empty() {
            s.push_str("- diagnostics: none open\n");
        } else {
            let parts: Vec<String> = self
                .diagnostics
                .iter()
                .map(|(sev, n)| format!("{} {}(s)", n, sev))
                .collect();
            s.push_str(&format!("- diagnostics: {}\n", parts.join(", ")));
        }
        s.push_str(&format!(
            "- board: {} goal(s) in this session; {} elsewhere; {} blocked on human answers\n",
            self.here, self.elsewhere, self.blocked
        ));
        s
    }
}

// The gate in one line, per kind. The kind's page under docs/compiler/goals/ states
// the full gate; this is the line the goal block carries.
pub fn gate_line(kind: &str) -> &'static str {
    match kind {
        "place-anchors" => "every proposal decided: place_anchor or orphan_anchor staged for each",
        "reconcile-section" => {
            "a coverage mark staged or recorded; stale anchors addressed; every covered claim honest"
        }
        "rejudge-pair" => {
            "a verdict for the pair in evidence; for an acting verdict, the mutation or diagnostic that carried it"
        }
        "review-entity" => "definition current; lookalikes judged; diagnostics filed or resolved",
        "retrace" => "nothing on the target points at a dead or missing node",
        "conform-instance" => {
            "values and links conform or nonconformant-instance filed; one verdict per attribute and link in evidence"
        }
        "bind" => "a current ledger row recorded by record_binding",
        "generate" => "record_generation landed with a live factHash",
        "verify" => "a verdict recorded on the row with a live factHash",
        "ratify" => "the human accepts the proposal (a dual write) or retracts the fact",
        "answer" => "the human answers; applying the answer is an answer session",
        "declare-edges" => {
            "typed edges declared, or the justification says the statement is not structural"
        }
        "dedupe-candidates" => "merged, or kept separate with the reasoning",
        "curate-view" => "membership decided: every matched node added, or excluded with a note",
        "split-view" => "the count is within the limit, no member lost, sub-views linked",
        "abstract-entity" => {
            "sub-entities carry parent and derived provenance, detail moved, docs proposals staged"
        }
        _ => "the kind's page states the gate",
    }
}

// The change in one line: the condensed change payload plus the cause.
pub fn change_line(goal: &Goal) -> String {
    let mut s = if goal.change.is_null() {
        "(no change payload)".to_string()
    } else {
        llm::truncate(&goal.change.to_string(), 160).to_string()
    };
    if let Some(c) = &goal.cause {
        s.push_str(&format!(" (g{} via {})", c.generation, c.via));
    }
    s
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
) -> String {
    let mut s = if include_contract {
        with_feedback_note(AGENT_CONTRACT)
    } else {
        String::from("(same agent contract as the earlier batch in this session; unchanged)\n")
    };
    if !s.ends_with('\n') {
        s.push('\n');
    }
    for (name, payload) in skills.active_payloads() {
        if delivered_skills.contains(&name) {
            s.push_str(&format!(
                "\n[skill: {} (active; delivered earlier)]\n",
                name
            ));
            continue;
        }
        s.push_str(&format!("\n[skill: {} (active)]\n", name));
        s.push_str(payload);
        if !s.ends_with('\n') {
            s.push('\n');
        }
    }
    s.push('\n');
    s.push_str(&project_block.render());
    s.push_str("\n## Goals\n");
    for g in batch {
        s.push_str(&format!(
            "- [{}] {}\n",
            g.id,
            if g.mandatory { "mandatory" } else { "optional" }
        ));
        if let Some(k) = goals::kind(&g.kind) {
            let paragraph = k.prompt().trim();
            if !paragraph.is_empty() {
                for line in paragraph.lines() {
                    s.push_str("  ");
                    s.push_str(line);
                    s.push('\n');
                }
            }
        }
        s.push_str(&format!("  Change: {}\n", change_line(g)));
        s.push_str(&format!("  Gate: {}\n", gate_line(&g.kind)));
        if !g.hints.is_empty() {
            s.push_str(&format!("  Hints: {}\n", g.hints.join("; ")));
        }
    }
    s.push('\n');
    let pinned: BTreeSet<String> = batch.iter().map(|g| g.target.clone()).collect();
    s.push_str(&loaded.render_status(&skills.index_line(), skills.rendered_chars(), &pinned));
    s.push('\n');
    s.push_str(&WORKER_PROTOCOL.replace("{target}", &project_block.batch));
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
            let primary = batch
                .iter()
                .any(|g| g.target == target || g.target.split('~').any(|part| part == target));
            if full && (primary || !loaded.over_high_water(0)) {
                let _ = loaded.load(store, target, 1);
                // The mark is a ceiling, not a suggestion: a supporting item that
                // lands the set past it downgrades to a stub, and the model loads
                // it back deliberately when it earns the budget. A goal's own
                // target stays full: the goal cannot be worked without its subject.
                if !primary && loaded.used() > loaded.high_water {
                    loaded.unload(target);
                    loaded.load_stub(store, target);
                }
            } else {
                loaded.load_stub(store, target);
            }
        }
    }
    (loaded, skills)
}

fn resolves(store: &Store, target: &str) -> bool {
    let id = store.resolve_id(target);
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
    let (loaded, skills) = initial_loaded(store, batch);
    let pb = ProjectBlock::compute(store, batch, "auto");
    session_prompt(store, batch, &loaded, &skills, &pb)
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

// The payload behind a condensed line, only when condensing cut something, capped so
// a huge pack cannot flood the trace file.
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

    // The initially loaded set obeys the high-water ceiling on the normal path.
    // Mirrors docs/compiler/context.md#policy.
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
        let s = with_feedback_note(AGENT_CONTRACT);
        assert!(s.contains("report_feedback"));
        let rest = AGENT_CONTRACT.split_once("\n\n").map(|(_, r)| r).unwrap();
        assert!(s.ends_with(rest), "nothing of the contract is lost");
    }

    // Auto-load once, the cap, and the inactive marking.
    // Mirrors docs/compiler/sessions.md#skills.
    #[test]
    fn skill_state_renders_once_caps_and_goes_inactive() {
        let mut sk = SkillState::new();
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
