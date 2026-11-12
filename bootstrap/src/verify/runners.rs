use super::*;

// Run one programmatic row: grep the artifact for the test name first (absent means
// stale-test, not failing), then execute the command; the exit code is the verdict.
// What one programmatic run produced. `tail` is the output alone: a run compares tails
// with other rows to tell one broken runner from many unmet requirements
// (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ProgRun {
    pub pass: bool,
    pub code: i32,
    pub tail: String,
    pub evidence: String,
}

// Run one programmatic test reference under the deliverable.
pub(crate) fn run_test_ref(gs: &GenSettings, t: &crate::gen::TestRef) -> Result<ProgRun, String> {
    if let Some(task) = t.run.strip_prefix("task:") {
        let root = crate::task_runtime::project_root(&gs.deliverable)?;
        let (id, result) = crate::task_runtime::execute_with_key(
            &root,
            task,
            &format!("test:{}:{}", t.name, t.run),
        )?;
        return task_case_outcome(&result, &t.name, &id);
    }
    let cwd = gs.deliverable.join(&t.cwd);
    let out = crate::sys::run_shell(&t.run, &cwd)
        .map_err(|e| format!("failed to run `{}` in {}: {}", t.run, cwd.display(), e))?;
    let pass = out.success;
    let mut evidence = String::from_utf8_lossy(&out.stdout).to_string();
    evidence.push_str(&String::from_utf8_lossy(&out.stderr));
    let tail: String = evidence
        .lines()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" | ");
    let tail = crate::llm::truncate(&tail, 300);
    Ok(ProgRun {
        pass,
        code: out.code,
        evidence: format!("{}: {}", t.run, tail),
        tail,
    })
}

pub(super) fn task_case_outcome(
    result: &Value,
    name: &str,
    run_id: &str,
) -> Result<ProgRun, String> {
    if result["complete"] != true {
        return Err(format!(
            "run {run_id}: incomplete collection; no test verdict"
        ));
    }
    let (verdict, observed) = crate::task_runtime::case_verdict(result, name);
    if verdict == "none" {
        return Err(format!("run {run_id}: {name}: {observed}; no test verdict"));
    }
    Ok(ProgRun {
        pass: verdict == "pass",
        code: result["exit_code"].as_i64().unwrap_or(1) as i32,
        tail: format!("{name}: {observed}"),
        evidence: format!("run {run_id}: {name}: {observed}"),
    })
}

