use crate::vfs::VPathExt;
// Binding: tie one requirement to the deliverable before generation runs. The bind
// task searches for an implementation, finds or writes the test, runs it, and records
// the ledger row; the verdict classifies the requirement (verified, unimplemented,
// failing). Mirrors docs/consumers/bind.md and docs/compiler/tools.md#binding-tools.
use crate::gen::{
    artifact_path, hash_file, hash_files, GenSettings, Ledger, ReqRow, RowHashes, TestRef,
};
use crate::model::hash_hex;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;

// The bind contract, served as begin_binding instructions and as the internal
// session's package preamble. Mirrors docs/consumers/bind.md#the-bind-goal.
pub fn instructions() -> String {
    include_str!("../../docs/compiler/goals/prompts/bind-contract.md").into()
}

// Why an existing row owes a bind, or None when it is current: the statement moved
// under it, or nothing judges it any more (the artifact is gone from disk, or a
// programmatic artifact lost the declared test name).
// Mirrors docs/consumers/bind.md#when-binding-runs.
fn bind_reason(
    store: &Store,
    gs: &GenSettings,
    _rid: &str,
    r: &crate::model::Requirement,
    row: &ReqRow,
) -> Option<&'static str> {
    if hash_hex(&r.statement) != row.hashes.requirement {
        return Some("requirement-changed");
    }
    let artifact = artifact_path(&store.out, gs, &row.test);
    if !artifact.vexists() {
        return Some("artifact-gone");
    }
    if row.test.kind == "programmatic"
        && !crate::vfs::read_to_string(&artifact)
            .map(|c| !row.test.name.trim().is_empty() && c.contains(&row.test.name))
            .unwrap_or(false)
    {
        return Some("artifact-gone");
    }
    // One test per criterion: a criterion without one, or whose statement moved since
    // its test was written, owes a bind (docs/consumers/bind.md#the-bind-goal).
    for cid in store.criteria_of(_rid) {
        let Some(c) = store.graph.criteria.get(&cid) else { continue };
        match row.tests.get(&cid) {
            None => return Some("criterion-unbound"),
            Some(t) => {
                if t.criterion_hash != hash_hex(&c.statement) {
                    return Some("criterion-changed");
                }
                if !artifact_path(&store.out, gs, &t.test).vexists() {
                    return Some("artifact-gone");
                }
            }
        }
    }
    None
}

// The criteria of a requirement as the binding package lists them: id, kind,
// statement, and the suggested test name. Mirrors docs/consumers/bind.md#the-bind-goal.
pub fn criteria_package(store: &Store, rid: &str) -> Vec<Value> {
    store
        .criteria_of(rid)
        .iter()
        .filter_map(|cid| {
            store.graph.criteria.get(cid).map(|c| {
                json!({
                    "id": cid,
                    "kind": c.kind,
                    "statement": c.statement,
                    "quote": c.source.as_ref().map(|s| s.quote.clone()),
                    "suggestedTestName": crate::gen::crit_test_name(cid, &c.statement),
                })
            })
        })
        .collect()
}

// The bind goal's gate: a row recorded by record_binding and current (its requirement
// hash equals the live statement hash, and its artifact exists and carries the
// declared name). Err names what is missing and the call that lands it, so a
// mark_goal_done claim without the row is bounced with the repair in hand.
// Mirrors docs/compiler/goals/bind.md#gate.
// The deliverable files naming any of the needles, deliverable-relative, capped: the
// walk skips build and dependency trees and files over 200k characters.
fn existing_tests(deliverable: &std::path::Path, needles: &[String]) -> Vec<serde_json::Value> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, needles: &[String], out: &mut Vec<serde_json::Value>, depth: usize) {
        if depth > 4 || out.len() >= 10 {
            return;
        }
        let Ok(rd) = crate::vfs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "dist" | "build") {
                continue;
            }
            if p.vis_dir() {
                walk(&p, root, needles, out, depth + 1);
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.len() > 200_000 {
                continue;
            }
            let Ok(text) = crate::vfs::read_to_string(&p) else { continue };
            // A needle hits at a word boundary only: crit:x-1 is no part of crit:x-13.
            let bounded = |n: &str| {
                text.match_indices(n).any(|(i, _)| {
                    text[i + n.len()..]
                        .chars()
                        .next()
                        .map(|c| !c.is_alphanumeric())
                        .unwrap_or(true)
                })
            };
            let hits: Vec<&String> = needles.iter().filter(|n| bounded(n.as_str())).collect();
            if !hits.is_empty() {
                let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().to_string();
                // A hash needle is eight hex characters; a file that names the id or
                // slug but no current hash is another statement's test.
                let is_hash = |n: &str| n.len() == 8 && n.chars().all(|c| c.is_ascii_hexdigit());
                let current = hits.iter().any(|n| is_hash(n));
                let mut hit = serde_json::json!({"file": rel, "names": hits, "carriesCurrentHash": current});
                if !current {
                    hit["note"] = serde_json::json!("names this id or slug but no current statement hash: the test of an earlier statement under this id, or of another one; rename only when its assertion is this statement's");
                }
                out.push(hit);
            }
            if out.len() >= 10 {
                return;
            }
        }
    }
    let mut out = Vec::new();
    walk(deliverable, deliverable, needles, &mut out, 0);
    out
}

pub fn gate(store: &Store, gs: &GenSettings, rid: &str) -> Result<(), String> {
    let rid = store.resolve_id(rid).to_string();
    let Some(r) = store.graph.requirements.get(&rid) else {
        return Err(format!("`{}` is not in the graph; nothing binds it", rid));
    };
    let ledger = Ledger::load(&store.out);
    let Some(row) = ledger.requirements.get(&rid) else {
        return Err(format!(
            "no ledger row for `{}`: record_binding has not landed; the gate is a current row recorded by record_binding",
            rid
        ));
    };
    match bind_reason(store, gs, &rid, r, row) {
        None => Ok(()),
        Some("requirement-changed") => Err(format!(
            "the row for `{}` records statement hash {} but the live statement hashes {}: the statement moved since the row was recorded; begin_binding again and record_binding against the current package",
            rid,
            &row.hashes.requirement[..row.hashes.requirement.len().min(8)],
            &hash_hex(&r.statement)[..8]
        )),
        Some("criterion-unbound") => Err(format!(
            "the row for `{}` carries no test for one of its criteria ({}); record_binding with tests, one per criterion",
            rid,
            store.criteria_of(&rid).join(", ")
        )),
        Some("criterion-changed") => Err(format!(
            "a criterion of `{}` moved since its test was written; begin_binding again and record the tests against the current criteria",
            rid
        )),
        Some(_) => Err(format!(
            "the row for `{}` names test `{}` in `{}`, but that artifact is missing or does not contain the name; write the test (or bind to the existing one under its real name) and record_binding again",
            rid, row.test.name, row.test.artifact
        )),
    }
}

