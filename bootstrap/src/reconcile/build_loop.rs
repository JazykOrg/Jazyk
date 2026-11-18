// The build loop: the build entry points, the session loop, the deterministic tail,
// and the dead-endpoint breaker.
use super::*;

#[cfg(not(target_arch = "wasm32"))]
mod concurrent;

// A status write the build loop makes, its refusal traced (the store already reports it
// on stderr). Returns the refusal. Mirrors docs/compiler/graph.md#concurrency.
fn write_status_traced(s: &mut Store, what: &str, trace: &Trace) -> Option<String> {
    let why = s.write_status(what).err()?;
    trace.line("build", &format!("{} not saved: {}", what, why));
    Some(why)
}

// Apply what a derivation learned to the store: records whose evidence lapsed, failed
// entries whose subject changed again, parked entries whose target is gone.
fn absorb_derivation(store: &mut Store, board: &Board) {
    if !board.lapsed.is_empty() {
        store.clear_changes(&board.lapsed);
    }
    if !board.reopened.is_empty()
        || !board.dropped_parked.is_empty()
        || !board.dropped_failed.is_empty()
    {
        store.status.failed.retain(|f| {
            !board.reopened.contains(&f.goal.id) && !board.dropped_failed.contains(&f.goal.id)
        });
        store
            .status
            .parked
            .retain(|p| !board.dropped_parked.contains(&p.goal.id));
        // A refusal is reported on stderr; the next derivation lapses the same goals.
        store.write_status("absorb derivation").ok();
    }
}

