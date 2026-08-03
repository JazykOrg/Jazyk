// Verification: run the tests the ledger records and feed verdicts back. Status is
// derived, never stored; every staleness flip is a hash comparison. Mirrors
// docs/consumers/gen.md (the ledger, runners) and
// docs/compiler/tools.md#verification-tools.
use crate::gen::{artifact_path, hash_file, hash_files, GenSettings, Ledger, ReqRow};
use crate::model::hash_hex;
use crate::store::Store;
use serde_json::{json, Value};
use std::path::PathBuf;

// Derived status, first match wins. Mirrors
// docs/consumers/gen.md#status-is-derived-never-stored.
pub fn status_of(store: &Store, rid: &str, row: &ReqRow, gs: &GenSettings) -> (String, String) {
    let live = store.graph.requirements.get(store.resolve_id(rid));
    let Some(r) = live else {
        return ("missing".into(), "requirement-gone".into());
    };
    let artifact = artifact_path(&store.out, gs, &row.test);
    if !artifact.exists() {
        return ("missing".into(), "artifact-gone".into());
    }
    if hash_hex(&r.ears) != row.hashes.requirement {
        return ("stale-requirement".into(), "requirement-changed".into());
    }
    if hash_file(&artifact) != row.hashes.test {
        return ("stale-test".into(), "test-changed".into());
    }
    if hash_files(gs, &row.files) != row.hashes.files {
        return ("stale-code".into(), "code-changed".into());
    }
    match row.verdict.as_str() {
        "pass" => ("verified".into(), "passed".into()),
        "fail" => ("failing".into(), "failed".into()),
        // A command ran and left a code, yet no verdict was recorded: the runner
        // failed and the requirement was never exercised
        // (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
        _ if row.exit_code.is_some() => ("unverified".into(), "runner-failed".into()),
        _ => ("unverified".into(), "never-run".into()),
    }
}

