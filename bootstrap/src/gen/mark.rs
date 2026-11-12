use super::*;

// One anchored implementing site: the file, the 1-based line in the stripped file,
// and the verbatim next significant line the marker sat above. Language-agnostic:
// relocation is a whitespace-insensitive line match, never a parse of the medium.
#[derive(Serialize, Deserialize, Clone)]
pub struct Site {
    pub file: String,
    pub line: usize,
    pub head: String,
}

// A site freshly stripped from a file, before the requirement id is resolved.
pub struct RawSite {
    pub rid: String,
    pub line: usize,
    pub head: String,
}

// Strip single-line markers (`req:<slug> hash:<hex>` with nothing but comment leaders
// around it) and return the clean text plus the sites they anchored. A marker line
// carrying other alphanumeric content is left alone: the harness never mangles code.
// Mirrors docs/consumers/gen.md#traceability.
pub fn strip_markers(text: &str) -> (String, Vec<RawSite>) {
    strip_markers_in(text, None)
}

fn strip_markers_in(text: &str, set_id: Option<&str>) -> (String, Vec<RawSite>) {
    // `(?:req:)+` folds a doubled prefix (`req:req:orders-1`, a model pasting the
    // full id after a template's literal `req:`) into one clean site.
    let re = regex::Regex::new(
        r"(?:req:)+((?:ds_[A-Za-z0-9_-]+::)?[A-Za-z0-9][A-Za-z0-9_-]*)\s+hash:([0-9a-fA-F]{4,64})",
    )
    .unwrap();
    let mut clean: Vec<&str> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut sites: Vec<RawSite> = Vec::new();
    for line in text.lines() {
        if let Some(c) = re.captures(line) {
            let rest = line.replacen(c.get(0).unwrap().as_str(), "", 1);
            let rid = format!("req:{}", &c[1]);
            let (scope, local) = crate::webapi::sets::unqualify(&rid);
            let matches_set = set_id.is_none() || scope.is_none() || scope.as_deref() == set_id;
            if matches_set && !rest.chars().any(|ch| ch.is_alphanumeric()) {
                pending.push(if set_id.is_some() { local } else { rid });
                continue;
            }
        }
        clean.push(line);
        if !pending.is_empty() && !line.trim().is_empty() {
            let lineno = clean.len();
            for rid in pending.drain(..) {
                sites.push(RawSite {
                    rid,
                    line: lineno,
                    head: line.to_string(),
                });
            }
        }
    }
    let mut out = clean.join("\n");
    if text.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    (out, sites)
}

// Locate a site's head in the current text: whitespace-insensitive line match, the
// occurrence nearest the recorded line wins. Returns (1-based line, still-at-hint).
// No match means the site is lost; the caller shows it as such, never guesses.
pub fn locate_head(text: &str, head: &str, hint: usize) -> Option<(usize, bool)> {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let h = norm(head);
    if h.is_empty() {
        return None;
    }
    let mut best: Option<usize> = None;
    for (i, line) in text.lines().enumerate() {
        if norm(line) == h {
            let ln = i + 1;
            let closer = best
                .map(|b| (ln as i64 - hint as i64).abs() < (b as i64 - hint as i64).abs())
                .unwrap_or(true);
            if closer {
                best = Some(ln);
            }
        }
    }
    best.map(|ln| (ln, ln == hint))
}

// Record a task done. The manifest binds the worker's files to the graph and seeds the
// verification rows. Mirrors docs/compiler/tools.md#generation-tools (gen_mark).
// Empty means absent (docs/compiler/tools.md#validation-and-errors), mirrored here
// for manifest rows: a row whose fields are all empty is a filled-in blank from a
// schema-filling model, not data.
fn hollow(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.iter().all(hollow),
        Value::Object(o) => o.values().all(hollow),
        _ => false,
    }
}

