use super::*;

// Record a verdict. Rebaselines the test and files hashes, never the requirement hash;
// a stale factHash is recorded but the row stays pending by derivation. A programmatic
// run passes its exit code and the row keeps it beside the output, so a verdict can be
// read back without rerunning it (docs/consumers/gen.md#runners); a judged llm verdict
// passes None, so exitCode stays a programmatic-only field. Either way the verdict
// clears a runner-failed mark: runner-failed derivation keys on verdict `none`.
pub fn mark(
    store: &Store,
    rid: &str,
    verdict: &str,
    fact_hash_seen: Option<&str>,
    evidence: Option<&str>,
    exit_code: Option<i32>,
    gs: &GenSettings,
) -> Result<Value, String> {
    if verdict != "pass" && verdict != "fail" {
        return Err(format!(
            "verdict must be `pass` or `fail`, got `{}`",
            verdict
        ));
    }
    let rid = store.resolve_id(rid).to_string();
    let _lock = Ledger::lock(&store.out);
    let mut ledger = Ledger::load(&store.out);
    let Some(row) = ledger.requirements.get_mut(&rid) else {
        return Err(format!("no ledger row for `{}`", rid));
    };
    ensure_current_source(store, gs, row)?;
    let files = crate::gen::hash_implementing(&store.out, gs, &row.files);
    rebaseline_files(row, files);
    row.verdict = verdict.to_string();
    row.exit_code = exit_code;
    row.last_run = Some(now_iso());
    row.evidence = evidence.map(|e| crate::llm::truncate(e, 400));
    row.hashes.test = hash_file(&artifact_path(&store.out, gs, &row.test));
    // The headline is a criterion's own test when criteria exist: its verdict is that
    // criterion's too, or the row reads partial against a stale entry forever
    // (docs/consumers/gen.md#tests-tie-criteria-to-the-deliverable).
    // Matched by name: two criteria may share one runner command.
    let headline_name = row.test.name.clone();
    for t in row.tests.values_mut() {
        if t.test.name == headline_name {
            t.verdict = verdict.to_string();
            t.exit_code = exit_code;
            t.last_run = row.last_run.clone();
            t.evidence = row.evidence.clone();
            t.test_hash = hash_file(&artifact_path(&store.out, gs, &t.test));
        }
    }
    let stale = match fact_hash_seen {
        Some(h) => store
            .graph
            .requirements
            .get(&rid)
            .map(|r| hash_hex(&r.statement) != h)
            .unwrap_or(true),
        None => false,
    };
    let (status, _) = status_of(store, &rid, ledger.requirements.get(&rid).unwrap(), gs);
    ledger.save(&store.out)?;
    Ok(json!({"recorded": rid, "verdict": verdict, "status": status, "graphMoved": stale}))
}

/// A fresh observation cannot carry unrelated tests across a source change.
pub(crate) fn rebaseline_files(row: &mut crate::gen::ReqRow, files: String) {
    if row.hashes.files != files {
        row.verdict = "none".into();
        row.exit_code = None;
        row.last_run = None;
        row.evidence = None;
        for test in row.tests.values_mut() {
            test.verdict = "none".into();
            test.exit_code = None;
            test.last_run = None;
            test.evidence = None;
        }
        row.hashes.files = files;
    }
}

