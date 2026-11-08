pub mod artifacts;
use crate::vfs::VPathExt;
// Verification: run the tests the ledger records and feed verdicts back. Status is
// derived, never stored, from byte hashes and current Source membership. Mirrors
// docs/consumers/gen.md (the ledger, runners) and
// docs/compiler/tools.md#verification-tools.
use crate::gen::{artifact_path, hash_file, GenSettings, Ledger, ReqRow};
use crate::model::hash_hex;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

// What a judge reads: the criterion or the row's criteria, the context, and the
// implementing files. One builder for every llm verdict.
// Mirrors docs/compiler/sessions.md#payload-files.
pub(crate) const JUDGE: crate::payload::Fragments = crate::payload::Fragments(include_str!(
    "../../docs/compiler/goals/prompts/judge-input.md"
));

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

fn ensure_current_source(store: &Store, gs: &GenSettings, row: &ReqRow) -> Result<(), String> {
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
        include_str!("../../docs/compiler/goals/prompts/verify-llm-contract.md").to_owned()
    } else if row.test.run.starts_with("task:") {
        include_str!("../../docs/compiler/goals/prompts/verify-task-contract.md").to_owned()
    } else {
        include_str!("../../docs/compiler/goals/prompts/verify-legacy-contract.md").to_owned()
    };
    if row.test.kind != "llm" && row.tests.values().any(|test| test.test.kind == "llm") {
        instructions.push('\n');
        instructions.push_str(include_str!(
            "../../docs/compiler/goals/prompts/verify-llm-contract.md"
        ));
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
fn parse_verdict(reply: &str) -> Option<bool> {
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
        return Err("No LLM criterion is bound. Use run_tests for programmatic evidence.".into());
    }
    let entries = criteria.and_then(Value::as_array).ok_or_else(|| format!("Supply record_verdict.criteria for every LLM criterion: {}. Copy ids and factHash values from begin_verification. Programmatic evidence is not a model verdict.",llm.join(", ")))?;
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
            "Top-level verdict must be `{combined}` to match the LLM criteria."
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

fn task_case_outcome(result: &Value, name: &str, run_id: &str) -> Result<ProgRun, String> {
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
            "`{}` has no recorded run command, so nothing can execute it; define a saved suite task and record its test declaration with record_binding, or record_generation for entity `{}`",
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

// Report existing task evidence separately from selected ledger verdicts.
// Atomic case identities stay exact; oversized/excess entries are counted only.
fn suite_summary(runs: &[Value]) -> Value {
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

fn verification_next(
    failed: u64,
    runner_failed: u64,
    rows: &[Value],
    skipped: &[Value],
    document_findings: bool,
    generated_candidate_failure: bool,
) -> &'static str {
    if failed > 0 && document_findings {
        "a failing row has an open document diagnostic: surface it to the document author before changing the deliverable or test; do not silently rewrite prose"
    } else if failed > 0 && generated_candidate_failure {
        "a generated candidate failed current checks: compare the documented obligation, implementation and test. If the implementation or test is wrong, repair through the owning entity's permitted generation tools, record_generation and run_tests again; otherwise report the finding when repair tools are unavailable. A document contradiction needs a diagnostic and an author decision, never silent prose changes. This guidance does not schedule automatic regeneration"
    } else if failed > 0 {
        "a failing row names its requirement: compare its documented obligation, current implementation and test, then follow its status and permitted repair tools. Test failure alone does not prove the documents are wrong; a document contradiction needs a diagnostic and an author decision"
    } else if runner_failed > 0 {
        "a runner-failed row was never judged: the command could not execute; fix the runner, then run_tests again"
    } else if rows.iter().any(|row| row["verdict"] == "none") {
        "some cases remain unverified; inspect their report evidence before retrying"
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
            skipped.push(json!({"requirement":rid,"reason":"Legacy command requires a native runner; use a saved task for hosted execution.","pendingCi":true,"criteria":criteria}));
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
        reply["next"] = json!("all selected rows pass, but the executed suite has failed cases or an unsuccessful run; inspect suite.failures and run evidence before claiming whole-suite success. No owner is inferred for an unbound case");
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

// Whether an artifact on disk still stands for the live statement: a programmatic
// artifact carries the test name derived from it (the name embeds the statement hash
// prefix); a criteria file carries the requirement id and the full statement hash in
// its front matter, since the built-in worker never writes the test name into it.
// Mirrors docs/consumers/gen.md#criteria-files-for-llm-tests.
fn artifact_stands_for(content: &str, rid: &str, statement: &str) -> bool {
    content.contains(&crate::gen::test_name(rid, statement))
        || (content.contains(&format!("requirement: {}", rid))
            && content.contains(&hash_hex(statement)))
}

// Rebuild the ledger from the artifacts, running nothing. Existing rows whose artifact
// still stands for the live statement refresh their test and files hashes; a refresh
// that moves a hash drops the verdict, because the old verdict judged other bytes.
// Rows whose requirement left the graph are pruned. Lost llm rows are recreated from
// their criteria files; a lost programmatic test is reported, never recreated, since
// only the bind goal can record the command that runs it. Sites are not rebuilt. The
// requirement hash is never rewritten from the live graph.
// Mirrors docs/consumers/gen.md#runners.
pub fn audit(store: &Store, gs: &GenSettings) -> Value {
    let _lock = Ledger::lock(&store.out);
    let mut ledger = Ledger::load(&store.out);
    let mut refreshed = 0;
    let mut reset: Vec<String> = Vec::new();
    let mut rebuilt: Vec<String> = Vec::new();
    let mut found: Vec<Value> = Vec::new();
    let mut orphans: Vec<String> = Vec::new();

    let rids: Vec<String> = ledger.requirements.keys().cloned().collect();
    for rid in rids {
        let row = ledger.requirements.get(&rid).unwrap().clone();
        let Some(r) = store.graph.requirements.get(store.resolve_id(&rid)) else {
            orphans.push(rid.clone());
            continue;
        };
        let artifact = artifact_path(&store.out, gs, &row.test);
        let content = crate::vfs::read_to_string(&artifact).unwrap_or_default();
        if !artifact_stands_for(&content, &rid, &r.statement) {
            continue;
        }
        if row.test.kind == "llm"
            && crate::gen::validate_criteria_file(store, &rid, None, &artifact).is_err()
        {
            continue;
        }
        let hashes = crate::gen::RowHashes {
            requirement: row.hashes.requirement.clone(),
            test: hash_file(&artifact),
            files: crate::gen::hash_implementing(&store.out, gs, &row.files),
        };
        let row = ledger.requirements.get_mut(&rid).unwrap();
        if hashes != row.hashes {
            // The verdict judged the bytes the old hashes name, not these. An audit
            // never turns a hand edit into a verified row nothing reran
            // (docs/consumers/gen.md#runners).
            if row.verdict != "none" {
                row.verdict = "none".into();
                row.exit_code = None;
                reset.push(rid.clone());
            }
            rebaseline_files(row, hashes.files.clone());
            row.hashes = hashes;
        }
        refreshed += 1;
    }
    for rid in &orphans {
        ledger.requirements.remove(rid);
        ledger.contradicted.remove(rid);
    }

    // Lost rows: scan for an artifact standing for the live statement. A criteria
    // file carries everything an llm row needs; a programmatic test found in the
    // deliverable is reported, and the requirement stays missing so the bind goal
    // records the command that runs it (docs/consumers/gen.md#runners).
    let scan: Vec<(PathBuf, bool)> = {
        let mut v = Vec::new();
        collect_files(&gs.deliverable, &mut v, false);
        collect_files(&store.out.join("gen").join("criteria"), &mut v, true);
        v
    };
    // Each scanned file is read once, the first time a requirement asks for it.
    let mut contents: Vec<Option<Option<String>>> = vec![None; scan.len()];
    for (rid, r) in &store.graph.requirements {
        if ledger.requirements.contains_key(rid) {
            continue;
        }
        let name = crate::gen::test_name(rid, &r.statement);
        for (i, (path, is_criteria)) in scan.iter().enumerate() {
            let Some(content) = contents[i]
                .get_or_insert_with(|| crate::vfs::read_to_string(path).ok())
                .as_deref()
            else {
                continue;
            };
            if !artifact_stands_for(content, rid, &r.statement) {
                continue;
            }
            let owner = r
                .entities
                .first()
                .map(|e| store.resolve_id(e).to_string())
                .unwrap_or_default();
            if !*is_criteria {
                let rel = path
                    .strip_prefix(&gs.deliverable)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push(json!({
                    "requirement": rid,
                    "artifact": rel,
                    "name": name,
                    "note": repair_for("missing", "not-generated", rid, &owner),
                }));
                break;
            }
            if crate::gen::validate_criteria_file(store, rid, None, path).is_err() {
                continue;
            }
            let artifact_rel = path
                .strip_prefix(store.out.join("gen"))
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let files = ledger
                .entities
                .get(crate::ids::entity_slug(&owner))
                .map(|e| e.files.clone())
                .unwrap_or_default();
            let test = crate::gen::TestRef {
                kind: "llm".into(),
                label: "llm".into(),
                artifact: artifact_rel,
                name: name.clone(),
                run: format!("jazyk test {}", rid),
                cwd: ".".into(),
            };
            // The artifact carries the live statement hash, so the requirement
            // baseline is honest here.
            let hashes = crate::gen::RowHashes {
                requirement: hash_hex(&r.statement),
                test: hash_file(path),
                files: crate::gen::hash_implementing(&store.out, gs, &files),
            };
            let tests = ledger
                .requirements
                .get(rid.as_str())
                .map(|row| row.tests.clone())
                .unwrap_or_default();
            ledger.requirements.insert(
                rid.clone(),
                ReqRow {
                    tests,
                    entity: owner,
                    files,
                    sites: Vec::new(),
                    test,
                    hashes,
                    verdict: "none".into(),
                    last_run: None,
                    exit_code: None,
                    evidence: None,
                },
            );
            rebuilt.push(rid.clone());
            break;
        }
    }
    let saved = ledger.save(&store.out);
    let mut reply = json!({
        "refreshed": refreshed,
        "verdictReset": reset,
        "rebuilt": rebuilt,
        "found": found,
        "prunedOrphans": orphans,
    });
    if !reset.is_empty() {
        reply["verdictResetNote"] = json!(
            "these rows' test or files bytes moved since their last run, so the verdict was dropped; `jazyk test` reruns them"
        );
    }
    if !found.is_empty() {
        reply["foundNote"] = json!(
            "programmatic tests found for requirements with no row; a row needs the command that runs the test, which only binding records, so each stays missing until bound"
        );
    }
    if let Err(e) = saved {
        reply["error"] = json!(e);
    }
    reply
}

// Files under a root, skipping build and VCS directories, bounded to text-sized files.
fn collect_files(root: &std::path::Path, v: &mut Vec<(PathBuf, bool)>, is_criteria: bool) {
    let Ok(rd) = crate::vfs::read_dir(root) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if path.vis_dir() {
            if !matches!(name.as_str(), "target" | "node_modules" | ".git")
                && !name.starts_with(".jazyk")
                && !name.starts_with("jazyk-out")
                && !crate::vfs::is_symlink(&path)
            {
                collect_files(&path, v, is_criteria);
            }
        } else if e.metadata().map(|m| m.len() <= 512 * 1024).unwrap_or(false) {
            v.push((path, is_criteria));
        }
    }
}

// UTC ISO 8601 from the system clock, no dependencies. Civil-from-days per Howard
// Hinnant's algorithm.
pub fn now_iso() -> String {
    let secs = crate::sys::now_unix() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mth <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mth, d, h, m, s)
}

#[cfg(test)]
mod tests {
    // A directory link back to an ancestor is skipped, not followed in a loop.
    #[cfg(unix)]
    #[test]
    fn file_walk_skips_symlinked_directories() {
        let root =
            std::env::temp_dir().join(format!("jazyk-verify-symlink-{}", crate::sys::process_id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("tests/a.rs"), "fn a() {}").unwrap();
        std::os::unix::fs::symlink(&root, root.join("tests/loop")).unwrap();
        let mut v = Vec::new();
        super::collect_files(&root, &mut v, false);
        assert_eq!(v, vec![(root.join("tests/a.rs"), false)]);
        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn suite_failure_summary_counts_unselected_cases_without_inventing_owners() {
        let mut tests = (0..25).map(|n| json!({"identity":format!("OtherSuite::case_{n}"),"status":"failed","report":"reports/other.xml"})).collect::<Vec<_>>();
        tests.push(json!({"identity":"selected","status":"passed"}));
        tests.push(json!({"identity":"skipped","status":"skipped"}));
        let summary = suite_summary(&[
            json!({"id":"run-1","task":"shared-suite","state":"failed","tests":tests}),
        ]);
        assert_eq!(summary["passedCases"], 1);
        assert_eq!(summary["failedCases"], 25);
        assert_eq!(summary["skippedCases"], 1);
        assert_eq!(summary["unsuccessfulRuns"], 1);
        assert_eq!(summary["failures"].as_array().unwrap().len(), 20);
        assert_eq!(summary["omittedFailures"], 5);
        assert_eq!(summary["failures"][0]["identity"], "OtherSuite::case_0");
        assert!(summary["failures"][0].get("owner").is_none());
        assert!(summary["failures"][0].get("requirement").is_none());
    }

    #[test]
    fn suite_failure_summary_keeps_atomic_identities_and_counts_oversized_entries() {
        let summary = suite_summary(&[
            json!({"id":"run-1","task":"shared-suite","state":"failed","tests":[
                {"identity":"x".repeat(40_000),"status":"failed","report":"reports/a.xml"},
                {"identity":"Exact::case","status":"failed","report":"reports/b.xml"}
            ]}),
        ]);
        assert_eq!(summary["failedCases"], 2);
        assert_eq!(summary["omittedFailures"], 1);
        assert_eq!(summary["failures"].as_array().unwrap().len(), 1);
        assert_eq!(summary["failures"][0]["identity"], "Exact::case");
        assert!(summary["failures"].to_string().len() < 32 * 1024);
    }

    #[test]
    fn generated_candidate_failure_guidance_permits_guarded_repair_without_rescheduling() {
        let next = verification_next(1, 0, &[], &[], false, true);
        assert!(next.contains("generated candidate"));
        assert!(next.contains("owning entity's permitted generation tools"));
        assert!(next.contains("record_generation and run_tests"));
        assert!(next.contains("does not schedule automatic regeneration"));
        assert!(!next.contains("not a row to regenerate"));
    }

    #[test]
    fn document_diagnostic_guidance_takes_priority_over_candidate_repair() {
        let next = verification_next(1, 0, &[], &[], true, true);
        assert!(next.contains("document author") && next.contains("do not silently rewrite prose"));
        assert!(!next.contains("record_generation"));
        let runner = verification_next(0, 1, &[], &[], false, false);
        assert!(runner.contains("never judged") && runner.contains("fix the runner"));
        let pass = verification_next(0, 0, &[json!({"verdict":"pass"})], &[], false, false);
        assert_eq!(pass, "all selected rows pass");
    }

    #[test]
    fn explicitly_selected_unbound_requirement_cannot_report_all_passed() {
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let out = std::path::Path::new("/project/.jazyk");
        let (store, gs) = fixture(out);
        let mut ledger = Ledger::load(out);
        ledger.requirements.clear();
        ledger.save(out).unwrap();
        for target in ["req:shop-1", "ent:cart"] {
            let result = run_selected(&store, &gs, &[target.into()]).unwrap();
            assert_eq!(result["passed"], 0);
            assert_eq!(result["skipped"][0]["requirement"], "req:shop-1");
            assert_ne!(result["next"], "all selected rows pass");
        }
        let empty = run_selected(&store, &gs, &["unknown".into()]).unwrap();
        assert_ne!(empty["next"], "all selected rows pass");
    }

    #[test]
    fn task_verification_package_uses_report_evidence() {
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let out = std::path::Path::new("/project/.jazyk");
        let (store, gs) = fixture(out);
        let mut ledger = Ledger::load(out);
        ledger.requirements.get_mut("req:shop-1").unwrap().test.run = "task:check".into();
        ledger.save(out).unwrap();
        let package = task(&store, "req:shop-1", &gs).unwrap();
        assert!(package["instructions"]
            .as_str()
            .unwrap()
            .contains("run_tests"));
        assert!(!package["instructions"]
            .as_str()
            .unwrap()
            .contains("Exit code 0 is a pass"));
    }

    #[test]
    fn task_reruns_require_a_unique_completed_case_verdict() {
        let result = serde_json::json!({"complete":true,"exit_code":1,"tests":[
            {"identity":"A::same","status":"passed"},
            {"identity":"B::same","status":"failed"},
            {"identity":"A::skipped","status":"skipped"}
        ]});
        assert!(
            super::task_case_outcome(&result, "A::same", "r1")
                .unwrap()
                .pass
        );
        assert!(
            !super::task_case_outcome(&result, "B::same", "r1")
                .unwrap()
                .pass
        );
        for selector in ["same", "A::skipped", "missing"] {
            assert!(super::task_case_outcome(&result, selector, "r1").is_err());
        }
        let mut incomplete = result.clone();
        incomplete["complete"] = serde_json::json!(false);
        assert!(super::task_case_outcome(&incomplete, "A::same", "r1").is_err());
    }
    // The judge contract puts the verdict on the first line; a reasoning-first reply
    // is read from its conclusion. Mirrors docs/compiler/goals/verify.md#the-one-shot-form.
    #[test]
    fn verdict_parse_reads_the_verdict_line_then_the_conclusion() {
        assert_eq!(parse_verdict("PASS\nAll criteria met."), Some(true));
        assert_eq!(
            parse_verdict("FAIL\nThe criteria says PASS only when the color is set."),
            Some(false)
        );
        // Reasoning first, quoting the criteria, conclusion last: the old
        // first-occurrence parse read this as a fail.
        assert_eq!(
            parse_verdict(
                "The criteria says FAIL when the title is missing. It is present.\nTherefore: PASS"
            ),
            Some(true)
        );
        assert_eq!(
            parse_verdict("Checked everything. The row does not hold. FAIL"),
            Some(false)
        );
        assert_eq!(
            parse_verdict("PASS. Note the criteria would FAIL a missing title."),
            Some(true)
        );
        assert_eq!(
            parse_verdict("**PASS**\n\nEvery criterion holds."),
            Some(true)
        );
        assert_eq!(
            parse_verdict("Verdict: FAIL\nThe title is absent."),
            Some(false)
        );
        assert_eq!(parse_verdict("no verdict words here"), None);
        assert_eq!(parse_verdict(""), None);
    }
    // Verdict words match whole words only, so a reply mentioning a password, a
    // bypass, or a failover is not read as a verdict.
    #[test]
    fn verdict_parse_matches_whole_words_only() {
        assert_eq!(parse_verdict("Password field is shown."), None);
        assert_eq!(parse_verdict("The bypass route is absent."), None);
        assert_eq!(parse_verdict("Failover is configured."), None);
        assert_eq!(
            parse_verdict("PASSWORD reset works, but the lockout is missing.\nFAIL"),
            Some(false)
        );
        assert_eq!(
            parse_verdict("The failover check holds and the bypass is closed. PASS"),
            Some(true)
        );
        assert_eq!(
            parse_verdict("FAIL\nThe password policy is absent, a bypass remains."),
            Some(false)
        );
    }

    use super::*;
    use crate::gen::{mark as gen_mark, test_name, GenSettings};
    use crate::model::*;

    pub(super) fn fixture(out: &std::path::Path) -> (Store, GenSettings) {
        let mut s = Store {
            out: out.to_path_buf(),
            ..Default::default()
        };
        s.graph.entities.insert(
            "ent:cart".into(),
            Entity {
                name: "Cart".into(),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                statement: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "holds".into(),
                }),
                ..Default::default()
            },
        );
        let gs = GenSettings {
            configured: true,
            deliverable: out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        crate::vfs::create_dir_all(gs.deliverable.join("src")).unwrap();
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// product").unwrap();
        crate::vfs::write(
            gs.deliverable.join("tests/cart.rs"),
            format!("// req:shop-1 hash:x\nfn {}() {{}}\n", name),
        )
        .unwrap();
        let manifest = serde_json::json!({
            "files": ["src/cart.rs", "tests/cart.rs"],
            "tests": [{
                "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
                "artifact": "tests/cart.rs", "name": name, "run": "true",
                "files": ["src/cart.rs"],
            }],
        });
        gen_mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
        (s, gs)
    }

    #[test]
    fn task_reports_update_each_criterion_without_refreshing_its_statement() {
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let out = std::path::Path::new("/project/.jazyk");
        let (mut store, gs) = fixture(out);
        let mut ledger = Ledger::load(out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        row.test.run = "task:check".into();
        let name = row.test.name.clone();
        for (id, selector) in [
            ("c:pass", name.as_str()),
            ("c:skip", "skipped"),
            ("c:missing", "missing"),
        ] {
            store.graph.criteria.insert(
                id.into(),
                Criterion {
                    requirement: "req:shop-1".into(),
                    statement: id.into(),
                    ..Default::default()
                },
            );
            let mut test = row.test.clone();
            test.name = selector.into();
            row.tests.insert(
                id.into(),
                crate::gen::CritTest {
                    test,
                    criterion_hash: hash_hex(id),
                    test_hash: row.hashes.test.clone(),
                    verdict: "pass".into(),
                    last_run: None,
                    exit_code: None,
                    evidence: None,
                },
            );
        }
        ledger.save(out).unwrap();
        let task: crate::task_runtime::Task = serde_json::from_value(json!({"id":"check","name":"Check","description":"Check all cases","command":"mvn test"})).unwrap();
        let result = json!({"exit_code":0,"tests":[{"identity":format!("Suite::{name}"),"status":"passed"},{"identity":"Suite::skipped","status":"skipped"}]});
        crate::task_runtime::import_evidence(&store, &gs, &task, &result, "first");
        let ledger = Ledger::load(out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.verdict, "pass");
        assert_eq!(row.tests["c:skip"].verdict, "none");
        assert_eq!(row.tests["c:missing"].verdict, "none");
        assert_eq!(
            status_of(&store, "req:shop-1", row, &gs).1,
            "criteria-incomplete"
        );
        store.graph.criteria.get_mut("c:pass").unwrap().statement = "Changed obligation".into();
        crate::task_runtime::import_evidence(&store, &gs, &task, &result, "second");
        let ledger = Ledger::load(out);
        assert_eq!(
            ledger.requirements["req:shop-1"].tests["c:pass"].criterion_hash,
            hash_hex("c:pass")
        );
        assert_eq!(
            status_of(
                &store,
                "req:shop-1",
                &ledger.requirements["req:shop-1"],
                &gs
            )
            .0,
            "stale-requirement"
        );
    }

    pub(super) fn multi_llm_fixture(out: &std::path::Path) -> (Store, GenSettings) {
        let (mut store, gs) = fixture(out);
        let mut ledger = Ledger::load(out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        for id in ["crit:first", "crit:second"] {
            store.graph.criteria.insert(
                id.into(),
                Criterion {
                    requirement: "req:shop-1".into(),
                    statement: format!("Check {id}"),
                    ..Default::default()
                },
            );
            let mut test = row.test.clone();
            test.kind = "llm".into();
            test.name = id.into();
            test.run = "llm".into();
            test.artifact = format!("tests/{}.md", id.replace(':', "-"));
            let hash = hash_hex(&store.graph.criteria[id].statement);
            crate::vfs::write(gs.deliverable.join(&test.artifact),format!("---\nrequirement: req:shop-1\ncriterion: {id}\nhash: {hash}\n---\nInspect source. PASS if satisfied. FAIL otherwise.\n")).unwrap();
            row.tests.insert(
                id.into(),
                crate::gen::CritTest {
                    test_hash: hash_file(&artifact_path(out, &gs, &test)),
                    test,
                    criterion_hash: hash,
                    verdict: "pass".into(),
                    last_run: None,
                    exit_code: None,
                    evidence: None,
                },
            );
        }
        row.test = row.tests["crit:first"].test.clone();
        row.hashes.test = row.tests["crit:first"].test_hash.clone();
        row.verdict = "pass".into();
        ledger.save(out).unwrap();
        (store, gs)
    }

    fn criterion_judgments(store: &Store, values: [&str; 2]) -> Value {
        json!(["crit:first","crit:second"].into_iter().zip(values).map(|(cid,verdict)|json!({"criterion":cid,"verdict":verdict,"factHash":hash_hex(&store.graph.criteria[cid].statement),"evidence":"Inspected current source"})).collect::<Vec<_>>())
    }

    #[test]
    fn llm_criteria_are_packaged_and_recorded_independently_and_atomically() {
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let out = std::path::Path::new("/project/.jazyk");
        let (store, gs) = multi_llm_fixture(out);
        let package = task(&store, "req:shop-1", &gs).unwrap();
        assert_eq!(package["criterionTests"].as_array().unwrap().len(), 2);
        assert!(package["criterionTests"][1]["criteria"]
            .as_str()
            .unwrap()
            .contains("crit:second"));
        let original = serde_json::to_value(Ledger::load(out)).unwrap();
        assert!(
            mark_llm(&store, "req:shop-1", "fail", None, None, None, &gs)
                .unwrap_err()
                .contains("criteria")
        );
        let valid = criterion_judgments(&store, ["fail", "fail"]);
        let mut stale = valid.clone();
        stale[1]["factHash"] = json!("old");
        for invalid in [json!([valid[0]]), json!([valid[0], valid[0]]), stale] {
            assert!(mark_llm(
                &store,
                "req:shop-1",
                "fail",
                None,
                None,
                Some(&invalid),
                &gs
            )
            .is_err());
            assert_eq!(serde_json::to_value(Ledger::load(out)).unwrap(), original);
        }
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// changed source").unwrap();
        let result = mark_llm(
            &store,
            "req:shop-1",
            "fail",
            None,
            Some("Both unmet"),
            Some(&valid),
            &gs,
        )
        .unwrap();
        assert_eq!(result["status"], "failing");
        let ledger = Ledger::load(out);
        assert!(ledger.requirements["req:shop-1"]
            .tests
            .values()
            .all(|t| t.verdict == "fail"));
        let split = criterion_judgments(&store, ["pass", "fail"]);
        assert_eq!(
            mark_llm(&store, "req:shop-1", "fail", None, None, Some(&split), &gs).unwrap()
                ["status"],
            "failing"
        );
        let split_row = Ledger::load(out).requirements["req:shop-1"].clone();
        assert_eq!(split_row.tests["crit:first"].verdict, "pass");
        assert_eq!(split_row.tests["crit:second"].verdict, "fail");
        let green = criterion_judgments(&store, ["pass", "pass"]);
        assert_eq!(
            mark_llm(&store, "req:shop-1", "pass", None, None, Some(&green), &gs).unwrap()
                ["status"],
            "verified"
        );
    }

    #[test]
    fn fresh_task_report_does_not_launder_an_old_llm_verdict_after_source_changes() {
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let out = std::path::Path::new("/project/.jazyk");
        let (store, gs) = multi_llm_fixture(out);
        let mut ledger = Ledger::load(out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        let test = row.tests.get_mut("crit:second").unwrap();
        test.test.kind = "programmatic".into();
        test.test.run = "task:check".into();
        test.test.name = "case".into();
        test.test.artifact = "tests/cart.rs".into();
        test.test_hash = hash_file(&artifact_path(out, &gs, &test.test));
        ledger.save(out).unwrap();
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// changed behavior").unwrap();
        let task: crate::task_runtime::Task = serde_json::from_value(
            json!({"id":"check","name":"Check","description":"Check cases","command":"pytest"}),
        )
        .unwrap();
        crate::task_runtime::import_evidence(
            &store,
            &gs,
            &task,
            &json!({"exit_code":0,"tests":[{"identity":"Suite::case","status":"passed"}]}),
            "fresh",
        );
        let ledger = Ledger::load(out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.tests["crit:first"].verdict, "none");
        assert_eq!(row.tests["crit:second"].verdict, "pass");
        assert_eq!(status_of(&store, "req:shop-1", row, &gs).0, "unverified");
        assert_eq!(
            mark_llm(
                &store,
                "req:shop-1",
                "pass",
                None,
                Some("Current source judged"),
                None,
                &gs
            )
            .unwrap()["status"],
            "verified"
        );
    }

    #[test]
    fn mixed_programmatic_headline_keeps_both_guides_and_independent_evidence() {
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let out = std::path::Path::new("/project/.jazyk");
        let (store, gs) = multi_llm_fixture(out);
        let mut ledger = Ledger::load(out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        let test = row.tests.get_mut("crit:second").unwrap();
        test.test.kind = "programmatic".into();
        test.test.run = "task:check".into();
        test.test.name = "case".into();
        test.test.artifact = "tests/cart.rs".into();
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"), "fn case() {}\n").unwrap();
        test.test_hash = hash_file(&artifact_path(out, &gs, &test.test));
        row.test = test.test.clone();
        row.hashes.test = test.test_hash.clone();
        ledger.save(out).unwrap();
        let package = task(&store, "req:shop-1", &gs).unwrap();
        let instructions = package["instructions"].as_str().unwrap();
        assert!(instructions.contains("Run `run_tests`"));
        assert!(instructions.contains("Inspect every LLM entry"));
        // Legacy commands need the same harness-owned evidence, plus LLM guidance.
        let mut legacy = Ledger::load(out);
        legacy.requirements.get_mut("req:shop-1").unwrap().test.run = "pytest -k case".into();
        legacy.save(out).unwrap();
        let package = task(&store, "req:shop-1", &gs).unwrap();
        let instructions = package["instructions"].as_str().unwrap();
        assert!(instructions.contains("Run `run_tests`"));
        assert!(instructions.contains("cannot execute the legacy command"));
        assert!(instructions.contains("Inspect every LLM entry"));
        ledger.save(out).unwrap();
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// changed source").unwrap();
        let judgments = json!([criterion_judgments(&store, ["pass", "pass"])[0]]);
        mark_llm(
            &store,
            "req:shop-1",
            "pass",
            None,
            None,
            Some(&judgments),
            &gs,
        )
        .unwrap();
        let row = Ledger::load(out).requirements["req:shop-1"].clone();
        assert_eq!(row.verdict, "none");
        assert_eq!(row.tests["crit:first"].verdict, "pass");
        assert_eq!(row.tests["crit:second"].verdict, "none");
        assert_eq!(status_of(&store, "req:shop-1", &row, &gs).0, "unverified");
        let task = serde_json::from_value(
            json!({"id":"check","name":"Check","description":"Run cases","command":"pytest"}),
        )
        .unwrap();
        crate::task_runtime::import_evidence(
            &store,
            &gs,
            &task,
            &json!({"exit_code":0,"tests":[{"identity":"Suite::case","status":"passed"}]}),
            "fresh",
        );
        let row = Ledger::load(out).requirements["req:shop-1"].clone();
        assert_eq!(status_of(&store, "req:shop-1", &row, &gs).0, "verified");
    }

    #[test]
    fn built_in_verifier_judges_every_llm_criterion_and_handles_inconclusive_output() {
        use std::cell::RefCell;
        struct Answers(RefCell<std::collections::VecDeque<String>>);
        impl crate::runner::SessionRunner for Answers {
            fn agent_name(&self) -> String {
                "fixture".into()
            }
            fn set_build_token(&self, _: Option<String>) {}
            fn run_item(
                &self,
                _: &crate::runner::BatchRun,
                _: &crate::session::Trace,
            ) -> crate::runner::SessionReport {
                panic!("No agent batch expected")
            }
            fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
                panic!("Use traced asks")
            }
            fn ask_traced(
                &self,
                system: &str,
                user: &str,
                _: &str,
                _: &str,
                _: Option<&crate::session::Trace>,
            ) -> Result<String, String> {
                assert!(system.contains("Judge only"));
                assert!(user.contains("Implementing files:"));
                Ok(self
                    .0
                    .borrow_mut()
                    .pop_front()
                    .expect("Exactly one request per criterion"))
            }
        }
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let _command = crate::session_recovery::Scope::enter(None);
        let out = std::path::Path::new("/project/.jazyk");
        let (store, gs) = multi_llm_fixture(out);
        let trace = crate::session::Trace::to_sink(
            crate::session::TraceLevel::Quiet,
            std::sync::Arc::new(|_| {}),
            Default::default(),
        );
        let runner = Answers(RefCell::new(
            ["PASS first observed".into(), "FAIL second unmet".into()].into(),
        ));
        let report = run_all(&store, &runner, &gs, &[], Some("llm"), true, &trace).unwrap();
        assert_eq!(report["verified"], 0);
        assert_eq!(report["failing"], 1);
        assert!(runner.0.borrow().is_empty());
        let row = Ledger::load(out).requirements["req:shop-1"].clone();
        assert_eq!(row.tests["crit:first"].verdict, "pass");
        assert_eq!(row.tests["crit:second"].verdict, "fail");
        // A lost command reply reuses its completed result and counters. No
        // criterion judgment is admitted a second time under the same execution.
        let no_calls = Answers(RefCell::new(Default::default()));
        assert_eq!(
            run_all(&store, &no_calls, &gs, &[], Some("llm"), true, &trace).unwrap(),
            report
        );
        let _next_command = crate::session_recovery::Scope::enter(None);
        let runner = Answers(RefCell::new(["No judgment available".into()].into()));
        run_all(&store, &runner, &gs, &[], Some("llm"), true, &trace).unwrap();
        assert!(Ledger::load(out).requirements["req:shop-1"]
            .tests
            .values()
            .all(|t| t.verdict == "none"));
    }

    #[test]
    fn a_review_transport_error_stops_before_the_next_criterion() {
        struct Broken(std::cell::Cell<usize>);
        impl crate::runner::SessionRunner for Broken {
            fn agent_name(&self) -> String {
                "broken".into()
            }
            fn set_build_token(&self, _: Option<String>) {}
            fn run_item(
                &self,
                _: &crate::runner::BatchRun,
                _: &crate::session::Trace,
            ) -> crate::runner::SessionReport {
                unreachable!()
            }
            fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
                unreachable!()
            }
            fn ask_traced(
                &self,
                _: &str,
                _: &str,
                _: &str,
                _: &str,
                _: Option<&crate::session::Trace>,
            ) -> Result<String, String> {
                self.0.set(self.0.get() + 1);
                Err("Review model transport closed.".into())
            }
        }
        let _vfs = crate::vfs::Scope::enter(&Default::default());
        let out = std::path::Path::new("/project/.jazyk/review-error");
        let (store, gs) = multi_llm_fixture(out);
        let before = Ledger::load(out);
        let runner = Broken(std::cell::Cell::new(0));
        let trace = crate::session::Trace::to_sink(
            crate::session::TraceLevel::Normal,
            std::sync::Arc::new(|_| {}),
            Default::default(),
        );
        let error = run_without_build(&store, &runner, &gs, &[], Some("llm"), true, &trace, false)
            .unwrap_err();
        assert_eq!(error, "Review model transport closed.");
        assert_eq!(runner.0.get(), 1);
        let after = Ledger::load(out);
        for (id, test) in &before.requirements["req:shop-1"].tests {
            assert_eq!(
                after.requirements["req:shop-1"].tests[id].verdict,
                test.verdict
            );
        }
    }

    #[test]
    fn audit_clears_all_criterion_verdicts_when_implementing_files_change() {
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let out = std::path::Path::new("/project/.jazyk");
        let (store, gs) = multi_llm_fixture(out);
        let mut ledger = Ledger::load(out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        // Audit recognizes a criteria headline by the requirement statement hash.
        let path = artifact_path(out, &gs, &row.test);
        let mut text = crate::vfs::read_to_string(&path).unwrap();
        text.push_str(&format!("\n{}\n", row.hashes.requirement));
        crate::vfs::write(path, &text).unwrap();
        row.hashes.test = hash_hex(&text);
        row.tests.get_mut("crit:first").unwrap().test_hash = row.hashes.test.clone();
        ledger.save(out).unwrap();
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// source changed").unwrap();
        audit(&store, &gs);
        assert!(Ledger::load(out).requirements["req:shop-1"]
            .tests
            .values()
            .all(|t| t.verdict == "none"));
    }

    #[test]
    fn hosted_programmatic_tests_remain_pending_ci_without_false_verdicts() {
        let _scope = crate::vfs::Scope::enter(&Default::default());
        let out = std::path::Path::new("/project/.jazyk");
        let (store, gs) = fixture(out);
        let before = Ledger::load(out);
        let result = run_selected(&store, &gs, &[]).unwrap();
        assert_eq!(result["pendingCi"], true);
        assert_eq!(result["passed"], 0);
        assert_eq!(result["failed"], 0);
        assert_eq!(
            serde_json::to_value(Ledger::load(out)).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        let project = crate::project::Project::load(std::path::Path::new("/project"));
        let llm = crate::llm::Llm {
            api: "chat_completions".into(),
            reasoning_summary: None,
            base_url: "must-not-connect".into(),
            model: "unused".into(),
            api_key: String::new(),
            temperature: None,
            trace: None,
        };
        let runner = crate::inprocess::InProcessRunner::start(&project, &llm, out).unwrap();
        let trace = crate::session::Trace::to_sink(
            crate::session::TraceLevel::Quiet,
            std::sync::Arc::new(|_| {}),
            Default::default(),
        );
        let report = run_all(&store, &runner, &gs, &[], None, false, &trace).unwrap();
        assert_eq!(report["verified"], 0);
        assert_eq!(report["failing"], 0);
        assert_eq!(report["pendingCi"].as_array().unwrap().len(), 1);
        assert_eq!(Ledger::load(out).requirements["req:shop-1"].verdict, "none");
    }

    // A row whose requirement left the graph is not actionable work: the queue
    // excludes it, and the next record buries it.
    // Mirrors docs/consumers/gen.md#deletion-prunes-the-ledger.
    #[test]
    fn requirement_gone_rows_are_not_actionable() {
        let out =
            std::env::temp_dir().join(format!("jazyk-verify-gone-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        // The row exists; then the requirement is deleted from the graph.
        s.graph.requirements.remove("req:shop-1");
        let listed = pending(&s, &gs, Some("all"), None);
        assert!(
            !listed.iter().any(|p| p["requirement"] == "req:shop-1"),
            "{:?}",
            listed
        );
    }

    // A command that could not run has judged nothing: the row stays unverified with
    // reason runner-failed, and a real verdict later clears it.
    // Mirrors docs/consumers/gen.md#a-test-that-could-not-run-says-nothing.
    #[test]
    fn a_runner_failure_is_not_a_failing_requirement() {
        let out =
            std::env::temp_dir().join(format!("jazyk-verify-runner-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);

        record_runner_failure(&s, "req:shop-1", 127, "sh: pytest: command not found").unwrap();
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.verdict, "none");
        assert_eq!(row.exit_code, Some(127));
        let (status, reason) = status_of(&s, "req:shop-1", row, &gs);
        assert_eq!(status, "unverified");
        assert_eq!(reason, "runner-failed");

        // A verdict from a working runner clears the mark and keeps the run's exit
        // code beside the output (docs/consumers/gen.md#runners).
        mark(
            &s,
            "req:shop-1",
            "fail",
            None,
            Some("assert failed"),
            Some(1),
            &gs,
        )
        .unwrap();
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.exit_code, Some(1));
        assert_eq!(status_of(&s, "req:shop-1", row, &gs).0, "failing");

        // A judged verdict carries no exit code: the field stays programmatic-only.
        mark(&s, "req:shop-1", "pass", None, Some("judged"), None, &gs).unwrap();
        let ledger = Ledger::load(&out);
        assert_eq!(ledger.requirements["req:shop-1"].exit_code, None);
        crate::vfs::remove_dir_all(&out).ok();
    }

    // The run_tests path never records a runner failure as a failing requirement:
    // exit 127 routes through record_runner_failure, the verdict stays none.
    // Mirrors docs/consumers/gen.md#a-test-that-could-not-run-says-nothing.
    #[test]
    fn run_selected_routes_runner_failures() {
        let out = std::env::temp_dir().join(format!("jazyk-runsel-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        {
            let mut ledger = Ledger::load(&out);
            ledger.requirements.get_mut("req:shop-1").unwrap().test.run =
                "jazyk-no-such-runner-xyz".into();
            ledger.save(&out).unwrap();
        }
        let reply = run_selected(&s, &gs, &[]).unwrap();
        assert_eq!(reply["failed"], 0, "{}", reply);
        assert_eq!(reply["runnerFailed"], 1, "{}", reply);
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.verdict, "none");
        assert_eq!(row.exit_code, Some(127));
        assert_eq!(status_of(&s, "req:shop-1", row, &gs).1, "runner-failed");
        crate::vfs::remove_dir_all(&out).ok();
    }

    // A runner failure on the headline clears the criterion entry that is the
    // headline's test: a stale verdict there must not outlive the failed run
    // (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
    #[test]
    fn headline_runner_failure_clears_its_criterion_entry() {
        let out = std::env::temp_dir().join(format!(
            "jazyk-headline-runner-{}",
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut store, gs) = fixture(&out);
        store.graph.criteria.insert(
            "c:check".into(),
            Criterion {
                requirement: "req:shop-1".into(),
                statement: "Check items".into(),
                ..Default::default()
            },
        );
        let mut ledger = Ledger::load(&out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        row.tests.insert(
            "c:check".into(),
            crate::gen::CritTest {
                test: row.test.clone(),
                criterion_hash: hash_hex("Check items"),
                test_hash: row.hashes.test.clone(),
                verdict: "fail".into(),
                last_run: None,
                exit_code: Some(1),
                evidence: Some("earlier fail".into()),
            },
        );
        ledger.save(&out).unwrap();
        record_runner_failure(&store, "req:shop-1", 127, "sh: not found").unwrap();
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.tests["c:check"].verdict, "none");
        assert_eq!(row.tests["c:check"].exit_code, Some(127));
        assert_eq!(status_of(&store, "req:shop-1", row, &gs).1, "runner-failed");
        crate::vfs::remove_dir_all(&out).ok();
    }

    // A criterion test sharing the headline's command shares its execution and gets
    // its verdict (docs/consumers/gen.md#test-artifact-selection).
    #[test]
    fn criterion_sharing_the_headline_command_gets_a_verdict() {
        let out =
            std::env::temp_dir().join(format!("jazyk-shared-command-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut store, gs) = fixture(&out);
        store.graph.criteria.insert(
            "c:other".into(),
            Criterion {
                requirement: "req:shop-1".into(),
                statement: "Other check".into(),
                ..Default::default()
            },
        );
        let artifact = gs.deliverable.join("tests/cart.rs");
        let text = crate::vfs::read_to_string(&artifact).unwrap();
        crate::vfs::write(&artifact, format!("{text}fn criterion_other() {{}}\n")).unwrap();
        let run = "echo ran >> runs.txt";
        let mut ledger = Ledger::load(&out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        row.test.run = run.into();
        let mut test = row.test.clone();
        test.name = "criterion_other".into();
        row.tests.insert(
            "c:other".into(),
            crate::gen::CritTest {
                test,
                criterion_hash: hash_hex("Other check"),
                test_hash: String::new(),
                verdict: "none".into(),
                last_run: None,
                exit_code: None,
                evidence: None,
            },
        );
        ledger.save(&out).unwrap();
        let reply = run_selected(&store, &gs, &[]).unwrap();
        assert_eq!(reply["passed"], 1, "{reply}");
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.tests["c:other"].verdict, "pass");
        assert_eq!(status_of(&store, "req:shop-1", row, &gs).0, "verified");
        let runs = crate::vfs::read_to_string(gs.deliverable.join("runs.txt")).unwrap();
        assert_eq!(runs.lines().count(), 1, "one shared execution");
        crate::vfs::remove_dir_all(&out).ok();
    }

    // A run whose verdict cannot be recorded is not counted as a pass.
    #[test]
    fn an_unrecorded_verdict_is_not_reported_as_passed() {
        let out =
            std::env::temp_dir().join(format!("jazyk-unrecorded-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (store, gs) = fixture(&out);
        let mut ledger = Ledger::load(&out);
        ledger.requirements.get_mut("req:shop-1").unwrap().test.run =
            format!("rm -rf '{}'", out.join("gen/requirements").display());
        ledger.save(&out).unwrap();
        let reply = run_selected(&store, &gs, &[]).unwrap();
        assert_eq!(reply["passed"], 0, "{reply}");
        let reason = reply["skipped"][0]["reason"].as_str().unwrap_or_default();
        assert!(reason.contains("not recorded"), "{reply}");
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn criterion_runner_failures_clear_previous_passing_evidence() {
        let out = std::env::temp_dir().join(format!(
            "jazyk-criterion-runner-{}",
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut store, gs) = fixture(&out);
        store.graph.criteria.insert(
            "c:check".into(),
            Criterion {
                requirement: "req:shop-1".into(),
                statement: "Check items".into(),
                ..Default::default()
            },
        );
        for (command, cwd, exit_code) in [
            ("jazyk-absent-criterion-runner", ".", Some(127)),
            ("printf checked", "missing-directory", None),
        ] {
            let mut ledger = Ledger::load(&out);
            let row = ledger.requirements.get_mut("req:shop-1").unwrap();
            row.verdict = "pass".into();
            let mut test = row.test.clone();
            test.run = command.into();
            test.cwd = cwd.into();
            row.tests.insert(
                "c:check".into(),
                crate::gen::CritTest {
                    test,
                    criterion_hash: hash_hex("Check items"),
                    test_hash: row.hashes.test.clone(),
                    verdict: "pass".into(),
                    last_run: None,
                    exit_code: Some(0),
                    evidence: Some("earlier pass".into()),
                },
            );
            assert_eq!(status_of(&store, "req:shop-1", row, &gs).0, "verified");
            ledger.save(&out).unwrap();
            let reply = run_criterion_tests(&store, "req:shop-1", &gs);
            assert_eq!(reply[0]["verdict"], "none", "{reply:?}");
            let ledger = Ledger::load(&out);
            let row = &ledger.requirements["req:shop-1"];
            let test = &row.tests["c:check"];
            assert_eq!(test.verdict, "none");
            assert_eq!(test.exit_code, exit_code);
            assert!(test.last_run.is_some());
            assert_ne!(test.evidence.as_deref(), Some("earlier pass"));
            assert_eq!(status_of(&store, "req:shop-1", row, &gs).0, "unverified");
        }
        crate::vfs::remove_dir_all(&out).ok();
    }

    // A criterion test renamed or deleted by hand never reads verified: the runner
    // greps for the name first, and derivation reads the row `missing`
    // (docs/consumers/gen.md#status-is-derived-never-stored).
    #[test]
    fn renamed_criterion_test_reads_artifact_gone() {
        let out = std::env::temp_dir().join(format!(
            "jazyk-criterion-renamed-{}",
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut store, gs) = fixture(&out);
        store.graph.criteria.insert(
            "c:check".into(),
            Criterion {
                requirement: "req:shop-1".into(),
                statement: "Check items".into(),
                ..Default::default()
            },
        );
        let mut ledger = Ledger::load(&out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        row.verdict = "pass".into();
        let mut test = row.test.clone();
        test.name = "criterion_case_renamed_away".into();
        test.run = "exit 0".into();
        test.label = "criterion".into();
        row.tests.insert(
            "c:check".into(),
            crate::gen::CritTest {
                test,
                criterion_hash: hash_hex("Check items"),
                test_hash: row.hashes.test.clone(),
                verdict: "pass".into(),
                last_run: None,
                exit_code: Some(0),
                evidence: Some("earlier pass".into()),
            },
        );
        assert_eq!(
            status_of(&store, "req:shop-1", row, &gs),
            ("missing".into(), "artifact-gone".into())
        );
        ledger.save(&out).unwrap();
        let reply = run_criterion_tests(&store, "req:shop-1", &gs);
        let line = reply.iter().find(|l| l["criterion"] == "c:check").unwrap();
        assert_eq!(line["verdict"], "none", "{reply:?}");
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.tests["c:check"].verdict, "none");
        assert_eq!(
            status_of(&store, "req:shop-1", row, &gs),
            ("missing".into(), "artifact-gone".into())
        );
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn status_matrix_and_cascade() {
        let out =
            std::env::temp_dir().join(format!("jazyk-verify-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);

        // Fresh row: unverified, never-run.
        let p = pending(&s, &gs, None, None);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["status"], "unverified");
        assert_eq!(p[0]["reason"], "never-run");

        // A pass verdict rebaselines and verifies.
        mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
        assert!(pending(&s, &gs, None, None).is_empty());

        // Hand-editing an implementing file flips stale-code.
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// edited product").unwrap();
        let p = pending(&s, &gs, None, None);
        assert_eq!(p[0]["status"], "stale-code");
        mark(&s, "req:shop-1", "pass", None, None, None, &gs).unwrap();

        // Editing the test artifact flips stale-test.
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        crate::vfs::write(
            gs.deliverable.join("tests/cart.rs"),
            format!("// changed\nfn {}() {{}}\n", name),
        )
        .unwrap();
        assert_eq!(pending(&s, &gs, None, None)[0]["status"], "stale-test");
        mark(&s, "req:shop-1", "pass", None, None, None, &gs).unwrap();

        // Rewording the requirement flips stale-requirement; task refuses; test refuses.
        s.graph
            .requirements
            .get_mut("req:shop-1")
            .unwrap()
            .statement = "The Cart shall hold many items.".into();
        let p = pending(&s, &gs, None, None);
        assert_eq!(p[0]["status"], "stale-requirement");
        assert!(task(&s, "req:shop-1", &gs)
            .unwrap_err()
            .contains("stale-requirement"));

        // A fail verdict surfaces as failing.
        s.graph
            .requirements
            .get_mut("req:shop-1")
            .unwrap()
            .statement = "The Cart shall hold items.".into();
        mark(&s, "req:shop-1", "fail", None, Some("boom"), Some(1), &gs).unwrap();
        // A failing row is a finding, never a verify goal: the default listing leaves
        // it alone; a rerun a person asked for still selects it
        // (docs/compiler/tools.md#verification-tools).
        assert!(pending(&s, &gs, None, None).is_empty());
        let runnable = pending(&s, &gs, Some("runnable"), None);
        assert_eq!(runnable[0]["status"], "failing");
        assert!(runnable[0]["repair"].as_str().unwrap().contains("finding"));
        assert_eq!(select_rows(&s, &gs, &[], None, false).len(), 1);

        // Programmatic run: `true` exits 0.
        let pass = run_programmatic(&s, "req:shop-1", &gs).unwrap().pass;
        assert!(pass);
    }

    fn scoped_fixture() -> (Store, GenSettings) {
        let (store, mut gs) = fixture(std::path::Path::new("/project/.jazyk/ds_main"));
        let old = gs.deliverable.clone();
        gs.deliverable = "/project".into();
        gs.code = vec!["src/**".into(), "tests/**".into()];
        for file in ["src/cart.rs", "tests/cart.rs"] {
            let path = gs.deliverable.join(file);
            crate::vfs::create_dir_all(path.parent().unwrap()).unwrap();
            crate::vfs::write(path, crate::vfs::read(old.join(file)).unwrap()).unwrap();
        }
        scoped_mapping(&[]);
        mark(&store, "req:shop-1", "pass", None, None, Some(0), &gs).unwrap();
        (store, gs)
    }

    fn scoped_mapping(extra_docs: &[&str]) {
        let docs: Vec<_> = std::iter::once("docs/**")
            .chain(extra_docs.iter().copied())
            .collect();
        crate::vfs::write("/project/.jazyk/jazyk.toml", format!(
            "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Main'\ndocs = {}\nsource = ['src/**', 'tests/**']\n",
            serde_json::to_string(&docs).unwrap()
        )).unwrap();
    }

    #[test]
    fn document_reclassification_reopens_binding_and_generation_without_touching_files() {
        let _vfs = crate::vfs::Scope::enter(&Default::default());
        let (store, gs) = scoped_fixture();
        assert!(pending(&store, &gs, None, None).is_empty());
        assert!(crate::bind::pending(&store, &gs).is_empty());
        assert!(crate::gen::pending(&store, &gs).is_empty());
        let before = crate::vfs::read(store.out.join("gen/ledger.yaml")).unwrap();
        let content = crate::vfs::read(gs.deliverable.join("src/cart.rs")).unwrap();
        scoped_mapping(&["src/cart.rs"]);
        let ledger = Ledger::load(&store.out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(
            status_of(&store, "req:shop-1", row, &gs),
            ("missing".into(), "source-selection-changed".into())
        );
        assert_eq!(
            crate::bind::pending(&store, &gs)[0]["reason"],
            "source-selection-changed"
        );
        assert_eq!(
            crate::gen::pending(&store, &gs)[0]["reason"],
            "source-selection-changed"
        );
        assert!(crate::bind::gate(&store, &gs, "req:shop-1")
            .unwrap_err()
            .contains("current Source"));
        assert!(gate(&store, &gs, "req:shop-1").is_err());
        assert!(crate::gen::gate(&store, &gs, "ent:cart", false)
            .unwrap_err()
            .contains("current Source"));
        assert!(task(&store, "req:shop-1", &gs)
            .unwrap_err()
            .contains("source-selection-changed"));
        assert!(mark(&store, "req:shop-1", "pass", None, None, Some(0), &gs)
            .unwrap_err()
            .contains("source-selection-changed"));
        assert!(run_programmatic(&store, "req:shop-1", &gs)
            .err()
            .unwrap()
            .contains("source-selection-changed"));
        assert!(run_criterion_tests(&store, "req:shop-1", &gs)[0]["note"]
            .as_str()
            .unwrap()
            .contains("source-selection-changed"));
        let test = json!({"kind":"programmatic","artifact":row.test.artifact,"name":row.test.name,"run":"true"});
        assert!(crate::bind::record(
            &store,
            "req:shop-1",
            &["src/cart.rs".into()],
            &test,
            &Value::Null,
            "pass",
            None,
            &gs
        )
        .is_err());
        let stale_manifest = json!({"files":["src/cart.rs"],"tests":[]});
        assert!(gen_mark(&store, "ent:cart", None, &stale_manifest, &gs).is_err());
        assert!(crate::gen::confine_in(&gs, "src/cart.rs", &mut Vec::new()).is_err());
        assert_eq!(
            crate::vfs::read(store.out.join("gen/ledger.yaml")).unwrap(),
            before
        );
        assert_eq!(
            crate::vfs::read(gs.deliverable.join("src/cart.rs")).unwrap(),
            content
        );
        // Binding honestly records no implementation, then generation writes inside
        // Source. The file assigned to Docs is preserved through that repair.
        crate::bind::record(
            &store,
            "req:shop-1",
            &[],
            &test,
            &Value::Null,
            "fail",
            None,
            &gs,
        )
        .unwrap();
        let ledger = Ledger::load(&store.out);
        assert!(ledger.requirements["req:shop-1"].files.is_empty());
        assert_eq!(
            status_of(
                &store,
                "req:shop-1",
                &ledger.requirements["req:shop-1"],
                &gs
            )
            .0,
            "unimplemented"
        );
        assert!(crate::bind::pending(&store, &gs).is_empty());
        assert!(!crate::gen::pending(&store, &gs).is_empty());
        assert_eq!(
            crate::vfs::read(gs.deliverable.join("src/cart.rs")).unwrap(),
            content
        );
        crate::vfs::write(
            gs.deliverable.join("src/cart-new.rs"),
            "// replacement product",
        )
        .unwrap();
        gen_mark(
            &store,
            "ent:cart",
            None,
            &json!({
                "files":["src/cart-new.rs"], "tests":[{
                    "requirement":"req:shop-1", "kind":"programmatic", "artifact":row.test.artifact,
                    "name":row.test.name, "run":"true", "files":["src/cart-new.rs"]
                }]
            }),
            &gs,
        )
        .unwrap();
        assert!(crate::gen::pending(&store, &gs).is_empty());
        assert_eq!(
            crate::vfs::read(gs.deliverable.join("src/cart.rs")).unwrap(),
            content
        );
    }

    #[test]
    fn test_reclassification_reopens_development_without_treating_tests_as_implementation() {
        let _vfs = crate::vfs::Scope::enter(&Default::default());
        let (store, gs) = scoped_fixture();
        scoped_mapping(&["tests/cart.rs"]);
        let ledger = Ledger::load(&store.out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(
            status_of(&store, "req:shop-1", row, &gs).1,
            "source-selection-changed"
        );
        assert_eq!(crate::bind::pending(&store, &gs).len(), 1);
        // Development can repair the missing current test binding. Its implementing
        // file remains Source, so this is incomplete coverage rather than lost code.
        assert_eq!(
            crate::gen::pending(&store, &gs),
            vec![json!({
                "entity":"ent:cart", "reason":"incomplete-bindings", "changed":["req:shop-1"]
            })]
        );
        assert_eq!(row.files, ["src/cart.rs"]);
        scoped_mapping(&[]);
        assert_eq!(status_of(&store, "req:shop-1", row, &gs).0, "verified");
        assert!(crate::gen::pending(&store, &gs).is_empty());
        // Internal LLM criteria are metadata, not an implementing path that must be
        // added to Source. Criterion artifacts outside it still need rebinding.
        let mut internal = row.clone();
        internal.test.kind = "llm".into();
        internal.test.artifact = "criteria/cart.md".into();
        crate::vfs::create_dir_all(store.out.join("gen/criteria")).unwrap();
        crate::vfs::write(store.out.join("gen/criteria/cart.md"), "metadata").unwrap();
        assert!(crate::gen::binding_source_current(
            &store.out, &gs, &ledger, &internal
        ));
        internal.tests.insert(
            "crit:cart-1".into(),
            crate::gen::CritTest {
                test: crate::gen::TestRef {
                    artifact: "docs/test.rs".into(),
                    ..row.test.clone()
                },
                criterion_hash: String::new(),
                test_hash: String::new(),
                verdict: "none".into(),
                last_run: None,
                exit_code: None,
                evidence: None,
            },
        );
        assert!(!crate::gen::binding_source_current(
            &store.out, &gs, &ledger, &internal
        ));
    }

    // A rebuilt artifact among a row's files is not a code change: the stale-code hash
    // covers implementing files alone. Mirrors docs/consumers/gen.md#status-is-derived-never-stored.
    #[test]
    fn a_rebuilt_artifact_never_reopens_the_row() {
        let out = std::env::temp_dir().join(format!(
            "jazyk-verify-artifact-{}",
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        let mut ledger = crate::gen::Ledger::load(&out);
        ledger.medium = Some(crate::gen::Medium {
            form: "deck".into(),
            produced: "built".into(),
            toolchain: "python3".into(),
            artifact: "dist/deck.pptx".into(),
        });
        ledger
            .requirements
            .get_mut("req:shop-1")
            .unwrap()
            .files
            .push("dist/deck.pptx".into());
        ledger.save(&out).unwrap();
        crate::vfs::create_dir_all(gs.deliverable.join("dist")).unwrap();
        crate::vfs::write(gs.deliverable.join("dist/deck.pptx"), "zip v1").unwrap();
        mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
        assert!(pending(&s, &gs, None, None).is_empty());
        // The build rewrites the artifact with fresh bytes: nothing reopens.
        crate::vfs::write(
            gs.deliverable.join("dist/deck.pptx"),
            "zip v2 fresh timestamps",
        )
        .unwrap();
        assert!(
            pending(&s, &gs, None, None).is_empty(),
            "{:?}",
            pending(&s, &gs, None, None)
        );
        // An implementing file edit still does.
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// edited").unwrap();
        assert_eq!(pending(&s, &gs, None, None)[0]["status"], "stale-code");
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn audit_never_launders_stale_requirement() {
        let out =
            std::env::temp_dir().join(format!("jazyk-audit-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        mark(&s, "req:shop-1", "pass", None, None, None, &gs).unwrap();
        // Reword the requirement: stale-requirement.
        s.graph
            .requirements
            .get_mut("req:shop-1")
            .unwrap()
            .statement = "The Cart shall hold many items.".into();
        assert_eq!(
            pending(&s, &gs, None, None)[0]["status"],
            "stale-requirement"
        );
        // Audit must not launder it into verified: the artifact carries the OLD name.
        audit(&s, &gs);
        assert_eq!(
            pending(&s, &gs, None, None)[0]["status"],
            "stale-requirement"
        );
    }

    // A lost programmatic row is reported, never recreated: the harness cannot know
    // the command that runs the test, and a row with none is a verify goal that can
    // never resolve. The requirement stays missing and binds. A lost llm row is
    // recreated from its criteria file, matched on the front matter the built-in
    // worker writes (the requirement id and the full statement hash), since the test
    // name is not in it. Mirrors docs/consumers/gen.md#runners.
    #[test]
    fn missing_rows_surface_and_audit_rebuilds() {
        let out =
            std::env::temp_dir().join(format!("jazyk-rebuild-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        // Wipe the ledger: the row must surface as missing/not-generated with a repair.
        for f in crate::gen::Ledger::files(&out) {
            crate::vfs::remove_file(f).unwrap();
        }
        let p = pending(&s, &gs, None, None);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["status"], "missing");
        assert_eq!(p[0]["reason"], "not-generated");
        assert!(p[0]["repair"]
            .as_str()
            .unwrap()
            .contains("jazyk gen ent:cart"));
        // Audit finds the programmatic test (tests/cart.rs carries the name) but
        // records no row for it: binding records the command.
        let r = audit(&s, &gs);
        assert!(r["rebuilt"].as_array().unwrap().is_empty(), "{}", r);
        assert_eq!(r["found"].as_array().unwrap().len(), 1, "{}", r);
        assert_eq!(r["found"][0]["artifact"], "tests/cart.rs");
        let p2 = pending(&s, &gs, None, None);
        assert_eq!(p2[0]["status"], "missing");
        assert!(crate::bind::pending(&s, &gs)
            .iter()
            .any(|b| b["requirement"] == "req:shop-1" && b["reason"] == "unbound"));

        // An llm row is rebuilt from its criteria file.
        s.graph.requirements.insert(
            "req:shop-2".into(),
            Requirement {
                statement: "The Cart shall look tidy.".into(),
                entities: vec!["ent:cart".into()],
                ..Default::default()
            },
        );
        let crit = out.join("gen").join("criteria");
        crate::vfs::create_dir_all(&crit).unwrap();
        crate::vfs::write(
            crit.join("req-shop-2.md"),
            format!(
                "---\nrequirement: req:shop-2\nhash: {}\n---\n\n# Verify req:shop-2\nInspect the Cart. PASS if tidy. FAIL otherwise.\n",
                hash_hex("The Cart shall look tidy.")
            ),
        )
        .unwrap();
        let r = audit(&s, &gs);
        assert_eq!(
            r["rebuilt"].as_array().unwrap(),
            &vec![json!("req:shop-2")],
            "{}",
            r
        );
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-2"];
        assert_eq!(row.test.kind, "llm");
        assert_eq!(row.test.artifact, "criteria/req-shop-2.md");
        assert_eq!(status_of(&s, "req:shop-2", row, &gs).0, "unverified");
        // A second audit refreshes it in place instead of losing it.
        let r = audit(&s, &gs);
        assert_eq!(r["refreshed"], 1, "{}", r);
        crate::vfs::remove_dir_all(&out).ok();
    }

    // An audit rebaselines hashes, never verdicts: a verified row whose implementing
    // file moved comes out unverified, not verified over bytes nothing reran.
    // Mirrors docs/consumers/gen.md#runners.
    #[test]
    fn audit_drops_a_verdict_whose_bytes_moved() {
        let out =
            std::env::temp_dir().join(format!("jazyk-audit-reset-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
        assert!(pending(&s, &gs, None, None).is_empty());
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// edited by hand").unwrap();
        assert_eq!(pending(&s, &gs, None, None)[0]["status"], "stale-code");
        let r = audit(&s, &gs);
        assert_eq!(r["verdictReset"].as_array().unwrap().len(), 1, "{}", r);
        let p = pending(&s, &gs, None, None);
        assert_eq!(p[0]["status"], "unverified", "{:?}", p);
        assert_eq!(p[0]["reason"], "never-run");
        // Unchanged bytes keep their verdict across an audit.
        mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
        let r = audit(&s, &gs);
        assert!(r["verdictReset"].as_array().unwrap().is_empty(), "{}", r);
        assert!(pending(&s, &gs, None, None).is_empty());
        crate::vfs::remove_dir_all(&out).ok();
    }

    // A programmatic artifact that lost its declared test name judges nothing: the
    // row is missing (artifact-gone), bind work, and never a verify goal that a
    // rerun could not resolve. Mirrors docs/consumers/gen.md#status-is-derived-never-stored.
    #[test]
    fn a_renamed_test_is_bind_work_not_a_rerun() {
        let out =
            std::env::temp_dir().join(format!("jazyk-name-lost-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
        crate::vfs::write(gs.deliverable.join("tests/cart.rs"), "fn renamed() {}\n").unwrap();
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(
            status_of(&s, "req:shop-1", row, &gs),
            ("missing".to_string(), "artifact-gone".to_string())
        );
        let owed = crate::bind::pending(&s, &gs);
        assert_eq!(owed.len(), 1);
        assert_eq!(owed[0]["reason"], "artifact-gone");
        assert!(crate::gen::ledger_stale_records(&s, &gs)
            .iter()
            .all(|c| c.detail["goal"] != "verify"));
        let err = match run_programmatic(&s, "req:shop-1", &gs) {
            Ok(_) => panic!("a renamed test must not execute"),
            Err(e) => e,
        };
        assert!(
            err.contains("artifact-gone") && err.contains("jazyk gen ent:cart"),
            "{}",
            err
        );
        crate::vfs::remove_dir_all(&out).ok();
    }

    // The verify gate holds exactly when a verdict stands on a current row; every
    // other state names the call that lands it. Mirrors docs/compiler/goals/verify.md#gate.
    #[test]
    fn the_verify_gate_reads_the_landed_verdict() {
        let out =
            std::env::temp_dir().join(format!("jazyk-verify-gate-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        let err = gate(&s, &gs, "req:shop-1").unwrap_err();
        assert!(
            err.contains("no verdict") && err.contains("run_tests"),
            "{}",
            err
        );
        mark(&s, "req:shop-1", "pass", None, Some("ok"), Some(0), &gs).unwrap();
        gate(&s, &gs, "req:shop-1").unwrap();
        mark(&s, "req:shop-1", "fail", None, Some("boom"), Some(1), &gs).unwrap();
        gate(&s, &gs, "req:shop-1").unwrap();
        crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// moved").unwrap();
        let err = gate(&s, &gs, "req:shop-1").unwrap_err();
        assert!(err.contains("stale-code"), "{}", err);
        mark(&s, "req:shop-1", "pass", None, None, None, &gs).unwrap();
        s.graph
            .requirements
            .get_mut("req:shop-1")
            .unwrap()
            .statement = "Reworded.".into();
        let err = gate(&s, &gs, "req:shop-1").unwrap_err();
        assert!(
            err.contains("moved since") && err.contains("jazyk gen"),
            "{}",
            err
        );
        assert!(gate(&s, &gs, "req:none").unwrap_err().contains("bind work"));
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn iso_clock_shape() {
        let t = now_iso();
        assert_eq!(t.len(), 20, "{}", t);
        assert!(t.ends_with('Z'));
        assert!(t.starts_with("20"));
    }
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

enum JudgmentError {
    Model(String),
    Invalid(String),
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
async fn judge_llm_criteria(
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
                include_str!("../../docs/compiler/goals/prompts/verify-criterion-contract.md"),
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

fn record_llm_failure(store: &Store, rid: &str, error: &str) -> Result<(), String> {
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
        return Err(
            "no ledger rows match the given target(s); run `jazyk gen` first or check the ids"
                .into(),
        );
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