pub fn mark(
    store: &Store,
    id: &str,
    fact_hash_seen: Option<&str>,
    manifest: &Value,
    gs: &GenSettings,
) -> Result<Value, String> {
    if !store.graph.entities.contains_key(id) {
        return Err(format!("unknown entity `{}`", id));
    }
    let rids = reqs_of_sorted(store, id);
    let referenced_assets = assets::referenced_assets(store, id, &rids);
    assets::ensure_available(&referenced_assets)?;
    let base_hash = fact_hash_base(store, id, &rids);
    let current_hash = assets::fingerprint(&base_hash, &referenced_assets);
    if current_hash != base_hash && fact_hash_seen.is_some_and(|seen| seen != current_hash) {
        return Err(include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/gen-mark/the-facts-or-referenced-input-bytes-moved-606d3.md"
        ))
        .into());
    }
    let slug = crate::ids::entity_slug(id).to_string();
    // Every path the manifest names is confined before any side effect: a path that
    // escapes the deliverable is rejected naming it, and file lists are sets
    // (docs/consumers/gen.md#file-ownership-and-conventions).
    let str_list = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        x.as_str()
                            .map(String::from)
                            .or_else(|| x["path"].as_str().map(String::from))
                            .filter(|s| !s.trim().is_empty())
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    // A path that restates the deliverable directory's own name is moved under the
    // root, and the reply says so (docs/consumers/gen.md#file-ownership-and-conventions).
    let mut path_notes: Vec<String> = Vec::new();
    let files: Vec<String> = confine_list_in(gs, &str_list(&manifest["files"]), &mut path_notes)
        .map_err(|e| format!("files: {}", e))?;
    // Support files are the deliverable's, recorded once and rewritable by any later
    // task (docs/consumers/gen.md#file-ownership-and-conventions).
    let support: Vec<String> =
        confine_list_in(gs, &str_list(&manifest["supportFiles"]), &mut path_notes)
            .map_err(|e| format!("supportFiles: {}", e))?;
    let build_produces: Vec<String> = confine_list_in(
        gs,
        &str_list(&manifest["build"]["produces"]),
        &mut path_notes,
    )
    .map_err(|e| format!("build.produces: {}", e))?;
    let build_cwd = confine_cwd_in(
        gs,
        manifest["build"]["cwd"].as_str().unwrap_or("."),
        &mut path_notes,
    )
    .map_err(|e| format!("build.cwd: {}", e))?;

    // Validate the invented choices before any side effect: the scope grades the
    // severity, and the tool layer stages one invented-choice diagnostic per entry
    // from this validated set. Mirrors docs/consumers/gen.md#invented-choices.
    let choices = parse_choices(store, manifest)?;

    // Validate the manifest's test rows before any side effect: a rejection must
    // leave the deliverable untouched, or the retry sees files already stripped. A
    // row's artifact must exist, and a programmatic artifact must carry the declared
    // name: the same shape gate record_binding applies.
    // Mirrors docs/compiler/goals/generate.md#gate.
    let mut rows: Vec<(String, Option<String>, TestRef, Vec<String>)> = Vec::new();
    if let Some(tests) = manifest["tests"].as_array() {
        for t in tests {
            // A row whose fields are all empty is a filled-in blank; drop it.
            if hollow(t) {
                continue;
            }
            let Some(rid) = t["requirement"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                let named = [&t["name"], &t["artifact"], &t["label"]]
                    .iter()
                    .find_map(|v| v.as_str().filter(|s| !s.trim().is_empty()))
                    .unwrap_or("?")
                    .to_string();
                return Err(format!(
                    include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/gen-mark/test-row-names-no-requirement-every-row-0a417.md"
                    )),
                    named
                ));
            };
            let rid = store.resolve_id(rid).to_string();
            let Some(r) = store.graph.requirements.get(&rid) else {
                return Err(format!("unknown requirement `{}` in manifest", rid));
            };
            let criterion = t["criterion"]
                .as_str()
                .filter(|id| !id.is_empty())
                .map(|id| store.resolve_id(id).to_string());
            if let Some(cid) = &criterion {
                if !store
                    .graph
                    .criteria
                    .get(cid)
                    .is_some_and(|criterion| criterion.requirement == rid)
                {
                    return Err(format!("criterion `{cid}` is not a criterion of `{rid}`"));
                }
            }
            if rows
                .iter()
                .any(|(previous, cid, _, _)| previous == &rid && cid == &criterion)
            {
                return Err(format!(
                    "duplicate test declaration for `{rid}`{}",
                    criterion
                        .as_deref()
                        .map(|cid| format!(" criterion `{cid}`"))
                        .unwrap_or_default()
                ));
            }
            let kind = t["kind"]
                .as_str()
                .unwrap_or("programmatic")
                .trim()
                .to_string();
            if kind != "programmatic" && kind != "llm" {
                return Err(format!(
                    "test row for {} has kind `{}`; a test is `programmatic` (a command runs it) or `llm` (a judgment with a criteria file)",
                    rid, kind
                ));
            }
            let artifact_raw = t["artifact"].as_str().unwrap_or("").trim().to_string();
            let run = t["run"].as_str().unwrap_or("").trim().to_string();
            if kind == "programmatic" {
                if artifact_raw.is_empty() {
                    return Err(format!(
                        include_str!(concat!(
                            env!("OUT_DIR"),
                            "/payloads/goals/prompts/inline/gen-mark/test-row-for-has-an-empty-artifact-77fd9.md"
                        )),
                        rid
                    ));
                }
                if run.is_empty() {
                    return Err(format!(
                        include_str!(concat!(
                            env!("OUT_DIR"),
                            "/payloads/goals/prompts/inline/gen-mark/test-row-for-has-an-empty-run-040fc.md"
                        )),
                        rid
                    ));
                }
            } else if artifact_raw.is_empty() {
                return Err(format!(
                    include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/gen-mark/test-row-for-is-llm-but-names-1477d.md"
                    )),
                    rid
                ));
            }
            let artifact = confine_in(gs, &artifact_raw, &mut path_notes)
                .map_err(|e| format!("test row for {}: artifact {}", rid, e))?;
            let test = TestRef {
                kind: kind.clone(),
                label: t["label"].as_str().unwrap_or("test").to_string(),
                artifact,
                name: t["name"]
                    .as_str()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .unwrap_or(&test_name(&rid, &r.statement))
                    .to_string(),
                run,
                cwd: confine_cwd_in(gs, t["cwd"].as_str().unwrap_or("."), &mut path_notes)
                    .map_err(|e| format!("test row for {}: cwd {}", rid, e))?,
            };
            let path = artifact_path(&store.out, gs, &test);
            if !path.vis_file() {
                return Err(format!(
                    include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/gen-mark/test-row-for-names-artifact-but-no-e04ae.md"
                    )),
                    rid,
                    test.artifact,
                    if kind == "llm" {
                        " or the out directory's gen/"
                    } else {
                        ""
                    }
                ));
            }
            if kind == "programmatic" && !crate::verify::artifact_names(&path, &test.name) {
                return Err(format!(
                    include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/gen-mark/test-row-for-declares-test-but-does-b9555.md"
                    )),
                    rid, test.name, test.artifact
                ));
            }
            if kind == "llm" {
                validate_criteria_file(store, &rid, criterion.as_deref(), &path)?;
            }
            // The row's own `files`; an empty list takes the default below.
            let row_files: Vec<String> =
                confine_list_in(gs, &str_list(&t["files"]), &mut path_notes)
                    .map_err(|e| format!("test row for {}: files {}", rid, e))?;
            rows.push((rid, criterion, test, row_files));
        }
    }
    // Ownership and realization, both before any side effect.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    let _lock = Ledger::lock(&store.out);
    let pre = Ledger::load(&store.out);
    let record_sequence = pre
        .entities
        .get(&slug)
        .map(|row| row.record_sequence)
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/gen-mark/the-entity-generation-record-sequence-is-exhausted-d1a37.md"
        )))?;
    for (rid, criterion, test, _) in &rows {
        let Some(cid) = criterion else {
            if let Some(row) = pre.requirements.get(rid) {
                let path = artifact_path(&store.out, gs, &row.test);
                if row.hashes.requirement == hash_hex(&store.graph.requirements[rid].statement)
                    && row.hashes.test == hash_file(&path)
                    && path.vis_file()
                    && ((row.test.kind == "llm" && path.starts_with(store.out.join("gen")))
                        || gs.allows_source(&row.test.artifact))
                    && !same_test_artifact(&row.test, test)
                {
                    return Err(format!(
                        include_str!(concat!(
                            env!("OUT_DIR"),
                            "/payloads/goals/prompts/inline/gen-mark/requirement-rid-already-has-a-current-bound-e5148.md"
                        )),
                        rid = rid
                    ));
                }
            }
            continue;
        };
        let Some(prior) = pre.requirements.get(rid).and_then(|row| row.tests.get(cid)) else {
            continue;
        };
        let path = artifact_path(&store.out, gs, &prior.test);
        let current = prior.criterion_hash == hash_hex(&store.graph.criteria[cid].statement)
            && prior.test_hash == hash_file(&path)
            && path.vis_file()
            && ((prior.test.kind == "llm" && path.starts_with(store.out.join("gen")))
                || gs.allows_source(&prior.test.artifact))
            && (prior.test.kind != "programmatic"
                || crate::verify::artifact_names(&path, &prior.test.name));
        if current && !same_test_artifact(&prior.test, test) {
            return Err(format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/gen-mark/criterion-cid-already-has-a-current-bound-8057f.md"
                )),
                cid = cid
            ));
        }
    }
    let mut artifacts = test_artifacts(&pre);
    for (_, _, t, _) in &rows {
        artifacts.insert(t.artifact.clone());
    }
    let outputs = build_outputs(&pre);
    // Implementation: a file that exists and is no test artifact, criteria file, or
    // build output.
    let implementing = |f: &String| {
        !artifacts.contains(f)
            && !outputs.contains(f)
            && !build_produces.contains(f)
            && gs.allows_source(f)
            && gs.deliverable.join(f).vis_file()
    };
    // A file an entity outside this entity's component group records is theirs. Test
    // artifacts are the rows' and support files the deliverable's: neither conflicts.
    for f in &files {
        if artifacts.contains(f) || support.contains(f) || pre.support.contains(f) {
            continue;
        }
        if let Some(owner) = file_owner(store, &pre, id, f) {
            return Err(format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/gen-mark/files-belongs-to-entity-outside-this-entity-074e5.md"
                )),
                f, owner, f
            ));
        }
    }
    // A row naming no files of its own takes the entity's implementing files, and
    // keeps the files other entities record for it: a shared requirement stays
    // carried where it is (docs/consumers/gen.md#file-ownership-and-conventions).
    let own_impl: Vec<String> = files.iter().filter(|f| implementing(f)).cloned().collect();
    let rows: Vec<(String, Option<String>, TestRef, Vec<String>)> = rows
        .into_iter()
        .map(|(rid, criterion, test, own)| {
            if !own.is_empty() {
                return (rid, criterion, test, own);
            }
            let mut v = own_impl.clone();
            if let Some(prior) = pre.requirements.get(&rid) {
                for f in &prior.files {
                    let carried = pre
                        .entities
                        .iter()
                        .any(|(s, e)| s != &slug && e.files.contains(f));
                    if carried && implementing(f) && !v.contains(f) {
                        v.push(f.clone());
                    }
                }
            }
            (rid, criterion, test, v)
        })
        .collect();
    // A record realizes the entity's requirements in files: with no implementing file
    // of its own, every requirement needs a row naming one, this manifest's or the
    // ledger's (an entity carried by other entities' files).
    if own_impl.is_empty() {
        let uncarried: Vec<String> = reqs_of_sorted(store, id)
            .into_iter()
            .filter(|rid| {
                let named = match rows.iter().find(|(r, _, _, _)| r == rid) {
                    Some((_, _, _, fs)) => fs.iter().any(&implementing),
                    None => pre
                        .requirements
                        .get(rid)
                        .is_some_and(|row| row.files.iter().any(&implementing)),
                };
                !named
            })
            .collect();
        if !uncarried.is_empty() {
            return Err(format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/gen-mark/files-names-no-file-that-realizes-s-b0a3c.md"
                )),
                id,
                uncarried.join(", ")
            ));
        }
    }
    // Under a built medium the build is the only thing that makes an artifact exist,
    // so a manifest without one is rejected, not quietly accepted
    // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
    // Every rejection runs before the strip below rewrites files: a rejected record
    // leaves the markers in place for the retry.
    let manifest_build = manifest["build"]["run"]
        .as_str()
        .is_some_and(|r| !r.trim().is_empty());
    if pre.build.is_none() && !manifest_build {
        if let Some(m) = pre.medium.as_ref().filter(|m| m.is_built()) {
            return Err(format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/gen-mark/this-deliverable-is-and-no-build-is-9b9d9.md"
                )),
                m.form, m.artifact, m.artifact
            ));
        }
    }
    // Strip marker lines from the manifest files and collect the sites they anchor.
    // The marker is a wire format: the worker localizes while writing, the harness
    // records and cleans. Runs before hashing so every hash sees the stripped bytes.
    // Mirrors docs/consumers/gen.md#traceability.
    let mut sites_by_rid: BTreeMap<String, Vec<Site>> = BTreeMap::new();
    // A marker-like line the strip cannot parse (trailing text after the hash, a
    // mangled id) stays in the file and anchors nothing; name each one, so the row
    // never reads green over a broken wire format.
    // Mirrors docs/consumers/gen.md#traceability.
    let malformed_re = regex::Regex::new(
        r"req:(?:ds_[A-Za-z0-9_-]+::)?[A-Za-z0-9][A-Za-z0-9_-]*\s+hash:[0-9a-fA-F]{4,}",
    )
    .unwrap();
    let mut marker_warnings: Vec<String> = Vec::new();
    for f in &files {
        let path = gs.deliverable.join(f);
        let Ok(text) = crate::vfs::read_to_string(&path) else {
            continue;
        };
        let (clean, raw) = strip_markers_in(&text, store.out.file_name().and_then(|s| s.to_str()));
        for (i, line) in clean.lines().enumerate() {
            if marker_warnings.len() >= 20 {
                break;
            }
            if malformed_re.is_match(line) {
                marker_warnings.push(format!("{}:{}", f, i + 1));
            }
        }
        if raw.is_empty() {
            continue;
        }
        // Through a sibling temporary file renamed into place: a crash mid-strip
        // never leaves a truncated source file.
        crate::store::write_atomic(&path, clean.as_bytes()).map_err(|e| e.to_string())?;
        for s in raw {
            let rid = store.resolve_id(&s.rid).to_string();
            sites_by_rid.entry(rid).or_default().push(Site {
                file: f.clone(),
                line: s.line,
                head: s.head,
            });
        }
    }
    let mut ledger = Ledger::load(&store.out);
    // Deletion prunes the ledger: a requirement gone from the graph has no obligation
    // left, and no manifest can name it. Any record buries the dead rows, whatever
    // entity it marks. Mirrors docs/consumers/gen.md#deletion-prunes-the-ledger.
    let before = ledger.requirements.len();
    ledger
        .requirements
        .retain(|rid, _| store.graph.requirements.contains_key(store.resolve_id(rid)));
    let pruned = before - ledger.requirements.len();
    ledger
        .contradicted
        .retain(|rid, _| store.graph.requirements.contains_key(store.resolve_id(rid)));
    // One build per deliverable: the first task that needs one establishes it, later
    // tasks receive it in their package and reuse it.
    // Mirrors docs/consumers/gen.md#the-build.
    // A built medium without a build was rejected before the strip.
    if ledger.build.is_none() {
        if let Some(run) = manifest["build"]["run"]
            .as_str()
            .map(str::trim)
            .filter(|r| !r.is_empty())
        {
            let mut produces = build_produces.clone();
            // A built medium names its artifact; a build that forgets to list it
            // would pass its own check while producing nothing.
            if let Some(m) = ledger.medium.as_ref().filter(|m| m.is_built()) {
                if !produces
                    .iter()
                    .any(|p| p.trim_start_matches("./") == m.artifact)
                {
                    produces.push(m.artifact.clone());
                }
            }
            ledger.build = Some(Build {
                run: run.to_string(),
                cwd: build_cwd.clone(),
                produces,
                last_run: None,
            });
        }
    }
    for path in &support {
        if !ledger.support.iter().any(|p| p == path) {
            ledger.support.push(path.clone());
        }
    }
    // The previous record's file set, before it is overwritten: what this manifest
    // omits is removed below (docs/consumers/gen.md#incremental-regeneration).
    let prev_files: Vec<String> = ledger
        .entities
        .get(&slug)
        .map(|e| e.files.clone())
        .unwrap_or_default();
    ledger.entities.insert(
        slug.clone(),
        EntityGen {
            fact_hash: fact_hash_seen
                .map(String::from)
                .unwrap_or_else(|| fact_hash(store, id)),
            requirements: reqs_of_sorted(store, id),
            files: files.clone(),
            tasks: str_list(&manifest["tasks"]),
            unattached: None,
            recorded_at: Some(crate::verify::now_iso()),
            record_sequence,
        },
    );
    let mut grouped: BTreeMap<String, Vec<(Option<String>, TestRef, Vec<String>)>> =
        BTreeMap::new();
    for (rid, criterion, test, row_files) in rows {
        grouped
            .entry(rid)
            .or_default()
            .push((criterion, test, row_files));
    }
    let mut seeded = 0;
    // Rows landing over an open error diagnostic: named in the reply and kept in
    // the ledger, so generation never runs green over a contradiction silently.
    // Mirrors docs/consumers/gen.md#rows-recorded-over-an-open-contradiction.
    let mut contradicted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    {
        for (rid, declarations) in grouped {
            let r = &store.graph.requirements[&rid];
            let previous = ledger.requirements.get(&rid).cloned();
            let mut tests = previous
                .as_ref()
                .map(|row| row.tests.clone())
                .unwrap_or_default();
            tests.retain(|cid, _| {
                store
                    .graph
                    .criteria
                    .get(cid)
                    .is_some_and(|criterion| criterion.requirement == rid)
            });
            let mut headline = None;
            let mut row_files = Vec::new();
            for (criterion, test, files) in declarations {
                for file in files {
                    if !row_files.contains(&file) {
                        row_files.push(file);
                    }
                }
                if let Some(cid) = criterion {
                    let criterion_hash = hash_hex(&store.graph.criteria[&cid].statement);
                    let test_hash = hash_file(&artifact_path(&store.out, gs, &test));
                    let existing = tests.get(&cid).filter(|old| {
                        old.criterion_hash == criterion_hash
                            && old.test_hash == test_hash
                            && same_test_ref(&old.test, &test)
                    });
                    let value = existing.cloned().unwrap_or_else(|| CritTest {
                        test,
                        criterion_hash,
                        test_hash,
                        verdict: "none".into(),
                        last_run: None,
                        exit_code: None,
                        evidence: None,
                    });
                    tests.insert(cid, value);
                } else {
                    headline = Some(test);
                }
            }
            let test = headline
                .or_else(|| tests.values().next().map(|test| test.test.clone()))
                .expect("each grouped row contains a test declaration");
            let hashes = RowHashes {
                requirement: hash_hex(&r.statement),
                test: hash_file(&artifact_path(&store.out, gs, &test)),
                files: hash_files(gs, &implementing_files(&ledger, &row_files)),
            };
            let owner = r
                .entities
                .first()
                .map(|e| store.resolve_id(e).to_string())
                .unwrap_or_else(|| id.to_string());
            // A re-record that changes nothing keeps its verdict: the hashes say the
            // requirement, the test, and the files are the very state the last run
            // judged, and discarding that judgment would punish an idempotent call.
            let prior = previous
                .as_ref()
                .filter(|row| row.hashes == hashes && same_test_ref(&row.test, &test));
            let (verdict, last_run, exit_code, evidence) = match prior {
                Some(row) => (
                    row.verdict.clone(),
                    row.last_run.clone(),
                    row.exit_code,
                    row.evidence.clone(),
                ),
                None => ("none".into(), None, None, None),
            };
            // A declaration cannot carry an old verdict onto different source or
            // requirement bytes. The next execution or judgment supplies evidence.
            if previous.as_ref().is_some_and(|row| {
                row.hashes.files != hashes.files || row.hashes.requirement != hashes.requirement
            }) {
                for test in tests.values_mut() {
                    test.verdict = "none".into();
                    test.last_run = None;
                    test.exit_code = None;
                    test.evidence = None;
                }
            }
            ledger.requirements.insert(
                rid.clone(),
                ReqRow {
                    tests,
                    entity: owner,
                    files: row_files,
                    // An unchanged support promotion has no markers left to strip.
                    sites: sites_by_rid
                        .remove(&rid)
                        .unwrap_or_else(|| prior.map(|row| row.sites.clone()).unwrap_or_default()),
                    test,
                    hashes,
                    verdict,
                    last_run,
                    exit_code,
                    evidence,
                },
            );
            let open = ledger.record_contradicted(store, &rid);
            if !open.is_empty() {
                contradicted.insert(rid.clone(), open);
            }
            seeded += 1;
        }
    }
    // A regeneration replaces the file set: files the previous record listed that
    // this manifest omits are removed, snapshotted first, unless something else
    // still claims them (another entity's record, the support list, a row's test
    // artifact, the build's output). Whichever worker records, the deliverable
    // never keeps a predecessor under an old name
    // (docs/consumers/gen.md#incremental-regeneration).
    let mut removed: Vec<String> = Vec::new();
    let mut baselined: std::collections::HashSet<String> = Default::default();
    for f in &prev_files {
        let claimed = files.contains(f)
            || ledger.support.contains(f)
            || ledger
                .entities
                .iter()
                .any(|(s, e)| s != &slug && e.files.contains(f))
            || ledger.requirements.values().any(|r| &r.test.artifact == f)
            || ledger
                .build
                .as_ref()
                .map(|b| b.produces.contains(f))
                .unwrap_or(false);
        if claimed || !gs.allows_source(f) || !gs.deliverable.join(f).vis_file() {
            continue;
        }
        snapshot_baseline(&store.out, gs, f, &mut baselined);
        if crate::vfs::remove_file(gs.deliverable.join(f)).is_ok() {
            removed.push(f.clone());
        }
    }
    // The deliverable itself measures how much was invented: owned mass no
    // requirement claims, recorded on the entity's entry so the grade and the
    // measure read together. Mirrors docs/consumers/gen.md#the-unattached-remainder.
    let unattached = measure_unattached(&ledger, gs, &slug);
    if let Some(e) = ledger.entities.get_mut(&slug) {
        e.unattached = Some(unattached.clone());
    }
    ledger.save(&store.out)?;
    let mut reply = json!({
        "marked": id, "files": files.len(), "tests": seeded,
        "unattached": {"files": unattached.files, "lines": unattached.lines, "ratio": unattached.ratio, "markerless": unattached.markerless},
        "unattachedNote": if unattached.markerless {
            "informational: no requirement site anchors any line of these files (the medium carries no traceability markers), so the ratio says nothing here; the generation is accepted (docs/consumers/gen.md#the-unattached-remainder)"
        } else {
            include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/gen-mark/informational-the-owned-files-no-requirement-row-4d214.md"))
        },
    });
    if !path_notes.is_empty() {
        reply["pathNote"] = json!(path_notes);
    }
    if !removed.is_empty() {
        reply["removed"] = json!(removed);
        reply["removedNote"] = json!(include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/gen-mark/files-the-previous-record-listed-and-this-b38de.md"
        )));
    }
    if !choices.is_empty() {
        reply["choices"] = json!(choices.len());
    }
    if pruned > 0 {
        reply["prunedRows"] = json!(pruned);
        reply["note"] = json!("pruned ledger row(s) whose requirement left the graph");
    }
    if !marker_warnings.is_empty() {
        reply["markerWarnings"] = json!(marker_warnings);
        reply["markerNote"] = json!(
            "marker-like lines the strip could not parse remain in these files and anchor no site; a marker is `req:<id> hash:<hash8>` alone on its line"
        );
    }
    if !contradicted.is_empty() {
        reply["contradicted"] = json!(contradicted);
        reply["contradictedNote"] = json!(
            "these rows were recorded while an open error diagnostic disputes their statement; the deliverable implements one side of an open question until it is resolved"
        );
    }
    // An entity is generated now, so the medium stands; the divergence is the record
    // (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
    if let Some(w) = medium_divergence(&ledger) {
        reply["mediumWarning"] = json!(w);
    }
    Ok(reply)
}