// Rows needing action, with derived status and reason. Requirements the graph holds
// but the ledger does not appear as `missing`, so ungenerated work is never silent.
// Deterministic; no model.
pub fn pending(store: &Store, gs: &GenSettings, filter: Option<&str>, entity: Option<&str>) -> Vec<Value> {
    let ledger = Ledger::load(&store.out);
    let mut out = Vec::new();
    for (rid, row) in &ledger.requirements {
        if let Some(ent) = entity {
            if store.resolve_id(&row.entity) != store.resolve_id(ent) {
                continue;
            }
        }
        let (status, reason) = status_of(store, rid, row, gs);
        let actionable = match filter.unwrap_or("stale") {
            "all" => true,
            "failing" => status == "failing",
            // Default: everything that needs a hand, excluding verified.
            _ => status != "verified",
        };
        if !actionable {
            continue;
        }
        out.push(json!({
            "requirement": rid,
            "entity": row.entity,
            "status": status,
            "reason": reason,
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
        let owner = r.entities.first().map(|e| store.resolve_id(e).to_string()).unwrap_or_default();
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
    for (rid, _r) in &store.graph.requirements {
        let entry = match ledger.requirements.get(rid) {
            Some(row) => {
                let (status, reason) = status_of(store, rid, row, gs);
                json!({
                    "status": status,
                    "reason": reason,
                    "kind": row.test.kind,
                    "label": row.test.label,
                    "name": row.test.name,
                    "run": row.test.run,
                    "lastRun": row.last_run,
                    "evidence": row.evidence,
                    "verdict": row.verdict,
                })
            }
            None => json!({"status": "missing", "reason": "not-generated"}),
        };
        out.insert(rid.clone(), entry);
    }
    out
}

// Counts by reason, for await_changes.
pub fn pending_counts(store: &Store, gs: &GenSettings) -> Value {
    let mut counts: std::collections::BTreeMap<String, u64> = Default::default();
    for p in pending(store, gs, Some("stale"), None) {
        *counts.entry(p["reason"].as_str().unwrap_or("?").to_string()).or_default() += 1;
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
    let (status, reason) = status_of(store, &rid, row, gs);
    if status == "stale-requirement" {
        return Err(format!(
            "`{}` is stale-requirement: the test verifies a statement that no longer exists; regenerate with `jazyk gen {}`",
            rid, row.entity
        ));
    }
    let pack = crate::context::assemble(
        store,
        &rid,
        &crate::context::Focus { parents: 1, mentions: 1, requirements: 2 },
        8_000,
    )
    .map(|p| p.pack)
    .unwrap_or_default();
    let artifact = artifact_path(&store.out, gs, &row.test);
    let criteria = if row.test.kind == "llm" {
        std::fs::read_to_string(&artifact).unwrap_or_default()
    } else {
        String::new()
    };
    Ok(json!({
        "requirement": rid,
        "entity": row.entity,
        "ears": r.ears,
        "quote": r.source.quote,
        "factHash": hash_hex(&r.ears),
        "status": status,
        "reason": reason,
        "deliverable": gs.deliverable.to_string_lossy(),
        "files": row.files,
        "test": {"kind": row.test.kind, "label": row.test.label, "artifact": row.test.artifact,
                 "name": row.test.name, "run": row.test.run, "cwd": row.test.cwd},
        "criteria": criteria,
        "context": pack,
        "instructions": if row.test.kind == "llm" {
            "Confirm the requirement is satisfied by the implementing files. Follow the criteria's confirm steps using the deliverable paths. Report verdict pass only if every criterion is met; state what you inspected or executed and what you observed."
        } else {
            "Execute the run command in the test's cwd under the deliverable. Exit code 0 is a pass. Report the verdict with the command output tail as evidence."
        },
    }))
}

// Record a verdict. Rebaselines the test and files hashes, never the requirement hash;
// a stale factHash is recorded but the row stays pending by derivation.
pub fn mark(store: &Store, rid: &str, verdict: &str, fact_hash_seen: Option<&str>, evidence: Option<&str>, gs: &GenSettings) -> Result<Value, String> {
    if verdict != "pass" && verdict != "fail" {
        return Err(format!("verdict must be `pass` or `fail`, got `{}`", verdict));
    }
    let rid = store.resolve_id(rid).to_string();
    let mut ledger = Ledger::load(&store.out);
    let Some(row) = ledger.requirements.get_mut(&rid) else {
        return Err(format!("no ledger row for `{}`", rid));
    };
    row.verdict = verdict.to_string();
    row.exit_code = None;
    row.last_run = Some(now_iso());
    row.evidence = evidence.map(|e| crate::llm::truncate(e, 400));
    row.hashes.test = hash_file(&artifact_path(&store.out, gs, &row.test));
    row.hashes.files = hash_files(gs, &row.files);
    let stale = match fact_hash_seen {
        Some(h) => {
            store.graph.requirements.get(&rid).map(|r| hash_hex(&r.ears) != h).unwrap_or(true)
        }
        None => false,
    };
    let (status, _) = status_of(store, &rid, ledger.requirements.get(&rid).unwrap(), gs);
    ledger.save(&store.out);
    Ok(json!({"recorded": rid, "verdict": verdict, "status": status, "graphMoved": stale}))
}

// Run one programmatic row: grep the artifact for the test name first (absent means
// stale-test, not failing), then execute the command; the exit code is the verdict.
// What one programmatic run produced. `tail` is the output alone: a run compares tails
// with other rows to tell one broken runner from many unmet requirements
// (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
pub struct ProgRun {
    pub pass: bool,
    pub code: i32,
    pub tail: String,
    pub evidence: String,
}

impl ProgRun {
    // The command was never executed: not found, or not executable. Nothing here says
    // anything about the requirement.
    pub fn runner_failed(&self) -> bool {
        self.code == 126 || self.code == 127
    }
}

pub fn run_programmatic(store: &Store, rid: &str, gs: &GenSettings) -> Result<ProgRun, String> {
    let rid = store.resolve_id(rid).to_string();
    let ledger = Ledger::load(&store.out);
    let Some(row) = ledger.requirements.get(&rid) else {
        return Err(format!("no ledger row for `{}`", rid));
    };
    if row.test.run.trim().is_empty() {
        return Err(format!(
            "`{}` has no recorded run command (audit-rebuilt row); regenerate with `jazyk gen {}`",
            rid, row.entity
        ));
    }
    let artifact = artifact_path(&store.out, gs, &row.test);
    let content = std::fs::read_to_string(&artifact).unwrap_or_default();
    if !content.contains(&row.test.name) {
        return Err(format!(
            "test `{}` not found in {}; the row is stale-test, regenerate or audit",
            row.test.name,
            artifact.display()
        ));
    }
    let cwd = gs.deliverable.join(&row.test.cwd);
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(&row.test.run)
        .current_dir(&cwd)
        .output()
        .map_err(|e| format!("failed to run `{}` in {}: {}", row.test.run, cwd.display(), e))?;
    let pass = out.status.success();
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
        code: out.status.code().unwrap_or(-1),
        evidence: format!("{}: {}", row.test.run, tail),
        tail,
    })
}

// The `run_tests` tool: the build once, then every selected programmatic row, verdicts
// recorded as a side effect. No model anywhere, so any toolset can serve it; `llm` rows
// are listed as skipped and go through record_verdict.
// Mirrors docs/compiler/tools.md#verification-tools.
pub fn run_selected(store: &Store, gs: &GenSettings, targets: &[String]) -> Result<Value, String> {
    let quiet = crate::turn::Trace::stderr(crate::turn::TraceLevel::Quiet);
    crate::gen::run_build(&store.out, gs, &quiet, "run_tests").map_err(|e| format!("{}; nothing was verified", e))?;
    let ledger = Ledger::load(&store.out);
    // Explicit targets (requirement or entity ids), or every non-verified row.
    let selected: Vec<String> = if targets.is_empty() {
        pending(store, gs, Some("stale"), None)
            .iter()
            .filter_map(|p| p["requirement"].as_str().map(String::from))
            .collect()
    } else {
        let mut v: Vec<String> = Vec::new();
        for t in targets {
            let id = store.resolve_id(t).to_string();
            if ledger.requirements.contains_key(&id) {
                v.push(id);
            } else {
                for (rid, row) in &ledger.requirements {
                    if store.resolve_id(&row.entity) == id && !v.contains(rid) {
                        v.push(rid.clone());
                    }
                }
            }
        }
        v
    };
    let (mut passed, mut failed) = (0u64, 0u64);
    let mut rows: Vec<Value> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    for rid in &selected {
        let Some(row) = ledger.requirements.get(rid) else {
            skipped.push(json!({"requirement": rid, "reason": "no ledger row; generate first"}));
            continue;
        };
        if row.test.kind != "programmatic" {
            skipped.push(json!({"requirement": rid, "reason": "llm row; judge it and record_verdict"}));
            continue;
        }
        let (status, reason) = status_of(store, rid, row, gs);
        if status == "stale-requirement" || status == "missing" {
            skipped.push(json!({"requirement": rid, "reason": format!("{} ({}); regenerate first", status, reason)}));
            continue;
        }
        match run_programmatic(store, rid, gs) {
            Ok(r) => {
                let verdict = if r.pass { "pass" } else { "fail" };
                mark(store, rid, verdict, None, Some(&r.evidence), gs).ok();
                if r.pass {
                    passed += 1;
                } else {
                    failed += 1;
                }
                rows.push(json!({"requirement": rid, "verdict": verdict, "exitCode": r.code, "evidence": r.tail}));
            }
            Err(e) => skipped.push(json!({"requirement": rid, "reason": e})),
        }
    }
    Ok(json!({
        "passed": passed,
        "failed": failed,
        "rows": rows,
        "skipped": skipped,
        "next": if failed > 0 {
            "a failing row names its requirement; fix the deliverable or the test, then run_tests again"
        } else if !skipped.is_empty() {
            "skipped rows say why; llm rows need begin_verification and record_verdict"
        } else {
            "all selected rows pass"
        }
    }))
}

// Record a run that judged nothing: the exit code and the output stay, the verdict does
// not move. A row with an exit code and no verdict reads as `runner-failed`
// (docs/consumers/gen.md#status-is-derived-never-stored).
pub fn record_runner_failure(store: &Store, rid: &str, code: i32, evidence: &str) {
    let rid = store.resolve_id(rid).to_string();
    let mut ledger = Ledger::load(&store.out);
    if let Some(row) = ledger.requirements.get_mut(&rid) {
        // The previous verdict is not today's answer: this run moved lastRun and
        // learned nothing, so the row is unknown rather than stale-but-confident.
        row.verdict = "none".into();
        row.exit_code = Some(code);
        row.evidence = Some(crate::llm::truncate(evidence, 300));
        row.last_run = Some(now_iso());
        ledger.save(&store.out);
    }
}

// Rebuild the ledger from the artifacts: existing rows refresh their test and files
// hashes when the artifact still carries the live statement hash; rows the ledger lost
// are recreated by scanning the deliverable and the criteria directory for the test
// names derived from the live statements. Sites are not rebuilt: only generation
// records them. The requirement hash is never rewritten from the live graph, so an
// artifact carrying an outdated statement stays stale-requirement until regeneration.
// Mirrors docs/consumers/gen.md#runners.
pub fn audit(store: &Store, gs: &GenSettings) -> Value {
    let mut ledger = Ledger::load(&store.out);
    let mut refreshed = 0;
    let mut rebuilt: Vec<String> = Vec::new();
    let mut orphans: Vec<String> = Vec::new();

    // Existing rows: refresh test/files baselines only when the artifact still names
    // the test derived from the LIVE statement (name embeds the statement hash prefix).
    let rids: Vec<String> = ledger.requirements.keys().cloned().collect();
    for rid in rids {
        let row = ledger.requirements.get(&rid).unwrap().clone();
        let Some(r) = store.graph.requirements.get(store.resolve_id(&rid)) else {
            orphans.push(rid.clone());
            continue;
        };
        let artifact = artifact_path(&store.out, gs, &row.test);
        let content = std::fs::read_to_string(&artifact).unwrap_or_default();
        if content.contains(&crate::gen::test_name(&rid, &r.ears)) {
            let row = ledger.requirements.get_mut(&rid).unwrap();
            row.hashes.test = hash_file(&artifact);
            row.hashes.files = hash_files(gs, &row.files);
            refreshed += 1;
        }
    }
    for rid in &orphans {
        ledger.requirements.remove(rid);
    }

    // Lost rows: scan for the test name derived from the live statement. Found in the
    // criteria directory means an llm row; found in the deliverable, programmatic.
    let scan: Vec<(PathBuf, bool)> = {
        let mut v = Vec::new();
        collect_files(&gs.deliverable, &mut v, false);
        collect_files(&store.out.join("gen").join("criteria"), &mut v, true);
        v
    };
    for (rid, r) in &store.graph.requirements {
        if ledger.requirements.contains_key(rid) {
            continue;
        }
        let name = crate::gen::test_name(rid, &r.ears);
        for (path, is_criteria) in &scan {
            let Ok(content) = std::fs::read_to_string(path) else { continue };
            if !content.contains(&name) {
                continue;
            }
            let owner = r.entities.first().map(|e| store.resolve_id(e).to_string()).unwrap_or_default();
            let artifact_rel = if *is_criteria {
                path.strip_prefix(store.out.join("gen")).unwrap_or(path).to_string_lossy().to_string()
            } else {
                path.strip_prefix(&gs.deliverable).unwrap_or(path).to_string_lossy().to_string()
            };
            let files = ledger
                .entities
                .get(&crate::gen::slug_of(&owner))
                .map(|e| e.files.clone())
                .unwrap_or_default();
            let test = crate::gen::TestRef {
                kind: if *is_criteria { "llm".into() } else { "programmatic".into() },
                label: if *is_criteria { "llm".into() } else { "audit".into() },
                artifact: artifact_rel,
                name: name.clone(),
                // A rebuilt programmatic row has no trustworthy command; the harness
                // never invents one. Regeneration records the real runner.
                run: if *is_criteria { format!("jazyk test {}", rid) } else { String::new() },
                cwd: ".".into(),
            };
            // The artifact carries the live statement hash in the test name, so the
            // requirement baseline is honest here.
            let hashes = crate::gen::RowHashes {
                requirement: hash_hex(&r.ears),
                test: hash_file(path),
                files: hash_files(gs, &files),
            };
            ledger.requirements.insert(
                rid.clone(),
                ReqRow {
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
    ledger.save(&store.out);
    json!({"refreshed": refreshed, "rebuilt": rebuilt, "prunedOrphans": orphans})
}

// Files under a root, skipping build and VCS directories, bounded to text-sized files.
fn collect_files(root: &std::path::Path, v: &mut Vec<(PathBuf, bool)>, is_criteria: bool) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | "node_modules" | ".git") && !name.starts_with("jazyk-out") {
                collect_files(&path, v, is_criteria);
            }
        } else if std::fs::metadata(&path).map(|m| m.len() <= 512 * 1024).unwrap_or(false) {
            v.push((path, is_criteria));
        }
    }
}

// UTC ISO 8601 from the system clock, no dependencies. Civil-from-days per Howard
// Hinnant's algorithm.
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
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
    use super::*;
    use crate::gen::{mark as gen_mark, test_name, GenSettings};
    use crate::model::*;

    fn fixture(out: &std::path::Path) -> (Store, GenSettings) {
        let mut s = Store { out: out.to_path_buf(), ..Default::default() };
        s.graph.entities.insert("ent:cart".into(), Entity { name: "Cart".into(), ..Default::default() });
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                ears: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: SourceRef { doc: "shop.md".into(), section: "/shop".into(), quote: "holds".into() },
                confidence: None,
                reasoning: None,
                created: None,
                updated: None,
            },
        );
        let gs = GenSettings { deliverable: out.join("product"), worker: "agentic".into() };
        std::fs::create_dir_all(gs.deliverable.join("src")).unwrap();
        std::fs::create_dir_all(gs.deliverable.join("tests")).unwrap();
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        std::fs::write(gs.deliverable.join("src/cart.rs"), "// product").unwrap();
        std::fs::write(
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

    // A command that could not run has judged nothing: the row stays unverified with
    // reason runner-failed, and a real verdict later clears it.
    // Mirrors docs/consumers/gen.md#a-test-that-could-not-run-says-nothing.
    #[test]
    fn a_runner_failure_is_not_a_failing_requirement() {
        let out = std::env::temp_dir().join(format!("jazyk-verify-runner-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);

        record_runner_failure(&s, "req:shop-1", 127, "sh: pytest: command not found");
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.verdict, "none");
        assert_eq!(row.exit_code, Some(127));
        let (status, reason) = status_of(&s, "req:shop-1", row, &gs);
        assert_eq!(status, "unverified");
        assert_eq!(reason, "runner-failed");

        // A verdict from a working runner clears the mark.
        mark(&s, "req:shop-1", "fail", None, Some("assert failed"), &gs).unwrap();
        let ledger = Ledger::load(&out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.exit_code, None);
        assert_eq!(status_of(&s, "req:shop-1", row, &gs).0, "failing");
        std::fs::remove_dir_all(&out).ok();
    }

    #[test]
    fn status_matrix_and_cascade() {
        let out = std::env::temp_dir().join(format!("jazyk-verify-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);

        // Fresh row: unverified, never-run.
        let p = pending(&s, &gs, None, None);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["status"], "unverified");
        assert_eq!(p[0]["reason"], "never-run");

        // A pass verdict rebaselines and verifies.
        mark(&s, "req:shop-1", "pass", None, Some("ok"), &gs).unwrap();
        assert!(pending(&s, &gs, None, None).is_empty());

        // Hand-editing an implementing file flips stale-code.
        std::fs::write(gs.deliverable.join("src/cart.rs"), "// edited product").unwrap();
        let p = pending(&s, &gs, None, None);
        assert_eq!(p[0]["status"], "stale-code");
        mark(&s, "req:shop-1", "pass", None, None, &gs).unwrap();

        // Editing the test artifact flips stale-test.
        let name = test_name("req:shop-1", "The Cart shall hold items.");
        std::fs::write(gs.deliverable.join("tests/cart.rs"), format!("// changed\nfn {}() {{}}\n", name)).unwrap();
        assert_eq!(pending(&s, &gs, None, None)[0]["status"], "stale-test");
        mark(&s, "req:shop-1", "pass", None, None, &gs).unwrap();

        // Rewording the requirement flips stale-requirement; task refuses; test refuses.
        s.graph.requirements.get_mut("req:shop-1").unwrap().ears = "The Cart shall hold many items.".into();
        let p = pending(&s, &gs, None, None);
        assert_eq!(p[0]["status"], "stale-requirement");
        assert!(task(&s, "req:shop-1", &gs).unwrap_err().contains("stale-requirement"));

        // A fail verdict surfaces as failing.
        s.graph.requirements.get_mut("req:shop-1").unwrap().ears = "The Cart shall hold items.".into();
        mark(&s, "req:shop-1", "fail", None, Some("boom"), &gs).unwrap();
        assert_eq!(pending(&s, &gs, None, None)[0]["status"], "failing");

        // Programmatic run: `true` exits 0.
        let pass = run_programmatic(&s, "req:shop-1", &gs).unwrap().pass;
        assert!(pass);
    }

    #[test]
    fn audit_never_launders_stale_requirement() {
        let out = std::env::temp_dir().join(format!("jazyk-audit-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (mut s, gs) = fixture(&out);
        mark(&s, "req:shop-1", "pass", None, None, &gs).unwrap();
        // Reword the requirement: stale-requirement.
        s.graph.requirements.get_mut("req:shop-1").unwrap().ears = "The Cart shall hold many items.".into();
        assert_eq!(pending(&s, &gs, None, None)[0]["status"], "stale-requirement");
        // Audit must not launder it into verified: the artifact carries the OLD name.
        audit(&s, &gs);
        assert_eq!(pending(&s, &gs, None, None)[0]["status"], "stale-requirement");
    }

    #[test]
    fn missing_rows_surface_and_audit_rebuilds() {
        let out = std::env::temp_dir().join(format!("jazyk-rebuild-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let (s, gs) = fixture(&out);
        // Wipe the ledger: the row must surface as missing/not-generated.
        std::fs::remove_file(crate::gen::Ledger::path(&out)).unwrap();
        let p = pending(&s, &gs, None, None);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["status"], "missing");
        assert_eq!(p[0]["reason"], "not-generated");
        // Audit rebuilds the row from the artifact (tests/cart.rs carries the name).
        let r = audit(&s, &gs);
        assert_eq!(r["rebuilt"].as_array().unwrap().len(), 1);
        let p2 = pending(&s, &gs, None, None);
        assert_eq!(p2.len(), 1);
        assert_eq!(p2[0]["status"], "unverified");
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

// Row selection shared by the worker and `jazyk test --list`: pending rows (all rows
// when forced), narrowed by targets (entity ids select their rows, requirement ids
// select directly) and by test kind.
pub fn select_rows(
    store: &Store,
    gs: &GenSettings,
    targets: &[String],
    kind: Option<&str>,
    force: bool,
) -> Vec<Value> {
    let filter = if force { "all" } else { "stale" };
    pending(store, gs, Some(filter), None)
        .into_iter()
        .filter(|r| {
            if targets.is_empty() {
                return true;
            }
            targets.iter().any(|t| {
                let t = store.resolve_id(t);
                r["requirement"].as_str() == Some(t) || store.resolve_id(r["entity"].as_str().unwrap_or("")) == t
            })
        })
        .filter(|r| match kind {
            Some(k) => r["test"]["kind"].as_str() == Some(k),
            None => true,
        })
        .collect()
}

// Run verification over the selected rows and record verdicts. Programmatic rows
// execute their command; llm rows drive the configured model against the verify_task
// package. Returns {rows, verified, failing, stale, skipped}.
// Mirrors docs/consumers/gen.md#runners.
pub fn run_all(
    store: &Store,
    llm: &crate::llm::Llm,
    gs: &GenSettings,
    targets: &[String],
    kind: Option<&str>,
    force: bool,
    trace: &crate::turn::Trace,
) -> Result<Value, String> {
    use crate::turn::TraceEvent;
    // Every prompt this run sends reports under the requirement it is judging.
    let llm = &llm.with_trace(trace);
    let selected = select_rows(store, gs, targets, kind, force);
    if !targets.is_empty() && selected.is_empty() {
        return Err("no ledger rows match the given target(s); run `jazyk gen` first or check the ids".into());
    }
    // The build runs once, before any row. A row cannot say anything true about an
    // artifact that was never produced, so a broken build stops the run instead of
    // failing every requirement and naming the wrong culprit.
    // Mirrors docs/consumers/gen.md#runners.
    crate::gen::run_build(&store.out, gs, trace, "verify").map_err(|e| format!("{}; nothing was verified", e))?;
    let (mut verified, mut failing, mut stale, mut skipped) = (0u64, 0u64, 0u64, 0u64);
    // Failed programmatic runs, judged together at the end of the run.
    let mut held: Vec<(String, ProgRun, String)> = Vec::new();
    for r in &selected {
        if trace.is_cancelled() {
            break;
        }
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
            continue;
        }
        let row_kind = r["test"]["kind"].as_str().unwrap_or("programmatic");
        trace.event(TraceEvent::VerifyRowStart { requirement: rid.clone(), test: row_kind.to_string() });
        let verdict = if row_kind == "llm" {
            match task(store, &rid, gs) {
                Ok(task_pkg) => {
                    let mut evidence_input = String::new();
                    for f in task_pkg["files"].as_array().cloned().unwrap_or_default() {
                        if let Some(f) = f.as_str() {
                            if let Ok(content) = std::fs::read_to_string(gs.deliverable.join(f)) {
                                evidence_input
                                    .push_str(&format!("\n=== {} ===\n{}", f, crate::llm::truncate(&content, 12_000)));
                            }
                        }
                    }
                    let user = format!(
                        "{}\n\nContext:\n{}\n\nImplementing files:{}\n\nReply with a verdict line `PASS` or `FAIL`, then your reasoning.",
                        task_pkg["criteria"].as_str().unwrap_or_default(),
                        task_pkg["context"].as_str().unwrap_or_default(),
                        evidence_input
                    );
                    match llm.chat(task_pkg["instructions"].as_str().unwrap_or_default(), &user, &format!("verify {}", rid), "judge") {
                        Ok(reply) => {
                            let up = reply.to_uppercase();
                            match (up.find("PASS"), up.find("FAIL")) {
                                (Some(p), Some(f)) => Some((p < f, crate::llm::truncate(reply.trim(), 300).to_string())),
                                (Some(_), None) => Some((true, crate::llm::truncate(reply.trim(), 300).to_string())),
                                (None, Some(_)) => Some((false, crate::llm::truncate(reply.trim(), 300).to_string())),
                                (None, None) => {
                                    trace.event(TraceEvent::VerifyRowError {
                                        requirement: rid.clone(),
                                        message: " verdict unparseable (no PASS or FAIL in reply)".into(),
                                    });
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            trace.event(TraceEvent::VerifyRowError {
                                requirement: rid.clone(),
                                message: format!(" llm run failed: {}", e),
                            });
                            None
                        }
                    }
                }
                Err(e) => {
                    trace.event(TraceEvent::VerifyRowError { requirement: rid.clone(), message: format!(": {}", e) });
                    None
                }
            }
        } else {
            match run_programmatic(store, &rid, gs) {
                Ok(run) if run.pass => Some((true, run.evidence)),
                // A failure waits: whether it is the deliverable's or the machine's is
                // decided once, when the run knows how the other rows went
                // (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
                Ok(run) => {
                    held.push((rid.clone(), run, r["test"]["run"].as_str().unwrap_or("").to_string()));
                    None
                }
                Err(e) => {
                    trace.event(TraceEvent::VerifyRowError { requirement: rid.clone(), message: format!(": {}", e) });
                    None
                }
            }
        };
        match verdict {
            Some((pass, evidence)) => {
                let v = if pass { "pass" } else { "fail" };
                mark(store, &rid, v, None, Some(&evidence), gs).ok();
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
    // One broken runner says the same thing to every row it touched; unmet requirements
    // do not agree with each other. When nothing passed and every failure carries the
    // identical output, the machine is what failed, and no row learned anything.
    let identical = held.len() > 1
        && verified == 0
        && held.windows(2).all(|w| w[0].1.tail == w[1].1.tail);
    let mut runner_failed = 0u64;
    skipped = skipped.saturating_sub(held.len() as u64);
    for (rid, run, cmd) in held {
        if run.runner_failed() || identical {
            record_runner_failure(store, &rid, run.code, &run.evidence);
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
        mark(store, &rid, "fail", None, Some(&run.evidence), gs).ok();
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
    Ok(json!({
        "rows": selected.len(),
        "verified": verified,
        "failing": failing,
        "runnerFailed": runner_failed,
        "stale": stale,
        "skipped": skipped,
    }))
}
