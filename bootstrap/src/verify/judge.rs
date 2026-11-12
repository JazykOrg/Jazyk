use super::*;

// What a judge reads: the criterion or the row's criteria, the context, and the
// implementing files. One builder for every llm verdict.
// Mirrors docs/compiler/sessions.md#payload-files.
pub(crate) const JUDGE: crate::payload::Fragments =
    crate::payload::Fragments(include_str!(concat!(
        env!("OUT_DIR"),
        "/payloads/goals/prompts/verification/judge-input.md.mustache"
    )));

// The implementing files as the judge sees them: each readable file under the
// deliverable, truncated, under its own `=== path ===` line.
pub(crate) fn judge_files<'a>(
    gs: &GenSettings,
    paths: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut files = String::new();
    for path in paths {
        if let Ok(text) = crate::vfs::read_to_string(gs.deliverable.join(path)) {
            files.push('\n');
            files.push_str(&JUDGE.fill(
                "file",
                &[
                    ("path", path),
                    ("text", &crate::llm::truncate(&text, 12_000)),
                ],
            ));
        }
    }
    files
}

// The judge's input. `statement` names one criterion when the judge decides one;
// `ask` is the closing instruction, filled.
pub(crate) fn judge_input(
    statement: Option<&str>,
    criteria: &str,
    context: &str,
    files: &str,
    ask: &str,
) -> String {
    let criterion = statement
        .map(|s| JUDGE.fill("criterion", &[("statement", s)]) + "\n")
        .unwrap_or_default();
    JUDGE.fill(
        "input",
        &[
            ("criterion", &criterion),
            ("criteria", criteria),
            ("context", context),
            ("files", files),
            ("ask", ask),
        ],
    )
}

// The package for one row: everything a worker needs to run or judge the test.
pub fn task(store: &Store, rid: &str, gs: &GenSettings) -> Result<Value, String> {
    let rid = store.resolve_id(rid).to_string();
    let ledger = Ledger::load(&store.out);
    let Some(row) = ledger.requirements.get(&rid) else {
        return Err(format!("no ledger row for `{}`; run generation first", rid));
    };
    let Some(r) = store.graph.requirements.get(&rid) else {
        return Err(format!("unknown requirement `{}`", rid));
    };
    let (status, reason) = status_with(store, &rid, row, gs, &ledger);
    // A row nothing can run says so with its repair, instead of packaging a test that
    // judges a dropped sentence or an artifact that is not there.
    if status == "stale-requirement" || status == "missing" {
        return Err(format!(
            "`{}` is {} ({}); {}",
            rid,
            status,
            reason,
            repair_for(&status, &reason, &rid, &row.entity)
        ));
    }
    // The goal's loaded set: the requirement in full with its entity as a stub.
    // Mirrors docs/consumers/bind.md#the-bind-goal.
    let pack = crate::context::ledger_context(store, &rid);
    let artifact = artifact_path(&store.out, gs, &row.test);
    let criteria = if row.test.kind == "llm" {
        crate::vfs::read_to_string(&artifact).unwrap_or_default()
    } else {
        String::new()
    };
    let mut instructions = if row.test.kind == "llm" {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/verification/verify-llm-contract.md.mustache"
        ))
        .to_owned()
    } else if row.test.run.starts_with("task:") {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/verification/verify-task-contract.md"
        ))
        .to_owned()
    } else {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/verification/verify-legacy-contract.md"
        ))
        .to_owned()
    };
    if row.test.kind != "llm" && row.tests.values().any(|test| test.test.kind == "llm") {
        instructions.push('\n');
        instructions.push_str(include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/verification/verify-llm-contract.md.mustache"
        )));
    }
    Ok(json!({
        "requirement": rid,
        "entity": row.entity,
        "statement": r.statement,
        "quote": r.source.as_ref().map(|s| s.quote.clone()).unwrap_or_default(),
        "provenance": crate::session::provenance_line(r),
        "factHash": hash_hex(&r.statement),
        "status": status,
        "reason": reason,
        "deliverable": gs.deliverable.to_string_lossy(),
        "files": row.files,
        "test": {"kind": row.test.kind, "label": row.test.label, "artifact": row.test.artifact,
                 "name": row.test.name, "run": row.test.run, "cwd": row.test.cwd},
        "criteria": criteria,
        "criterionTests": row.tests.iter().map(|(cid, test)| {
            let artifact = artifact_path(&store.out, gs, &test.test);
            json!({"criterion":cid,"factHash":test.criterion_hash,"statement":store.graph.criteria.get(cid).map(|c| &c.statement),
                "kind":test.test.kind,"artifact":test.test.artifact,"name":test.test.name,"verdict":test.verdict,
                "criteria":if test.test.kind=="llm"{crate::vfs::read_to_string(&artifact).unwrap_or_default()}else{String::new()}})
        }).collect::<Vec<_>>(),
        "context": pack,
        "instructions": instructions,
    }))
}