fn reload(store: &mut Store, out: &Path, proj: &Project) {
    *store = Store::load(out);
    let (parsed, _) = parse_all(proj);
    store.sync_docs(&parsed);
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn compile(proj: &Project, llm: &Llm, out: &Path, trace: &Trace) -> BuildReport {
    compile_with(proj, llm, out, trace, None).await
}

// `max_sessions` is the `--sessions` count: at most that many sessions, then an
// honest incomplete, so a project advances one batch at a time.
// Mirrors docs/frontends/cli.md#jazyk-compile.
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn compile_with(
    proj: &Project,
    llm: &Llm,
    out: &Path,
    trace: &Trace,
    max_sessions: Option<usize>,
) -> BuildReport {
    let refused = |e: String| {
        trace.line("build", &format!("build refused: {}", e));
        BuildReport {
            verdict: "incomplete".into(),
            errors: 1,
            ..Default::default()
        }
    };
    // A store of a newer version is refused before the lease, the runner, or any file
    // write (docs/compiler/graph.md#store-version).
    if let Some(why) = crate::store::newer_store(out) {
        return refused(why);
    }
    // The control plane's build contract: one coarse lease for the run, refused while
    // an agent is mid-task, and the run itself is an approval in manual mode.
    let _build = match crate::control::begin_internal_build(proj, out, "compile") {
        Ok(g) => g,
        Err(e) => return refused(e),
    };
    // Every session runs against a runner: natively an ACP worker session against the
    // configured agent, whose process lives for the run and spawns on the first
    // session; in a browser tab the loop in this same module
    // (docs/frontends/wasm.md#sessions).
    #[cfg(not(target_arch = "wasm32"))]
    let runner: Box<dyn crate::runner::SessionRunner> = if crate::vfs::is_scoped() {
        match crate::inprocess::InProcessRunner::start(proj, llm, out) {
            Ok(r) => Box::new(r),
            Err(e) => return refused(e),
        }
    } else {
        match crate::acp::runner::AcpRunner::start(proj, llm, out) {
            Ok(r) => Box::new(r),
            Err(e) => return refused(e),
        }
    };
    #[cfg(target_arch = "wasm32")]
    let runner: Box<dyn crate::runner::SessionRunner> =
        match crate::inprocess::InProcessRunner::start(proj, llm, out) {
            Ok(r) => Box::new(r),
            Err(e) => return refused(e),
        };
    compile_under(proj, out, trace, max_sessions, runner.as_ref(), _build).await
}

// A build with its lease held and its runner chosen: the rest of `compile_with`, and
// what a test drives with a scripted runner (docs/compiler/sessions.md#execution).
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub(crate) async fn compile_under(
    proj: &Project,
    out: &Path,
    trace: &Trace,
    max_sessions: Option<usize>,
    runner: &dyn crate::runner::SessionRunner,
    _build: crate::control::BuildGuard,
) -> BuildReport {
    runner.set_build_token(Some(_build.token().to_owned()));
    trace.line("build", &format!("agent: {}", runner.agent_name()));

    let command_key = continuation::key(out);
    let saved = match Continuation::load(&command_key, max_sessions) {
        Ok(saved) => saved,
        Err(reason) => return suspended_build(reason, 0, 0, 0, 0),
    };
    if let Some(result) = saved.as_ref().and_then(|state| state.result.as_ref()) {
        return result.clone();
    }
    let resuming = saved.is_some();
    // A store of a newer version stops the build before it writes any file or runs any
    // session (docs/compiler/graph.md#store-version).
    if let Some(why) = crate::store::newer_store(out) {
        trace.line("build", &why);
        return suspended_build(why, 0, 0, 0, 0);
    }

    // Every build keeps the generated ignore file current
    // (docs/compiler/document-sets.md#persistence-and-identity).
    if let Err(e) = crate::document_sets::ensure_ignore(&proj.root) {
        trace.line("build", &format!("cannot write .jazyk/.gitignore: {}", e));
    }
    // Logs past retention go when a build starts (docs/compiler/jazyk-folder.md#logs).
    crate::logs::prune(out);
    // The merge driver `.jazyk/.gitattributes` names is registered
    // (docs/compiler/jazyk-folder.md#gitattributes).
    crate::document_sets::register_merge_driver(&proj.root);
    let mut store = Store::open_for_build(out);
    if !resuming {
        // The build's own cost entry opens empty; it clears the latest builds' entries
        // only when no other build runs on the set (docs/compiler/graph.md#storage-layout).
        let alone = !crate::control::builds(out)
            .iter()
            .any(|l| l.worker != _build.token());
        if let Err(why) = store.write_status_costs(
            "build opening",
            Some((_build.token(), &Costs::default())),
            alone,
            0,
        ) {
            trace.line("build", &format!("build opening not saved: {}", why));
        }
    }
    let (parsed, _links) = parse_all(proj);
    if !resuming {
        store.sync_docs(&parsed);
    }
    // Stamp the documents matching the project roots, so commits with no Project at
    // hand (edits, answers, MCP sessions) order documents by the same link levels.
    // Mirrors docs/compiler/graph.md#storage-layout.
    if !resuming {
        store.status.roots = store
            .docs
            .keys()
            .filter(|d| proj.is_root_file(d))
            .cloned()
            .collect();
        write_status_traced(&mut store, "build roots", trace);
        sweep(&mut store, trace);
    }
    // A typed command is an approval (docs/compiler/control-plane.md#modes-and-releases):
    // record the compile release so manual mode gates nothing this run asked for.
    crate::control::release(proj, out, Some("compile"));
    let control = Control::load(proj, out);
    // The build's own progress file, beside any other build's on this set; it goes
    // when the build ends (docs/compiler/graph.md#a-builds-progress).
    let mut record = crate::control::RunRecord::start(out, _build.token(), _build.stage());
    let run = run_sessions_recorded(
        proj,
        out,
        trace,
        max_sessions,
        runner,
        &mut store,
        &parsed,
        &control,
        Some(&mut record),
    )
    .await;
    if let Some(reason) = run.suspend_reason {
        return suspended_build(
            reason,
            run.derived,
            run.sessions,
            run.applied,
            run.costs.tokens,
        );
    }
    let mut report = finalize(
        &mut store,
        proj,
        run.parked,
        Some((_build.token(), &run.costs)),
        trace,
    );
    // A standalone build sweeps under its own lease; a workflow sweeps once at its end.
    // A cancelled build leaves the task list as its sessions left it.
    if !crate::task_runtime::sweep_deferred() && !trace.is_cancelled() {
        report.retired_tasks = sweep_tasks(proj, out, trace);
    }
    report.goals = run.derived;
    report.sessions = run.sessions;
    report.applied = run.applied;
    if let Some(mut state) = match Continuation::load(&command_key, max_sessions) {
        Ok(state) => state,
        Err(reason) => {
            return suspended_build(
                reason,
                run.derived,
                run.sessions,
                run.applied,
                run.costs.tokens,
            )
        }
    } {
        state.result = Some(report.clone());
        if let Err(reason) = state.save(&command_key).await {
            return suspended_build(
                reason,
                run.derived,
                run.sessions,
                run.applied,
                run.costs.tokens,
            );
        }
    }
    report
}

fn suspended_build(
    reason: String,
    goals: usize,
    sessions: u32,
    applied: usize,
    tokens: u64,
) -> BuildReport {
    BuildReport {
        verdict: "suspended".into(),
        goals,
        sessions,
        applied,
        tokens,
        suspend_reason: Some(reason),
        ..Default::default()
    }
}

// What the session loop leaves for the tail: the goals the board derived when the
// build opened, the sessions run, the mutations applied, the spend, and the goals
// parked along the way.
pub(crate) struct SessionsRun {
    pub derived: usize,
    pub sessions: u32,
    pub applied: usize,
    pub costs: Costs,
    pub parked: Vec<ParkedGoal>,
    pub suspend_reason: Option<String>,
}

use crate::control::InFlightBatch;
use crate::runner::SessionReport;

// The tallies of one build's session loop. A resumable build keeps them in its command
// continuation between sessions, so a resumed build counts on where it stopped.
#[derive(Default)]
pub(super) struct Tally {
    pub derived_ids: BTreeSet<String>,
    pub sessions: u32,
    pub applied: usize,
    pub costs: Costs,
    pub attempts: BTreeMap<String, u32>,
    pub parked: Vec<ParkedGoal>,
    pub parked_ids: BTreeSet<String>,
    pub known: BTreeSet<String>,
    pub resolved_changes: BTreeMap<String, serde_json::Value>,
    pub cycles: BTreeMap<String, (serde_json::Value, u32)>,
    pub dead_endpoint: usize,
    pub endpoint_error: String,
}

impl Tally {
    fn take(saved: &mut Continuation) -> Tally {
        Tally {
            derived_ids: std::mem::take(&mut saved.derived_ids),
            sessions: saved.sessions,
            applied: saved.applied,
            costs: std::mem::take(&mut saved.costs),
            attempts: std::mem::take(&mut saved.attempts),
            parked: std::mem::take(&mut saved.parked),
            parked_ids: std::mem::take(&mut saved.parked_ids),
            known: std::mem::take(&mut saved.known),
            resolved_changes: std::mem::take(&mut saved.resolved_changes),
            cycles: std::mem::take(&mut saved.cycles),
            dead_endpoint: saved.dead_endpoint,
            endpoint_error: std::mem::take(&mut saved.endpoint_error),
        }
    }

    fn put(&self, saved: &mut Continuation) {
        saved.derived_ids = self.derived_ids.clone();
        saved.sessions = self.sessions;
        saved.applied = self.applied;
        saved.costs = self.costs.clone();
        saved.attempts = self.attempts.clone();
        saved.parked = self.parked.clone();
        saved.parked_ids = self.parked_ids.clone();
        saved.known = self.known.clone();
        saved.resolved_changes = self.resolved_changes.clone();
        saved.cycles = self.cycles.clone();
        saved.dead_endpoint = self.dead_endpoint;
        saved.endpoint_error = self.endpoint_error.clone();
    }

    // The run file's view of the build: its sessions, spend, and parks.
    fn record(
        &self,
        record: &mut Option<&mut crate::control::RunRecord>,
        in_flight: Vec<InFlightBatch>,
    ) {
        if let Some(r) = record.as_mut() {
            r.update(self.sessions, self.costs.tokens, in_flight, &self.parked);
        }
    }
}

// The run file's line for one batch in flight.
fn in_flight_line(active: &ActiveBatch) -> InFlightBatch {
    InFlightBatch::new(
        &active.run.id,
        active.run.goal_ids(),
        active.run.executor.clone(),
        &active.class,
        active.tier,
    )
}

// What every step of the session loop reads: the project, the set, the trace, the
// control plane, and the `--sessions` count.
pub(super) struct Loop<'a> {
    pub proj: &'a Project,
    pub out: &'a Path,
    pub trace: &'a Trace,
    pub control: &'a Control,
    pub max_sessions: Option<usize>,
}

