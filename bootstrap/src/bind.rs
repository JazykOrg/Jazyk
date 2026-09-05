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
    if !artifact.exists() {
        return Some("artifact-gone");
    }
    if row.test.kind == "programmatic"
        && !std::fs::read_to_string(&artifact)
            .map(|c| !row.test.name.trim().is_empty() && c.contains(&row.test.name))
            .unwrap_or(false)
    {
        return Some("artifact-gone");
    }
    None
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
        .map(|row| json!({"kind": row.test.kind, "artifact": row.test.artifact, "run": row.test.run}))
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
    Ok(json!({
        "requirement": rid,
        "entity": entity,
        "reason": reason,
        "statement": r.statement,
        "quote": r.source.as_ref().map(|s| s.quote.clone()).unwrap_or_default(),
        "provenance": crate::session::provenance_line(r),
        "factHash": hash_hex(&r.statement),
        "deliverable": gs.deliverable.to_string_lossy(),
        "suggestedTestName": crate::gen::test_name(&rid, &r.statement),
        "medium": ledger.medium.as_ref().map(|m| m.line()),
        // The medium's standing divergence from the recorded commands, if any
        // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
        "mediumWarning": crate::gen::medium_divergence(&ledger),
        "build": ledger.build.as_ref().map(|b| json!({"run": b.run, "cwd": b.cwd})),
        "testConventions": conventions,
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
pub fn record(
    store: &Store,
    rid: &str,
    files: &[String],
    test: &Value,
    verdict: &str,
    evidence: Option<&str>,
    gs: &GenSettings,
) -> Result<Value, String> {
    let rid = store.resolve_id(rid).to_string();
    let Some(r) = store.graph.requirements.get(&rid) else {
        return Err(format!("unknown requirement `{}`", rid));
    };
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
        test["artifact"].as_str().unwrap_or_default().to_string(),
        test["name"].as_str().unwrap_or_default().to_string(),
        test["run"].as_str().unwrap_or_default().to_string(),
    );
    if artifact.is_empty() || name.is_empty() {
        return Err("test.artifact and test.name are required".into());
    }
    if kind == "programmatic" && run.is_empty() {
        return Err(
            "a programmatic test needs test.run: the command whose exit code is the verdict".into(),
        );
    }
    let tref = TestRef {
        kind: kind.into(),
        label: test["label"].as_str().unwrap_or("bound").into(),
        artifact,
        name: name.clone(),
        run,
        cwd: test["cwd"].as_str().unwrap_or(".").into(),
    };
    let path = artifact_path(&store.out, gs, &tref);
    if !path.exists() {
        return Err(format!(
            "test artifact `{}` does not exist; write it before recording",
            tref.artifact
        ));
    }
    if kind == "programmatic" {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        if !content.contains(&name) {
            return Err(format!(
                "test artifact `{}` does not contain the test name `{}`",
                tref.artifact, name
            ));
        }
    }
    // Implementing files must exist: a binding names what carries the requirement, and
    // a path that is not there carries nothing. An empty list is the honest record of
    // an unimplemented requirement.
    let mut files: Vec<String> = files
        .iter()
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .collect();
    files.sort();
    files.dedup();
    for f in &files {
        if !gs.deliverable.join(f).exists() {
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
        "note": match status.as_str() {
            "verified" => "the deliverable already satisfies the statement",
            "unimplemented" => "nothing implements the statement; the entity is generation work and this test is its acceptance gate",
            "failing" => "implementing files exist but the test fails: the deliverable contradicts the statement; surface it to the author, never regenerate silently",
            _ => "recorded",
        },
    });
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
    let Ok(entries) = std::fs::read_dir(dir) else {
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
        if p.is_dir() {
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
    runner: &crate::acp::runner::AcpRunner,
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
    std::fs::create_dir_all(&gs.deliverable).ok();
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
        let batch = crate::acp::runner::BatchRun::single(crate::model::Goal {
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
        let ledger = Ledger::load(&store.out);
        let live = store
            .graph
            .requirements
            .get(&rid)
            .map(|r| hash_hex(&r.statement))
            .unwrap_or_default();
        match ledger.requirements.get(&rid) {
            Some(row) if row.hashes.requirement == live => bound += 1,
            Some(_) => {
                trace.line(
                    "bind",
                    &format!("{} recorded a stale statement hash; still owed", rid),
                );
                failures += 1;
            }
            None => {
                trace.line(
                    "bind",
                    &format!("{} ended without record_binding; still owed", rid),
                );
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
            std::env::temp_dir().join(format!("jazyk-bind-test-{}-{}", std::process::id(), n));
        std::fs::remove_dir_all(&out).ok();
        std::fs::create_dir_all(&out).unwrap();
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
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        let p = pending(&s, &gs);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["reason"], "unbound");
        assert_eq!(p[0]["requirement"], "req:shop-1");
    }

    #[test]
    fn recording_a_failing_bind_with_no_files_reads_unimplemented() {
        let s = tmp_store();
        let gs = GenSettings {
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        std::fs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        let name = crate::gen::test_name("req:shop-1", "The shop shall list items.");
        std::fs::write(
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
            "fail",
            Some("no implementation found"),
            &gs,
        )
        .unwrap();
        assert_eq!(v["status"], "unimplemented");
        // Bound: no longer bind work.
        assert!(pending(&s, &gs).is_empty());
    }

    #[test]
    fn recording_a_passing_bind_with_files_reads_verified() {
        let s = tmp_store();
        let gs = GenSettings {
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        std::fs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        std::fs::write(gs.deliverable.join("shop.sh"), "echo items\n").unwrap();
        let name = crate::gen::test_name("req:shop-1", "The shop shall list items.");
        std::fs::write(
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
        std::fs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        let name = crate::gen::test_name("req:shop-1", "The shop shall list items.");
        std::fs::write(
            gs.deliverable.join("tests/shop.rs"),
            format!("fn {}() {{}}\n", name),
        )
        .unwrap();
        let test = json!({"kind": "programmatic", "artifact": "tests/shop.rs", "name": name, "run": format!("cargo test {}", name)});
        let v = record(&s, "req:shop-1", &[], &test, "fail", None, &gs).unwrap();
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
            },
        );
        ledger.save(&s.out);
        let v = record(&s, "req:shop-1", &[], &test, "fail", None, &gs).unwrap();
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

    #[test]
    fn a_missing_test_artifact_is_rejected() {
        let s = tmp_store();
        let gs = GenSettings {
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        let test = json!({"kind": "programmatic", "artifact": "tests/none.sh", "name": "req_x", "run": "sh tests/none.sh"});
        assert!(record(&s, "req:shop-1", &[], &test, "fail", None, &gs).is_err());
    }

    #[test]
    fn the_unclaimed_report_lists_code_no_binding_names() {
        let s = tmp_store();
        let gs = GenSettings {
            deliverable: s.out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        std::fs::create_dir_all(&gs.deliverable).unwrap();
        std::fs::write(gs.deliverable.join("orphan.sh"), "echo lonely\n").unwrap();
        let mut proj = crate::project::Project::default();
        proj.root = s.out.clone();
        let un = unclaimed(&proj, &s, &gs);
        assert!(un.contains(&"orphan.sh".to_string()), "{:?}", un);
    }
}
