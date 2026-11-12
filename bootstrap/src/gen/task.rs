use super::*;

pub fn req_slug(id: &str) -> String {
    id.strip_prefix("req:").unwrap_or(id).to_string()
}

// The suggested test name: requirement id + hash prefix, sanitized. A reworded
// requirement mechanically breaks the recorded run filter.
pub fn test_name(rid: &str, statement: &str) -> String {
    let sanitized: String = req_slug(rid)
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("req_{}_{}", sanitized, &hash_hex(statement)[..8])
}

// The suggested test name of a criterion: `crit_<slug>_<hash8>`, the hash of the
// criterion statement (docs/consumers/gen.md#tests-tie-criteria-to-the-deliverable).
pub fn crit_test_name(cid: &str, statement: &str) -> String {
    let sanitized: String = cid
        .strip_prefix("crit:")
        .unwrap_or(cid)
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("crit_{}_{}", sanitized, &hash_hex(statement)[..8])
}

pub fn reqs_of_sorted(store: &Store, id: &str) -> Vec<String> {
    let mut v = store.requirements_referencing(id);
    v.sort();
    v
}

// What `reqs_of_sorted` answers for every entity, built in one pass over the
// requirements: the requirement ids naming the entity (redirects followed), sorted.
// A scan over the entities that asked per entity read the five thousand requirements
// of a large store two thousand times.
pub struct ReqsByEntity(BTreeMap<String, Vec<String>>);

impl ReqsByEntity {
    pub fn build(store: &Store) -> ReqsByEntity {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (rid, r) in &store.graph.requirements {
            let mut named: Vec<&str> = Vec::new();
            for e in &r.entities {
                let id = store.resolve_id(e);
                if !named.contains(&id) {
                    named.push(id);
                    map.entry(id.to_string()).or_default().push(rid.clone());
                }
            }
        }
        for v in map.values_mut() {
            v.sort();
        }
        ReqsByEntity(map)
    }

    pub fn of(&self, id: &str) -> &[String] {
        self.0.get(id).map(Vec::as_slice).unwrap_or(&[])
    }
}

// The facts generation consumes, folded into one hash: name, definition,
// stereotype, attributes, and every referencing statement with its edges. Any of
// them changing flips the entity to generation work.
// Mirrors docs/consumers/gen.md#the-ledger.
pub fn fact_hash(store: &Store, id: &str) -> String {
    fact_hash_over(store, id, &reqs_of_sorted(store, id))
}

// `fact_hash` with the entity's sorted requirement ids already in hand.
pub fn fact_hash_over(store: &Store, id: &str, rids: &[String]) -> String {
    assets::fingerprint(
        &fact_hash_base(store, id, rids),
        &assets::referenced_assets(store, id, rids),
    )
}

pub(super) fn fact_hash_base(store: &Store, id: &str, rids: &[String]) -> String {
    let e = &store.graph.entities[id];
    let mut facts = format!(
        "{}|{}|{}|",
        e.name,
        e.definition.as_deref().unwrap_or(""),
        e.stereotype.as_deref().unwrap_or("")
    );
    for a in &e.attributes {
        facts.push_str(&format!(
            "{}:{}:{}|",
            a.name,
            a.r#type.as_deref().unwrap_or(""),
            a.value.as_deref().unwrap_or("")
        ));
    }
    for rid in rids {
        if let Some(r) = store.graph.requirements.get(rid) {
            facts.push_str(&r.statement);
            for edge in &r.edges {
                facts.push_str(&format!(
                    "{}>{}:{}:{}",
                    edge.a,
                    edge.b,
                    edge.rel_type.as_deref().unwrap_or(""),
                    edge.cardinality.as_deref().unwrap_or("")
                ));
            }
            facts.push('|');
        }
    }
    hash_hex(&facts)
}

// The generation contract, identical for every worker. It never names a medium: what
// the deliverable is (a language, a format, a genre) is a fact the documents state,
// carried by the context pack.
pub fn contract() -> String {
    crate::payload::fill(
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/generation/generate-contract.md.mustache"
        )),
        &[("GROUP", &GROUP.to_string())],
    )
}

pub fn instructions() -> String {
    contract() + "\n\n" + crate::task_runtime::PROMPT
}