impl Loop<'_> {
    // The session cap: the `--sessions` count, or the build cap over the goals derived
    // so far. Mirrors docs/compiler/sessions.md#budgets.
    fn cap(&self, t: &Tally) -> usize {
        self.max_sessions.unwrap_or(
            crate::limits::BUILD_SESSION_FACTOR * t.derived_ids.len()
                + crate::limits::BUILD_SESSION_FLOOR,
        )
    }

    // A selection widened since the last derivation applies from here on.
    fn widen(&self, store: &Store, board: &mut Board) {
        if let Some(handle) = crate::selection::handle() {
            let widened = handle.take_widened();
            if !widened.is_empty() {
                for added in &widened {
                    self.trace.event(TraceEvent::Selection {
                        label: "build".into(),
                        added: if added.is_empty() {
                            serde_json::Value::Null
                        } else {
                            json!(added)
                        },
                        selection: json!(handle.items()),
                    });
                }
                *board = Board::derive(store, self.proj, self.control);
            }
        }
    }

    // The batches the loop may open next, in the order it prefers them: a GC burst
    // first, unless the cap is tight, when compile outranks GC and GC waits.
    // Mirrors docs/compiler/reconciler.md#scheduling.
    fn candidates(&self, board: &Board, t: &Tally) -> Vec<crate::board::Batch> {
        let runnable = |b: &crate::board::Batch| {
            !crate::workflow::graph_only()
                || b.goals.iter().all(|id| {
                    board
                        .goals
                        .iter()
                        .find(|g| &g.id == id)
                        .is_some_and(|g| !crate::board::LEDGER_KINDS.contains(&g.kind.as_str()))
                })
        };
        let compile: Vec<crate::board::Batch> = board
            .batches
            .iter()
            .filter(|b| b.class == goals::Class::Compile && runnable(b))
            .cloned()
            .collect();
        let gc = board
            .batches
            .iter()
            .filter(|b| b.class == goals::Class::Gc && runnable(b))
            .cloned();
        let remaining = self.cap(t).saturating_sub(t.sessions as usize);
        if remaining <= compile.len() {
            compile
        } else {
            gc.chain(compile).collect()
        }
    }

    fn exhausted(&self, t: &Tally) -> bool {
        t.sessions as usize >= self.cap(t) || t.dead_endpoint >= crate::limits::ENDPOINT_BREAKER
    }

    // Exhaustion parks every open goal the loop could still run.
    fn exhaust(&self, t: &mut Tally, board: &Board) {
        let cap = self.cap(t);
        let reason = if t.dead_endpoint >= crate::limits::ENDPOINT_BREAKER {
            format!(
                "the endpoint answers only errors ({}); the build stopped early",
                t.endpoint_error
            )
        } else if self.max_sessions.is_some() {
            format!("the session limit of {} (--sessions) ran out", cap)
        } else {
            format!("the build cap of {} sessions ran out", cap)
        };
        for g in board.selected_open_goals() {
            if !t.parked_ids.contains(&g.id) {
                t.parked_ids.insert(g.id.clone());
                t.parked.push(ParkedGoal::new(g.clone(), reason.clone()));
                self.trace.event(TraceEvent::Goal {
                    label: "build".into(),
                    goal: g.id.clone(),
                    event: "parked".into(),
                    cause: None,
                    justification: None,
                    reason: Some(reason.clone()),
                });
            }
        }
    }

    // Open one batch: the graph as it sees it, its trace events, and the run its
    // session gets, at the generation it starts from.
    fn open(&self, board: &Board, batch: &crate::board::Batch) -> ActiveBatch {
        // The graph as this batch sees it, for replays (docs/compiler/sessions.md#snapshots).
        crate::store::snapshot_graph(self.out, &batch.id);
        self.trace.event(TraceEvent::BatchStart {
            label: batch.id.clone(),
            class: batch.class.name().into(),
            tier: batch.tier,
            goals: batch
                .goals
                .iter()
                .filter_map(|id| board.goal(id))
                .map(|g| json!({"id": g.id, "kind": g.kind, "target": g.target}))
                .collect(),
            executor: batch.executor.clone(),
        });
        if batch.class == goals::Class::Gc {
            for g in batch.goals.iter().filter_map(|id| board.goal(id)) {
                let (count, limit, what) = burst_measure(g);
                self.trace.event(TraceEvent::GcBurst {
                    label: batch.id.clone(),
                    goal_kind: g.kind.clone(),
                    target: g.target.clone(),
                    count,
                    limit,
                    detail: what,
                });
            }
        }
        let generation = crate::store::read_generation(self.out);
        // One session per batch: the runner resolves the executor, ships the
        // assembled prompt, and the serving commits under its own gates.
        ActiveBatch {
            records: batch
                .goals
                .iter()
                .map(|id| (id.clone(), board.records_of(id)))
                .collect(),
            run: batch_run(board, batch),
            class: batch.class.name().into(),
            tier: batch.tier,
            generation,
            report: None,
        }
    }

    // Fold one finished session into the build: its spend, the breaker, the store
    // catching up with its commits, and each of its goals resolved, failed, retried,
    // or parked; then the reopen cycles and the goals its commits opened. With
    // `own_entries`, other sessions of this build committed beside it, so only the
    // journal entries of this batch take its spend and its opened goals go to the
    // entry that caused them. Mirrors docs/compiler/reconciler.md#scheduling.
    fn settle(
        &self,
        t: &mut Tally,
        store: &mut Store,
        board: &mut Board,
        active: &ActiveBatch,
        report: &SessionReport,
        own_entries: bool,
    ) -> u64 {
        let (proj, out, trace, control) = (self.proj, self.out, self.trace, self.control);
        let batch_id = active.run.id.clone();
        let batch_goals = active.run.goal_ids();
        let gen_before = active.generation;
        t.applied += report.applied;
        t.costs
            .charge(&active.run.kind(), &active.run.class(), report.tokens);
        // Count consecutive failures that spent nothing: an endpoint answering
        // only errors, not sessions worth retrying. Any success or any spend
        // resets the streak. Mirrors docs/compiler/reconciler.md#escalation.
        let endpoint_down;
        (t.dead_endpoint, endpoint_down) =
            breaker_step(t.dead_endpoint, report.tokens, report.failed.as_deref());
        if endpoint_down {
            if let Some(e) = report.failed.as_ref() {
                t.endpoint_error = e.clone();
            }
        }
        if let Some(e) = report.failed.clone().filter(|_| !trace.is_cancelled()) {
            trace.event(TraceEvent::SessionFailed {
                label: batch_id.clone(),
                goals: batch_goals.clone(),
                attempt: t.attempts.get(&batch_goals[0]).copied().unwrap_or(0) + 1,
                error: e,
            });
        } else if report.applied == 0 {
            trace.line(&batch_id, "no mutations staged");
        }
        // The session's commits live on disk; the store catches up, the sweep runs, the
        // board re-derives, and the goals the commit opened are recorded on its entry.
        // The turn's measured spend lands on the entries it wrote: the commit itself
        // happened inside the serving, before the cost was known
        // (docs/compiler/graph.md#journal).
        crate::store::Store::stamp_tokens_of(
            out,
            gen_before,
            crate::store::read_generation(out),
            report.tokens,
            report.cached,
            own_entries.then_some(batch_goals.as_slice()),
        );
        reload(store, out, proj);
        sweep(store, trace);
        crate::derive::record_ledger_stale(store, &crate::gen::GenSettings::resolve(proj));
        *board = Board::derive(store, proj, control);
        absorb_derivation(store, board);
        let gen_after = crate::store::read_generation(out);
        // Only the entries this session wrote (docs/compiler/graph.md#journal).
        let written = goals::read_journal_range(out, gen_before, gen_after);
        let resolved_in_journal: BTreeMap<String, String> = written
            .iter()
            .flat_map(|e| {
                e.resolved_goals
                    .iter()
                    .map(|r| (r.goal.clone(), r.justification.clone()))
            })
            .collect();
        for id in &batch_goals {
            let still_open = board.open(id);
            let resolved = resolved_in_journal.contains_key(id);
            // A pair the commit sent back to waiting under the cap is unresolved,
            // not gone: its records stand and it opens again when a slot frees.
            // Mirrors docs/compiler/reconciler.md#pairs.
            if !still_open && !resolved && board.waiting.contains_key(id) {
                trace.event(TraceEvent::Goal {
                    label: batch_id.clone(),
                    goal: id.clone(),
                    event: "waiting".into(),
                    cause: None,
                    justification: None,
                    reason: Some("sent back to waiting under the pair degree cap".into()),
                });
                *t.attempts.entry(id.clone()).or_insert(0) += 1;
                continue;
            }
            if !still_open || resolved {
                let justification = resolved_in_journal
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| "the goal no longer derives".into());
                trace.event(TraceEvent::Goal {
                    label: batch_id.clone(),
                    goal: id.clone(),
                    event: "resolved".into(),
                    cause: None,
                    justification: Some(justification),
                    reason: None,
                });
                let ids = active.records.get(id).cloned().unwrap_or_default();
                if !ids.is_empty() {
                    store.clear_resolved(&ids);
                }
                if let Some(g) = active.run.goals.iter().find(|goal| goal.id == *id) {
                    t.resolved_changes.insert(id.clone(), g.change.clone());
                }
                // Resolving a parked goal clears its entry with its record; the
                // tail only adds and replaces. Mirrors docs/compiler/reconciler.md#parked-and-failed.
                store.status.parked.retain(|p| p.goal.id != *id);
                t.attempts.remove(id);
                continue;
            }
            if let Some(GoalState::Failed { reason }) = board.goal(id).map(|g| g.state.clone()) {
                trace.event(TraceEvent::Goal {
                    label: batch_id.clone(),
                    goal: id.clone(),
                    event: "failed".into(),
                    cause: None,
                    justification: None,
                    reason: Some(reason),
                });
                continue;
            }
            // A session that spent nothing never saw its goals: their retry stays
            // theirs (docs/compiler/reconciler.md#escalation).
            if endpoint_down {
                continue;
            }
            if trace.is_cancelled() {
                continue;
            }
            let n = t.attempts.entry(id.clone()).or_insert(0);
            *n += 1;
            if *n >= crate::limits::GOAL_ATTEMPTS {
                if let Some(g) = board.goal(id) {
                    let reason = format!("left open after {} attempts in session {}", n, batch_id);
                    t.parked_ids.insert(id.clone());
                    t.parked.push(ParkedGoal::new(g.clone(), reason.clone()));
                    trace.event(TraceEvent::Goal {
                        label: batch_id.clone(),
                        goal: id.clone(),
                        event: "parked".into(),
                        cause: None,
                        justification: None,
                        reason: Some(reason),
                    });
                }
            }
        }
        if !resolved_in_journal.is_empty() || !board.lapsed.is_empty() {
            reload(store, out, proj);
            *board = Board::derive(store, proj, control);
        }
        // A resolved goal back on the board with the change it was resolved under is a
        // cycle: the commit recomputed the record behind it and a session would mark
        // the same work done again. The second cycle parks it without spending the
        // remaining unfinished-goal attempts. A different change is new work under an old id
        // and starts the count over. Mirrors docs/compiler/reconciler.md#parked-and-failed.
        for g in board.open_goals() {
            let Some(change) = t.resolved_changes.remove(&g.id) else {
                continue;
            };
            if change != g.change {
                t.cycles.remove(&g.id);
                continue;
            }
            let n = match t.cycles.get(&g.id) {
                Some((c, n)) if *c == g.change => n + 1,
                _ => 1,
            };
            t.cycles.insert(g.id.clone(), (g.change.clone(), n));
            trace.event(TraceEvent::Goal {
                label: batch_id.clone(),
                goal: g.id.clone(),
                event: "reopened".into(),
                cause: None,
                justification: None,
                reason: Some(format!(
                    "re-derived with the same change after its resolution ({} of {})",
                    n,
                    crate::limits::REOPEN_CYCLES
                )),
            });
            if n >= crate::limits::REOPEN_CYCLES && !t.parked_ids.contains(&g.id) {
                let reason = format!(
                    "reopened-after-resolve: resolved and re-derived with the same change {} times in this build",
                    n
                );
                t.parked_ids.insert(g.id.clone());
                t.parked.push(ParkedGoal::new(g.clone(), reason.clone()));
                trace.event(TraceEvent::Goal {
                    label: batch_id.clone(),
                    goal: g.id.clone(),
                    event: "parked".into(),
                    cause: None,
                    justification: None,
                    reason: Some(reason),
                });
            }
        }
        let opened: Vec<OpenedGoal> = board
            .open_goals()
            .iter()
            .filter(|g| !t.known.contains(&g.id))
            .map(|g| OpenedGoal {
                goal: g.id.clone(),
                cause: g.cause.clone().unwrap_or_default(),
            })
            .collect();
        if !opened.is_empty() {
            let sessions_written = written.iter().filter(|e| e.kind == "session");
            let last_session = if own_entries {
                sessions_written
                    .clone()
                    .filter(|e| e.batch.iter().any(|g| batch_goals.contains(g)))
                    .map(|e| e.generation)
                    .max()
            } else {
                sessions_written.clone().map(|e| e.generation).max()
            }
            .unwrap_or(gen_after);
            for o in &opened {
                trace.event(TraceEvent::Goal {
                    label: batch_id.clone(),
                    goal: o.goal.clone(),
                    event: "opened".into(),
                    cause: Some(o.cause.clone()),
                    justification: None,
                    reason: None,
                });
            }
            if own_entries {
                // Another session's commit in the range opened its own goals: each
                // goal lands on the entry its cause names.
                let mut by_entry: BTreeMap<u64, Vec<OpenedGoal>> = BTreeMap::new();
                for o in opened {
                    let entry = if sessions_written
                        .clone()
                        .any(|e| e.generation == o.cause.generation)
                    {
                        o.cause.generation
                    } else {
                        last_session
                    };
                    by_entry.entry(entry).or_default().push(o);
                }
                for (generation, opened) in by_entry {
                    store.record_opened_goals(generation, opened);
                }
            } else {
                store.record_opened_goals(last_session, opened);
            }
        }
        t.known = board.open_goals().iter().map(|g| g.id.clone()).collect();
        // The session already printed its own done line; this is the board after the
        // commit and the re-derivation, one note per batch.
        trace.event(TraceEvent::Note {
            label: batch_id.clone(),
            text: format!(
                "board: {} open, {} blocked",
                board.counts().open,
                board.counts().blocked
            ),
            verbose: false,
        });
        gen_after
    }
}