// Requirements owing a bind, with a reason. Deterministic; no model.
// Mirrors docs/consumers/bind.md#when-binding-runs.
pub fn pending(store: &Store, gs: &GenSettings) -> Vec<Value> {
    let ledger = Ledger::load(&store.out);
    let mut out = Vec::new();
    for (rid, r) in &store.graph.requirements {
        let reason = match ledger.requirements.get(rid) {
            None => "unbound",
            Some(row) => match bind_reason(store, gs, rid, r, row) {
                Some(reason) => reason,
                None => continue,
            },
        };
        let entity = r
            .entities
            .first()
            .map(|e| store.resolve_id(e).to_string())
            .unwrap_or_default();
        out.push(json!({
            "kind": "bind",
            "requirement": rid,
            "entity": entity,
            "reason": reason,
            "statement": r.statement,
        }));
    }
    out
}

// The package for one bind task: everything a worker needs to search, test, and
// record. Mirrors docs/compiler/tools.md#binding-tools.
pub fn task(store: &Store, rid: &str, gs: &GenSettings) -> Result<Value, String> {
    let rid = store.resolve_id(rid).to_string();
    let Some(r) = store.graph.requirements.get(&rid) else {
        return Err(format!("unknown requirement `{}`", rid));
    };
    let ledger = Ledger::load(&store.out);
    let entity = r
        .entities
        .first()
        .map(|e| store.resolve_id(e).to_string())
        .unwrap_or_default();
    // The goal's loaded set: the requirement in full with its entity as a stub.
    // Mirrors docs/consumers/bind.md#the-bind-goal.
    let pack = crate::context::ledger_context(store, &rid);
    // The established test conventions: what the ledger already records, so a second
    // toolchain is never introduced.
    let conventions: Vec<Value> = ledger
        .requirements
        .values()
        .map(|row| json!({"kind": row.test.kind, "artifact": row.test.artifact, "run": row.test.run, "files": row.files}))
        .take(5)
        .collect();
    let entity_files = ledger
        .entities
        .get(&crate::gen::slug_of(&entity))
        .map(|e| e.files.clone())
        .unwrap_or_default();
    // The reason the row owes a bind; a current row begun anyway reads `current`,
    // so the session knows it is re-binding by choice, not by the board's word.
    let reason = match ledger.requirements.get(&rid) {
        None => "unbound",
        Some(row) => bind_reason(store, gs, &rid, r, row).unwrap_or("current"),
    };
    // The deliverable files that already name this requirement: by id (a marker
    // comment), by test-name slug (an earlier hash), or by statement hash (an earlier
    // id). Mirrors docs/consumers/bind.md#the-bind-goal.
    let suggested = crate::gen::test_name(&rid, &r.statement);
    let (slug, hash8) = suggested
        .rsplit_once('_')
        .map(|(s, h)| (s.to_string(), h.to_string()))
        .unwrap_or((suggested.clone(), String::new()));
    let mut needles: Vec<String> = [rid.clone(), slug, hash8]
        .into_iter()
        .filter(|n| n.len() >= 6)
        .collect();
    // The criteria's own ids and hashes find a test left under an earlier id.
    let criteria = criteria_package(store, &rid);
    for c in &criteria {
        if let Some(id) = c["id"].as_str() {
            needles.push(id.to_string());
        }
        if let Some(n) = c["suggestedTestName"].as_str() {
            if let Some((_, h)) = n.rsplit_once('_') {
                needles.push(h.to_string());
            }
        }
    }
    let existing_tests = existing_tests(&gs.deliverable, &needles);
    Ok(json!({
        "requirement": rid,
        "entity": entity,
        "criteria": criteria,
        "criteriaNote": if criteria.is_empty() {
            "no criteria on this requirement; one test encodes the statement"
        } else {
            "one test per criterion, named by suggestedTestName; record them under tests in record_binding. One file per requirement may hold them all, but each entry's run must select its own test by name (cargo test <name>, npx tsx --test --test-name-pattern <name>, python3 tests/<file>.py <name>): a run of the whole file judges every criterion at once"
        },
        "existingTests": existing_tests,
        "pathsNote": "existingTests, files, and test artifact paths are relative to the deliverable directory",
        "reason": reason,
        "statement": r.statement,
        "quote": r.source.as_ref().map(|s| s.quote.clone()).unwrap_or_default(),
        "provenance": crate::session::provenance_line(r),
        "factHash": hash_hex(&r.statement),
        "deliverable": gs.deliverable.to_string_lossy(),
        "suggestedTestName": crate::gen::test_name(&rid, &r.statement),
        "medium": ledger.medium.as_ref().map(|m| m.line()),
        "mediumNote": crate::gen::medium_note(&ledger),
        // The medium's standing divergence from the recorded commands, if any
        // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
        "mediumWarning": crate::gen::medium_divergence(&ledger),
        "build": ledger.build.as_ref().map(|b| json!({"run": b.run, "cwd": b.cwd})),
        "testConventions": conventions,
        "testConventionsNote": "the toolchain and command style already recorded, to reuse exactly; an earlier row's assertion style is not a convention, and a test that reads a description of the artifact stays wrong however many rows share it",
        // The statement is disputed while these stand; the test judges one side of an
        // open question (docs/consumers/gen.md#rows-recorded-over-an-open-contradiction).
        "openDiagnostics": crate::gen::open_errors_on(store, &rid),
        "entityFiles": entity_files,
        "context": pack,
        "instructions": instructions(),
    }))
}