// The change diff for one entity versus the ledger.
fn change_diff(ledger: &Ledger, slug: &str, current: &[String]) -> (String, Vec<String>) {
    match ledger.entities.get(slug) {
        None => (
            "new".to_string(),
            current.iter().map(|r| format!("{} (added)", r)).collect(),
        ),
        Some(e) => {
            let mut changed: Vec<String> = Vec::new();
            for r in current {
                if !e.requirements.contains(r) {
                    changed.push(format!("{} (added)", r));
                }
            }
            for r in &e.requirements {
                if !current.contains(r) {
                    changed.push(format!("{} (removed)", r));
                }
            }
            if changed.is_empty() {
                changed.push(
                    "(reworded: same requirement set, changed statements or definition)"
                        .to_string(),
                );
            }
            ("changed".to_string(), changed)
        }
    }
}

// Entities that are generation work, the first reason that holds naming it: the
// entry's facts moved (`changed`), a recorded file is gone from the deliverable
// (`files-gone`), a binding is incomplete, or a bound requirement is unimplemented.
// An entity with no entry and only current adopted rows is never generated over.
// Mirrors docs/compiler/goals/generate.md#created-when.
pub fn pending(store: &Store, gs: &GenSettings) -> Vec<Value> {
    let ledger = Ledger::load(&store.out);
    let binding_owed: std::collections::BTreeSet<String> = crate::bind::pending(store, gs)
        .iter()
        .filter_map(|row| row["requirement"].as_str().map(String::from))
        .collect();
    let by_entity = ReqsByEntity::build(store);
    let scan = assets::AssetScan::default();
    let mut out = Vec::new();
    for id in store.graph.entities.keys() {
        let rids = by_entity.of(id);
        if rids.is_empty() {
            continue;
        }
        let slug = crate::ids::entity_slug(id).to_string();
        let referenced_assets = assets::referenced_assets_in(&scan, store, id, rids);
        let hash = assets::fingerprint(&fact_hash_base(store, id, rids), &referenced_assets);
        let incomplete: Vec<&String> = rids
            .iter()
            .filter(|rid| binding_owed.contains(*rid))
            .collect();
        let unimplemented: Vec<&String> = rids
            .iter()
            .filter(|rid| {
                ledger
                    .requirements
                    .get(*rid)
                    .map(|row| {
                        matches!(
                            crate::verify::status_with(store, rid, row, gs, &ledger)
                                .0
                                .as_str(),
                            "unimplemented" | "partial"
                        )
                    })
                    .unwrap_or(false)
            })
            .collect();
        let (reason, changed) = match ledger.entities.get(&slug) {
            Some(e) if e.fact_hash != hash => {
                let (r, c) = change_diff(&ledger, &slug, rids);
                (r, json!(c))
            }
            Some(e) => {
                let gone: Vec<&String> = e
                    .files
                    .iter()
                    .filter(|f| !gs.deliverable.join(f).vexists())
                    .collect();
                // A record with no files is an entity carried by other entities' files
                // (an actor whose statements the tool implements); nothing of its own
                // can be gone. Mirrors docs/compiler/goals/generate.md#created-when.
                if implementation_outside_source(gs, &ledger, &e.files) {
                    ("source-selection-changed".to_string(), json!(e.files))
                } else if !gone.is_empty() {
                    ("files-gone".to_string(), json!(gone))
                } else if !unimplemented.is_empty() {
                    ("unimplemented-bindings".to_string(), json!(unimplemented))
                } else if !incomplete.is_empty() {
                    ("incomplete-bindings".to_string(), json!(incomplete))
                } else {
                    continue;
                }
            }
            None if !unimplemented.is_empty() => {
                ("unimplemented-bindings".to_string(), json!(unimplemented))
            }
            None if !incomplete.is_empty() => {
                ("incomplete-bindings".to_string(), json!(incomplete))
            }
            None if referenced_assets.iter().any(|asset| asset.is_input()) => (
                "referenced-inputs".to_string(),
                json!(referenced_assets
                    .iter()
                    .filter(|asset| asset.is_input())
                    .map(|asset| &asset.path)
                    .collect::<Vec<_>>()),
            ),
            None => continue,
        };
        out.push(json!({
            "entity": id,
            "reason": reason,
            "changed": changed,
        }));
    }
    out
}