/// Explicit model judgments cover LLM criteria only. Validate the whole list
/// before committing, so a bad second entry cannot partially refresh a row.
pub fn mark_llm(
    store: &Store,
    rid: &str,
    verdict: &str,
    fact_hash: Option<&str>,
    evidence: Option<&str>,
    criteria: Option<&Value>,
    gs: &GenSettings,
) -> Result<Value, String> {
    let rid = store.resolve_id(rid).to_owned();
    let _lock = Ledger::lock(&store.out);
    let mut ledger = Ledger::load(&store.out);
    let row = ledger
        .requirements
        .get_mut(&rid)
        .ok_or_else(|| format!("no ledger row for `{rid}`"))?;
    ensure_current_source(store, gs, row)?;
    let llm: Vec<String> = row
        .tests
        .iter()
        .filter(|(_, t)| t.test.kind == "llm")
        .map(|(id, _)| id.clone())
        .collect();
    if criteria.is_none() && row.test.kind == "llm" && llm.len() <= 1 {
        return mark(store, &rid, verdict, fact_hash, evidence, None, gs);
    }
    if llm.is_empty() {
        return Err(include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/verify-mark/no-llm-criterion-is-bound-use-run-20b31.md"
        ))
        .into());
    }
    let entries = criteria.and_then(Value::as_array).ok_or_else(|| {
        format!(
            include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/inline/verify-mark/supply-record-verdict-criteria-for-every-llm-c125f.md"
            )),
            llm.join(", ")
        )
    })?;
    let mut judgments = std::collections::BTreeMap::new();
    for entry in entries {
        let cid = entry["criterion"]
            .as_str()
            .ok_or("Each criteria entry requires criterion")?;
        let cid = store.resolve_id(cid).to_owned();
        if !llm.contains(&cid) {
            return Err(format!("`{cid}` is not an LLM criterion bound to `{rid}`"));
        }
        let value = entry["verdict"]
            .as_str()
            .filter(|v| matches!(*v, "pass" | "fail"))
            .ok_or("Criterion verdict must be pass or fail")?;
        let live = store
            .graph
            .criteria
            .get(&cid)
            .ok_or("Criterion no longer exists")?;
        let hash = hash_hex(&live.statement);
        if entry["factHash"].as_str() != Some(hash.as_str())
            || row.tests[&cid].criterion_hash != hash
        {
            return Err(format!("Criterion `{cid}` changed. Reload begin_verification before recording its verdict."));
        }
        if judgments
            .insert(
                cid,
                (
                    value.to_owned(),
                    entry["evidence"]
                        .as_str()
                        .map(|e| crate::llm::truncate(e, 400).to_owned()),
                ),
            )
            .is_some()
        {
            return Err("Duplicate criterion verdict".into());
        }
    }
    if judgments.len() != llm.len() {
        return Err(
            "Every bound LLM criterion needs an explicit verdict; reload begin_verification."
                .into(),
        );
    }
    let combined = if judgments.values().all(|(v, _)| v == "pass") {
        "pass"
    } else {
        "fail"
    };
    if verdict != combined {
        return Err(format!(
            include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/inline/verify-mark/top-level-verdict-must-be-combined-to-43ff7.md"
            )),
            combined = combined
        ));
    }
    let files = crate::gen::hash_implementing(&store.out, gs, &row.files);
    rebaseline_files(row, files);
    let now = Some(now_iso());
    for (cid, (value, observed)) in judgments {
        let test = row.tests.get_mut(&cid).unwrap();
        test.verdict = value;
        test.last_run = now.clone();
        test.exit_code = None;
        test.evidence = observed;
        test.test_hash = hash_file(&artifact_path(&store.out, gs, &test.test));
    }
    if row.test.kind == "llm" {
        row.verdict = verdict.into();
        row.last_run = now;
        row.exit_code = None;
        row.evidence = evidence.map(|e| crate::llm::truncate(e, 400));
        row.hashes.test = hash_file(&artifact_path(&store.out, gs, &row.test));
    }
    let stale = fact_hash.is_some_and(|h| {
        store
            .graph
            .requirements
            .get(&rid)
            .is_some_and(|r| hash_hex(&r.statement) != h)
    });
    let (status, _) = status_of(store, &rid, row, gs);
    let results: Vec<_> = llm
        .iter()
        .map(|cid| json!({"criterion":cid,"verdict":row.tests[cid].verdict}))
        .collect();
    ledger.save(&store.out)?;
    Ok(
        json!({"recorded":rid,"verdict":verdict,"status":status,"graphMoved":stale,"criteria":results}),
    )
}
