use super::*;

// Derived status, first match wins. Mirrors
// docs/consumers/gen.md#status-is-derived-never-stored.
pub fn status_of(store: &Store, rid: &str, row: &ReqRow, gs: &GenSettings) -> (String, String) {
    status_with(store, rid, row, gs, &Ledger::load(&store.out))
}

// `status_of` against a ledger already loaded: the loops over many rows load it once,
// not once per row.
pub fn status_with(
    store: &Store,
    rid: &str,
    row: &ReqRow,
    gs: &GenSettings,
    ledger: &Ledger,
) -> (String, String) {
    let live = store.graph.requirements.get(store.resolve_id(rid));
    let Some(r) = live else {
        return ("missing".into(), "requirement-gone".into());
    };
    if !crate::gen::binding_source_current(&store.out, gs, ledger, row) {
        return ("missing".into(), "source-selection-changed".into());
    }
    let artifact = artifact_path(&store.out, gs, &row.test);
    if !artifact.vexists() {
        return ("missing".into(), "artifact-gone".into());
    }
    // A programmatic artifact that no longer carries the declared test name judges
    // nothing: the test was renamed or deleted by hand. That is bind work, not a
    // rerun (docs/consumers/gen.md#status-is-derived-never-stored).
    if row.test.kind == "programmatic" && !artifact_names(&artifact, &row.test.name) {
        return ("missing".into(), "artifact-gone".into());
    }
    if hash_hex(&r.statement) != row.hashes.requirement {
        return ("stale-requirement".into(), "requirement-changed".into());
    }
    if hash_file(&artifact) != row.hashes.test {
        return ("stale-test".into(), "test-changed".into());
    }
    if row.test.kind == "llm"
        && crate::gen::validate_criteria_file(store, store.resolve_id(rid), None, &artifact)
            .is_err()
    {
        return ("missing".into(), "artifact-gone".into());
    }
    // Implementing files alone: a rebuilt artifact among the files is not a code change
    // (docs/consumers/gen.md#status-is-derived-never-stored).
    if crate::gen::hash_implementing(&store.out, gs, &row.files) != row.hashes.files {
        return ("stale-code".into(), "code-changed".into());
    }
    for cid in store.criteria_of(rid) {
        let Some(test) = row.tests.get(&cid) else {
            return ("unverified".into(), "criterion-unbound".into());
        };
        // A legacy criterion command renamed or deleted by hand judges nothing, like
        // the headline's: bind work, never a verdict read from an empty run. A task
        // report reads a missing case as unverified on its own.
        if test.test.kind == "programmatic" && !test.test.run.starts_with("task:") {
            let path = artifact_path(&store.out, gs, &test.test);
            if !path.vexists() || !artifact_names(&path, &test.test.name) {
                return ("missing".into(), "artifact-gone".into());
            }
        }
        if store
            .graph
            .criteria
            .get(&cid)
            .is_some_and(|c| hash_hex(&c.statement) != test.criterion_hash)
        {
            return ("stale-requirement".into(), "criterion-changed".into());
        }
        if test.test.kind == "llm"
            && crate::gen::validate_criteria_file(
                store,
                store.resolve_id(rid),
                Some(&cid),
                &artifact_path(&store.out, gs, &test.test),
            )
            .is_err()
        {
            return ("missing".into(), "artifact-gone".into());
        }
        if hash_file(&artifact_path(&store.out, gs, &test.test)) != test.test_hash {
            return ("stale-test".into(), "criterion-test-changed".into());
        }
    }
    // Criteria judged one by one: files present with some criterion tests green and
    // some red is a partly implemented requirement, generation work like an
    // unimplemented one, never a contradiction
    // (docs/consumers/gen.md#status-is-derived-never-stored).
    let crit_pass = row.tests.values().filter(|t| t.verdict == "pass").count();
    let crit_fail = row.tests.values().filter(|t| t.verdict == "fail").count();
    // Generation gets one turn per set of facts: red criteria after a generation
    // over the current facts are a finding, not more generation work (a criterion
    // contradicting its neighbors once regenerated sort's entity on every build).
    // Mirrors docs/consumers/gen.md#status-is-derived-never-stored.
    let generated_here = || {
        ledger
            .entities
            .get(crate::ids::entity_slug(&row.entity))
            .map(|e| {
                e.fact_hash == crate::gen::fact_hash(store, &row.entity)
                    && !crate::gen::implementation_outside_source(gs, ledger, &e.files)
            })
            .unwrap_or(false)
    };
    if (crit_fail > 0 || row.verdict == "fail") && generated_here() {
        return ("failing".into(), "criteria-failing-after-generation".into());
    }
    if !row.files.is_empty() && crit_pass > 0 && crit_fail > 0 {
        return ("partial".into(), "criteria-failing".into());
    }
    // Every required criterion needs conclusive current evidence. A passing
    // headline must not hide skipped or unexecuted members of a suite.
    if row.verdict == "pass" && row.tests.values().any(|test| test.verdict != "pass") {
        return ("unverified".into(), "criteria-incomplete".into());
    }
    match row.verdict.as_str() {
        "pass" => ("verified".into(), "passed".into()),
        // A failing test with nothing implementing it is not a defect, it is a bound
        // spec awaiting generation; with implementing files it is a contradiction
        // between deliverable and statement (docs/consumers/bind.md#what-the-verdict-means).
        "fail" if row.files.is_empty() => ("unimplemented".into(), "unimplemented".into()),
        "fail" => ("failing".into(), "failed".into()),
        // A command ran and left a code, yet no verdict was recorded: the runner
        // failed and the requirement was never exercised
        // (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
        _ if row.exit_code.is_some() => ("unverified".into(), "runner-failed".into()),
        _ => ("unverified".into(), "never-run".into()),
    }
}