// The generate goal's gate: record_generation landed for the entity with a factHash
// equal to the live graph's and files that exist; with `require_run`, run_tests ran
// afterwards too (no programmatic row of the entity is still never-run). Ok carries
// the recorded file count. Err names what is missing and the call that lands it.
// Mirrors docs/compiler/goals/generate.md#gate.
pub fn gate(store: &Store, gs: &GenSettings, id: &str, require_run: bool) -> Result<usize, String> {
    let id = store.resolve_id(id).to_string();
    if !store.graph.entities.contains_key(&id) {
        return Err(format!(
            "`{}` is not in the graph; nothing generates it",
            id
        ));
    }
    assets::ensure_available(&assets::referenced_assets(
        store,
        &id,
        &reqs_of_sorted(store, &id),
    ))?;
    let ledger = Ledger::load(&store.out);
    let Some(e) = ledger.entities.get(crate::ids::entity_slug(&id)) else {
        return Err(format!(
            "no ledger entry for `{}`: record_generation has not landed; the gate is record_generation with the package's factHash, then run_tests",
            id
        ));
    };
    let live = fact_hash(store, &id);
    if e.fact_hash != live {
        return Err(format!(
            include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/inline/gen-task/the-record-for-carries-facthash-but-the-30589.md"
            )),
            id, e.fact_hash, live
        ));
    }
    if implementation_outside_source(gs, &ledger, &e.files) {
        return Err(format!(
            include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/inline/gen-task/the-record-for-names-implementing-files-outside-cee82.md"
            )),
            id
        ));
    }
    let gone: Vec<&String> = e
        .files
        .iter()
        .filter(|f| !gs.deliverable.join(f).vis_file())
        .collect();
    if !gone.is_empty() {
        return Err(format!(
            include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/inline/gen-task/the-record-for-lists-files-that-do-bceac.md"
            )),
            id,
            gone.iter()
                .map(|f| f.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    // A record with no implementing file of its own stands only when every
    // requirement of the entity has a row naming an implementing file that exists:
    // the entity is carried by other entities' files. Test artifacts and criteria
    // files are not implementation. Mirrors docs/compiler/goals/generate.md#gate.
    let artifacts = test_artifacts(&ledger);
    let outputs = build_outputs(&ledger);
    let implementing = |f: &String| {
        !artifacts.contains(f)
            && !outputs.contains(f)
            && gs.allows_source(f)
            && gs.deliverable.join(f).vis_file()
    };
    if !e.files.iter().any(&implementing) {
        let missing: Vec<String> = reqs_of_sorted(store, &id)
            .into_iter()
            .filter(|rid| {
                !ledger
                    .requirements
                    .get(rid)
                    .is_some_and(|row| row.files.iter().any(&implementing))
            })
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/gen-task/the-record-for-lists-no-implementing-file-4efb2.md"
                )),
                id,
                missing.join(", ")
            ));
        }
    }
    let missing_tests: Vec<String> = reqs_of_sorted(store, &id)
        .into_iter()
        .filter(|rid| crate::bind::gate(store, gs, rid).is_err())
        .collect();
    if !missing_tests.is_empty() {
        return Err(format!(
            "required test bindings are missing or stale for {}; record_generation with each current requirement and criterion test",
            missing_tests.join(", ")
        ));
    }
    if require_run {
        let never_run: Vec<String> = reqs_of_sorted(store, &id)
            .into_iter()
            .filter(|rid| {
                ledger
                    .requirements
                    .get(rid)
                    .map(|row| {
                        (row.test.kind == "programmatic"
                            && crate::verify::status_with(store, rid, row, gs, &ledger)
                                == ("unverified".to_string(), "never-run".to_string()))
                            || row.tests.values().any(|test| {
                                test.test.kind == "programmatic"
                                    && (test.verdict == "none" || test.last_run.is_none())
                            })
                    })
                    .unwrap_or(false)
            })
            .collect();
        if !never_run.is_empty() {
            return Err(format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/gen-task/run-tests-has-not-run-since-the-366e1.md"
                )),
                never_run.join(", ")
            ));
        }
    }
    Ok(e.files.len())
}

// Why `jazyk gen` leaves an entity alone, for the person reading the trace: the
// entity is current, or its adopted rows need no generation.
// Mirrors docs/consumers/gen.md#incremental-regeneration.
pub fn skip_reason(store: &Store, gs: &GenSettings, id: &str) -> String {
    if let Some(row) = pending(store, gs).iter().find(|row| row["entity"] == id) {
        return format!(
            "generation work remains ({})",
            row["reason"].as_str().unwrap_or("incomplete")
        );
    }
    let ledger = Ledger::load(&store.out);
    if ledger.entities.contains_key(crate::ids::entity_slug(id)) {
        "unchanged".to_string()
    } else {
        "no ledger entry and all adopted rows are current: there is nothing to generate (`--force` generates it anyway)".to_string()
    }
}

