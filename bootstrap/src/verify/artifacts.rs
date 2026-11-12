//! Exact artifact selection for the file tree, independent of requirement-row scope.
use super::*;
use crate::gen::TestRef;
use crate::project::Project;
use crate::runner::SessionRunner;
use crate::session::{Trace, TraceEvent};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Bound {
    set_id: Option<String>,
    execution_dir: String,
    requirement: String,
    criterion: Option<String>,
    test: TestRef,
    artifact: String,
}

fn execution_dir(gs: &GenSettings, cwd: &str) -> String {
    let path = gs.deliverable.join(cwd);
    crate::vfs::canonicalize(&path)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn reference(test: &TestRef) -> String {
    serde_json::to_string(test).expect("serializable test reference")
}

pub fn parse_paths(body: &Value) -> Result<Option<Vec<String>>, String> {
    let Some(value) = body.get("testPaths") else {
        return Ok(None);
    };
    let values = value
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or(include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/verify-artifacts/testpaths-must-be-a-nonempty-list-of-446c8.md"
        )))?;
    if body.get("targets").is_some_and(|value| {
        !value.is_null() && value.as_array().is_none_or(|values| !values.is_empty())
    }) {
        return Err("testPaths cannot be combined with targets".into());
    }
    if body.get("testKind").is_some_and(|value| !value.is_null()) {
        return Err("testPaths cannot be combined with testKind".into());
    }
    let paths = values
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "testPaths must contain only filenames".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    for path in &paths {
        safe_path(path)?;
    }
    Ok(Some(paths))
}

fn safe_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.contains('\\')
        || path.contains(':')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(format!("Invalid project-relative test path: {path}"));
    }
    Ok(())
}

fn all_bound(project: &Project, store: &Store, gs: &GenSettings) -> Result<Vec<Bound>, String> {
    let ledger = Ledger::load(&store.out);
    if let Some(error) = ledger.unreadable {
        return Err(error);
    }
    let mut all = Vec::new();
    for (rid, row) in ledger.requirements {
        let headline = reference(&row.test);
        let has_headline = row.tests.values().any(|t| reference(&t.test) == headline);
        let refs = row
            .tests
            .into_iter()
            .map(|(cid, test)| (Some(cid), test.test))
            .chain((!has_headline).then_some((None, row.test)));
        for (criterion, test) in refs {
            let artifact = artifact_path(&store.out, gs, &test)
                .strip_prefix(&project.root)
                .map_err(|_| "Test artifact is outside the project")?
                .to_string_lossy()
                .replace('\\', "/");
            safe_path(&artifact)?;
            all.push(Bound {
                set_id: project.set_id.clone(),
                execution_dir: execution_dir(gs, &test.cwd),
                requirement: rid.clone(),
                criterion,
                test,
                artifact,
            });
        }
    }
    Ok(all)
}

fn validate_runner_scope(
    runs: &[(String, String)],
    all: &[Bound],
    paths: &BTreeSet<String>,
    selector: Option<&str>,
) -> Result<(), String> {
    for (run, cwd) in runs {
        if all.iter().any(|other| {
            other.test.kind == "programmatic"
                && other.test.run == *run
                && (run.starts_with("task:") || other.execution_dir == *cwd)
                && (!paths.contains(&other.artifact)
                    || selector.is_some_and(|set| other.set_id.as_deref() != Some(set)))
        }) {
            return Err(format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/verify-artifacts/runner-run-also-covers-tests-outside-the-39174.md"
                )),
                run = run
            ));
        }
    }
    Ok(())
}

