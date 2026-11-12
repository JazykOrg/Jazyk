use super::*;

// Report existing task evidence separately from selected ledger verdicts.
// Atomic case identities stay exact; oversized/excess entries are counted only.
pub(super) fn suite_summary(runs: &[Value]) -> Value {
    let (mut passed, mut failed, mut skipped, mut other, mut unsuccessful) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut bytes, mut omitted) = (2usize, 0u64);
    let mut failures = Vec::new();
    for run in runs {
        if run["state"] != "succeeded" {
            unsuccessful += 1;
        }
        for case in run["tests"].as_array().into_iter().flatten() {
            match case["status"].as_str() {
                Some("passed") => passed += 1,
                Some("skipped") => skipped += 1,
                Some("failed") => {
                    failed += 1;
                    let failure = json!({"runId":run["id"],"task":run["task"],"identity":case["identity"],"status":case["status"],"report":case["report"]});
                    let size = failure.to_string().len() + usize::from(!failures.is_empty());
                    if failures.len() < 20 && bytes.saturating_add(size) <= 32 * 1024 {
                        bytes += size;
                        failures.push(failure);
                    } else {
                        omitted += 1;
                    }
                }
                _ => other += 1,
            }
        }
    }
    json!({"passedCases":passed,"failedCases":failed,"skippedCases":skipped,"otherCases":other,"unsuccessfulRuns":unsuccessful,"failures":failures,"omittedFailures":omitted})
}

pub(super) fn verification_next(
    failed: u64,
    runner_failed: u64,
    rows: &[Value],
    skipped: &[Value],
    document_findings: bool,
    generated_candidate_failure: bool,
) -> &'static str {
    if failed > 0 && document_findings {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/verify-select/a-failing-row-has-an-open-document-680d5.md"
        ))
    } else if failed > 0 && generated_candidate_failure {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/verify-select/a-generated-candidate-failed-current-checks-compare-47078.md"
        ))
    } else if failed > 0 {
        "a failing row names its requirement: compare its documented obligation, current implementation and test, then follow its status and permitted repair tools. Test failure alone does not prove the documents are wrong; a document contradiction needs a diagnostic and an author decision"
    } else if runner_failed > 0 {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/verify-select/a-runner-failed-row-was-never-judged-d0872.md"
        ))
    } else if rows.iter().any(|row| row["verdict"] == "none") {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/verify-select/some-cases-remain-unverified-inspect-their-report-f3b72.md"
        ))
    } else if !skipped.is_empty() {
        "skipped rows say why; llm rows need begin_verification and record_verdict"
    } else if rows.is_empty() {
        "no selected tests ran; inspect the target and its bindings"
    } else {
        "all selected rows pass"
    }
}

// The `run_tests` tool: the build once, then every selected programmatic row, verdicts
// recorded as a side effect. No model anywhere, so any toolset can serve it; `llm` rows
// are listed as skipped and go through record_verdict.
// Mirrors docs/compiler/tools.md#verification-tools.
pub fn run_selected(store: &Store, gs: &GenSettings, targets: &[String]) -> Result<Value, String> {
    let quiet = crate::session::Trace::stderr(crate::session::TraceLevel::Quiet);
    crate::gen::run_build(&store.out, gs, &quiet, "run_tests")
        .map_err(|e| format!("{}; nothing was verified", e))?;
    run_selected_after_build(store, gs, targets)
}