// Whether a test artifact carries the declared test name. An empty name matches
// nothing: a row without one names no test.
pub(crate) fn artifact_names(artifact: &std::path::Path, name: &str) -> bool {
    !name.trim().is_empty()
        && !name.ends_with("::")
        && crate::vfs::read_to_string(artifact)
            .map(|c| {
                c.contains(name)
                    || name
                        .rsplit_once("::")
                        .is_some_and(|(_, case)| !case.trim().is_empty() && c.contains(case))
            })
            .unwrap_or(false)
}

pub(super) fn ensure_current_source(
    store: &Store,
    gs: &GenSettings,
    row: &ReqRow,
) -> Result<(), String> {
    if crate::gen::binding_source_current(&store.out, gs, &Ledger::load(&store.out), row) {
        Ok(())
    } else {
        Err("source-selection-changed: the binding names files outside current Source; begin_binding again and record_binding using the current source selection".into())
    }
}

// The one sentence a person needs beside a status: which command or goal clears it.
// A `failing` row names no command, because it is a finding, not a task
// (docs/consumers/gen.md#status-is-derived-never-stored).
pub fn repair_for(status: &str, reason: &str, rid: &str, entity: &str) -> String {
    let gen = if entity.is_empty() {
        "jazyk gen".to_string()
    } else {
        format!("jazyk gen {}", entity)
    };
    match (status, reason) {
        ("missing", "not-generated") => format!("develop its source and tests together: `{}`", gen),
        ("missing", "artifact-gone") => format!("the test artifact is gone or no longer contains the test name; re-bind it: `{}`", gen),
        ("missing", "requirement-gone") => "the requirement left the graph; the next record_generation prunes the row".into(),
        ("missing", "source-selection-changed") => format!("the binding names files outside current Source; re-bind it: `{}`", gen),
        ("stale-requirement", _) => format!("the statement was reworded since the row was bound; re-bind it: `{}`", gen),
        ("stale-test", _) => format!("the test artifact changed; rerun it: `jazyk test {}`", rid),
        ("stale-code", _) => format!("an implementing file changed; rerun it: `jazyk test {}`", rid),
        ("unverified", "runner-failed") => format!("the last run could not execute the command; fix the runner, then `jazyk test {}`", rid),
        ("unverified", _) => format!("never run; `jazyk test {}`", rid),
        ("unimplemented", _) => format!("nothing implements it; generate: `{}`", gen),
        ("partial", _) => format!("implemented in part, a criterion test fails; generate: `{}`", gen),
        ("failing", _) => "the deliverable contradicts the statement; a finding for the author (fix the docs or the code), never automatic regeneration".into(),
        ("verified", _) => "nothing; the last run passed against the current statement, test, and files".into(),
        _ => String::new(),
    }
}