// The unattached remainder for one entity, over the files it owns: files no
// requirement row names, and significant lines outside every site's run. A site's
// run starts at its head line and ends before the next site in the same file, or at
// the end of the file, so the unattached lines of a file with sites are the ones
// before its first site; a file with none is unattached whole. Test artifacts are
// claimed by their rows and excluded, support files never enter an entity's list.
// Mirrors docs/consumers/gen.md#the-unattached-remainder.
pub fn measure_unattached(ledger: &Ledger, gs: &GenSettings, slug: &str) -> Unattached {
    let Some(e) = ledger.entities.get(slug) else {
        return Unattached::default();
    };
    let mut named: std::collections::BTreeSet<&str> = Default::default();
    let mut artifacts: std::collections::BTreeSet<&str> = Default::default();
    let mut first_site: BTreeMap<&str, usize> = BTreeMap::new();
    for row in ledger.requirements.values() {
        for f in &row.files {
            named.insert(f);
        }
        artifacts.insert(row.test.artifact.as_str());
        for s in &row.sites {
            let e = first_site.entry(s.file.as_str()).or_insert(s.line);
            *e = (*e).min(s.line);
        }
    }
    let files = e
        .files
        .iter()
        .filter(|f| !named.contains(f.as_str()) && !artifacts.contains(f.as_str()))
        .count() as u64;
    let (mut significant, mut lines) = (0u64, 0u64);
    for f in &e.files {
        if artifacts.contains(f.as_str()) {
            continue;
        }
        let Ok(text) = crate::vfs::read_to_string(gs.deliverable.join(f)) else {
            continue;
        };
        let covered_from = first_site.get(f.as_str()).copied();
        for (i, line) in text.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || is_comment_line(t) {
                continue;
            }
            significant += 1;
            if covered_from.map(|start| i + 1 < start).unwrap_or(true) {
                lines += 1;
            }
        }
    }
    let ratio = if significant == 0 {
        0.0
    } else {
        ((lines as f64 / significant as f64) * 100.0).round() / 100.0
    };
    let markerless =
        significant > 0 && e.files.iter().all(|f| !first_site.contains_key(f.as_str()));
    Unattached {
        files,
        lines,
        ratio,
        markerless,
    }
}