pub(crate) fn run_selected_after_build(
    store: &Store,
    gs: &GenSettings,
    targets: &[String],
) -> Result<Value, String> {
    let ledger = Ledger::load(&store.out);
    // Explicit targets (requirement or entity ids), or every non-verified row, the
    // failing ones included: an explicit run is a rerun a worker asked for.
    let selected: Vec<String> = if targets.is_empty() {
        pending(store, gs, Some("runnable"), None)
            .iter()
            .filter_map(|p| p["requirement"].as_str().map(String::from))
            .collect()
    } else {
        let mut v: Vec<String> = Vec::new();
        for t in targets {
            let id = store.resolve_id(t).to_string();
            if ledger.requirements.contains_key(&id) || store.graph.requirements.contains_key(&id) {
                v.push(id);
            } else {
                for (rid, requirement) in &store.graph.requirements {
                    if requirement
                        .entities
                        .iter()
                        .any(|entity| store.resolve_id(entity) == id)
                        && !v.contains(rid)
                    {
                        v.push(rid.clone());
                    }
                }
                for (rid, row) in &ledger.requirements {
                    if store.resolve_id(&row.entity) == id && !v.contains(rid) {
                        v.push(rid.clone());
                    }
                }
            }
        }
        v
    };
    let mut tasks = std::collections::BTreeSet::new();
    for rid in &selected {
        let Some(row) = ledger.requirements.get(rid) else {
            continue;
        };
        let (status, _) = status_of(store, rid, row, gs);
        if status == "missing" || status == "stale-requirement" {
            continue;
        }
        for test in std::iter::once(&row.test).chain(row.tests.values().map(|t| &t.test)) {
            if test.kind == "programmatic" {
                if let Some(id) = test.run.strip_prefix("task:") {
                    tasks.insert(id.to_owned());
                }
            }
        }
    }
    let mut runs = Vec::new();
    if !tasks.is_empty() {
        let root = crate::task_runtime::project_root(&store.out)?;
        for id in tasks {
            let (run_id, result) = crate::task_runtime::execute_with_key(
                &root,
                &id,
                &format!("verify:{}", store.out.display()),
            )?;
            runs.push(
                json!({"id":run_id,"task":id,"state":result["status"],"tests":result["tests"]}),
            );
        }
    }
    // Runs import independent case evidence. Reload before observing their rows.
    let ledger = Ledger::load(&store.out);
    let (mut passed, mut failed) = (0u64, 0u64);
    let mut rows: Vec<Value> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    // Failed runs, judged together after the loop.
    let mut held: Vec<(String, ProgRun)> = Vec::new();
    for rid in &selected {
        let Some(row) = ledger.requirements.get(rid) else {
            skipped.push(json!({"requirement": rid, "reason": "no ledger row: the requirement is not bound yet; binding_tasks lists it, begin_binding then record_binding"}));
            continue;
        };
        if let Err(reason) = ensure_current_source(store, gs, row) {
            skipped.push(json!({"requirement":rid,"reason":reason}));
            continue;
        }
        if row.test.kind != "programmatic" {
            // The headline is judged, not run; its programmatic criterion tests run
            // all the same, or three checks vanish behind a green reply
            // (docs/consumers/gen.md#tests-tie-criteria-to-the-deliverable).
            let crit_rows = run_criterion_tests(store, rid, gs);
            let ran: Vec<&Value> = crit_rows
                .iter()
                .filter(|c| c.get("note").is_none())
                .collect();
            if !ran.is_empty() {
                let crit_failed = ran.iter().any(|c| c["verdict"] == "fail");
                let crit_unknown = ran.iter().any(|c| c["verdict"] == "none");
                if crit_failed {
                    failed += 1
                } else if !crit_unknown {
                    passed += 1
                }
                rows.push(json!({"requirement": rid, "verdict": if crit_failed { "fail" } else if crit_unknown { "none" } else { "pass" }, "note": "the headline is an llm test (judge it with record_verdict); its programmatic criterion tests ran", "criteria": crit_rows}));
            } else {
                skipped.push(
                    json!({"requirement": rid, "reason": "llm row with no programmatic criterion test; judge it and record_verdict"}),
                );
            }
            continue;
        }
        let (status, reason) = status_with(store, rid, row, gs, &ledger);
        // A dead row is not repairable work; regenerating cannot bring its
        // requirement back (docs/consumers/gen.md#deletion-prunes-the-ledger).
        if reason == "requirement-gone" {
            skipped.push(json!({"requirement": rid, "reason": "requirement left the graph; the row is pruned at the next record_generation, nothing to verify"}));
            continue;
        }
        if status == "stale-requirement" || status == "missing" {
            skipped.push(json!({
                "requirement": rid,
                "reason": format!("{} ({}); {}", status, reason, repair_for(&status, &reason, rid, &row.entity)),
            }));
            continue;
        }
        if row.test.run.starts_with("task:") {
            let criteria = run_criterion_tests(store, rid, gs);
            let current = Ledger::load(&store.out);
            let (status, reason) = status_of(store, rid, &current.requirements[rid], gs);
            let verdict = match status.as_str() {
                "verified" => {
                    passed += 1;
                    "pass"
                }
                "failing" | "partial" | "unimplemented" => {
                    failed += 1;
                    "fail"
                }
                _ => "none",
            };
            rows.push(json!({"requirement":rid,"verdict":verdict,"status":status,"reason":reason,"criteria":criteria}));
            continue;
        }
        if crate::vfs::is_scoped() {
            let criteria = run_criterion_tests(store, rid, gs);
            skipped.push(json!({"requirement":rid,"reason":include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/verify-select/legacy-command-requires-a-native-runner-use-e5622.md")),"pendingCi":true,"criteria":criteria}));
            continue;
        }
        match run_programmatic(store, rid, gs) {
            Ok(r) if r.pass => {
                // A verdict that did not land is not a pass: the reply says what the
                // ledger holds, never what the run hoped to record.
                if let Err(e) = mark(
                    store,
                    rid,
                    "pass",
                    None,
                    Some(&r.evidence),
                    Some(r.code),
                    gs,
                ) {
                    skipped.push(json!({"requirement": rid, "reason": format!("the test passed but its verdict was not recorded: {e}"), "exitCode": r.code}));
                    continue;
                }
                // The criteria's own tests ride behind the headline.
                let crit_rows = run_criterion_tests(store, rid, gs);
                let crit_failed = crit_rows.iter().any(|c| c["verdict"] == "fail");
                let crit_unknown = crit_rows.iter().any(|c| c["verdict"] == "none");
                if crit_failed {
                    failed += 1;
                } else if !crit_unknown {
                    passed += 1;
                }
                rows.push(json!({"requirement": rid, "verdict": if crit_failed { "fail" } else if crit_unknown { "none" } else { "pass" }, "exitCode": r.code, "evidence": r.tail, "criteria": crit_rows}));
            }
            // A failure waits: whether it is the deliverable's or the machine's is
            // decided once, when the run knows how the other rows went
            // (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
            Ok(r) => held.push((rid.clone(), r)),
            Err(e) => skipped.push(json!({"requirement": rid, "reason": e})),
        }
    }
    // One broken runner says the same thing to every row it touched; unmet
    // requirements do not agree with each other. A run that never executed is
    // recorded as runner-failed, never as a failing requirement.
    let identical =
        held.len() > 1 && passed == 0 && held.windows(2).all(|w| w[0].1.tail == w[1].1.tail);
    let mut runner_failed = 0u64;
    for (rid, r) in held {
        if r.runner_failed() || identical {
            if let Err(e) = record_runner_failure(store, &rid, r.code, &r.evidence) {
                skipped.push(json!({"requirement": rid, "reason": format!("the runner failed and that was not recorded: {e}"), "exitCode": r.code}));
                continue;
            }
            runner_failed += 1;
            rows.push(json!({"requirement": rid, "verdict": "none", "reason": "runner-failed", "exitCode": r.code, "evidence": r.tail}));
            continue;
        }
        if let Err(e) = mark(
            store,
            &rid,
            "fail",
            None,
            Some(&r.evidence),
            Some(r.code),
            gs,
        ) {
            skipped.push(json!({"requirement": rid, "reason": format!("the test failed but its verdict was not recorded: {e}"), "exitCode": r.code, "evidence": r.tail}));
            continue;
        }
        failed += 1;
        rows.push(
            json!({"requirement": rid, "verdict": "fail", "exitCode": r.code, "evidence": r.tail}),
        );
    }
    // A failing row whose requirement sits inside an open judged diagnostic (a
    // contradiction, say) may be failing because the documents are wrong, not the
    // deliverable; the repair then belongs to the document author.
    let doc_diags: Vec<Value> = rows
        .iter()
        .filter(|r| r["verdict"] == "fail")
        .filter_map(|r| r["requirement"].as_str())
        .flat_map(|rid| {
            store
                .graph
                .diagnostics
                .iter()
                .filter(|(_, d)| {
                    // Any judged finding or open question on the statement: a decision
                    // or an invented choice names the documents as much as a
                    // contradiction does.
                    d.lifecycle == "open"
                        && (crate::tools::REVIEW_RULES.contains(&d.rule.as_str()) || d.prompt.is_some())
                        && d.subjects.iter().any(|s| s == rid)
                })
                .map(|(did, d)| json!({"requirement": rid, "diagnostic": did, "rule": d.rule, "message": d.message}))
                .collect::<Vec<Value>>()
        })
        .collect();
    // Use the current landed ledger after all verdict imports/legacy marks.
    // This changes guidance, not derived statuses or goal scheduling.
    let current = Ledger::load(&store.out);
    let generated_candidate_failure = rows
        .iter()
        .filter(|row| row["verdict"] == "fail")
        .filter_map(|row| row["requirement"].as_str())
        .filter_map(|rid| current.requirements.get(rid).map(|row| (rid, row)))
        .any(|(rid, row)| status_of(store, rid, row, gs).1 == "criteria-failing-after-generation");
    let mut reply = json!({
        "runs": runs,
        "suite": suite_summary(&runs),
        "passed": passed,
        "failed": failed,
        "runnerFailed": runner_failed,
        "rows": rows,
        "skipped": skipped,
        "next": verification_next(failed, runner_failed, &rows, &skipped, !doc_diags.is_empty(), generated_candidate_failure)
    });
    if !rows.is_empty()
        && skipped.is_empty()
        && rows.iter().all(|row| row["verdict"] == "pass")
        && (reply["suite"]["failedCases"].as_u64().unwrap_or(0) > 0
            || reply["suite"]["unsuccessfulRuns"].as_u64().unwrap_or(0) > 0)
    {
        reply["next"] = json!(include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/verify-select/all-selected-rows-pass-but-the-executed-dc793.md"
        )));
    }
    if skipped.iter().any(|r| r["pendingCi"] == true) {
        reply["pendingCi"] = json!(true);
        reply["pending"] = json!(skipped
            .iter()
            .filter(|r| r["pendingCi"] == true)
            .collect::<Vec<_>>());
    }
    if !doc_diags.is_empty() {
        reply["openDiagnosticsOnFailingRows"] = json!(doc_diags);
    }
    Ok(reply)
}

// ---- the built-in worker ----
// Moved out of the CLI so the GUI job runner drives the same loop. The CLI wrapper
// renders the worker events on the historical output format; the GUI streams them.

// Row selection shared by the worker and `jazyk test --list`: every row that can be
// rerun, failing ones included (all rows when forced), narrowed by targets (entity ids
// select their rows, requirement ids select directly) and by test kind. Each row
// carries its `reason` and `repair` beside the status, so a listing says why a row
// is stale and what clears it (docs/consumers/gen.md#status-is-derived-never-stored).
pub fn select_rows(
    store: &Store,
    gs: &GenSettings,
    targets: &[String],
    kind: Option<&str>,
    force: bool,
) -> Vec<Value> {
    let filter = if force { "all" } else { "runnable" };
    let ledger = Ledger::load(&store.out);
    pending(store, gs, Some(filter), None)
        .into_iter()
        .filter(|r| {
            if targets.is_empty() {
                return true;
            }
            targets.iter().any(|t| {
                let t = store.resolve_id(t);
                r["requirement"].as_str() == Some(t)
                    || store.resolve_id(r["entity"].as_str().unwrap_or("")) == t
            })
        })
        .filter(|r| match kind {
            Some(k) => {
                r["test"]["kind"].as_str() == Some(k)
                    || r["requirement"]
                        .as_str()
                        .and_then(|id| ledger.requirements.get(id))
                        .is_some_and(|row| row.tests.values().any(|t| t.test.kind == k))
            }
            None => true,
        })
        .collect()
}

// Run verification over the selected rows and record verdicts. Programmatic rows
// execute their command; llm rows drive the configured model against the verify_task
// package. Returns {rows, verified, failing, stale, skipped}.
// Mirrors docs/consumers/gen.md#runners.
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn run_all(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    targets: &[String],
    kind: Option<&str>,
    force: bool,
    trace: &crate::session::Trace,
) -> Result<Value, String> {
    run_without_build(store, runner, gs, targets, kind, force, trace, true).await
}

#[derive(serde::Serialize, serde::Deserialize)]
struct VerifyProgress {
    selected: Vec<Value>,
    task_rows: Vec<String>,
    build_done: bool,
    tasks_done: bool,
    next: usize,
    verified: u64,
    failing: u64,
    stale: u64,
    skipped: u64,
    held: Vec<(String, ProgRun, String)>,
    result: Option<Value>,
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub(crate) async fn run_without_build(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    targets: &[String],
    kind: Option<&str>,
    force: bool,
    trace: &crate::session::Trace,
    build_product: bool,
) -> Result<Value, String> {
    use crate::session::TraceEvent;
    // Every prompt this run sends reports under the requirement it is judging.

    let signature =
        json!({"targets":targets,"kind":kind,"force":force,"settings":gs,"build":build_product});
    let key = crate::gen::progress::command_key("verify", &store.out, &signature);
    let mut progress = crate::gen::progress::Progress::load(key, signature, || {
        let selected = select_rows(store, gs, targets, kind, force);
        let ledger = Ledger::load(&store.out);
        let task_rows = selected
            .iter()
            .filter_map(|r| r["requirement"].as_str())
            .filter(|rid| {
                ledger.requirements.get(*rid).is_some_and(|row| {
                    row.test.run.starts_with("task:")
                        || row.tests.values().any(|t| t.test.run.starts_with("task:"))
                })
            })
            .map(str::to_owned)
            .collect();
        VerifyProgress {
            selected,
            task_rows,
            build_done: false,
            tasks_done: false,
            next: 0,
            verified: 0,
            failing: 0,
            stale: 0,
            skipped: 0,
            held: Vec::new(),
            result: None,
        }
    })?;
    if let Some(result) = &progress.data.result {
        return Ok(result.clone());
    }
    let selected = progress.data.selected.clone();
    progress.save().await?;
    if !targets.is_empty() && selected.is_empty() {
        return Err(include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/verify-select/no-ledger-rows-match-the-given-target-e8cce.md"
        ))
        .into());
    }
    // The build runs once, before any row. A row cannot say anything true about an
    // artifact that was never produced, so a broken build stops the run instead of
    // failing every requirement and naming the wrong culprit.
    // Mirrors docs/consumers/gen.md#runners.
    if build_product && !progress.data.build_done {
        crate::gen::run_build(&store.out, gs, trace, "verify").map_err(|e| {
            if crate::session_recovery::is_interruption(&e) {
                e
            } else {
                format!("{e}; nothing was verified")
            }
        })?;
        progress.data.build_done = true;
        progress.save().await?;
    }
    let task_rows = progress.data.task_rows.clone();
    if !progress.data.tasks_done {
        if !task_rows.is_empty() && kind != Some("llm") {
            run_selected_after_build(store, gs, &task_rows)?;
        }
        progress.data.tasks_done = true;
        progress.save().await?;
    }
    let (mut verified, mut failing, mut stale, mut skipped) = (
        progress.data.verified,
        progress.data.failing,
        progress.data.stale,
        progress.data.skipped,
    );
    let mut held = progress.data.held.clone();
    for (index, r) in selected.iter().enumerate().skip(progress.data.next) {
        if trace.is_cancelled() {
            break;
        }
        if crate::session_recovery::suspension_requested() {
            return Err("suspended".into());
        }
        'row: {
            let rid = r["requirement"].as_str().unwrap_or_default().to_string();
            let status = r["status"].as_str().unwrap_or_default();
            if status == "stale-requirement" || status == "missing" {
                trace.event(TraceEvent::VerifyRowStale {
                    requirement: rid.clone(),
                    entity: r["entity"].as_str().unwrap_or("").to_string(),
                    status: status.to_string(),
                    reason: r["reason"].as_str().unwrap_or("").to_string(),
                });
                stale += 1;
                break 'row;
            }
            let row_kind = r["test"]["kind"].as_str().unwrap_or("programmatic");
            let current = Ledger::load(&store.out);
            let has_llm_criteria = current
                .requirements
                .get(&rid)
                .is_some_and(|row| row.tests.values().any(|t| t.test.kind == "llm"));
            let has_programmatic = current.requirements.get(&rid).is_some_and(|row| {
                row.test.kind == "programmatic"
                    || row.tests.values().any(|t| t.test.kind == "programmatic")
            });
            if has_llm_criteria && kind != Some("programmatic") {
                if !task_rows.contains(&rid) && has_programmatic && kind != Some("llm") {
                    run_selected_after_build(store, gs, std::slice::from_ref(&rid))?;
                }
                match judge_llm_criteria(store, runner, &rid, gs, trace).await {
                    Ok(result) => {
                        match result["status"].as_str() {
                            Some("verified") => verified += 1,
                            Some("failing" | "partial") => failing += 1,
                            _ => skipped += 1,
                        }
                        trace.event(TraceEvent::VerifyRowDone {
                            requirement: rid.clone(),
                            verdict: result["verdict"].as_str().unwrap_or("none").into(),
                            run: "llm".into(),
                            evidence: "Each bound LLM criterion received a separate judgment."
                                .into(),
                        });
                    }
                    Err(JudgmentError::Model(error)) => {
                        if !crate::session_recovery::is_interruption(&error) {
                            trace.event(TraceEvent::VerifyRowError {
                                requirement: rid.clone(),
                                message: error.clone(),
                            });
                        }
                        return Err(error);
                    }
                    Err(JudgmentError::Invalid(error)) => {
                        record_llm_failure(store, &rid, &error)?;
                        skipped += 1;
                        trace.event(TraceEvent::VerifyRowError {
                            requirement: rid.clone(),
                            message: error,
                        });
                    }
                }
                break 'row;
            }
            if row_kind == "llm" && kind == Some("programmatic") {
                if !task_rows.contains(&rid) {
                    run_selected_after_build(store, gs, std::slice::from_ref(&rid))?;
                }
                let ledger = Ledger::load(&store.out);
                let (status, _) = status_of(store, &rid, &ledger.requirements[&rid], gs);
                match status.as_str() {
                    "verified" => verified += 1,
                    "failing" | "partial" => failing += 1,
                    _ => skipped += 1,
                }
                break 'row;
            }
            if row_kind != "llm" && task_rows.contains(&rid) {
                let ledger = Ledger::load(&store.out);
                let (status, _) = status_of(store, &rid, &ledger.requirements[&rid], gs);
                match status.as_str() {
                    "verified" => verified += 1,
                    "failing" | "partial" => failing += 1,
                    _ => skipped += 1,
                }
                break 'row;
            }
            if crate::vfs::is_scoped() && row_kind != "llm" {
                skipped += 1;
                break 'row;
            }
            trace.event(TraceEvent::VerifyRowStart {
                requirement: rid.clone(),
                test: row_kind.to_string(),
            });
            let verdict = if row_kind == "llm" {
                match task(store, &rid, gs) {
                    Ok(task_pkg) => {
                        let evidence_input = judge_files(
                            gs,
                            task_pkg["files"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str),
                        );
                        let user = judge_input(
                            None,
                            task_pkg["criteria"].as_str().unwrap_or_default(),
                            task_pkg["context"].as_str().unwrap_or_default(),
                            &evidence_input,
                            JUDGE.get("ask-row"),
                        );
                        match runner
                            .ask_traced(
                                task_pkg["instructions"].as_str().unwrap_or_default(),
                                &user,
                                &format!("verify {}", rid),
                                "judge",
                                Some(trace),
                            )
                            .await
                        {
                            Ok(reply) => match parse_verdict(&reply) {
                                Some(pass) => Some((
                                    pass,
                                    crate::llm::truncate(reply.trim(), 300).to_string(),
                                    None,
                                )),
                                None => {
                                    trace.event(TraceEvent::VerifyRowError {
                                        requirement: rid.clone(),
                                        message: " verdict unparseable (no PASS or FAIL in reply)"
                                            .into(),
                                    });
                                    None
                                }
                            },
                            Err(e) if crate::session_recovery::is_interruption(&e) => {
                                return Err(e)
                            }
                            Err(e) => {
                                trace.event(TraceEvent::VerifyRowError {
                                    requirement: rid.clone(),
                                    message: format!(" llm run failed: {}", e),
                                });
                                return Err(e);
                            }
                        }
                    }
                    Err(e) => {
                        trace.event(TraceEvent::VerifyRowError {
                            requirement: rid.clone(),
                            message: format!(": {}", e),
                        });
                        None
                    }
                }
            } else {
                match run_programmatic(store, &rid, gs) {
                    Ok(run) if run.pass => Some((true, run.evidence, Some(run.code))),
                    // A failure waits: whether it is the deliverable's or the machine's is
                    // decided once, when the run knows how the other rows went
                    // (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
                    Ok(run) => {
                        held.push((
                            rid.clone(),
                            run,
                            r["test"]["run"].as_str().unwrap_or("").to_string(),
                        ));
                        None
                    }
                    Err(e) => {
                        trace.event(TraceEvent::VerifyRowError {
                            requirement: rid.clone(),
                            message: format!(": {}", e),
                        });
                        None
                    }
                }
            };
            match verdict {
                Some((pass, evidence, code)) => {
                    let v = if pass { "pass" } else { "fail" };
                    mark(store, &rid, v, None, Some(&evidence), code, gs).ok();
                    trace.event(TraceEvent::VerifyRowDone {
                        requirement: rid.clone(),
                        verdict: v.to_string(),
                        run: r["test"]["run"].as_str().unwrap_or("").to_string(),
                        evidence,
                    });
                    if pass {
                        verified += 1;
                    } else {
                        failing += 1;
                    }
                }
                None => skipped += 1,
            }
        }
        progress.data.next = index + 1;
        progress.data.verified = verified;
        progress.data.failing = failing;
        progress.data.stale = stale;
        progress.data.skipped = skipped;
        progress.data.held = held.clone();
        progress.save().await?;
    }
    // One broken runner says the same thing to every row it touched; unmet requirements
    // do not agree with each other. When nothing passed and every failure carries the
    // identical output, the machine is what failed, and no row learned anything.
    let identical =
        held.len() > 1 && verified == 0 && held.windows(2).all(|w| w[0].1.tail == w[1].1.tail);
    let mut runner_failed = 0u64;
    skipped = skipped.saturating_sub(held.len() as u64);
    for (rid, run, cmd) in held {
        if run.runner_failed() || identical {
            record_runner_failure(store, &rid, run.code, &run.evidence)?;
            runner_failed += 1;
            trace.event(TraceEvent::VerifyRowError {
                requirement: rid.clone(),
                message: format!(
                    " not judged: the runner failed (exit {}): {}",
                    run.code,
                    crate::llm::truncate(&run.tail, 120)
                ),
            });
            continue;
        }
        mark(
            store,
            &rid,
            "fail",
            None,
            Some(&run.evidence),
            Some(run.code),
            gs,
        )
        .ok();
        failing += 1;
        trace.event(TraceEvent::VerifyRowDone {
            requirement: rid,
            verdict: "fail".into(),
            run: cmd,
            evidence: run.evidence,
        });
    }
    if runner_failed > 0 {
        trace.line(
            "verify",
            &format!(
                "{} row(s) were not judged: the test runner itself failed. Fix the runner, then rerun; the deliverable was never exercised",
                runner_failed
            ),
        );
    }
    // The verify goals this run resolved, by the gate's word rather than the tally's
    // (docs/compiler/goals/verify.md#gate).
    let resolved = selected
        .iter()
        .filter_map(|r| r["requirement"].as_str())
        .filter(|rid| gate(store, gs, rid).is_ok())
        .count();
    let result = json!({
        "rows": selected.len(),
        "pendingCi": if crate::vfs::is_scoped() { selected.iter().filter(|r| r["test"]["kind"] != "llm" && !task_rows.iter().any(|id| r["requirement"]==*id)).cloned().collect::<Vec<_>>() } else { Vec::new() },
        "verified": verified,
        "failing": failing,
        "runnerFailed": runner_failed,
        "stale": stale,
        "skipped": skipped,
        "resolved": resolved,
    });
    progress.data.result = Some(result.clone());
    progress.save().await?;
    Ok(result)
}