// The verify goal's gate: a verdict (pass or fail) on the row with a requirement hash
// equal to the live statement hash and the test and files hashes rebaselined, which
// is exactly a row whose derived status is verified, failing, or unimplemented. Err
// names what is missing and the call that lands it.
// Mirrors docs/compiler/goals/verify.md#gate.
pub fn gate(store: &Store, gs: &GenSettings, rid: &str) -> Result<(), String> {
    let rid = store.resolve_id(rid).to_string();
    let ledger = Ledger::load(&store.out);
    let Some(row) = ledger.requirements.get(&rid) else {
        return Err(format!(
            "no ledger row for `{}`: nothing binds it, so there is no verdict to record; the row is bind work",
            rid
        ));
    };
    let (status, reason) = status_with(store, &rid, row, gs, &ledger);
    match status.as_str() {
        "verified" | "failing" | "unimplemented" | "partial" => Ok(()),
        "unverified" => Err(format!(
            "no verdict on `{}` ({}): run_tests for a programmatic row, or begin_verification then record_verdict with the package's factHash for an llm row",
            rid, reason
        )),
        "stale-requirement" => Err(format!(
            "the statement of `{}` moved since the row was recorded, so a verdict on it verifies an old sentence; {}",
            rid,
            repair_for(&status, &reason, &rid, &row.entity)
        )),
        "stale-test" | "stale-code" => Err(format!(
            "`{}` is {} ({}): the bytes moved after the last verdict, so it judged something else; run again (run_tests or record_verdict rebaselines the hashes)",
            rid, status, reason
        )),
        _ => Err(format!(
            "`{}` is {} ({}); {}",
            rid,
            status,
            reason,
            repair_for(&status, &reason, &rid, &row.entity)
        )),
    }
}

// Rows needing action, with derived status and reason. Requirements the graph holds
// but the ledger does not appear as `missing`, so ungenerated work is never silent.
// Filters: `stale` (the default) lists what the board owes a verify goal, so a
// `failing` row is left alone until its test or files change; `runnable` adds the
// `failing` rows for a rerun a person asked for; `failing` lists those alone; `all`
// lists every row. Deterministic; no model.
// Mirrors docs/compiler/tools.md#verification-tools.
pub fn pending(
    store: &Store,
    gs: &GenSettings,
    filter: Option<&str>,
    entity: Option<&str>,
) -> Vec<Value> {
    let ledger = Ledger::load(&store.out);
    let mut out = Vec::new();
    for (rid, row) in &ledger.requirements {
        if let Some(ent) = entity {
            if store.resolve_id(&row.entity) != store.resolve_id(ent) {
                continue;
            }
        }
        let (status, reason) = status_with(store, rid, row, gs, &ledger);
        // A row whose requirement left the graph is not work: no repair applies, and
        // the next record_generation prunes it. Listing it as actionable is a livelock
        // (docs/consumers/gen.md#deletion-prunes-the-ledger).
        if reason == "requirement-gone" {
            continue;
        }
        let actionable = match filter.unwrap_or("stale") {
            "all" => true,
            "failing" => status == "failing",
            "runnable" => status != "verified" && status != "unimplemented" && status != "partial",
            // Default: everything that needs a verifier's hand. Verified rows are
            // done; unimplemented rows are generation work, their bound test is the
            // acceptance gate (docs/consumers/bind.md#what-the-verdict-means); a
            // failing row is a finding for the author, re-verified only when its
            // test or files change (docs/compiler/goals/verify.md#created-when).
            _ => {
                status != "verified"
                    && status != "unimplemented"
                    && status != "partial"
                    && status != "failing"
            }
        };
        if !actionable {
            continue;
        }
        out.push(json!({
            "requirement": rid,
            "entity": row.entity,
            "status": status,
            "reason": reason,
            "repair": repair_for(&status, &reason, rid, &row.entity),
            "test": {"kind": row.test.kind, "label": row.test.label, "artifact": row.test.artifact,
                     "name": row.test.name, "run": row.test.run, "cwd": row.test.cwd},
            "lastVerdict": row.verdict,
        }));
    }
    // Graph requirements with no ledger row at all.
    for (rid, r) in &store.graph.requirements {
        if ledger.requirements.contains_key(rid) {
            continue;
        }
        let owner = r
            .entities
            .first()
            .map(|e| store.resolve_id(e).to_string())
            .unwrap_or_default();
        if let Some(ent) = entity {
            if owner != store.resolve_id(ent) {
                continue;
            }
        }
        if matches!(filter, Some("failing")) {
            continue;
        }
        out.push(json!({
            "requirement": rid,
            "entity": owner,
            "status": "missing",
            "reason": "not-generated",
            "repair": repair_for("missing", "not-generated", rid, &owner),
            "test": Value::Null,
            "lastVerdict": "none",
        }));
    }
    out
}