// Run a row's criterion tests (the programmatic ones other than the headline itself)
// and record each verdict on its entry; the row's verdict fails when any
// criterion fails. Returns one line per criterion run.
// Mirrors docs/consumers/gen.md#tests-tie-criteria-to-the-deliverable.
pub fn run_criterion_tests(store: &Store, rid: &str, gs: &GenSettings) -> Vec<Value> {
    let rid = store.resolve_id(rid).to_string();
    // The commands run on a snapshot, outside the ledger lock; their outcomes land
    // on a fresh load under the lock (docs/consumers/gen.md#the-ledger).
    let mut ledger = Ledger::load(&store.out);
    let Some(row) = ledger.requirements.get_mut(&rid) else {
        return Vec::new();
    };
    if let Err(reason) = ensure_current_source(store, gs, row) {
        return vec![json!({"requirement":rid,"note":reason})];
    }
    if row.tests.values().any(|t| t.test.kind == "programmatic") {
        let files = crate::gen::hash_implementing(&store.out, gs, &row.files);
        rebaseline_files(row, files);
    }
    let headline_run = row.test.run.clone();
    let headline_name = row.test.name.clone();
    let headline_verdict = row.verdict.clone();
    // Identical legacy commands in one directory share one execution
    // (docs/consumers/gen.md#test-artifact-selection): a criterion test sharing the
    // headline's command takes the headline's fresh outcome, and criteria sharing a
    // command with each other run it once.
    let mut runs: BTreeMap<(String, String), Result<ProgRun, String>> = BTreeMap::new();
    if row.test.kind == "programmatic" && matches!(row.verdict.as_str(), "pass" | "fail") {
        if let Some(code) = row.exit_code {
            let evidence = row.evidence.clone().unwrap_or_default();
            runs.insert(
                (row.test.run.clone(), row.test.cwd.clone()),
                Ok(ProgRun {
                    pass: row.verdict == "pass",
                    code,
                    tail: evidence.clone(),
                    evidence,
                }),
            );
        }
    }
    let mut lines = Vec::new();
    let mut any_fail = false;
    for (cid, t) in row.tests.iter_mut() {
        if t.test.kind == "programmatic" && t.test.run.starts_with("task:") {
            any_fail |= t.verdict == "fail";
            lines.push(json!({"requirement":rid,"criterion":cid,"verdict":t.verdict,"evidence":t.evidence,"source":"task report"}));
            continue;
        }
        if t.test.name == headline_name && t.test.run == headline_run {
            // The headline is this criterion's test: its verdict is the row's, and
            // the list says so instead of leaving the criterion out.
            lines.push(json!({"requirement": rid, "criterion": cid, "verdict": headline_verdict, "note": "the row's headline test"}));
            continue;
        }
        if t.test.kind != "programmatic" {
            continue;
        }
        if crate::vfs::is_scoped() {
            lines.push(json!({"requirement":rid,"criterion":cid,"verdict":"none","note":"Legacy command requires a native runner.","pendingCi":true}));
            continue;
        }
        // Grep before running, as for the headline: a filter that matches no test
        // exits 0 and would read as a pass.
        let artifact = artifact_path(&store.out, gs, &t.test);
        if !artifact_names(&artifact, &t.test.name) {
            let reason = format!(
                "test `{}` not found in {}; artifact-gone, nothing executed",
                t.test.name,
                artifact.display()
            );
            t.verdict = "none".into();
            t.exit_code = None;
            t.last_run = Some(now_iso());
            t.evidence = Some(crate::llm::truncate(&reason, 400).to_string());
            t.test_hash = hash_file(&artifact);
            lines.push(
                json!({"requirement": rid, "criterion": cid, "verdict": "none", "reason": reason}),
            );
            continue;
        }
        let outcome = runs
            .entry((t.test.run.clone(), t.test.cwd.clone()))
            .or_insert_with(|| run_test_ref(gs, &t.test))
            .clone();
        match outcome {
            Ok(r) if r.runner_failed() => {
                t.verdict = "none".into();
                t.exit_code = Some(r.code);
                t.last_run = Some(now_iso());
                t.evidence = Some(crate::llm::truncate(&r.evidence, 400).to_string());
                t.test_hash = hash_file(&artifact_path(&store.out, gs, &t.test));
                lines.push(json!({"requirement":rid,"criterion":cid,"verdict":"none","exitCode":r.code,"reason":"runner-failed","evidence":r.tail}));
            }
            Ok(r) => {
                t.verdict = if r.pass { "pass".into() } else { "fail".into() };
                t.exit_code = Some(r.code);
                t.last_run = Some(now_iso());
                t.evidence = Some(crate::llm::truncate(&r.evidence, 400).to_string());
                t.test_hash = hash_file(&artifact_path(&store.out, gs, &t.test));
                any_fail |= !r.pass;
                lines.push(json!({"requirement": rid, "criterion": cid, "verdict": t.verdict, "exitCode": r.code, "evidence": r.tail}));
            }
            Err(e) => {
                t.verdict = "none".into();
                t.exit_code = None;
                t.last_run = Some(now_iso());
                t.evidence = Some(crate::llm::truncate(&e, 400).to_string());
                t.test_hash = hash_file(&artifact_path(&store.out, gs, &t.test));
                lines.push(
                    json!({"requirement": rid, "criterion": cid, "verdict": "none", "reason": e}),
                );
            }
        }
    }
    let observed = row.tests.clone();
    let landed = Ledger::update(&store.out, |fresh| {
        let Some(row) = fresh.requirements.get_mut(&rid) else {
            return Err(format!(
                "the ledger row for `{rid}` left while its criterion tests ran"
            ));
        };
        let files = crate::gen::hash_implementing(&store.out, gs, &row.files);
        rebaseline_files(row, files);
        for line in &lines {
            let Some(cid) = line["criterion"].as_str() else {
                continue;
            };
            if line.get("note").is_some() || line["source"] == "task report" {
                continue;
            }
            let (Some(seen), Some(t)) = (observed.get(cid), row.tests.get_mut(cid)) else {
                continue;
            };
            // A test re-bound while the command ran is not the test that ran.
            let same = |a: &crate::gen::TestRef, b: &crate::gen::TestRef| {
                (&a.kind, &a.artifact, &a.name, &a.run, &a.cwd)
                    == (&b.kind, &b.artifact, &b.name, &b.run, &b.cwd)
            };
            if !same(&seen.test, &t.test) {
                continue;
            }
            t.verdict = seen.verdict.clone();
            t.exit_code = seen.exit_code;
            t.last_run = seen.last_run.clone();
            t.evidence = seen.evidence.clone();
            t.test_hash = seen.test_hash.clone();
        }
        if any_fail && row.verdict == "pass" {
            row.verdict = "fail".into();
            row.evidence = Some("a criterion test failed".into());
        }
        Ok(())
    });
    if let Err(e) = landed {
        lines.push(json!({"requirement": rid, "verdict": "none", "note": format!("criterion verdicts were not recorded: {e}")}));
    }
    lines
}