// The run one batch's session gets: its goals as the board holds them and the
// scheduler's executor hint.
fn batch_run(board: &Board, batch: &crate::board::Batch) -> crate::runner::BatchRun {
    crate::runner::BatchRun {
        id: batch.id.clone(),
        goals: batch
            .goals
            .iter()
            .filter_map(|id| board.goal(id))
            .cloned()
            .collect(),
        executor: batch.executor.clone(),
    }
}

// The interruption a session's report names, when the session stopped for the
// process rather than for its goals.
fn interruption(report: &SessionReport) -> Option<String> {
    report
        .stopped
        .iter()
        .chain(report.failed.iter())
        .find(|reason| crate::session_recovery::is_interruption(reason))
        .cloned()
}

// The session loop of a build: derive the board, run sessions on its batches, highest
// ready tier first, until no batch is runnable or a budget is spent. The store is
// synced and swept; the runner is whatever runs a batch (an ACP worker, the
// in-process loop, a test's stub). A runner that runs sessions concurrently gets up to
// its executors' `max_sessions` at once; any other runs one at a time.
// Mirrors docs/compiler/compilation.md#a-build and docs/compiler/reconciler.md#scheduling.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub(crate) async fn run_sessions(
    proj: &Project,
    out: &Path,
    trace: &Trace,
    max_sessions: Option<usize>,
    runner: &dyn crate::runner::SessionRunner,
    store: &mut Store,
    parsed: &ParsedDocs,
    control: &Control,
) -> SessionsRun {
    run_sessions_recorded(
        proj,
        out,
        trace,
        max_sessions,
        runner,
        store,
        parsed,
        control,
        None,
    )
    .await
}