// Record a binding: the row is born (or re-anchored) with the requirement hash, the
// test, the files, and the first verdict. Rejects a test whose artifact is missing or
// does not carry the declared name, the same shape gate record_generation applies.
// Mirrors docs/compiler/tools.md#binding-tools.
// One test reference parsed and checked against the deliverable: the artifact exists
// and, for a programmatic test, carries the name.
fn parse_test_ref(store: &Store, gs: &GenSettings, test: &Value, label: &str) -> Result<(TestRef, std::path::PathBuf), String> {
    let kind = test["kind"].as_str().unwrap_or_default();
    if kind != "programmatic" && kind != "llm" {
        return Err(format!("{}.kind must be `programmatic` or `llm`", label));
    }
    let (artifact, name, run) = (
        test["artifact"].as_str().unwrap_or_default().trim().to_string(),
        test["name"].as_str().unwrap_or_default().trim().to_string(),
        test["run"].as_str().unwrap_or_default().trim().to_string(),
    );
    if artifact.is_empty() || name.is_empty() {
        return Err(format!("{}.artifact and {}.name are required", label, label));
    }
    if kind == "programmatic" && run.is_empty() {
        return Err(format!(
            "a programmatic test needs {}.run: the command whose exit code is the verdict",
            label
        ));
    }
    let tref = TestRef {
        kind: kind.into(),
        label: test["label"].as_str().unwrap_or("bound").into(),
        artifact: crate::gen::confine_rel(&artifact).map_err(|e| format!("{}.artifact: {}", label, e))?,
        name: name.clone(),
        run,
        cwd: crate::gen::confine_cwd(test["cwd"].as_str().unwrap_or("."))
            .map_err(|e| format!("{}.cwd: {}", label, e))?,
    };
    let path = artifact_path(&store.out, gs, &tref);
    if !path.vexists() {
        return Err(format!(
            "test artifact `{}` does not exist; write it before recording",
            tref.artifact
        ));
    }
    if kind == "programmatic" {
        let content = crate::vfs::read_to_string(&path).unwrap_or_default();
        let bare = name.rsplit("::").next().unwrap_or(&name);
        if !content.contains(bare) {
            return Err(format!(
                "test artifact `{}` does not contain the test name `{}`",
                tref.artifact, bare
            ));
        }
    }
    Ok((tref, path))
}

