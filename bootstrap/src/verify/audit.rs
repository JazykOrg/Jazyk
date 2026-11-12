use super::*;

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
        reply["verdictResetNote"] = json!(include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/verify-audit/these-rows-test-or-files-bytes-moved-5e56d.md"
        )));
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
pub(super) fn collect_files(
    root: &std::path::Path,
    v: &mut Vec<(PathBuf, bool)>,
    is_criteria: bool,
) {
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