// The session loop, with the build's run file kept current when it has one
// (docs/compiler/graph.md#a-builds-progress).
#[allow(clippy::too_many_arguments)]
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub(crate) async fn run_sessions_recorded(
    proj: &Project,
    out: &Path,
    trace: &Trace,
    max_sessions: Option<usize>,
    runner: &dyn crate::runner::SessionRunner,
    store: &mut Store,
    _parsed: &ParsedDocs,
    control: &Control,
    mut record: Option<&mut crate::control::RunRecord>,
) -> SessionsRun {
    let command_key = continuation::key(out);
    let mut saved = match Continuation::load(&command_key, max_sessions) {
        Ok(saved) => saved.unwrap_or_else(|| Continuation::fresh(max_sessions)),
        Err(reason) => {
            return SessionsRun {
                derived: 0,
                sessions: 0,
                applied: 0,
                costs: Costs::default(),
                parked: Vec::new(),
                suspend_reason: Some(reason),
            }
        }
    };
    // Resume the exact saved batch before deriving any new work. Its committed
    // journal is authoritative when the worker's report did not reach the parent.
    if let Some(active) = saved.active.as_mut() {
        if active.report.is_none() {
            let report = match active.committed_report(out) {
                Some(report) => report,
                None => runner.run_item(&active.run, trace).await,
            };
            if let Some(reason) = interruption(&report) {
                return SessionsRun {
                    derived: saved.derived_ids.len(),
                    sessions: saved.sessions,
                    applied: saved.applied,
                    costs: saved.costs,
                    parked: saved.parked,
                    suspend_reason: Some(reason),
                };
            }
            active.report = Some(report);
            if let Err(reason) = saved.save(&command_key).await {
                return SessionsRun {
                    derived: saved.derived_ids.len(),
                    sessions: saved.sessions,
                    applied: saved.applied,
                    costs: saved.costs,
                    parked: saved.parked,
                    suspend_reason: Some(reason),
                };
            }
        }
    }
    crate::derive::record_ledger_stale(store, &crate::gen::GenSettings::resolve(proj));
    let mut board = Board::derive(store, proj, control);
    absorb_derivation(store, &board);
    let selection = crate::selection::current();
    trace.event(TraceEvent::Board {
        label: "build".into(),
        goals: board.selected_open_goals().len(),
        kinds: if selection.is_some() {
            selected_kinds_line(&board)
        } else {
            kinds_line(&board)
        },
        blocked: board.counts().blocked,
        selection: selection.as_ref().map(|items| json!(items)),
    });
    let mut t = Tally::take(&mut saved);
    t.derived_ids.extend(
        board
            .selected_open_goals()
            .iter()
            .map(|goal| goal.id.clone()),
    );
    if t.known.is_empty() {
        t.known = board
            .open_goals()
            .iter()
            .map(|goal| goal.id.clone())
            .collect();
    }
    let lp = Loop {
        proj,
        out,
        trace,
        control,
        max_sessions,
    };
    let mut active = saved.active.take();
    let mut suspend_reason = None;
    macro_rules! persist {
        () => {{
            t.put(&mut saved);
            saved.active = active.clone();
            #[cfg(not(target_arch = "wasm32"))]
            {
                saved.save(&command_key)
            }
            #[cfg(target_arch = "wasm32")]
            {
                saved.save(&command_key).await
            }
        }};
    }
    // A resumable build, or a runner that serves one session at a time, runs the
    // sequential loop below. Mirrors docs/compiler/reconciler.md#scheduling.
    #[cfg(not(target_arch = "wasm32"))]
    let concurrent =
        if active.is_none() && !continuation::enabled() && runner.max_sessions_any() > 1 {
            runner.concurrent()
        } else {
            None
        };
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(runner) = concurrent {
        // The build's worker id registers each batch it opens as open work.
        let worker = record.as_ref().map_or_else(
            || format!("internal-{}-loop", crate::sys::process_id()),
            |r| r.run().to_string(),
        );
        suspend_reason =
            concurrent::run(&lp, runner, store, &mut board, &mut t, &mut record, &worker);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let sequential = concurrent.is_none();
    #[cfg(target_arch = "wasm32")]
    let sequential = true;
    if sequential {
        loop {
            if active.is_none() {
                if crate::session_recovery::suspension_requested() {
                    suspend_reason = Some(match persist!() {
                        Ok(()) => "suspended".into(),
                        Err(reason) => reason,
                    });
                    break;
                }
                lp.widen(store, &mut board);
                t.derived_ids
                    .extend(board.selected_open_goals().iter().map(|g| g.id.clone()));
                // Goals parked this build take no batch; their neighbors still run.
                board.withhold(&t.parked_ids, store, proj);
                let Some(batch) = lp.candidates(&board, &t).into_iter().next() else {
                    // A selection widened during the last batch runs before the build
                    // ends; otherwise the selection closes and later requests queue.
                    match crate::selection::handle() {
                        Some(h) if !h.seal_unless_widened() => continue,
                        _ => break,
                    }
                };
                if trace.is_cancelled() {
                    break;
                }
                if lp.exhausted(&t) {
                    lp.exhaust(&mut t, &board);
                    break;
                }
                let opened = lp.open(&board, &batch);
                t.sessions += 1;
                active = Some(opened);
                if let Err(reason) = persist!() {
                    suspend_reason = Some(reason);
                    break;
                }
            }
            let current = active.as_ref().expect("selected batch");
            t.record(&mut record, vec![in_flight_line(current)]);
            let report = match current.report.clone() {
                Some(report) => report,
                None => runner.run_item(&current.run, trace).await,
            };
            if let Some(reason) = interruption(&report) {
                suspend_reason = Some(reason);
                break;
            }
            active.as_mut().unwrap().report = Some(report.clone());
            if let Err(reason) = persist!() {
                suspend_reason = Some(reason);
                break;
            }
            let current = active.take().expect("selected batch");
            let generation = lp.settle(&mut t, store, &mut board, &current, &report, false);
            saved.completed.push(Receipt {
                session: t.sessions,
                batch: current.run.id.clone(),
                generation,
                report,
            });
            crate::session_recovery::remove_frame(&crate::session_recovery::batch_key(
                out,
                &current.run.id,
            ));
            t.record(&mut record, Vec::new());
            if let Err(reason) = persist!() {
                suspend_reason = Some(reason);
                break;
            }
        }
    }
    if suspend_reason.is_none() {
        if let Err(reason) = persist!() {
            suspend_reason = Some(reason);
        }
    }
    // The sessions are over: a later selection needs its own build.
    if let Some(h) = crate::selection::handle() {
        h.seal();
    }

    SessionsRun {
        derived: t.derived_ids.len(),
        sessions: t.sessions,
        applied: t.applied,
        costs: t.costs,
        parked: t.parked,
        suspend_reason,
    }
}

// The deterministic sweep (garbage collection and both settles), traced. Every
// commit already runs it behind its own entry (Store::apply); the build runs it here
// after alignment, which commits no changeset, and after each session so the trace
// says what a session's commit swept. Mirrors docs/compiler/graph.md#the-sweep.
fn sweep(store: &mut Store, trace: &Trace) {
    let actions = store.sweep();
    if !actions.is_empty() {
        trace.line("build", &format!("sweep: {}", actions.join("; ")));
    }
}

// The deterministic tail of a build: the checks, flip detection, the verdict with its
// counts, costs, rendering, docsgen, status. No model anywhere, so whichever consumer
// empties the board runs it, the internal loop and the MCP serving alike.
// Mirrors docs/compiler/compilation.md#a-build.
// Session-owned tasks no ledger uses leave the task list when a build closes.
// Mirrors docs/consumers/gen.md#session-tasks-are-swept-when-unused.
pub fn sweep_tasks(proj: &Project, out: &Path, trace: &Trace) -> Vec<String> {
    let removed = crate::task_runtime::sweep_unused(&proj.root, Some(out)).unwrap_or_else(|e| {
        trace.line("build", &format!("task sweep skipped: {}", e));
        Vec::new()
    });
    if !removed.is_empty() {
        trace.line("build", &format!("tasks retired: {}", removed.join(", ")));
    }
    removed
}

pub fn finalize(
    s: &mut Store,
    proj: &Project,
    parked: Vec<ParkedGoal>,
    costs: Option<(&str, &Costs)>,
    trace: &Trace,
) -> BuildReport {
    // The tail lands the build's own spend beside the verdict; a tail with no build
    // (an MCP consumer's) lands only the token delta.
    // Mirrors docs/compiler/graph.md#storage-layout.
    let own_tokens = costs.map(|(_, c)| c.tokens);
    let write_tail = |s: &mut Store, what: &str| -> Option<String> {
        let tokens = crate::llm::take_tokens_delta();
        let why = s.write_status_costs(what, costs, false, tokens).err()?;
        trace.line("build", &format!("{} not saved: {}", what, why));
        Some(why)
    };
    // Parked goals persist whole, deduplicated by id; a cancelled build keeps the
    // ones it parked before the stop. Mirrors docs/compiler/reconciler.md#parked-and-failed.
    let keep_parked = |s: &mut Store, parked: Vec<ParkedGoal>| {
        for g in parked {
            match s.status.parked.iter_mut().find(|p| p.goal.id == g.goal.id) {
                Some(p) => *p = g,
                None => s.status.parked.push(g),
            }
        }
    };
    if trace.is_cancelled() {
        keep_parked(s, parked);
        s.status
            .parked
            .retain(|p| !s.status.failed.iter().any(|f| f.goal.id == p.goal.id));
        let control = Control::load(proj, &s.out);
        s.status.verdict = Board::derive(s, proj, &control).verdict();
        s.status.verdict.state = "incomplete".into();
        let unsaved = write_tail(s, "stopped build");
        trace.line("build", "stopped; completed commits retained");
        return BuildReport {
            unsaved,
            verdict: s.status.verdict.to_string(),
            tokens: own_tokens.unwrap_or(s.status.costs.tokens),
            failed: s.status.verdict.failed as usize,
            blocked: s.status.verdict.blocked as usize,
            optional: s.status.verdict.optional as usize,
            ..Default::default()
        };
    }
    let settled = s.settle_dangling_diags();
    if !settled.is_empty() {
        trace.line("build", &format!("settle: {}", settled.join("; ")));
    }
    // Flip detection adds its pairs to the build's parks.
    let (flips, flipped) = flip_detection(s);
    keep_parked(s, parked.into_iter().chain(flipped).collect());
    s.status
        .parked
        .retain(|p| !s.status.failed.iter().any(|f| f.goal.id == p.goal.id));
    let mut findings = checks(s, proj);
    findings.extend(flips);
    s.reconcile_check_diags(findings);

    // The verdict: the board after the checks, blocked and optional riding as counts.
    crate::derive::record_ledger_stale(s, &crate::gen::GenSettings::resolve(proj));
    let control = Control::load(proj, &proj.out);
    let board = Board::derive(s, proj, &control);
    absorb_derivation(s, &board);
    // A pending flip ruling holds its pair `blocked` on a human, so it rides in the
    // blocked count and never keeps the build from converging.
    // Mirrors docs/compiler/compilation.md#convergence.
    s.status.verdict = board.verdict();

    let n = crate::docsgen::write_all(s, &crate::gen::GenSettings::resolve(proj));
    if n > 0 {
        trace.line("build", &format!("docsgen: {} page(s)", n));
    }
    let rendered = crate::render::render_all(s, &s.out);
    if !rendered.rendered.is_empty() || !rendered.failed.is_empty() {
        trace.line(
            "build",
            &format!(
                "render: {} view(s) drawn, {} unchanged, {} failed",
                rendered.rendered.len(),
                rendered.skipped.len(),
                rendered.failed.len()
            ),
        );
        for (id, e) in &rendered.failed {
            trace.line("build", &format!("render failed: {}: {}", id, e));
        }
    }
    // The tail adds this build's process delta: spent.tokens is cumulative across
    // builds and processes, and llm::tokens_spent() is a process-lifetime counter, so
    // an assignment would clobber the spend of builds this process did not run.
    s.status.diagnostics = s.open_diag_counts();
    let unsaved = write_tail(s, "verdict");
    trace.line("build", &s.status.verdict.to_string());

    let (mut errors, mut warnings) = (0usize, 0usize);
    for d in s.graph.diagnostics.values() {
        if d.lifecycle == "open" && d.triage.as_deref() != Some("suppressed") {
            match d.severity.as_str() {
                "error" => errors += 1,
                "warning" => warnings += 1,
                _ => {}
            }
        }
    }
    let (mut total_secs, mut covered_secs) = (0usize, 0usize);
    for rec in s.docs.values() {
        for (r, sec) in &rec.sections {
            if !goals::section_has_body(sec) {
                continue;
            }
            total_secs += 1;
            if rec.coverage.contains_key(r) {
                covered_secs += 1;
            }
        }
    }
    let v = &s.status.verdict;
    BuildReport {
        verdict: v.to_string(),
        goals: 0,
        sessions: 0,
        applied: 0,
        parked: s.status.parked.len(),
        failed: v.failed as usize,
        blocked: v.blocked as usize,
        optional: v.optional as usize,
        errors,
        warnings,
        coverage_pct: (covered_secs * 100)
            .checked_div(total_secs)
            .map(|pct| pct as u32)
            .unwrap_or(100),
        tokens: own_tokens.unwrap_or(s.status.costs.tokens),
        suspend_reason: None,
        retired_tasks: Vec::new(),
        unsaved,
    }
}

// One session's step of the dead-endpoint breaker: a failure that spent nothing is the
// endpoint's, not the goals', and advances the streak (a rate limit to the breaker at
// once, since a quota does not reset between sessions); anything else resets it.
// Returns the new streak and whether the endpoint was down.
// Mirrors docs/compiler/reconciler.md#escalation.
pub(crate) fn breaker_step(streak: usize, tokens: u64, failed: Option<&str>) -> (usize, bool) {
    match failed {
        Some(e) if tokens == 0 => {
            if rate_limited(e) || context_overflow(e) {
                (crate::limits::ENDPOINT_BREAKER, true)
            } else {
                (streak + 1, true)
            }
        }
        _ => (0, false),
    }
}

// An error saying the prompt does not fit the model's context: the fixed part of a
// session's prompt is the same for every batch, so a retry cannot help and the build
// stops at the first one (a browser's built-in model failed every batch of a sync
// with "The input is too large."). Mirrors docs/compiler/reconciler.md#escalation.
pub(crate) fn context_overflow(error: &str) -> bool {
    let e = error.to_lowercase();
    e.contains("too large")
        || e.contains("too long")
        || e.contains("context length")
        || e.contains("maximum context")
        || e.contains("exceeds the context")
        || e.contains("context window")
}

// An error naming a rate or quota limit: HTTP 429, "rate limit", "usage limit", "quota".
// Mirrors docs/compiler/reconciler.md#escalation.
pub(crate) fn rate_limited(error: &str) -> bool {
    let e = error.to_lowercase();
    e.contains("http 429")
        || e.contains(" 429 ")
        || e.contains("rate limit")
        || e.contains("rate_limit")
        || e.contains("usage limit")
        || e.contains("hit your limit")
        || e.contains("quota")
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod input_reload_tests {
    use super::*;

    #[test]
    fn reload_keeps_current_classification_and_coverage() {
        let files = std::collections::BTreeMap::from([
            (
                ".jazyk/jazyk.toml".to_string(),
                b"version=1\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['**']\n".to_vec(),
            ),
            (
                "article.md".to_string(),
                b"# Article\n\nSupplied content.\n".to_vec(),
            ),
        ]);
        let _fs = crate::vfs::Scope::enter(&files);
        let project = crate::document_sets::Workspace::load(Path::new("/project"))
            .unwrap()
            .resolve(Some("ds_main"))
            .unwrap();
        let out = project.out.clone();
        let mut store = Store::load(&out);
        store.sync_docs(&parse_all(&project).0);
        assert!(store.docs["article.md"].sections.contains_key("/article"));
        store.decisions.inputs.insert(
            "article.md".into(),
            crate::model::InputDecision {
                mode: "asset".into(),
                sha256: crate::task_runtime::digest(&files["article.md"]),
                reason: "Supplied article.".into(),
            },
        );
        store.save_for_test();
        store.sync_docs(&parse_all(&project).0);
        store.docs.get_mut("article.md").unwrap().coverage.insert(
            "/".into(),
            crate::model::Coverage {
                state: "non-normative".into(),
                note: Some("Reference material.".into()),
                claimed_by: None,
            },
        );
        store.save_for_test();
        reload(&mut store, &out, &project);
        assert_eq!(store.docs["article.md"].sections["/"].kind, "blob");
        assert_eq!(
            store.docs["article.md"].coverage["/"].state,
            "non-normative"
        );
        assert!(!store.docs["article.md"].sections.contains_key("/article"));
    }
}