// The reader-facing word for a requirement, first match wins: an open question,
// then failing, stale, verified, and everything else not built.
// Mirrors docs/consumers/gen.md#status-words.
pub fn status_word(status: &str, reason: &str, question: bool) -> &'static str {
    if question {
        "question"
    } else if matches!(status, "failing" | "partial") {
        "failing"
    } else if status.starts_with("stale-") || (status == "missing" && reason == "artifact-gone") {
        "stale"
    } else if status == "verified" {
        "verified"
    } else {
        "not built"
    }
}

// The requirements an open, unanswered, unsuppressed prompted diagnostic names as a
// subject (a failed answer counts as unanswered), resolved through redirects.
// Mirrors docs/consumers/gen.md#status-words.
pub fn questioned(store: &Store) -> std::collections::BTreeSet<String> {
    store
        .graph
        .diagnostics
        .values()
        .filter(|d| d.lifecycle == "open" && d.prompt.is_some())
        .filter(|d| d.triage.as_deref() != Some("suppressed"))
        .filter(|d| d.answer.as_ref().is_none_or(|a| a.status == "failed"))
        .flat_map(|d| d.subjects.iter())
        .map(|s| store.resolve_id(s).to_string())
        .filter(|s| store.graph.requirements.contains_key(s))
        .collect()
}

// One status entry per graph requirement, for renderers (viewer, docsgen, LSP hover).
// Requirements without a ledger row read missing/not-generated.
pub fn status_map(store: &Store, gs: &GenSettings) -> std::collections::BTreeMap<String, Value> {
    let ledger = Ledger::load(&store.out);
    let mut out = std::collections::BTreeMap::new();
    let project_root = crate::project::find_root(&store.out);
    let project_path = |test: &crate::gen::TestRef| {
        project_root.as_ref().and_then(|root| {
            artifact_path(&store.out, gs, test)
                .strip_prefix(root)
                .ok()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
        })
    };
    for rid in store.graph.requirements.keys() {
        let entry = match ledger.requirements.get(rid) {
            Some(row) => {
                let (status, reason) = status_with(store, rid, row, gs, &ledger);
                json!({
                    "status": status,
                    "reason": reason,
                    "repair": repair_for(&status, &reason, rid, &row.entity),
                    "kind": row.test.kind,
                    "artifactPath": project_path(&row.test),
                    "label": row.test.label,
                    "name": row.test.name,
                    "run": row.test.run,
                    "lastRun": row.last_run,
                    "evidence": row.evidence,
                    "verdict": row.verdict,
                    // Each criterion's test and its last verdict, the implementing
                    // files, and the anchored sites (docs/frontends/gui.md#api).
                    "criteria": row.tests.iter().map(|(cid, ct)| (cid.clone(), json!({
                        "name": ct.test.name,
                        "artifact": ct.test.artifact,
                        "path": project_path(&ct.test),
                        "kind": ct.test.kind,
                        "verdict": ct.verdict,
                        "lastRun": ct.last_run,
                    }))).collect::<serde_json::Map<String, Value>>(),
                    "files": row.files,
                    "sites": row.sites.iter().map(|s| json!({ "file": s.file, "line": s.line })).collect::<Vec<_>>(),
                })
            }
            None => json!({
                "status": "missing",
                "reason": "not-generated",
                "repair": repair_for("missing", "not-generated", rid, ""),
            }),
        };
        out.insert(rid.clone(), entry);
    }
    out
}

// Counts by reason, for await_changes.
pub fn pending_counts(store: &Store, gs: &GenSettings) -> Value {
    let mut counts: std::collections::BTreeMap<String, u64> = Default::default();
    for p in pending(store, gs, Some("stale"), None) {
        *counts
            .entry(p["reason"].as_str().unwrap_or("?").to_string())
            .or_default() += 1;
    }
    json!(counts)
}