fn plan(
    project: &Project,
    store: &Store,
    gs: &GenSettings,
    paths: &[String],
) -> Result<Vec<Bound>, String> {
    if paths.is_empty() {
        return Err("Select at least one test artifact".into());
    }
    let paths = paths
        .iter()
        .map(|p| {
            safe_path(p)?;
            Ok(p.clone())
        })
        .collect::<Result<BTreeSet<_>, String>>()?;
    let all = all_bound(project, store, gs)?;
    for path in &paths {
        if !all.iter().any(|test| &test.artifact == path) {
            return Err(format!(
                "No recorded test binding for {path}. Link the test before running it."
            ));
        }
    }
    let selected = all
        .iter()
        .filter(|test| paths.contains(&test.artifact))
        .cloned()
        .collect::<Vec<_>>();
    let ledger = Ledger::load(&store.out);
    let mut runners = selected
        .iter()
        .filter(|test| test.test.kind == "programmatic" && !test.test.run.trim().is_empty())
        .map(|test| (test.test.run.clone(), test.execution_dir.clone()))
        .collect::<Vec<_>>();
    if let Some(build) = &ledger.build {
        runners.push((build.run.clone(), execution_dir(gs, &build.cwd)));
    }
    validate_runner_scope(&runners, &all, &paths, None)?;
    for bound in &selected {
        let row = &ledger.requirements[&bound.requirement];
        ensure_current_source(store, gs, row)?;
        let requirement = store
            .graph
            .requirements
            .get(&bound.requirement)
            .ok_or("Bound requirement no longer exists")?;
        if hash_hex(&requirement.statement) != row.hashes.requirement {
            return Err(format!(
                "Requirement {} changed. Rebind its tests before running them.",
                bound.requirement
            ));
        }
        if let Some(cid) = &bound.criterion {
            let criterion = store
                .graph
                .criteria
                .get(cid)
                .ok_or("Bound criterion no longer exists")?;
            if hash_hex(&criterion.statement) != row.tests[cid].criterion_hash {
                return Err(format!(
                    "Criterion {cid} changed. Rebind its test before running it."
                ));
            }
        }
        if !crate::vfs::is_file(&project.root.join(&bound.artifact)) {
            return Err(format!("Test artifact is missing: {}", bound.artifact));
        }
        if !matches!(bound.test.kind.as_str(), "llm" | "programmatic") {
            return Err(format!("Unsupported test kind for {}", bound.artifact));
        }
        if bound.test.kind == "programmatic" {
            if !artifact_names(
                &artifact_path(&store.out, gs, &bound.test),
                &bound.test.name,
            ) {
                return Err(format!(
                    "Test {} is missing from {}. Rebind its selector.",
                    bound.test.name, bound.artifact
                ));
            }
            if bound.test.run.trim().is_empty() {
                return Err(format!("No runner is recorded for {}", bound.artifact));
            }
            if crate::vfs::is_scoped() && !bound.test.run.starts_with("task:") {
                return Err(format!(
                    "{} needs a saved scoped task on this backend",
                    bound.artifact
                ));
            }
            if let Some(id) = bound.test.run.strip_prefix("task:") {
                if !crate::task_runtime::load(&project.root)?
                    .1
                    .iter()
                    .any(|task| task.id == id)
                {
                    return Err(format!("Saved test task {id} is missing"));
                }
            }
        } else {
            crate::gen::validate_criteria_file(
                store,
                &bound.requirement,
                bound.criterion.as_deref(),
                &artifact_path(&store.out, gs, &bound.test),
            )?;
        }
    }
    Ok(selected)
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn run(
    project: &Project,
    store: &Store,
    runner: &dyn SessionRunner,
    gs: &GenSettings,
    paths: &[String],
    trace: &Trace,
) -> Result<Value, String> {
    run_cached(
        project,
        store,
        runner,
        gs,
        paths,
        trace,
        &mut RunCache::default(),
    )
    .await
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
async fn prepare_build(
    store: &Store,
    gs: &GenSettings,
    paths: &[String],
    trace: &Trace,
    cache: &mut RunCache,
) -> Result<crate::gen::progress::Progress<Progress>, String> {
    let signature = json!({"paths":paths,"settings":gs});
    let key = crate::gen::progress::command_key("verify-artifacts", &store.out, &signature);
    let mut progress = crate::gen::progress::Progress::load(key, signature, || Progress {
        built: false,
        results: BTreeMap::new(),
        task_receipts: BTreeMap::new(),
        inputs: None,
    })?;
    cache.tasks.extend(progress.data.task_receipts.clone());
    if !progress.data.built {
        let build = Ledger::load(&store.out).build;
        if let Some(build) = build.as_ref().filter(|build| {
            build
                .run
                .strip_prefix("task:")
                .is_some_and(|id| cache.tasks.contains_key(id))
        }) {
            let id = build
                .run
                .strip_prefix("task:")
                .expect("selected cached build task");
            let (_, result) = &cache.tasks[id];
            if result["status"] != "succeeded" || result["complete"] != true {
                return Err(include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/verify-artifacts/build-task-did-not-complete-successfully-inspect-f9304.md"
                ))
                .into());
            }
            crate::gen::check_build_products(&store.out, gs, build)?;
        } else if let Some((task, id, result)) =
            crate::gen::run_build_receipt(&store.out, gs, trace, "verify-artifacts")?
        {
            cache.tasks.insert(task, (id, receipt(&result)));
        }
        progress.data.task_receipts = cache.tasks.clone();
        progress.data.built = true;
        progress.save().await?;
    }
    Ok(progress)
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
async fn run_cached(
    project: &Project,
    store: &Store,
    runner: &dyn SessionRunner,
    gs: &GenSettings,
    paths: &[String],
    trace: &Trace,
    cache: &mut RunCache,
) -> Result<Value, String> {
    let selected = plan(project, store, gs, paths)?;
    let mut progress = prepare_build(store, gs, paths, trace, cache).await?;
    retain_inputs(store, gs, &selected, &mut progress, cache).await?;
    let mut lines = Vec::new();
    for bound in selected {
        cache.validate_inputs()?;
        if trace.is_cancelled() {
            return Err("Verification cancelled".into());
        }
        if crate::session_recovery::suspension_requested() {
            return Err("suspended".into());
        }
        let id = if bound.test.kind == "llm" {
            format!(
                "{}:{:?}:{}",
                bound.requirement,
                bound.criterion,
                reference(&bound.test)
            )
        } else {
            reference(&bound.test)
        };
        let result = if let Some(result) = progress.data.results.get(&id) {
            result.clone()
        } else {
            trace.event(TraceEvent::VerifyRowStart {
                requirement: bound.requirement.clone(),
                test: bound.test.kind.clone(),
            });
            let observed = if bound.test.kind == "programmatic" {
                let run = match cache.programmatic(
                    gs,
                    &bound.test,
                    |id| crate::task_runtime::execute_with_key(&project.root, id, "artifact-suite"),
                    || run_test_ref(gs, &bound.test),
                ) {
                    Ok(run) => run,
                    Err(error) if crate::session_recovery::is_interruption(&error) => {
                        return Err(error)
                    }
                    Err(error) => {
                        let observed = Observed {
                            verdict: "none".into(),
                            code: None,
                            evidence: error,
                        };
                        trace.event(TraceEvent::VerifyRowError {
                            requirement: bound.requirement.clone(),
                            message: observed.evidence.clone(),
                        });
                        cache.validate_inputs()?;
                        if !bound.test.run.starts_with("task:") {
                            record(store, gs, &bound, &observed)?;
                        }
                        lines.push(json!({"requirement":bound.requirement,"criterion":bound.criterion,"artifact":bound.artifact,"verdict":"none","evidence":observed.evidence}));
                        continue;
                    }
                };
                Observed {
                    verdict: if run.runner_failed() {
                        "none"
                    } else if run.pass {
                        "pass"
                    } else {
                        "fail"
                    }
                    .into(),
                    code: Some(run.code),
                    evidence: run.evidence,
                }
            } else {
                let package = task(store, &bound.requirement, gs)?;
                let criteria =
                    crate::vfs::read_to_string(artifact_path(&store.out, gs, &bound.test))
                        .map_err(|e| e.to_string())?;
                let statement = bound
                    .criterion
                    .as_ref()
                    .and_then(|id| store.graph.criteria.get(id))
                    .map(|c| c.statement.as_str())
                    .unwrap_or("");
                let files = judge_files(
                    gs,
                    package["files"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str),
                );
                let input = judge_input(
                    Some(statement),
                    &criteria,
                    package["context"].as_str().unwrap_or_default(),
                    &files,
                    &JUDGE.fill("ask-test", &[("test", &bound.test.name)]),
                );
                let reply = runner
                    .ask_traced(
                        include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/verification/verify-criterion-contract.md.mustache")),
                        &input,
                        &format!("verify {} {}", bound.requirement, bound.test.name),
                        "judge",
                        Some(trace),
                    )
                    .await?;
                let pass =
                    parse_verdict(&reply).ok_or("Model returned no conclusive test verdict")?;
                Observed {
                    verdict: if pass { "pass" } else { "fail" }.into(),
                    code: None,
                    evidence: crate::llm::truncate(reply.trim(), 400),
                }
            };
            cache.validate_inputs()?;
            progress.data.task_receipts = cache.tasks.clone();
            progress.data.results.insert(id, observed.clone());
            progress.save().await?;
            observed
        };
        cache.validate_inputs()?;
        let result = if let Some(task) = bound.test.run.strip_prefix("task:") {
            let run_id = &cache
                .tasks
                .get(task)
                .ok_or("checkpoint-incompatible: missing selected task receipt")?
                .0;
            imported_task_observation(store, gs, &bound, run_id).ok_or_else(|| {
                format!(
                    include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/verify-artifacts/test-inputs-changed-or-run-run-id-04a8d.md"
                    )),
                    run_id = run_id
                )
            })?
        } else {
            record(store, gs, &bound, &result)?;
            result
        };
        trace.event(TraceEvent::VerifyRowDone {
            requirement: bound.requirement.clone(),
            verdict: result.verdict.clone(),
            run: bound.test.run.clone(),
            evidence: result.evidence.clone(),
        });
        lines.push(json!({"requirement":bound.requirement,"criterion":bound.criterion,"artifact":bound.artifact,"verdict":result.verdict,"evidence":result.evidence}));
    }
    cache.validate_inputs()?;
    let verified = lines
        .iter()
        .filter(|test| test["verdict"] == "pass")
        .count();
    let failing = lines
        .iter()
        .filter(|test| test["verdict"] == "fail")
        .count();
    Ok(
        json!({"rows":lines.len(),"verified":verified,"failing":failing,"skipped":lines.len()-verified-failing,"tests":lines,"testPaths":paths}),
    )
}

