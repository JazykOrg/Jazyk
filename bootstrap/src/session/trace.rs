use super::*;

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
        // The items of a selected build (docs/compiler/compilation.md#selected-builds).
        #[serde(skip_serializing_if = "Option::is_none")]
        selection: Option<serde_json::Value>,
    },
    // A selected build widened: the items added and the selection now in force
    // (`null` for the whole set).
    #[serde(rename = "selection")]
    Selection {
        label: String,
        added: serde_json::Value,
        selection: serde_json::Value,
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
            selection,
            ..
        } => {
            let per_kind: Vec<String> = kinds.iter().map(|(k, n)| format!("{} {}", n, k)).collect();
            let mut s = format!(
                "compile{}: {} goals",
                if selection.is_some() {
                    " (selection)"
                } else {
                    ""
                },
                goals
            );
            if !per_kind.is_empty() {
                s.push_str(&format!(" ({})", per_kind.join(", ")));
            }
            if *blocked > 0 {
                s.push_str(&format!(", {} blocked", blocked));
            }
            eprintln!("{}", s)
        }
        TraceEvent::Selection { added, .. } => match added.as_array() {
            Some(items) => {
                let names: Vec<String> = items
                    .iter()
                    .filter_map(|i| {
                        serde_json::from_value::<crate::selection::Item>(i.clone()).ok()
                    })
                    .map(|i| i.label())
                    .collect();
                eprintln!("selection widened: {}", names.join(", "))
            }
            None => eprintln!("selection widened to the whole set"),
        },
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