impl ProgRun {
    // The command was never executed (not found, not executable) or never finished (the
    // command timeout killed it, exit 124). Nothing here says anything about the
    // requirement. Mirrors docs/consumers/gen.md#a-test-that-could-not-run-says-nothing.
    pub fn runner_failed(&self) -> bool {
        self.code == 124 || self.code == 126 || self.code == 127
    }
}

pub fn run_programmatic(store: &Store, rid: &str, gs: &GenSettings) -> Result<ProgRun, String> {
    let rid = store.resolve_id(rid).to_string();
    let ledger = Ledger::load(&store.out);
    let Some(row) = ledger.requirements.get(&rid) else {
        return Err(format!("no ledger row for `{}`", rid));
    };
    ensure_current_source(store, gs, row)?;
    if row.test.run.trim().is_empty() {
        return Err(format!(
            include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/inline/verify-runners/has-no-recorded-run-command-so-nothing-c413a.md"
            )),
            rid, row.entity
        ));
    }
    let artifact = artifact_path(&store.out, gs, &row.test);
    if !artifact_names(&artifact, &row.test.name) {
        return Err(format!(
            "test `{}` not found in {}; the row is missing (artifact-gone) and nothing executes; {}",
            row.test.name,
            artifact.display(),
            repair_for("missing", "artifact-gone", &rid, &row.entity)
        ));
    }
    if row.test.run.starts_with("task:") {
        return run_test_ref(gs, &row.test);
    }
    let cwd = gs.deliverable.join(&row.test.cwd);
    let out = crate::sys::run_shell(&row.test.run, &cwd).map_err(|e| {
        format!(
            "failed to run `{}` in {}: {}",
            row.test.run,
            cwd.display(),
            e
        )
    })?;
    let pass = out.success;
    let mut evidence = String::from_utf8_lossy(&out.stdout).to_string();
    evidence.push_str(&String::from_utf8_lossy(&out.stderr));
    let tail: String = evidence
        .lines()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" | ");
    let tail = crate::llm::truncate(&tail, 300);
    Ok(ProgRun {
        pass,
        code: out.code,
        evidence: format!("{}: {}", row.test.run, tail),
        tail,
    })
}

// Record a run that judged nothing: the exit code and the output stay, the verdict does
// not move. A row with an exit code and no verdict reads as `runner-failed`
// (docs/consumers/gen.md#status-is-derived-never-stored).
pub fn record_runner_failure(
    store: &Store,
    rid: &str,
    code: i32,
    evidence: &str,
) -> Result<(), String> {
    let rid = store.resolve_id(rid).to_string();
    let _lock = Ledger::lock(&store.out);
    let mut ledger = Ledger::load(&store.out);
    if let Some(row) = ledger.requirements.get_mut(&rid) {
        // The previous verdict is not today's answer: this run moved lastRun and
        // learned nothing, so the row is unknown rather than stale-but-confident.
        row.verdict = "none".into();
        row.exit_code = Some(code);
        row.evidence = Some(crate::llm::truncate(evidence, 300));
        row.last_run = Some(now_iso());
        // A criterion entry that is the headline's test, by name or by its shared
        // command, learned nothing either: its previous verdict clears too
        // (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
        let (name, run, cwd) = (
            row.test.name.clone(),
            row.test.run.clone(),
            row.test.cwd.clone(),
        );
        for t in row.tests.values_mut() {
            if t.test.kind == "programmatic"
                && (t.test.name == name || (t.test.run == run && t.test.cwd == cwd))
            {
                t.verdict = "none".into();
                t.exit_code = Some(code);
                t.evidence = row.evidence.clone();
                t.last_run = row.last_run.clone();
            }
        }
        return ledger.save(&store.out);
    }
    Ok(())
}