fn receipt(result: &Value) -> Value {
    json!({"status":result["status"],"complete":result["complete"],"exit_code":result["exit_code"],"tests":result["tests"]})
}

#[derive(Default)]
struct RunCache {
    inputs: BTreeMap<std::path::PathBuf, InputScope>,
    tasks: BTreeMap<String, (String, Value)>,
    commands: BTreeMap<String, ProgRun>,
}
impl RunCache {
    fn validate_inputs(&self) -> Result<(), String> {
        for scope in self.inputs.values() {
            if input_fingerprint(&scope.store, &scope.gs, &scope.selected)? != scope.fingerprint {
                return Err(changed_inputs());
            }
        }
        Ok(())
    }
    fn programmatic(
        &mut self,
        gs: &GenSettings,
        test: &TestRef,
        execute_task: impl FnOnce(&str) -> Result<(String, Value), String>,
        execute_legacy: impl FnOnce() -> Result<ProgRun, String>,
    ) -> Result<ProgRun, String> {
        if let Some(id) = test.run.strip_prefix("task:") {
            if !self.tasks.contains_key(id) {
                let (run, result) = execute_task(id)?;
                self.tasks.insert(id.into(), (run, receipt(&result)));
            }
            let (run, result) = &self.tasks[id];
            return task_case_outcome(result, &test.name, run);
        }
        let key = json!([execution_dir(gs, &test.cwd), test.run]).to_string();
        if !self.commands.contains_key(&key) {
            self.commands.insert(key.clone(), execute_legacy()?);
        }
        Ok(self.commands[&key].clone())
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Observed {
    verdict: String,
    code: Option<i32>,
    evidence: String,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Progress {
    built: bool,
    results: BTreeMap<String, Observed>,
    #[serde(default)]
    task_receipts: BTreeMap<String, (String, Value)>,
    #[serde(default)]
    inputs: Option<String>,
}

struct InputScope {
    store: Store,
    gs: GenSettings,
    selected: Vec<Bound>,
    fingerprint: String,
}

fn changed_inputs() -> String {
    "Test inputs changed or the retained observation has no input fingerprint. Rerun the selected tests in a new job.".into()
}

fn input_fingerprint(
    store: &Store,
    gs: &GenSettings,
    selected: &[Bound],
) -> Result<String, String> {
    let ledger = Ledger::load(&store.out);
    let mut inputs = Vec::new();
    for bound in selected {
        let row = ledger
            .requirements
            .get(&bound.requirement)
            .ok_or_else(changed_inputs)?;
        ensure_current_source(store, gs, row)?;
        let test = if let Some(cid) = &bound.criterion {
            &row.tests.get(cid).ok_or_else(changed_inputs)?.test
        } else {
            &row.test
        };
        if reference(test) != reference(&bound.test) {
            return Err(changed_inputs());
        }
        inputs.push(json!({"reference":test,
            "requirement":store.graph.requirements.get(&bound.requirement),
            "criterion":bound.criterion.as_ref().and_then(|id|store.graph.criteria.get(id)),
            "context":crate::context::ledger_context(store,&bound.requirement),
            "files":row.files,"fileBytes":crate::gen::hash_files(gs,&row.files),
            "artifact":hash_file(&artifact_path(&store.out,gs,test))}));
    }
    let config = crate::project::find_root(&store.out)
        .map(|root| hash_file(&root.join(".jazyk/jazyk.toml")));
    Ok(hash_hex(
        &json!({"settings":gs,"inputs":inputs,"support":ledger.support,
            "supportBytes":crate::gen::hash_files(gs,&ledger.support),"projectConfig":config})
        .to_string(),
    ))
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
async fn retain_inputs(
    store: &Store,
    gs: &GenSettings,
    selected: &[Bound],
    progress: &mut crate::gen::progress::Progress<Progress>,
    cache: &mut RunCache,
) -> Result<(), String> {
    let current = input_fingerprint(store, gs, selected)?;
    if progress
        .data
        .inputs
        .as_ref()
        .is_some_and(|saved| saved != &current)
        || (progress.data.inputs.is_none() && !progress.data.results.is_empty())
    {
        return Err(changed_inputs());
    }
    if progress.data.inputs.is_none() {
        progress.data.inputs = Some(current.clone());
        progress.save().await?;
    }
    if let Some(previous) = cache.inputs.get(&store.out) {
        if previous.fingerprint != current {
            return Err(changed_inputs());
        }
    } else {
        cache.inputs.insert(
            store.out.clone(),
            InputScope {
                store: store.clone(),
                gs: gs.clone(),
                selected: selected.to_vec(),
                fingerprint: current,
            },
        );
    }
    cache.validate_inputs()
}

fn imported_task_observation(
    store: &Store,
    gs: &GenSettings,
    bound: &Bound,
    run_id: &str,
) -> Option<Observed> {
    let ledger = Ledger::load(&store.out);
    let row = ledger.requirements.get(&bound.requirement)?;
    ensure_current_source(store, gs, row).ok()?;
    let live = store.graph.requirements.get(&bound.requirement)?;
    if row.hashes.requirement != hash_hex(&live.statement)
        || row.hashes.files != crate::gen::hash_implementing(&store.out, gs, &row.files)
    {
        return None;
    }
    if let Some(cid) = &bound.criterion {
        if row.tests.get(cid)?.criterion_hash != hash_hex(&store.graph.criteria.get(cid)?.statement)
        {
            return None;
        }
        if reference(&row.tests[cid].test) != reference(&bound.test) {
            return None;
        }
    } else if reference(&row.test) != reference(&bound.test) {
        return None;
    }
    let (verdict, code, evidence, hash) = if let Some(id) = &bound.criterion {
        let test = row.tests.get(id)?;
        (
            &test.verdict,
            test.exit_code,
            test.evidence.as_ref()?,
            &test.test_hash,
        )
    } else {
        (
            &row.verdict,
            row.exit_code,
            row.evidence.as_ref()?,
            &row.hashes.test,
        )
    };
    if !evidence.starts_with(&format!("run {run_id}:"))
        || *hash != hash_file(&artifact_path(&store.out, gs, &bound.test))
    {
        return None;
    }
    Some(Observed {
        verdict: verdict.clone(),
        code,
        evidence: evidence.clone(),
    })
}

fn record(store: &Store, gs: &GenSettings, bound: &Bound, result: &Observed) -> Result<(), String> {
    let _lock = Ledger::lock(&store.out);
    let mut ledger = Ledger::load(&store.out);
    let row = ledger
        .requirements
        .get_mut(&bound.requirement)
        .ok_or("Test binding disappeared")?;
    ensure_current_source(store, gs, row)?;
    let files = crate::gen::hash_implementing(&store.out, gs, &row.files);
    rebaseline_files(row, files);
    let now = Some(now_iso());
    if let Some(cid) = &bound.criterion {
        let test = row
            .tests
            .get_mut(cid)
            .ok_or("Criterion binding disappeared")?;
        test.verdict = result.verdict.clone();
        test.exit_code = result.code;
        test.evidence = Some(result.evidence.clone());
        test.last_run = now.clone();
        test.test_hash = hash_file(&artifact_path(&store.out, gs, &test.test));
    }
    if let Some(test) = row
        .tests
        .values()
        .find(|test| reference(&test.test) == reference(&row.test))
    {
        row.verdict = test.verdict.clone();
        row.exit_code = test.exit_code;
        row.evidence = test.evidence.clone();
        row.last_run = test.last_run.clone();
        row.hashes.test = test.test_hash.clone();
    } else if reference(&row.test) == reference(&bound.test) {
        row.verdict = result.verdict.clone();
        row.exit_code = result.code;
        row.evidence = Some(result.evidence.clone());
        row.last_run = now;
        row.hashes.test = hash_file(&artifact_path(&store.out, gs, &row.test));
    }
    ledger.save(&store.out)
}

/// One durable job validates all affected sets, then runs them sequentially.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_workspace(
    root: &std::path::Path,
    selector: Option<&str>,
    paths: &[String],
    trace: &Trace,
    make_runner: impl Fn(&Project) -> Result<Box<dyn SessionRunner>, String>,
) -> Result<Value, String> {
    let workspace = crate::document_sets::Workspace::load(root).map_err(|e| e.1)?;
    let mut workspace_bound = Vec::new();
    for set in &workspace.sets {
        let project = workspace.resolve(Some(&set.id)).map_err(|e| e.1)?;
        workspace_bound.extend(all_bound(
            &project,
            &Store::load(&project.out),
            &GenSettings::resolve(&project),
        )?);
    }
    let projects = if let Some(id) = selector {
        vec![workspace.resolve(Some(id)).map_err(|e| e.1)?]
    } else {
        workspace
            .sets
            .iter()
            .map(|set| workspace.resolve(Some(&set.id)).map_err(|e| e.1))
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut groups = Vec::new();
    let mut found = BTreeSet::new();
    for project in projects {
        let store = Store::load(&project.out);
        let gs = GenSettings::resolve(&project);
        let bound = all_bound(&project, &store, &gs)?;
        let selected = paths
            .iter()
            .filter(|path| bound.iter().any(|test| &test.artifact == *path))
            .cloned()
            .collect::<Vec<_>>();
        if selected.is_empty() {
            continue;
        }
        plan(&project, &store, &gs, &selected)?;
        found.extend(selected.iter().cloned());
        groups.push((project, store, gs, selected));
    }
    if paths.is_empty() {
        return Err("Select at least one test artifact".into());
    }
    for path in paths {
        safe_path(path)?;
        if !found.contains(path) {
            return Err(format!(
                "No recorded test binding for {path}. Link the test before running it."
            ));
        }
    }
    let allowed = paths.iter().cloned().collect::<BTreeSet<_>>();
    for (project, store, gs, selected_paths) in &groups {
        let selected = plan(project, store, gs, selected_paths)?;
        let mut runs = selected
            .iter()
            .filter(|test| test.test.kind == "programmatic")
            .map(|test| (test.test.run.clone(), test.execution_dir.clone()))
            .collect::<Vec<_>>();
        if let Some(build) = Ledger::load(&store.out).build {
            runs.push((build.run, execution_dir(gs, &build.cwd)));
        }
        validate_runner_scope(&runs, &workspace_bound, &allowed, selector)?;
    }
    let mut results = Vec::new();
    let mut cache = RunCache::default();
    // Complete every set's build before any shared suite is consumed.
    for (_, store, gs, paths) in &groups {
        if trace.is_cancelled() {
            return Err("Verification cancelled".into());
        }
        prepare_build(store, gs, paths, trace, &mut cache)?;
    }
    for (project, store, gs, paths) in &groups {
        let selected = plan(project, store, gs, paths)?;
        let mut progress = prepare_build(store, gs, paths, trace, &mut cache)?;
        retain_inputs(store, gs, &selected, &mut progress, &mut cache)?;
    }
    for (project, store, gs, paths) in groups {
        if trace.is_cancelled() {
            return Err("Verification cancelled".into());
        }
        trace.line(
            "verify",
            &format!(
                "Selected test artifacts in document set {}",
                project.set_id.as_deref().unwrap_or("main")
            ),
        );
        let runner = make_runner(&project)?;
        let result = run_cached(
            &project,
            &store,
            runner.as_ref(),
            &gs,
            &paths,
            trace,
            &mut cache,
        )?;
        results.push(json!({"setId":project.set_id,"report":result}));
    }
    cache.validate_inputs()?;
    let total = |field: &str| {
        results
            .iter()
            .map(|set| set["report"][field].as_u64().unwrap_or(0))
            .sum::<u64>()
    };
    Ok(
        json!({"rows":total("rows"),"verified":total("verified"),"failing":total("failing"),"skipped":total("skipped"),"sets":results,"testPaths":paths}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Judge(Cell<usize>);
    impl SessionRunner for Judge {
        fn agent_name(&self) -> String {
            "fixture".into()
        }
        fn set_build_token(&self, _: Option<String>) {}
        fn run_item(&self, _: &crate::runner::BatchRun, _: &Trace) -> crate::runner::SessionReport {
            panic!("No agent batch")
        }
        fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
            panic!("Use traced ask")
        }
        fn ask_traced(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: Option<&Trace>,
        ) -> Result<String, String> {
            self.0.set(self.0.get() + 1);
            Ok("PASS: inspected the selected criterion".into())
        }
    }
    fn fixture() -> (Project, Store, GenSettings) {
        let project = Project::load(std::path::Path::new("/project"));
        let (store, gs) =
            super::super::tests::multi_llm_fixture(std::path::Path::new("/project/.jazyk"));
        (project, store, gs)
    }
    fn path(gs: &GenSettings, artifact: &str) -> String {
        gs.deliverable
            .join(artifact)
            .strip_prefix("/project")
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    }
    #[test]
    fn selectors_are_nonempty_explicit_safe_filenames() {
        for body in [
            json!({"testPaths":[]}),
            json!({"testPaths":[1]}),
            json!({"testPaths":["../tests/a.rs"]}),
            json!({"testPaths":["/tests/a.rs"]}),
            json!({"testPaths":["C:/tests/a.rs"]}),
            json!({"testPaths":["tests/a.rs"],"targets":["req:a"]}),
        ] {
            assert!(parse_paths(&body).is_err(), "{body}");
        }
        assert_eq!(parse_paths(&json!({"targets":[]})).unwrap(), None);
        assert_eq!(
            parse_paths(&json!({"testPaths":["tests/deep/a.rs"]})).unwrap(),
            Some(vec!["tests/deep/a.rs".into()])
        );
    }
    #[test]
    fn selected_artifact_does_not_execute_or_verify_sibling_criterion() {
        let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let (project, store, gs) = fixture();
        let mut ledger = Ledger::load(&store.out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        for test in row.tests.values_mut() {
            test.verdict = "none".into();
            test.evidence = Some("old evidence".into());
        }
        row.verdict = "none".into();
        let sibling = serde_json::to_value(&row.tests["crit:second"]).unwrap();
        let selected = path(&gs, &row.tests["crit:first"].test.artifact);
        ledger.save(&store.out).unwrap();
        let judge = Judge(Cell::new(0));
        let result = run(
            &project,
            &store,
            &judge,
            &gs,
            std::slice::from_ref(&selected),
            &Trace::stderr(crate::session::TraceLevel::Quiet),
        )
        .unwrap();
        assert_eq!(result["tests"].as_array().unwrap().len(), 1);
        assert_eq!(judge.0.get(), 1);
        let ledger = Ledger::load(&store.out);
        let row = &ledger.requirements["req:shop-1"];
        assert_eq!(row.tests["crit:first"].verdict, "pass");
        assert_eq!(
            serde_json::to_value(&row.tests["crit:second"]).unwrap(),
            sibling
        );
        assert_ne!(status_of(&store, "req:shop-1", row, &gs).0, "verified");
    }
    #[test]
    fn unbound_selection_fails_before_build_or_model() {
        let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let (project, store, gs) = fixture();
        let mut ledger = Ledger::load(&store.out);
        ledger.build = Some(
            serde_json::from_value(json!({"run":"must-not-run","cwd":".","produces":[]})).unwrap(),
        );
        ledger.save(&store.out).unwrap();
        let judge = Judge(Cell::new(0));
        let selected = path(&gs, "tests/crit-first.md");
        let error = run(
            &project,
            &store,
            &judge,
            &gs,
            &[selected, "tests/unbound.rs".into()],
            &Trace::stderr(crate::session::TraceLevel::Quiet),
        )
        .unwrap_err();
        assert!(error.contains("No recorded test binding"), "{error}");
        assert_eq!(judge.0.get(), 0);
    }
    #[test]
    fn shared_task_outside_selection_is_refused_before_dispatch() {
        let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let (project, store, gs) = fixture();
        let mut ledger = Ledger::load(&store.out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        for test in row.tests.values_mut() {
            test.test.kind = "programmatic".into();
            test.test.run = "task:whole-suite".into();
        }
        row.test = row.tests["crit:first"].test.clone();
        let selected = path(&gs, &row.test.artifact);
        ledger.save(&store.out).unwrap();
        let error = plan(&project, &store, &gs, &[selected]).err().unwrap();
        assert!(
            error.contains("also covers tests outside the selection"),
            "{error}"
        );
    }
    #[test]
    fn path_prefix_does_not_select_another_artifact() {
        let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let (project, store, gs) = fixture();
        let ledger = Ledger::load(&store.out);
        let first = &ledger.requirements["req:shop-1"].tests["crit:first"].test;
        let exact = path(&gs, &first.artifact);
        assert_eq!(
            plan(&project, &store, &gs, std::slice::from_ref(&exact))
                .unwrap()
                .len(),
            1
        );
        assert!(plan(&project, &store, &gs, &[format!("{exact}.backup")]).is_err());
    }
    fn workspace_fixture() -> Vec<Project> {
        let mut prepared = Vec::new();
        for id in ["ds_one", "ds_two"] {
            let (store, gs) = super::super::tests::multi_llm_fixture(std::path::Path::new(
                &format!("/project/.jazyk/{id}"),
            ));
            prepared.push((id, store, gs));
        }
        crate::vfs::write("/project/.jazyk/jazyk.toml", "version=1\n[[sets]]\nid='ds_one'\nname='One'\ndocs=['docs/**']\nsource=['src/**','tests/**']\n[[sets]]\nid='ds_two'\nname='Two'\ndocs=['docs/**']\nsource=['src/**','tests/**']\n[[tasks]]\nid='suite'\nname='Suite'\ndescription='Test cases'\ncommand='must-not-run'\n").unwrap();
        let workspace =
            crate::document_sets::Workspace::load(std::path::Path::new("/project")).unwrap();
        let mut projects = Vec::new();
        for (id, store, old) in prepared {
            let project = workspace.resolve(Some(id)).unwrap();
            let gs = GenSettings::resolve(&project);
            let mut ledger = Ledger::load(&store.out);
            let row = ledger.requirements.get_mut("req:shop-1").unwrap();
            for file in &row.files {
                let bytes = crate::vfs::read(old.deliverable.join(file)).unwrap();
                let dest = gs.deliverable.join(file);
                crate::vfs::create_dir_all(dest.parent().unwrap()).unwrap();
                crate::vfs::write(dest, bytes).unwrap();
            }
            for (cid, test) in &mut row.tests {
                let bytes = crate::vfs::read(artifact_path(&store.out, &old, &test.test)).unwrap();
                test.test.artifact = format!("tests/{id}/{}.md", cid.replace(':', "-"));
                let dest = gs.deliverable.join(&test.test.artifact);
                crate::vfs::create_dir_all(dest.parent().unwrap()).unwrap();
                crate::vfs::write(&dest, bytes).unwrap();
                test.test_hash = hash_file(&dest);
                test.verdict = "none".into();
            }
            row.test = row.tests["crit:first"].test.clone();
            row.hashes.test = row.tests["crit:first"].test_hash.clone();
            row.hashes.files = crate::gen::hash_implementing(&store.out, &gs, &row.files);
            row.verdict = "none".into();
            ledger.save(&store.out).unwrap();
            store.save_for_test();
            projects.push(project);
        }
        projects
    }
    #[test]
    fn workspace_validates_shared_task_before_any_runner_or_outside_evidence_change() {
        let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let projects = workspace_fixture();
        for project in &projects {
            let mut ledger = Ledger::load(&project.out);
            let row = ledger.requirements.get_mut("req:shop-1").unwrap();
            for test in row.tests.values_mut() {
                test.test.kind = "programmatic".into();
                test.test.run = "task:suite".into();
            }
            row.test = row.tests["crit:first"].test.clone();
            ledger.save(&project.out).unwrap();
        }
        let before = serde_json::to_value(Ledger::load(&projects[1].out)).unwrap();
        let calls = Cell::new(0);
        let error = run_workspace(
            std::path::Path::new("/project"),
            Some("ds_one"),
            &[
                "tests/ds_one/crit-first.md".into(),
                "tests/ds_one/crit-second.md".into(),
            ],
            &Trace::stderr(crate::session::TraceLevel::Quiet),
            |_| {
                calls.set(calls.get() + 1);
                Ok(Box::new(Judge(Cell::new(0))))
            },
        )
        .unwrap_err();
        assert!(error.contains("also covers tests outside"), "{error}");
        assert_eq!(calls.get(), 0);
        assert_eq!(
            serde_json::to_value(Ledger::load(&projects[1].out)).unwrap(),
            before
        );
    }
    #[test]
    fn shared_build_task_outside_selection_fails_before_any_runner() {
        let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let projects = workspace_fixture();
        let mut first = Ledger::load(&projects[0].out);
        first.build = Some(
            serde_json::from_value(json!({"run":"task:suite","cwd":".","produces":[]})).unwrap(),
        );
        first.save(&projects[0].out).unwrap();
        let mut second = Ledger::load(&projects[1].out);
        let row = second.requirements.get_mut("req:shop-1").unwrap();
        for test in row.tests.values_mut() {
            test.test.kind = "programmatic".into();
            test.test.run = "task:suite".into();
        }
        row.test = row.tests["crit:first"].test.clone();
        second.save(&projects[1].out).unwrap();
        let error = run_workspace(
            std::path::Path::new("/project"),
            Some("ds_one"),
            &["tests/ds_one/crit-first.md".into()],
            &Trace::stderr(crate::session::TraceLevel::Quiet),
            |_| panic!("No execution before complete preflight"),
        )
        .unwrap_err();
        assert!(error.contains("also covers tests outside"), "{error}");
    }
    #[test]
    fn multiple_sets_execute_sequentially_and_expose_exact_artifact_paths() {
        let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let projects = workspace_fixture();
        let order = std::cell::RefCell::new(Vec::new());
        let paths = vec![
            "tests/ds_one/crit-first.md".into(),
            "tests/ds_two/crit-second.md".into(),
        ];
        let result = run_workspace(
            std::path::Path::new("/project"),
            None,
            &paths,
            &Trace::stderr(crate::session::TraceLevel::Quiet),
            |project| {
                order.borrow_mut().push(project.set_id.clone().unwrap());
                Ok(Box::new(Judge(Cell::new(0))))
            },
        )
        .unwrap();
        assert_eq!(*order.borrow(), vec!["ds_one", "ds_two"]);
        assert_eq!(result["sets"].as_array().unwrap().len(), 2);
        for (index, criterion) in [(0, "crit:first"), (1, "crit:second")] {
            let store = Store::load(&projects[index].out);
            let gs = GenSettings::resolve(&projects[index]);
            let metadata = status_map(&store, &gs);
            assert_eq!(
                metadata["req:shop-1"]["criteria"][criterion]["path"],
                paths[index]
            );
            let row = Ledger::load(&store.out).requirements["req:shop-1"].clone();
            assert_eq!(row.tests[criterion].verdict, "pass");
            assert_ne!(status_of(&store, "req:shop-1", &row, &gs).0, "verified");
        }
    }
    #[test]
    fn suite_dispatch_is_shared_by_distinct_case_names_and_legacy_commands() {
        let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let (_, store, gs) = fixture();
        let ledger = Ledger::load(&store.out);
        let calls = Cell::new(0);
        let mut cache = RunCache::default();
        for name in ["Suite::first", "Suite::second"] {
            let mut test = ledger.requirements["req:shop-1"].test.clone();
            test.kind = "programmatic".into();
            test.run = "task:suite".into();
            test.name = name.into();
            let result=cache.programmatic(&gs,&test,|_|{calls.set(calls.get()+1);Ok(("run-1".into(),json!({"complete":true,"exit_code":0,"tests":[{"identity":"Suite::first","status":"passed"},{"identity":"Suite::second","status":"passed"}]})))},||panic!("Not legacy")).unwrap();
            assert!(result.pass);
        }
        assert_eq!(calls.get(), 1);
        let legacy = Cell::new(0);
        for name in ["first", "second"] {
            let mut test = ledger.requirements["req:shop-1"].test.clone();
            test.kind = "programmatic".into();
            test.run = "scope-tests".into();
            test.name = name.into();
            cache
                .programmatic(
                    &gs,
                    &test,
                    |_| panic!("Not task"),
                    || {
                        legacy.set(legacy.get() + 1);
                        Ok(ProgRun {
                            pass: true,
                            code: 0,
                            tail: "ok".into(),
                            evidence: "observed".into(),
                        })
                    },
                )
                .unwrap();
        }
        assert_eq!(legacy.get(), 1);
    }
    #[test]
    fn later_legacy_command_cannot_rebaseline_or_reuse_an_earlier_pass() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let project = Project::load(root);
        let (mut store, gs) = super::super::tests::multi_llm_fixture(&root.join(".jazyk"));
        let mut ledger = Ledger::load(&store.out);
        let row = ledger.requirements.get_mut("req:shop-1").unwrap();
        let third = row.tests["crit:second"].clone();
        row.tests.insert("crit:third".into(), third);
        let mut criterion = store.graph.criteria["crit:second"].clone();
        criterion.statement = "Check third".into();
        store.graph.criteria.insert("crit:third".into(), criterion);
        for (cid, test) in &mut row.tests {
            test.test.kind = "programmatic".into();
            test.test.name = cid.clone();
            test.test.artifact = format!("tests/{}.rs", cid.replace(':', "-"));
            test.test.run = if cid == "crit:second" {
                "printf changed > src/cart.rs"
            } else {
                "printf x >> runs.log"
            }
            .into();
            test.criterion_hash = hash_hex(&store.graph.criteria[cid].statement);
            test.verdict = "none".into();
            test.evidence = None;
            crate::vfs::write(
                gs.deliverable.join(&test.test.artifact),
                format!("// {cid}"),
            )
            .unwrap();
            test.test_hash = hash_file(&artifact_path(&store.out, &gs, &test.test));
        }
        row.test = row.tests["crit:first"].test.clone();
        row.hashes.test = row.tests["crit:first"].test_hash.clone();
        row.verdict = "none".into();
        let old = row.hashes.files.clone();
        let paths = row
            .tests
            .values()
            .map(|test| {
                gs.deliverable
                    .join(&test.test.artifact)
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect::<Vec<_>>();
        ledger.save(&store.out).unwrap();
        let result = run(
            &project,
            &store,
            &Judge(Cell::new(0)),
            &gs,
            &paths,
            &Trace::stderr(crate::session::TraceLevel::Quiet),
        );
        assert!(
            result.is_err(),
            "Changed inputs received cached PASS: {result:?}"
        );
        let row = Ledger::load(&store.out).requirements["req:shop-1"].clone();
        assert_eq!(
            row.hashes.files, old,
            "Never stamp an old observation with new implementation bytes"
        );
        assert_eq!(row.tests["crit:third"].verdict, "none");
        assert_eq!(
            crate::vfs::read_to_string(gs.deliverable.join("runs.log")).unwrap(),
            "x"
        );
        assert_ne!(status_of(&store, "req:shop-1", &row, &gs).0, "verified");
    }
    #[test]
    fn suspended_non_task_observation_cannot_rebaseline_changed_implementation() {
        struct SuspendSecond(Cell<usize>);
        impl SessionRunner for SuspendSecond {
            fn agent_name(&self) -> String {
                "fixture".into()
            }
            fn set_build_token(&self, _: Option<String>) {}
            fn run_item(
                &self,
                _: &crate::runner::BatchRun,
                _: &Trace,
            ) -> crate::runner::SessionReport {
                panic!("No batch")
            }
            fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
                panic!("Use traced ask")
            }
            fn ask_traced(
                &self,
                _: &str,
                _: &str,
                _: &str,
                _: &str,
                _: Option<&Trace>,
            ) -> Result<String, String> {
                self.0.set(self.0.get() + 1);
                if self.0.get() == 1 {
                    Ok("PASS: inspected original implementation".into())
                } else {
                    Err("suspended".into())
                }
            }
        }
        for mutation in [
            "implementation",
            "artifact",
            "criterion",
            "requirement",
            "context",
            "missing-epoch",
            "unchanged",
        ] {
            let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
            let (project, mut store, gs) = fixture();
            let mut ledger = Ledger::load(&store.out);
            let row = ledger.requirements.get_mut("req:shop-1").unwrap();
            for test in row.tests.values_mut() {
                test.verdict = "none".into();
                test.evidence = None;
            }
            row.verdict = "none".into();
            let paths = row
                .tests
                .values()
                .map(|test| path(&gs, &test.test.artifact))
                .collect::<Vec<_>>();
            ledger.save(&store.out).unwrap();
            let signature = json!({"paths":paths,"settings":gs});
            let key = crate::gen::progress::command_key("verify-artifacts", &store.out, &signature);
            let mut progress = {
                let _recovery = crate::session_recovery::Scope::enter(None);
                let error = run(
                    &project,
                    &store,
                    &SuspendSecond(Cell::new(0)),
                    &gs,
                    &paths,
                    &Trace::stderr(crate::session::TraceLevel::Quiet),
                )
                .unwrap_err();
                assert_eq!(error, "suspended");
                crate::session_recovery::parent(&key).unwrap().unwrap()
            };
            assert_eq!(progress["data"]["results"].as_object().unwrap().len(), 1);
            if mutation == "missing-epoch" {
                progress["data"].as_object_mut().unwrap().remove("inputs");
            }
            let capsule = json!({"version":1,"gdkVersion":"0.1.0-alpha.13","operationSet":"jazyk-gdk-3","frames":[],"parent":{key:progress},"claims":[],"deliveredInputs":[]});
            crate::session_recovery::validate(&capsule).unwrap();
            // Shared engine consumers restore owned snapshots, not necessarily Cloud's revision-fenced snapshot.
            let old = Ledger::load(&store.out).requirements["req:shop-1"]
                .hashes
                .files
                .clone();
            match mutation {
                "implementation" => crate::vfs::write(
                    gs.deliverable.join("src/cart.rs"),
                    "// changed after suspension",
                )
                .unwrap(),
                "artifact" => {
                    let artifact = artifact_path(
                        &store.out,
                        &gs,
                        &Ledger::load(&store.out).requirements["req:shop-1"].tests["crit:first"]
                            .test,
                    );
                    let text = crate::vfs::read_to_string(&artifact).unwrap();
                    crate::vfs::write(artifact, format!("{text}\nChanged observable condition.\n"))
                        .unwrap();
                }
                "criterion" => {
                    store
                        .graph
                        .criteria
                        .get_mut("crit:first")
                        .unwrap()
                        .statement = "A rebound criterion".into();
                    let mut ledger = Ledger::load(&store.out);
                    let test = ledger
                        .requirements
                        .get_mut("req:shop-1")
                        .unwrap()
                        .tests
                        .get_mut("crit:first")
                        .unwrap();
                    test.criterion_hash = hash_hex(&store.graph.criteria["crit:first"].statement);
                    let artifact = artifact_path(&store.out, &gs, &test.test);
                    crate::vfs::write(artifact,format!("---\nrequirement: req:shop-1\ncriterion: crit:first\nhash: {}\n---\nInspect source. PASS if satisfied. FAIL otherwise.\n",test.criterion_hash)).unwrap();
                    ledger.save(&store.out).unwrap();
                }
                "requirement" => {
                    store
                        .graph
                        .requirements
                        .get_mut("req:shop-1")
                        .unwrap()
                        .statement = "The Cart shall use the new behavior.".into();
                    let mut ledger = Ledger::load(&store.out);
                    ledger
                        .requirements
                        .get_mut("req:shop-1")
                        .unwrap()
                        .hashes
                        .requirement = hash_hex(&store.graph.requirements["req:shop-1"].statement);
                    ledger.save(&store.out).unwrap();
                }
                "context" => {
                    store.graph.entities.get_mut("ent:cart").unwrap().name =
                        "Changed context".into()
                }
                "missing-epoch" | "unchanged" => {}
                _ => unreachable!(),
            }
            let before = serde_json::to_value(Ledger::load(&store.out)).unwrap();
            let judge = Judge(Cell::new(0));
            let _recovery = crate::session_recovery::Scope::enter(Some(&capsule));
            let resumed = run(
                &project,
                &store,
                &judge,
                &gs,
                &paths,
                &Trace::stderr(crate::session::TraceLevel::Quiet),
            );
            if mutation == "unchanged" {
                assert!(
                    resumed.is_ok(),
                    "Unchanged input should resume: {resumed:?}"
                );
                assert_eq!(
                    judge.0.get(),
                    1,
                    "Reuse only the already observed criterion"
                );
                continue;
            }
            assert!(
                resumed.is_err(),
                "{mutation}: a retained PASS was applied to changed input: {resumed:?}"
            );
            assert_eq!(judge.0.get(), 0, "Must refuse before new judgments");
            assert_eq!(
                serde_json::to_value(Ledger::load(&store.out)).unwrap(),
                before
            );
            assert_eq!(
                Ledger::load(&store.out).requirements["req:shop-1"]
                    .hashes
                    .files,
                old
            );
        }
    }
    #[test]
    fn later_set_build_mutation_cannot_reuse_prior_shared_suite_observation() {
        let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let projects = workspace_fixture();
        let task: crate::task_runtime::Task = serde_json::from_value(
            json!({"id":"suite","name":"Suite","description":"Selected cases","command":"fixture"}),
        )
        .unwrap();
        for project in &projects {
            let store = Store::load(&project.out);
            let gs = GenSettings::resolve(project);
            let mut ledger = Ledger::load(&project.out);
            let row = ledger.requirements.get_mut("req:shop-1").unwrap();
            for test in row.tests.values_mut() {
                test.test.kind = "programmatic".into();
                test.test.run = "task:suite".into();
            }
            row.test = row.tests["crit:first"].test.clone();
            ledger.save(&project.out).unwrap();
            crate::task_runtime::import_evidence(
                &store,
                &gs,
                &task,
                &json!({"complete":true,"exit_code":0,"tests":[{"identity":"crit:first","status":"passed"},{"identity":"crit:second","status":"passed"}]}),
                "shared-run",
            );
        }
        let store = Store::load(&projects[1].out);
        let gs = GenSettings::resolve(&projects[1]);
        let selected = plan(
            &projects[1],
            &store,
            &gs,
            &[
                "tests/ds_two/crit-first.md".into(),
                "tests/ds_two/crit-second.md".into(),
            ],
        )
        .unwrap();
        assert_eq!(
            imported_task_observation(&store, &gs, &selected[0], "shared-run")
                .unwrap()
                .verdict,
            "pass"
        );
        let old = Ledger::load(&store.out).requirements["req:shop-1"]
            .hashes
            .files
            .clone();
        // A later build changes implementation, not the test bytes or receipt ID.
        crate::vfs::write(
            gs.deliverable.join("src/cart.rs"),
            "// Later build changed the implementation",
        )
        .unwrap();
        assert!(imported_task_observation(&store, &gs, &selected[0], "shared-run").is_none());
        let row = Ledger::load(&store.out).requirements["req:shop-1"].clone();
        assert_eq!(row.hashes.files, old);
        assert_ne!(status_of(&store, "req:shop-1", &row, &gs).0, "verified");
    }
}