// A comment by leader alone: anchoring never parses the medium, and neither does
// this measure. Covers the common comment syntaxes; a shebang counts as a comment.
fn is_comment_line(trimmed: &str) -> bool {
    ["//", "#", "--", ";", "*", "/*", "*/", "<!--"]
        .iter()
        .any(|lead| trimmed.starts_with(lead))
}

// ---- invented choices ----
// Anything the deliverable needed that the documents do not state. The manifest
// carries the choices; the harness grades each by the scope of the invention and
// files one invented-choice diagnostic per entry.
// Mirrors docs/consumers/gen.md#invented-choices.

#[derive(Clone)]
pub struct Choice {
    pub choice: String,
    // product | behavior | detail.
    pub scope: String,
    pub reasoning: String,
    pub requirements: Vec<String>,
}

// The severity a scope grades: the invention of the product is an error, of an
// observable behavior a warning, of a cosmetic detail a suppressible info.
pub fn severity_for_scope(scope: &str) -> &'static str {
    match scope {
        "product" => "error",
        "behavior" => "warning",
        _ => "info",
    }
}

// Parse and validate the manifest's `choices`. Rejections name the fix; requirement
// references resolve through redirects and must exist.
pub fn parse_choices(store: &Store, manifest: &Value) -> Result<Vec<Choice>, String> {
    let Some(list) = manifest["choices"].as_array() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for c in list {
        // An all-empty entry is a filled-in blank, not an invented choice.
        if hollow(c) {
            continue;
        }
        let text = c["choice"]
            .as_str()
            .or_else(|| c["message"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty() {
            return Err(
                "a choices entry needs `choice`: the invented choice in one sentence".into(),
            );
        }
        let scope = c["scope"].as_str().unwrap_or("").trim().to_lowercase();
        if !matches!(scope.as_str(), "product" | "behavior" | "detail") {
            return Err(format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/gen-mark/choice-has-scope-it-must-be-product-ecff4.md"
                )),
                crate::llm::truncate(&text, 60),
                scope
            ));
        }
        let reasoning = c["reasoning"]
            .as_str()
            .or_else(|| c["reason"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let mut raw: Vec<String> = c["requirements"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        x.as_str()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let Some(r) = c["requirement"].as_str() {
            raw.push(r.to_string());
        }
        let mut requirements = Vec::new();
        for r in raw {
            let rid = store.resolve_id(&r).to_string();
            if !store.graph.requirements.contains_key(&rid) {
                return Err(format!("unknown requirement `{}` in choices", r));
            }
            if !requirements.contains(&rid) {
                requirements.push(rid);
            }
        }
        out.push(Choice {
            choice: text,
            scope,
            reasoning,
            requirements,
        });
    }
    Ok(out)
}

// The mutations the tool layer stages for one entity's invented set: one
// invented-choice diagnostic per new choice (severity by scope, subjects the entity
// plus the requirements the choice fills in, the message naming the unattached
// remainder), and a resolve for every open one the new record omits, so a
// regeneration under better documents clears its own debt while a repeated choice
// keeps its diagnostic and its triage. The prompt proposes the sentence for the
// governing section (the requirement's source, else the entity's first mention) with
// an answer option to keep the choice unstated.
// Mirrors docs/consumers/gen.md#invented-choices.
pub fn choice_ops(
    store: &Store,
    id: &str,
    choices: &[Choice],
    unattached: Option<&Unattached>,
) -> Vec<crate::store::Op> {
    use crate::model::{Diagnostic, DiagnosticPrompt, PromptOption, SuggestedEdit};
    let mut ops = Vec::new();
    let open: Vec<(String, String)> = store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| {
            d.lifecycle == "open"
                && d.rule == "invented-choice"
                && d.subjects.iter().any(|s| store.resolve_id(s) == id)
        })
        .map(|(did, d)| (did.clone(), d.message.clone()))
        .collect();
    for (did, message) in &open {
        if !choices.iter().any(|c| message.contains(&c.choice)) {
            ops.push(crate::store::Op::ResolveDiagnostic {
                id: did.clone(),
                reason: "re-recorded without this choice".into(),
            });
        }
    }
    let measure = unattached
        .map(|u| {
            format!(
                " Unattached remainder on the entity: {} file(s), {} line(s), ratio {:.2}.",
                u.files, u.lines, u.ratio
            )
        })
        .unwrap_or_default();
    for c in choices {
        if open.iter().any(|(_, m)| m.contains(&c.choice)) {
            continue;
        }
        let mut subjects = vec![id.to_string()];
        subjects.extend(c.requirements.iter().cloned());
        let anchor = c
            .requirements
            .iter()
            .find_map(|rid| {
                store
                    .graph
                    .requirements
                    .get(rid)
                    .and_then(|r| r.source.as_ref())
                    .map(|s| (s.doc.clone(), s.section.clone()))
            })
            .or_else(|| {
                store
                    .graph
                    .entities
                    .get(id)
                    .and_then(|e| e.mentions.first())
                    .map(|m| (m.doc.clone(), m.section.clone()))
            });
        let mut options = Vec::new();
        if let Some((doc, section)) = anchor {
            options.push(PromptOption {
                label: format!("Insert into {} {}", doc, section),
                edit: Some(SuggestedEdit {
                    doc,
                    section,
                    old_text: String::new(),
                    new_text: c.choice.clone(),
                    facts: Vec::new(),
                }),
                answer: None,
            });
        }
        options.push(PromptOption {
            label: "Keep the choice unstated".into(),
            edit: None,
            answer: Some("keep unstated".into()),
        });
        ops.push(crate::store::Op::ReportDiagnostic {
            id: String::new(),
            diagnostic: Diagnostic {
                goal: None,
                rule: "invented-choice".into(),
                severity: severity_for_scope(&c.scope).into(),
                subjects,
                message: format!("Invented ({}): {}{}", c.scope, c.choice, measure),
                reasoning: (!c.reasoning.is_empty()).then(|| c.reasoning.clone()),
                lifecycle: "open".into(),
                triage: None,
                prompt: Some(DiagnosticPrompt {
                    question: "Should the documents state this choice?".into(),
                    options,
                    freeform: true,
                }),
                answer: None,
                created: None,
                updated: None,
            },
        });
    }
    ops
}