// The full package a worker needs for one task.
// The file the build command runs, with its current content. The command names it as
// an argument, so the entry is the first token that exists under the deliverable.
// Mirrors docs/consumers/gen.md#the-build.
fn build_entry(ledger: &Ledger, gs: &GenSettings) -> Value {
    let Some(b) = &ledger.build else {
        return Value::Null;
    };
    let dir = gs.deliverable.join(&b.cwd);
    for token in b.run.split_whitespace() {
        let token = token.trim_matches(|c| c == '"' || c == '\'');
        if token.starts_with('-') || token.is_empty() {
            continue;
        }
        let candidate = dir.join(token);
        if candidate.vis_file() {
            let rel = norm_rel(
                &pathdiff(&candidate, &gs.deliverable).unwrap_or_else(|| token.to_string()),
            );
            let content = crate::vfs::read_to_string(&candidate).unwrap_or_default();
            return json!({"path": rel, "content": crate::llm::truncate(&content, 12_000)});
        }
    }
    Value::Null
}

// The file a build command runs, by name alone: the first token that looks like a
// path rather than a flag or an interpreter.
pub(super) fn entry_from_run(run: &str) -> Option<String> {
    run.split_whitespace()
        .map(|t| t.trim_matches(|c| c == '"' || c == '\''))
        .find(|t| {
            !t.starts_with('-')
                && t.contains('.')
                && !t.ends_with("python")
                && !t.ends_with("python3")
        })
        .map(String::from)
}

// Deliverable-relative paths as the manifest writes them: no leading `./`, no
// duplicate separators. Joining a build's `cwd` produces the other spelling, and a
// path compared in the wrong spelling silently matches nothing.
pub(super) fn norm_rel(p: &str) -> String {
    p.replace("/./", "/").trim_start_matches("./").to_string()
}