pub fn record(
    store: &Store,
    rid: &str,
    files_in: &[String],
    test: &Value,
    tests: &Value,
    verdict: &str,
    evidence: Option<&str>,
    gs: &GenSettings,
) -> Result<Value, String> {
    let rid = store.resolve_id(rid).to_string();
    let Some(r) = store.graph.requirements.get(&rid) else {
        return Err(format!("unknown requirement `{}`", rid));
    };
    // One test per criterion (docs/consumers/bind.md#the-bind-goal): every criterion
    // of the requirement has an entry under `tests`, each checked like the headline.
    let criteria = store.criteria_of(&rid);
    let mut crit_tests: BTreeMap<String, crate::gen::CritTest> = BTreeMap::new();
    if let Some(list) = tests.as_array() {
        for (i, entry) in list.iter().enumerate() {
            let cid = entry["criterion"].as_str().unwrap_or_default().to_string();
            let Some(c) = store.graph.criteria.get(&cid).filter(|c| c.requirement == rid) else {
                return Err(format!(
                    "tests[{}].criterion `{}` is not a criterion of {}; its criteria: {}",
                    i, cid, rid, criteria.join(", ")
                ));
            };
            let v = entry["verdict"].as_str().unwrap_or_default();
            if v != "pass" && v != "fail" {
                return Err(format!("tests[{}].verdict must be `pass` or `fail`", i));
            }
            let (tref, path) = parse_test_ref(store, gs, entry, &format!("tests[{}]", i))?;
            crit_tests.insert(
                cid,
                crate::gen::CritTest {
                    test: tref,
                    criterion_hash: hash_hex(&c.statement),
                    test_hash: hash_file(&path),
                    verdict: v.to_string(),
                    last_run: Some(crate::verify::now_iso()),
                    exit_code: None,
                    evidence: entry["evidence"].as_str().map(|e| crate::llm::truncate(e, 400).to_string()),
                },
            );
        }
    }
    let missing: Vec<String> = criteria.iter().filter(|c| !crit_tests.contains_key(*c)).cloned().collect();
    if !missing.is_empty() {
        let names: Vec<String> = missing
            .iter()
            .filter_map(|cid| store.graph.criteria.get(cid).map(|c| format!("{} ({})", cid, crate::gen::crit_test_name(cid, &c.statement))))
            .collect();
        return Err(format!(
            "{} carries criteria without a test: {}; record_binding takes tests: [{{criterion, kind, label, artifact, name, run, cwd?, verdict, evidence?}}], one per criterion",
            rid,
            names.join(", ")
        ));
    }
    // The headline test: given, or the first criterion's; its verdict aggregates.
    let (test, verdict): (Value, String) = if test.is_object() {
        (test.clone(), verdict.to_string())
    } else if let Some((_, first)) = crit_tests.iter().next() {
        let t = &first.test;
        let agg = if crit_tests.values().any(|t| t.verdict == "fail") { "fail" } else { "pass" };
        (
            json!({"kind": t.kind, "label": t.label, "artifact": t.artifact, "name": t.name, "run": t.run, "cwd": t.cwd}),
            agg.to_string(),
        )
    } else {
        return Err("test is required: {kind: programmatic|llm, label, artifact, name, run, cwd?}".into());
    };
    let verdict = verdict.as_str();
    let test = &test;
    if verdict != "pass" && verdict != "fail" {
        return Err(format!(
            "verdict must be `pass` or `fail`, got `{}`",
            verdict
        ));
    }
    let kind = test["kind"].as_str().unwrap_or_default();
    if kind != "programmatic" && kind != "llm" {
        return Err("test.kind must be `programmatic` or `llm`".into());
    }
    let (artifact, name, run) = (
        test["artifact"].as_str().unwrap_or_default().trim().to_string(),
        test["name"].as_str().unwrap_or_default().trim().to_string(),
        test["run"].as_str().unwrap_or_default().trim().to_string(),
    );
    if artifact.is_empty() || name.is_empty() {
        return Err("test.artifact and test.name are required".into());
    }
    if kind == "programmatic" && run.is_empty() {
        return Err(
            "a programmatic test needs test.run: the command whose exit code is the verdict".into(),
        );
    }
    // Every path the binding names stays under the deliverable
    // (docs/consumers/gen.md#file-ownership-and-conventions).
    let tref = TestRef {
        kind: kind.into(),
        label: test["label"].as_str().unwrap_or("bound").into(),
        artifact: crate::gen::confine_rel(&artifact).map_err(|e| format!("test.artifact: {}", e))?,
        name: name.clone(),
        run,
        cwd: crate::gen::confine_cwd(test["cwd"].as_str().unwrap_or("."))
            .map_err(|e| format!("test.cwd: {}", e))?,
    };
    let path = artifact_path(&store.out, gs, &tref);
    if !path.vexists() {
        return Err(format!(
            "test artifact `{}` does not exist; write it before recording",
            tref.artifact
        ));
    }
    if kind == "programmatic" {
        // The name a runner addresses may carry a module path (`tests::req_x` for a
        // Rust test inside `mod tests`); the artifact holds the function, so the last
        // path segment is what must be found. Mirrors docs/compiler/goals/generate.md#gate.
        let content = crate::vfs::read_to_string(&path).unwrap_or_default();
        let bare = name.rsplit("::").next().unwrap_or(&name);
        if !content.contains(bare) {
            return Err(format!(
                "test artifact `{}` does not contain the test name `{}`",
                tref.artifact, bare
            ));
        }
    }
    // Implementing files must exist: a binding names what carries the requirement, and
    // a path that is not there carries nothing. An empty list is the honest record of
    // an unimplemented requirement.
    let mut files: Vec<String> = Vec::new();
    for f in files_in.iter().filter(|f| !f.trim().is_empty()) {
        files.push(crate::gen::confine_rel(f).map_err(|e| format!("files: {}", e))?);
    }
    files.sort();
    files.dedup();
    // A test is never an implementing file: this binding's own test artifacts and every
    // row's recorded ones are dropped and named in the reply
    // (docs/consumers/bind.md#the-bind-goal).
    let test_artifacts: std::collections::BTreeSet<String> = {
        let ledger = crate::gen::Ledger::load(&store.out);
        let mut a: std::collections::BTreeSet<String> = ledger
            .requirements
            .values()
            .flat_map(|row| {
                std::iter::once(row.test.artifact.clone())
                    .chain(row.tests.values().map(|t| t.test.artifact.clone()))
            })
            .filter(|p| !p.is_empty())
            .collect();
        a.extend(crit_tests.values().map(|t| t.test.artifact.clone()));
        if let Some(p) = test["artifact"].as_str() {
            a.insert(p.to_string());
        }
        a
    };
    let dropped: Vec<String> = files
        .iter()
        .filter(|f| test_artifacts.contains(f.as_str()))
        .cloned()
        .collect();
    files.retain(|f| !test_artifacts.contains(f.as_str()));
    for f in &files {
        if !gs.deliverable.join(f).vis_file() {
            return Err(format!("implementing file `{}` does not exist under the deliverable; an unimplemented requirement records an empty files list", f));
        }
    }
    let entity = r
        .entities
        .first()
        .map(|e| store.resolve_id(e).to_string())
        .unwrap_or_default();
    let mut ledger = Ledger::load(&store.out);
    let row = ReqRow {
        entity,
        files: files.clone(),
        sites: Vec::new(),
        hashes: RowHashes {
            requirement: hash_hex(&r.statement),
            test: hash_file(&path),
            files: hash_files(gs, &files),
        },
        test: tref,
        tests: crit_tests,
        verdict: verdict.into(),
        last_run: Some(crate::verify::now_iso()),
        exit_code: None,
        evidence: evidence.map(|e| crate::llm::truncate(e, 400).to_string()),
    };
    ledger.requirements.insert(rid.clone(), row);
    // The row lands over an open error diagnostic or not; either way the ledger says
    // which (docs/consumers/gen.md#rows-recorded-over-an-open-contradiction).
    let contradicted = ledger.record_contradicted(store, &rid);
    // The decided medium against what this binding actually wrote. With no entity
    // generated yet the medium is cleared and the next session re-decides it from the
    // recorded commands; once one is generated the medium stands and the warning is
    // the record (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
    let medium_warning = crate::gen::medium_divergence(&ledger).map(|w| {
        if ledger.entities.is_empty() {
            ledger.medium = None;
            format!(
                "{}; no entity is generated yet, so the medium is cleared and the next session re-decides it from the recorded commands",
                w
            )
        } else {
            w
        }
    });
    ledger.save(&store.out);
    let ledger = Ledger::load(&store.out);
    let (status, reason) =
        crate::verify::status_of(store, &rid, ledger.requirements.get(&rid).unwrap(), gs);
    let mut reply = json!({
        "recorded": rid,
        "verdict": verdict,
        "status": status,
        "reason": reason,
        "note": match (status.as_str(), reason.as_str()) {
            ("failing", "criteria-failing-after-generation") => "a generation over the current facts has landed and a criterion test is still red: generation had its turn, so this is a finding for the author (the failing-binding check files it), not generation work; it becomes work again when the docs or the deliverable change",
            (status, _) => match status {
            "verified" => "the deliverable already satisfies the statement",
            "unimplemented" => "nothing implements the statement; the entity is generation work and this test is its acceptance gate",
            "partial" => "implementing files exist and some criterion tests pass while others fail: the entity is generation work for the failing criteria",
            "failing" => "implementing files exist but the tests fail: the deliverable contradicts the statement; the failing-binding check files it for the author at the next check run, never regenerate silently",
            _ => "recorded",
            },
        },
    });
    if !dropped.is_empty() {
        reply["droppedFiles"] = json!(dropped);
        reply["droppedFilesNote"] = json!(
            "test artifacts are never implementing files; list the product files a criterion test inspects, or none"
        );
    }
    if !contradicted.is_empty() {
        reply["contradicted"] = json!(contradicted);
        reply["contradictedNote"] = json!(
            "the row was recorded while an open error diagnostic disputes its statement; the verdict judges one side of an open question until it is resolved"
        );
    }
    if let Some(w) = medium_warning {
        reply["mediumWarning"] = json!(w);
    }
    Ok(reply)
}