// The judge's verdict. The contract asks for a verdict line first, so a first line
// that LEADS with PASS or FAIL (bare, bolded, or after "Verdict:") is the answer.
// Anything else is a reasoning-first reply and is read from its conclusion: the
// later of PASS or FAIL wins, so quoting the criteria's own words on the way there
// never flips the verdict.
// Mirrors docs/compiler/goals/verify.md#the-one-shot-form.
pub(super) fn parse_verdict(reply: &str) -> Option<bool> {
    // Whole words only: `password`, `bypass`, and `failover` are not verdicts.
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|w| !w.is_empty())
            .map(|w| w.to_uppercase())
            .collect()
    };
    let verdict = |w: &str| match w {
        "PASS" => Some(true),
        "FAIL" => Some(false),
        _ => None,
    };
    let first = reply.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let head = words(first);
    let lead = match head.first().map(String::as_str) {
        Some("VERDICT") => head.get(1),
        _ => head.first(),
    };
    if let Some(v) = lead.and_then(|w| verdict(w)) {
        return Some(v);
    }
    words(reply).iter().rev().find_map(|w| verdict(w))
}

pub(super) enum JudgmentError {
    Model(String),
    Invalid(String),
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub(super) async fn judge_llm_criteria(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    rid: &str,
    gs: &GenSettings,
    trace: &crate::session::Trace,
) -> Result<Value, JudgmentError> {
    let package = task(store, rid, gs).map_err(JudgmentError::Invalid)?;
    let files = judge_files(
        gs,
        package["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str),
    );
    let mut results = Vec::new();
    for test in package["criterionTests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["kind"] == "llm")
    {
        if trace.is_cancelled() {
            return Err(JudgmentError::Invalid("Verification cancelled".into()));
        }
        let input = judge_input(
            test["statement"].as_str(),
            test["criteria"].as_str().unwrap_or_default(),
            package["context"].as_str().unwrap_or_default(),
            &files,
            JUDGE.get("ask-criterion"),
        );
        let reply = runner
            .ask_traced(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/verification/verify-criterion-contract.md.mustache"
                )),
                &input,
                &format!(
                    "verify {rid} {}",
                    test["criterion"].as_str().unwrap_or_default()
                ),
                "judge",
                Some(trace),
            )
            .await
            .map_err(JudgmentError::Model)?;
        let pass = parse_verdict(&reply).ok_or_else(|| {
            JudgmentError::Invalid("Model returned no conclusive criterion verdict".into())
        })?;
        results.push(json!({"criterion":test["criterion"],"factHash":test["factHash"],"verdict":if pass{"pass"}else{"fail"},"evidence":crate::llm::truncate(reply.trim(),300)}));
    }
    let verdict = if results.iter().all(|r| r["verdict"] == "pass") {
        "pass"
    } else {
        "fail"
    };
    mark_llm(
        store,
        rid,
        verdict,
        package["factHash"].as_str(),
        Some("Each LLM criterion was inspected separately."),
        Some(&json!(results)),
        gs,
    )
    .map_err(JudgmentError::Invalid)
}

pub(super) fn record_llm_failure(store: &Store, rid: &str, error: &str) -> Result<(), String> {
    let _lock = Ledger::lock(&store.out);
    let mut ledger = Ledger::load(&store.out);
    if let Some(row) = ledger.requirements.get_mut(rid) {
        if row.test.kind == "llm" {
            row.verdict = "none".into();
            row.exit_code = None;
            row.evidence = Some(error.into());
        }
        for test in row.tests.values_mut().filter(|t| t.test.kind == "llm") {
            test.verdict = "none".into();
            test.exit_code = None;
            test.last_run = Some(now_iso());
            test.evidence = Some(crate::llm::truncate(error, 400));
        }
        return ledger.save(&store.out);
    }
    Ok(())
}