// A deliverable-relative path, lexically.
fn pathdiff(path: &Path, base: &Path) -> Option<String> {
    path.strip_prefix(base)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

pub fn task_package(store: &Store, id: &str, gs: &GenSettings) -> Result<Value, String> {
    if !store.graph.entities.contains_key(id) {
        return Err(format!("unknown entity `{}`", id));
    }
    let ledger = Ledger::load(&store.out);
    let e = &store.graph.entities[id];
    let slug = crate::ids::entity_slug(id).to_string();
    let rids = reqs_of_sorted(store, id);
    let referenced_assets = assets::referenced_assets(store, id, &rids);
    let (_, changed) = change_diff(&ledger, &slug, &rids);
    let groups: Vec<Vec<Value>> = rids
        .chunks(GROUP)
        .map(|chunk| {
            chunk
                .iter()
                .filter_map(|rid| {
                    store.graph.requirements.get(rid).map(|r| {
                        json!({
                            "id": rid,
                            "statement": r.statement,
                            "quote": r.source.as_ref().map(|s| s.quote.clone()).unwrap_or_default(),
                            "provenance": crate::session::provenance_line(r),
                            "hash": hash_hex(&r.statement),
                            "criteriaFrontMatter": criteria_front_matter(rid, None, &r.statement),
                            "testName": test_name(rid, &r.statement),
                            // The criteria with the names binding gives their tests,
                            // so a test generation must write (a requirement with no
                            // bound row) carries the name binding would look for.
                            // Mirrors docs/compiler/goals/generate.md#what-the-model-sees.
                            "criteria": store.criteria_of(rid).iter().filter_map(|cid| {
                                let c = store.graph.criteria.get(cid)?;
                                Some(json!({
                                    "id": cid, "statement": c.statement,
                                    "suggestedTestName": crit_test_name(cid, &c.statement),
                                    "criteriaFrontMatter": criteria_front_matter(rid, Some(cid), &c.statement),
                                }))
                            }).collect::<Vec<Value>>(),
                            "criteriaPath": format!("criteria/req-{}.md", req_slug(rid)),
                            // The statement is disputed while these stand; the session
                            // writes against one side of an open question and must
                            // know it (docs/consumers/gen.md#rows-recorded-over-an-open-contradiction).
                            "openDiagnostics": open_errors_on(store, rid),
                        })
                    })
                })
                .collect()
        })
        .collect();
    let rels: Vec<String> = store
        .graph
        .relationships
        .values()
        .filter(|rel| rel.members.iter().any(|m| m == id))
        .map(|rel| {
            format!(
                "{} {}",
                rel.strongest(),
                rel.members
                    .iter()
                    .filter(|m| *m != id)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .collect();
    // The goal's loaded set: the entity in full with its neighbors as stubs.
    // Mirrors docs/consumers/gen.md.
    let pack = crate::context::ledger_context(store, id);
    // The manifest of other tasks' work. A composite deliverable is assembled from parts
    // other entities wrote, so a path alone is not enough: the entry names what the files
    // hold, by the statements the task implemented.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    let manifest: BTreeMap<&String, Value> = ledger
        .entities
        .iter()
        .map(|(k, v)| {
            let holds: Vec<String> = v
                .requirements
                .iter()
                .filter_map(|rid| {
                    store
                        .graph
                        .requirements
                        .get(rid)
                        .map(|r| r.statement.clone())
                })
                .collect();
            // Under a built medium the entry this task rewrites has to call into
            // these files, so their content travels with them: a path and a
            // statement do not say what a part is called
            // (docs/consumers/gen.md#the-build).
            let show = ledger
                .medium
                .as_ref()
                .map(|m| m.is_built())
                .unwrap_or(false);
            let contents: BTreeMap<String, String> = if show {
                v.files
                    .iter()
                    .filter(|f| !f.contains("test"))
                    .filter_map(|f| {
                        crate::vfs::read_to_string(gs.deliverable.join(f))
                            .ok()
                            .map(|c| (f.clone(), crate::llm::truncate(&c, 4_000)))
                    })
                    .collect()
            } else {
                BTreeMap::new()
            };
            (
                k,
                json!({"files": v.files, "holds": holds, "contents": contents}),
            )
        })
        .collect();
    // The established conventions: run commands other tasks already recorded. One
    // toolchain per deliverable (docs/consumers/gen.md#file-ownership-and-conventions).
    let mut run_commands: Vec<String> = ledger
        .requirements
        .values()
        .filter(|r| r.test.kind == "programmatic" && !r.test.run.trim().is_empty())
        .map(|r| r.test.run.clone())
        .collect();
    run_commands.sort();
    run_commands.dedup();
    Ok(json!({
        "entity": id,
        "name": e.name,
        "deliverable": gs.deliverable.to_string_lossy(),
        "sourceSelection": gs.code,
        "factHash": assets::fingerprint(&fact_hash_base(store, id, &rids), &referenced_assets),
        "referencedAssets": referenced_assets,
        "instructions": instructions(),
        "context": pack,
        "relationships": rels,
        "requirementGroups": groups,
        "changed": changed,
        "generatedFiles": manifest,
        "runCommands": run_commands,
        // The requirements under an open error diagnostic, and the medium's standing
        // divergence from the recorded commands, so neither is news at record time.
        "contradicted": rids
            .iter()
            .filter(|rid| !open_errors_on(store, rid).is_empty())
            .cloned()
            .collect::<Vec<String>>(),
        "mediumWarning": medium_divergence(&ledger),
        "supportFiles": ledger.support,
        // The entry the build runs, with what it holds today: a later task returns it
        // updated so the artifact carries its part too
        // (docs/consumers/gen.md#the-build).
        "buildEntry": build_entry(&ledger, gs),
        // A standing build failure: the artifact does not exist until it is fixed. The
        // package says whether this entity's own files are what the failure names, so
        // the task that can fix it knows it is the one (docs/consumers/gen.md#the-build).
        "buildError": ledger
            .build
            .as_ref()
            .and_then(|b| b.last_run.as_ref())
            .filter(|r| !r.ok)
            .map(|r| {
                let mine: Vec<String> = ledger
                    .entities
                    .get(&slug)
                    .map(|e| e.files.iter().filter(|f| r.error.contains(f.as_str())).cloned().collect())
                    .unwrap_or_default();
                json!({"at": r.at, "error": r.error, "yours": mine})
            }),
        // Decided once for the deliverable; a task states it, never re-decides it.
        "medium": ledger.medium.as_ref().map(|m| json!({
            "form": m.form, "produced": m.produced, "toolchain": m.toolchain, "artifact": m.artifact,
        })),
        "mediumNote": medium_note(&ledger),
        "build": ledger.build.as_ref().map(|b| json!({"run": b.run, "cwd": b.cwd, "produces": b.produces})),
        // The tests binding already wrote: they define the interface, and the product
        // conforms to them. A task that cannot make one pass without changing it
        // reports that; the repair is a re-bind
        // (docs/consumers/bind.md#generation-makes-bound-tests-pass).
        // Shared requirements confused a Codex session into re-recording them
        // (docs/compiler/goals/generate.md#what-the-model-sees).
        "ownershipNote": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/gen-task/a-requirement-already-carried-by-another-entity-005ff.md")),
        // The entity's component group: members whose files it may write and record
        // too (docs/consumers/gen.md#file-ownership-and-conventions).
        "sharedWith": shared_with(store, id),
        "boundTests": rids
            .iter()
            .filter_map(|rid| {
                ledger.requirements.get(rid).map(|row| {
                    let (status, _) = crate::verify::status_with(store, rid, row, gs, &ledger);
                    json!({
                        "requirement": rid,
                        "status": status,
                        "files": row.files,
                        "test": {"kind": row.test.kind, "artifact": row.test.artifact,
                                 "name": row.test.name, "run": row.test.run, "cwd": row.test.cwd},
                        "criteriaTests": row.tests.iter().map(|(cid, t)| json!({
                            "criterion": cid, "kind": t.test.kind, "artifact": t.test.artifact,
                            "name": t.test.name, "run": t.test.run, "cwd": t.test.cwd, "verdict": t.verdict,
                        })).collect::<Vec<Value>>(),
                    })
                })
            })
            .collect::<Vec<Value>>(),
    }))
}

// ---- ledger-stale change records ----
// One record per requirement or entity the ledger and the graph disagree about,
// derived from the same pending predicates the goals read, with the resolving goal
// and the reason in `detail`, so the board derives the bind, generate, and verify
// goals from them. Mirrors docs/compiler/goals/bind.md#created-when,
// docs/compiler/goals/generate.md#created-when, docs/compiler/goals/verify.md#created-when.
pub fn ledger_stale_records(store: &Store, gs: &GenSettings) -> Vec<crate::model::ChangeRecord> {
    let generation = store.status.generation;
    let mut index = store
        .status
        .changes
        .iter()
        .filter(|c| c.generation == generation)
        .map(|c| c.mutation)
        .max()
        .unwrap_or(0);
    let mut out: Vec<crate::model::ChangeRecord> = Vec::new();
    let mut push = |index: &mut usize, subject: String, detail: Value| {
        *index += 1;
        out.push(crate::model::ChangeRecord {
            id: format!("c{}-{}", generation, index),
            generation,
            mutation: *index,
            kind: crate::goals::CHANGE_LEDGER_STALE.to_string(),
            subject,
            via: "ledger".into(),
            detail,
        });
    };
    let bind_owed: std::collections::BTreeSet<String> = crate::bind::pending(store, gs)
        .iter()
        .filter_map(|p| p["requirement"].as_str().map(String::from))
        .collect();
    for p in crate::bind::pending(store, gs) {
        push(
            &mut index,
            p["requirement"].as_str().unwrap_or_default().to_string(),
            json!({"goal": "bind", "reason": p["reason"], "entity": p["entity"]}),
        );
    }
    for p in pending(store, gs) {
        let reason = match p["reason"].as_str() {
            Some("unimplemented-bindings") => json!("unimplemented"),
            Some("files-gone") => json!("files-gone"),
            Some("incomplete-bindings") => json!("incomplete-bindings"),
            Some("referenced-inputs") => json!("referenced-inputs"),
            _ => json!("facts-changed"),
        };
        push(
            &mut index,
            p["entity"].as_str().unwrap_or_default().to_string(),
            json!({"goal": "generate", "reason": reason, "changed": p["changed"]}),
        );
    }
    for p in crate::verify::pending(store, gs, Some("stale"), None) {
        if p["status"] == "unimplemented"
            || p["status"] == "partial"
            || p["reason"] == "not-generated"
        {
            continue;
        }
        if bind_owed.contains(p["requirement"].as_str().unwrap_or_default()) {
            continue;
        }
        push(
            &mut index,
            p["requirement"].as_str().unwrap_or_default().to_string(),
            json!({"goal": "verify", "reason": p["reason"], "kind": p["test"]["kind"], "entity": p["entity"]}),
        );
    }
    out
}