// The unclaimed report: deliverable files no binding names. Scope comes from the
// [gen] code globs when set, otherwise everything minus the standard exclusions.
// Mirrors docs/consumers/bind.md#the-unclaimed-report.
pub fn unclaimed(proj: &crate::project::Project, store: &Store, gs: &GenSettings) -> Vec<String> {
    let ledger = Ledger::load(&store.out);
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    for e in ledger.entities.values() {
        claimed.extend(e.files.iter().cloned());
    }
    for row in ledger.requirements.values() {
        claimed.extend(row.files.iter().cloned());
        claimed.insert(row.test.artifact.clone());
    }
    claimed.extend(ledger.support.iter().cloned());
    if let Some(b) = &ledger.build {
        claimed.extend(b.produces.iter().cloned());
    }
    // Docs are source, never unclaimed implementation.
    let docs: BTreeSet<std::path::PathBuf> = proj.doc_files().into_iter().collect();
    let mut all: Vec<std::path::PathBuf> = Vec::new();
    collect_code_files(&gs.deliverable, &store.out, &mut all);
    let mut out: Vec<String> = Vec::new();
    for f in all {
        if docs.contains(&f) {
            continue;
        }
        let rel = match f.strip_prefix(&gs.deliverable) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        if !gs.code.is_empty() {
            let mut included = false;
            for p in &gs.code {
                if let Some(neg) = p.strip_prefix('!') {
                    if crate::project::glob_match(neg, &rel) {
                        included = false;
                    }
                } else if crate::project::glob_match(p, &rel) {
                    included = true;
                }
            }
            if !included {
                continue;
            }
        }
        if !claimed.contains(&rel) {
            out.push(rel);
        }
    }
    out.sort();
    out
}

fn collect_code_files(dir: &Path, out_dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = crate::vfs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p == out_dir
            || name.starts_with('.')
            || name.starts_with("jazyk-out")
            || name == "jazyk.toml"
        {
            continue;
        }
        if p.vis_dir() {
            if name == "target" || name == "node_modules" {
                continue;
            }
            collect_code_files(&p, out_dir, out);
        } else {
            out.push(p);
        }
    }
}

// The built-in bind worker: each owed bind runs as a session with read, file, and
// command tools over the deliverable, the same harness the generation session uses.
// Success is the ledger's word: the session must have left record_binding's row
// with the current statement hash. Mirrors docs/consumers/bind.md#when-binding-runs.
pub fn run_all(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    targets: &[String],
    trace: &crate::session::Trace,
) -> Result<Value, String> {
    let owed: Vec<Value> = pending(store, gs)
        .into_iter()
        .filter(|t| {
            targets.is_empty()
                || targets.iter().any(|x| {
                    let x = store.resolve_id(x);
                    t["requirement"] == x || t["entity"] == x
                })
        })
        .collect();
    if owed.is_empty() {
        return Ok(json!({"bound": 0, "failures": 0, "note": "every requirement is bound"}));
    }
    crate::vfs::create_dir_all(&gs.deliverable).ok();
    let (mut bound, mut failures) = (0u64, 0u64);
    for t in &owed {
        if trace.is_cancelled() {
            break;
        }
        // A test is written in the medium's toolchain, so a deliverable with no decided
        // medium decides it before the bind, the same way the first generation task
        // does. Checked per goal: a record that found the medium diverged from the
        // recorded commands while nothing is generated clears it, and the next bind
        // re-decides it from those commands
        // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
        {
            let mut ledger = Ledger::load(&store.out);
            if ledger.medium.is_none() {
                let medium = crate::gen::decide_medium(store, runner, &gs.deliverable)?;
                trace.line("bind medium", &medium.line());
                ledger.medium = Some(medium);
                ledger.save(&store.out);
            }
        }
        let rid = t["requirement"].as_str().unwrap_or_default().to_string();
        trace.line(
            "bind",
            &format!("{} ({})", rid, t["reason"].as_str().unwrap_or("")),
        );
        let batch = crate::runner::BatchRun::single(crate::model::Goal {
            id: format!("g:bind:{}", rid),
            kind: "bind".into(),
            class: "compile".into(),
            mandatory: true,
            target: rid.clone(),
            unit: "requirement".into(),
            change: json!({"goal": "bind", "reason": t["reason"]}),
            cause: None,
            state: crate::model::GoalState::Open,
            hints: Vec::new(),
        });
        let out = runner.run_item(&batch, trace);
        if let Some(e) = out.failed {
            trace.line("bind", &format!("{} failed: {}", rid, e));
            failures += 1;
            continue;
        }
        // Success is the gate's word, the same check mark_goal_done faces
        // (docs/compiler/goals/bind.md#gate).
        match gate(store, gs, &rid) {
            Ok(()) => bound += 1,
            Err(e) => {
                trace.line("bind", &format!("{} still owed: {}", rid, e));
                failures += 1;
            }
        }
    }
    Ok(json!({"bound": bound, "failures": failures}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store() -> Store {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let out =
            std::env::temp_dir().join(format!("jazyk-bind-test-{}-{}", crate::sys::process_id(), n));
        crate::vfs::remove_dir_all(&out).ok();
        crate::vfs::create_dir_all(&out).unwrap();
        let mut s = Store {
            out,
            ..Default::default()
        };
        s.graph.entities.insert(
            "ent:shop".into(),
            crate::model::Entity {
                name: "Shop".into(),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:shop-1".into(),
            crate::model::Requirement {
                statement: "The shop shall list items.".into(),
                entities: vec!["ent:shop".into()],
                edges: vec![],
                source: Some(crate::model::SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "lists items".into(),
                }),
                ..Default::default()
            },
        );
        s
    }

    #[test]
    fn an_unbound_requirement_is_bind_work() {
        let s = tmp_store();
        let gs = GenSettings {
            configured: true,
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        let p = pending(&s, &gs);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["reason"], "unbound");
        assert_eq!(p[0]["requirement"], "req:shop-1");
    }

    // Mirrors docs/consumers/bind.md#the-bind-goal: a requirement with criteria records
    // one test per criterion, is refused without them, and owes a bind again when a
    // criterion's statement moves.
    #[test]
    fn criteria_take_one_test_each_and_rebind_when_they_move() {
        let mut s = tmp_store();
        for (id, st) in [("crit:shop-1", "Two items listed show two lines."), ("crit:shop-2", "An empty shop shows no line.")] {
            s.graph.criteria.insert(
                id.into(),
                crate::model::Criterion {
                    requirement: "req:shop-1".into(),
                    statement: st.into(),
                    kind: "positive".into(),
                    provenance: Some(crate::model::Provenance::Derived { from: vec!["req:shop-1".into()], reasoning: "evidence".into() }),
                    ..Default::default()
                },
            );
        }
        let gs = GenSettings {
            configured: true,
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        let pkg = task(&s, "req:shop-1", &gs).unwrap();
        assert_eq!(pkg["criteria"].as_array().unwrap().len(), 2, "{}", pkg);
        let n1 = crate::gen::crit_test_name("crit:shop-1", "Two items listed show two lines.");
        let n2 = crate::gen::crit_test_name("crit:shop-2", "An empty shop shows no line.");
        assert!(n1.starts_with("crit_shop_1_"), "{}", n1);
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        crate::vfs::write(gs.deliverable.join("tests/shop.sh"), format!("# {}\n# {}\nexit 1\n", n1, n2)).unwrap();
        let headline = json!({"kind": "programmatic", "artifact": "tests/shop.sh", "name": n1, "run": "sh tests/shop.sh"});
        let err = record(&s, "req:shop-1", &[], &headline, &json!([]), "fail", None, &gs).unwrap_err();
        assert!(err.contains("criteria without a test") && err.contains("crit:shop-1"), "{}", err);
        let tests = json!([
            {"criterion": "crit:shop-1", "kind": "programmatic", "artifact": "tests/shop.sh", "name": n1, "run": "sh tests/shop.sh", "verdict": "fail"},
            {"criterion": "crit:shop-2", "kind": "programmatic", "artifact": "tests/shop.sh", "name": n2, "run": "sh tests/shop.sh", "verdict": "fail"}
        ]);
        let v = record(&s, "req:shop-1", &[], &Value::Null, &tests, "", None, &gs).unwrap();
        assert_eq!(v["status"], "unimplemented", "{}", v);
        let row = Ledger::load(&s.out).requirements["req:shop-1"].clone();
        assert_eq!(row.tests.len(), 2);
        assert_eq!(row.test.name, n1);
        assert!(pending(&s, &gs).is_empty());
        // A criterion moves: the row owes a bind again.
        s.graph.criteria.get_mut("crit:shop-2").unwrap().statement = "An empty shop prints nothing.".into();
        let p = pending(&s, &gs);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["reason"], "criterion-changed");
    }

    // Mirrors docs/consumers/gen.md#status-is-derived-never-stored: implementing files
    // with the criterion tests split read partial, generation work, not a contradiction.
    #[test]
    fn split_criterion_tests_read_partial() {
        let mut s = tmp_store();
        for (id, st) in [("crit:shop-1", "Two items listed show two lines."), ("crit:shop-2", "A nameless item is refused.")] {
            s.graph.criteria.insert(
                id.into(),
                crate::model::Criterion {
                    requirement: "req:shop-1".into(),
                    statement: st.into(),
                    kind: "positive".into(),
                    provenance: Some(crate::model::Provenance::Derived { from: vec!["req:shop-1".into()], reasoning: "evidence".into() }),
                    ..Default::default()
                },
            );
        }
        let gs = GenSettings {
            configured: true,
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        std::fs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        std::fs::create_dir_all(gs.deliverable.join("src")).unwrap();
        std::fs::write(gs.deliverable.join("src/shop.rs"), "pub fn list() {}\n").unwrap();
        let n1 = crate::gen::crit_test_name("crit:shop-1", "Two items listed show two lines.");
        let n2 = crate::gen::crit_test_name("crit:shop-2", "A nameless item is refused.");
        std::fs::write(gs.deliverable.join("tests/shop.sh"), format!("# {}\n# {}\nexit 0\n", n1, n2)).unwrap();
        let tests = json!([
            {"criterion": "crit:shop-1", "kind": "programmatic", "artifact": "tests/shop.sh", "name": n1, "run": "sh tests/shop.sh", "verdict": "pass"},
            {"criterion": "crit:shop-2", "kind": "programmatic", "artifact": "tests/shop.sh", "name": n2, "run": "sh tests/shop.sh", "verdict": "fail"}
        ]);
        let v = record(&s, "req:shop-1", &["src/shop.rs".into()], &Value::Null, &tests, "", None, &gs).unwrap();
        assert_eq!(v["status"], "partial", "{}", v);
        assert_eq!(v["reason"], "criteria-failing");
        // The headline is crit:shop-1's test: a pass recorded on the row lands on it.
        let row = Ledger::load(&s.out).requirements["req:shop-1"].clone();
        assert_eq!(row.test.name, n1);
        crate::verify::mark(&s, "req:shop-1", "pass", None, Some("green"), Some(0), &gs).unwrap();
        let row = Ledger::load(&s.out).requirements["req:shop-1"].clone();
        assert_eq!(row.tests["crit:shop-1"].verdict, "pass");
        assert_eq!(row.tests["crit:shop-2"].verdict, "fail");
    }

    // Mirrors docs/consumers/bind.md#the-bind-goal: existingTests matches whole names
    // (crit:x-1 is no part of crit:x-13) and says whether a hit carries the current hash.
    #[test]
    fn existing_tests_match_whole_names_and_label_the_hash() {
        let dir = std::env::temp_dir().join(format!("jazyk-existing-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("tests/a.py"), "# crit:x-13 crit_x_13_0d4edd0c\n").unwrap();
        std::fs::write(dir.join("tests/b.py"), "# crit:x-1 crit_x_1_aaaaaaaa\n").unwrap();
        std::fs::write(dir.join("tests/c.py"), "# crit_x_1_deadbeef\n").unwrap();
        let hits = existing_tests(&dir, &["crit:x-1".to_string(), "crit_x_1".to_string(), "deadbeef".to_string()]);
        let files: Vec<&str> = hits.iter().map(|h| h["file"].as_str().unwrap()).collect();
        assert_eq!(files, vec!["tests/b.py", "tests/c.py"], "{:?}", hits);
        assert_eq!(hits[0]["carriesCurrentHash"], false);
        assert!(hits[0]["note"].as_str().unwrap().contains("another"), "{}", hits[0]);
        assert_eq!(hits[1]["carriesCurrentHash"], true);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn recording_a_failing_bind_with_no_files_reads_unimplemented() {
        let s = tmp_store();
        let gs = GenSettings {
            configured: true,
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        let name = crate::gen::test_name("req:shop-1", "The shop shall list items.");
        crate::vfs::write(
            gs.deliverable.join("tests/shop.sh"),
            format!("# {}\nexit 1\n", name),
        )
        .unwrap();
        let test = json!({"kind": "programmatic", "artifact": "tests/shop.sh", "name": name, "run": "sh tests/shop.sh"});
        let v = record(
            &s,
            "req:shop-1",
            &[],
            &test,
            &json!([]),
            "fail",
            Some("no implementation found"),
            &gs,
        )
        .unwrap();
        assert_eq!(v["status"], "unimplemented");
        // Bound: no longer bind work.
        assert!(pending(&s, &gs).is_empty());
    }

    // A test is never an implementing file: 22 of 389 recorded rows listed their own
    // test as implementation and read failing where they were unimplemented
    // (docs/consumers/bind.md#the-bind-goal).
    #[test]
    fn a_test_artifact_listed_as_implementation_is_dropped() {
        let s = tmp_store();
        let gs = GenSettings {
            configured: true,
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        let name = crate::gen::test_name("req:shop-1", "The shop shall list items.");
        crate::vfs::write(
            gs.deliverable.join("tests/shop.sh"),
            format!("# {}\nexit 1\n", name),
        )
        .unwrap();
        let test = json!({"kind": "programmatic", "artifact": "tests/shop.sh", "name": name, "run": "sh tests/shop.sh"});
        let v = record(
            &s,
            "req:shop-1",
            &["tests/shop.sh".into()],
            &test,
            &json!([]),
            "fail",
            Some("the only file is the test itself"),
            &gs,
        )
        .unwrap();
        assert_eq!(v["status"], "unimplemented", "{}", v);
        assert_eq!(v["droppedFiles"], json!(["tests/shop.sh"]), "{}", v);
        let ledger = crate::gen::Ledger::load(&s.out);
        assert!(ledger.requirements["req:shop-1"].files.is_empty());
    }

    #[test]
    fn recording_a_passing_bind_with_files_reads_verified() {
        let s = tmp_store();
        let gs = GenSettings {
            configured: true,
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        crate::vfs::write(gs.deliverable.join("shop.sh"), "echo items\n").unwrap();
        let name = crate::gen::test_name("req:shop-1", "The shop shall list items.");
        crate::vfs::write(
            gs.deliverable.join("tests/shop.sh"),
            format!("# {}\nexit 0\n", name),
        )
        .unwrap();
        let test = json!({"kind": "programmatic", "artifact": "tests/shop.sh", "name": name, "run": "sh tests/shop.sh"});
        let v = record(
            &s,
            "req:shop-1",
            &["shop.sh".into()],
            &test,
            &json!([]),
            "pass",
            None,
            &gs,
        )
        .unwrap();
        assert_eq!(v["status"], "verified");
        // The implementing file is claimed; the unclaimed report excludes it.
        let mut proj = crate::project::Project::default();
        proj.root = s.out.clone();
        let un = unclaimed(&proj, &s, &gs);
        assert!(
            !un.contains(&"shop.sh".to_string()),
            "claimed file listed unclaimed: {:?}",
            un
        );
    }

    // A bind recorded over a diverged medium while nothing is generated clears the
    // medium so the next session re-decides it from the recorded commands.
    // Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
    #[test]
    fn a_bind_over_a_diverged_medium_re_decides_while_nothing_is_generated() {
        let s = tmp_store();
        let gs = GenSettings {
            configured: true,
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        let mut ledger = Ledger::default();
        ledger.medium = Some(crate::gen::Medium {
            form: "Python package".into(),
            produced: "written".into(),
            toolchain: "python3 with pytest".into(),
            artifact: String::new(),
        });
        ledger.save(&s.out);
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        let name = crate::gen::test_name("req:shop-1", "The shop shall list items.");
        crate::vfs::write(
            gs.deliverable.join("tests/shop.rs"),
            format!("fn {}() {{}}\n", name),
        )
        .unwrap();
        let test = json!({"kind": "programmatic", "artifact": "tests/shop.rs", "name": name, "run": format!("cargo test {}", name)});
        let v = record(&s, "req:shop-1", &[], &test, &json!([]), "fail", None, &gs).unwrap();
        let w = v["mediumWarning"].as_str().expect("the record warns");
        assert!(w.contains("re-decides"), "{}", w);
        assert!(
            Ledger::load(&s.out).medium.is_none(),
            "nothing generated: the medium is cleared for re-decision"
        );
        // The re-bind package carries nothing stale: no medium, no warning.
        let pkg = task(&s, "req:shop-1", &gs).unwrap();
        assert!(pkg["mediumWarning"].is_null(), "{}", pkg);

        // With an entity generated, the medium stands and the warning is the record.
        let mut ledger = Ledger::load(&s.out);
        ledger.medium = Some(crate::gen::Medium {
            form: "Python package".into(),
            produced: "written".into(),
            toolchain: "python3 with pytest".into(),
            artifact: String::new(),
        });
        ledger.entities.insert(
            "shop".into(),
            crate::gen::EntityGen {
                fact_hash: "x".into(),
                requirements: vec!["req:shop-1".into()],
                files: vec!["tests/shop.rs".into()],
                unattached: None,
                recorded_at: None,
            },
        );
        ledger.save(&s.out);
        let v = record(&s, "req:shop-1", &[], &test, &json!([]), "fail", None, &gs).unwrap();
        let w = v["mediumWarning"].as_str().expect("the record still warns");
        assert!(!w.contains("re-decides"), "{}", w);
        assert!(Ledger::load(&s.out).medium.is_some(), "the medium stands");
        let pkg = task(&s, "req:shop-1", &gs).unwrap();
        assert!(pkg["mediumWarning"].as_str().is_some(), "{}", pkg);
        assert!(pkg["openDiagnostics"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false));
    }

    // Every path a binding names stays under the deliverable: a climbing or absolute
    // implementing file or artifact is rejected naming it, and the ledger is untouched.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    #[test]
    fn binding_paths_are_confined_to_the_deliverable() {
        let s = tmp_store();
        let gs = GenSettings {
            configured: true,
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        crate::vfs::write(s.out.join("outside.sh"), "echo secret\n").unwrap();
        let name = crate::gen::test_name("req:shop-1", "The shop shall list items.");
        crate::vfs::write(
            gs.deliverable.join("tests/shop.sh"),
            format!("# {}\nexit 0\n", name),
        )
        .unwrap();
        let test = json!({"kind": "programmatic", "artifact": "tests/shop.sh", "name": name, "run": "sh tests/shop.sh"});
        let err = record(&s, "req:shop-1", &["../outside.sh".into()], &test, &json!([]), "pass", None, &gs)
            .unwrap_err();
        assert!(err.contains("files") && err.contains(".."), "{}", err);
        let climbing = json!({"kind": "programmatic", "artifact": "../outside.sh", "name": "echo", "run": "sh ../outside.sh"});
        let err = record(&s, "req:shop-1", &[], &climbing, &json!([]), "pass", None, &gs).unwrap_err();
        assert!(err.contains("test.artifact"), "{}", err);
        let absolute = json!({"kind": "programmatic", "artifact": "tests/shop.sh", "name": name, "run": "sh tests/shop.sh", "cwd": "/tmp"});
        let err = record(&s, "req:shop-1", &[], &absolute, &json!([]), "pass", None, &gs).unwrap_err();
        assert!(err.contains("test.cwd"), "{}", err);
        assert!(Ledger::load(&s.out).requirements.is_empty());
        // The clean spelling normalizes.
        let ok = json!({"kind": "programmatic", "artifact": "./tests/shop.sh", "name": name, "run": "sh tests/shop.sh"});
        record(&s, "req:shop-1", &[], &ok, &json!([]), "fail", None, &gs).unwrap();
        assert_eq!(
            Ledger::load(&s.out).requirements["req:shop-1"].test.artifact,
            "tests/shop.sh"
        );
    }

    // The bind gate is the ledger's word: no row, a stale statement hash, or an
    // artifact that lost the test each bounce a done claim naming the repair; a
    // current row passes whatever its verdict. Mirrors docs/compiler/goals/bind.md#gate.
    #[test]
    fn the_bind_gate_reads_the_landed_row() {
        let mut s = tmp_store();
        let gs = GenSettings {
            configured: true,
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        let err = gate(&s, &gs, "req:shop-1").unwrap_err();
        assert!(err.contains("record_binding has not landed"), "{}", err);
        crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        let name = crate::gen::test_name("req:shop-1", "The shop shall list items.");
        crate::vfs::write(gs.deliverable.join("tests/shop.sh"), format!("# {}\nexit 1\n", name)).unwrap();
        let test = json!({"kind": "programmatic", "artifact": "tests/shop.sh", "name": name, "run": "sh tests/shop.sh"});
        record(&s, "req:shop-1", &[], &test, &json!([]), "fail", None, &gs).unwrap();
        gate(&s, &gs, "req:shop-1").unwrap();
        // The test renamed by hand: the row names a test that is not there.
        crate::vfs::write(gs.deliverable.join("tests/shop.sh"), "# renamed\nexit 1\n").unwrap();
        let err = gate(&s, &gs, "req:shop-1").unwrap_err();
        assert!(err.contains("does not contain the name"), "{}", err);
        crate::vfs::write(gs.deliverable.join("tests/shop.sh"), format!("# {}\nexit 1\n", name)).unwrap();
        gate(&s, &gs, "req:shop-1").unwrap();
        // The statement reworded: the row judges a dropped sentence.
        s.graph.requirements.get_mut("req:shop-1").unwrap().statement = "The shop shall list items, sorted.".into();
        let err = gate(&s, &gs, "req:shop-1").unwrap_err();
        assert!(err.contains("statement moved") && err.contains("begin_binding again"), "{}", err);
        assert_eq!(task(&s, "req:shop-1", &gs).unwrap()["reason"], "requirement-changed");
    }

    #[test]
    fn a_missing_test_artifact_is_rejected() {
        let s = tmp_store();
        let gs = GenSettings {
            configured: true,
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        let test = json!({"kind": "programmatic", "artifact": "tests/none.sh", "name": "req_x", "run": "sh tests/none.sh"});
        assert!(record(&s, "req:shop-1", &[], &test, &json!([]), "fail", None, &gs).is_err());
    }

    #[test]
    fn the_unclaimed_report_lists_code_no_binding_names() {
        let s = tmp_store();
        let gs = GenSettings {
            configured: true,
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        crate::vfs::create_dir_all(&gs.deliverable).unwrap();
        crate::vfs::write(gs.deliverable.join("orphan.sh"), "echo lonely\n").unwrap();
        let mut proj = crate::project::Project::default();
        proj.root = s.out.clone();
        let un = unclaimed(&proj, &s, &gs);
        assert!(un.contains(&"orphan.sh".to_string()), "{:?}", un);
    }
}
