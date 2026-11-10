use super::*;

// ---- the built-in worker ----
// Moved out of the CLI so the GUI job runner drives the same loop. The CLI wrapper
// renders the worker events on the historical output format; the GUI streams them.

// Run generation over the given entities (all entities with requirements when empty),
// leaf-first over the relationship graph, skipping entities whose facts are unchanged
// unless forced. Returns {regenerated, skipped, failures}.
#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn run_all(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    entities: &[String],
    force: bool,
    trace: &crate::session::Trace,
) -> Result<Value, String> {
    run_selected(store, runner, gs, entities, force, trace, true).await
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct GenCommand {
    groups: Option<Vec<(String, Vec<String>)>>,
    pending: std::collections::BTreeSet<String>,
    cursor: usize,
    active: Option<GenGroup>,
    medium_decided: bool,
    regenerated: u64,
    skipped: u64,
    failures: u64,
    streak: usize,
    stopped: Option<String>,
    #[serde(default)]
    pub(super) first_error: Option<String>,
    result: Option<Value>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct GenGroup {
    root: String,
    ready: Vec<String>,
    pub(super) pipeline_cursor: usize,
    pipeline_task: Option<Value>,
    pub(super) pipeline_results: Vec<(String, Result<usize, String>)>,
    #[serde(default)]
    pub(super) pipeline_failed: Option<String>,
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub(crate) async fn run_selected(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    entities: &[String],
    force: bool,
    trace: &crate::session::Trace,
    build_product: bool,
) -> Result<Value, String> {
    use crate::session::TraceEvent;
    let admitted = progress::admitted_settings(
        "generate",
        &store.out,
        json!({"entities":entities,"force":force,"buildProduct":build_product}),
        gs,
    )
    .await?;
    let signature = json!({"settings": admitted, "entities": entities, "force": force, "buildProduct": build_product});
    let key = progress::command_key("generate", &store.out, &signature);
    let mut command = progress::Progress::load(key, signature, GenCommand::default)?;
    if let Some(result) = &command.data.result {
        return Ok(result.clone());
    }
    if command.data.groups.is_none() {
        let targets: Vec<String> = if entities.is_empty() {
            store
                .graph
                .entities
                .keys()
                .filter(|id| !store.requirements_referencing(id).is_empty())
                .cloned()
                .collect()
        } else {
            let mut targets = Vec::new();
            for entity in entities {
                let id = store.resolve_id(entity).to_string();
                if !store.graph.entities.contains_key(&id) {
                    return Err(format!("unknown entity `{entity}`"));
                }
                targets.push(id);
            }
            targets
        };
        if targets.is_empty() {
            return Err("no entities with requirements; run `jazyk compile` first".into());
        }
        command.data.groups = Some(groups_in_order(store, &generation_order(store, &targets)));
        command.data.pending = pending(store, gs)
            .iter()
            .filter_map(|item| item["entity"].as_str().map(String::from))
            .collect();
        command.save().await?;
    }
    // Medium selection is a command step. An empty ledger on resume does not
    // start this decision again after a later group has already begun.
    if !command.data.medium_decided {
        // The decision is a model call, made outside the ledger lock; it lands on a
        // fresh load unless another writer decided first.
        if Ledger::load(&store.out).medium.is_none() {
            let medium = decide_medium(store, runner, &gs.deliverable).await?;
            trace.line("gen medium", &medium.line());
            Ledger::update(&store.out, |ledger| {
                ledger.medium.get_or_insert(medium);
                Ok(())
            })?;
        }
        command.data.medium_decided = true;
        command.save().await?;
    }
    crate::vfs::create_dir_all(&gs.deliverable).ok();
    while command.data.cursor < command.data.groups.as_ref().unwrap().len() {
        let current_settings = gs.current_for_store(store);
        let gs = &current_settings;
        if command.data.stopped.is_some() {
            break;
        }
        if trace.is_cancelled() {
            break;
        }
        if crate::session_recovery::suspension_requested() {
            command.save().await?;
            return Err("suspended".into());
        }
        if command.data.active.is_none() {
            let (root, members) =
                command.data.groups.as_ref().unwrap()[command.data.cursor].clone();
            let mut ready = Vec::new();
            for id in members {
                if !force && !command.data.pending.contains(&id) {
                    command.data.skipped += 1;
                    trace.event(TraceEvent::GenEntitySkipped {
                        entity: id.clone(),
                        reason: skip_reason(store, gs, &id),
                    });
                } else {
                    ready.push(id);
                }
            }
            if ready.is_empty() {
                command.data.cursor += 1;
                command.save().await?;
                continue;
            }
            command.data.active = Some(GenGroup {
                root,
                ready,
                pipeline_cursor: 0,
                pipeline_task: None,
                pipeline_results: Vec::new(),
                pipeline_failed: None,
            });
            command.save().await?;
        }
        let active = command.data.active.as_ref().unwrap();
        let (root, ready) = (active.root.clone(), active.ready.clone());
        for id in &ready {
            trace.event(TraceEvent::GenEntityStart { entity: id.clone() });
        }
        let outcome = if gs.worker == "pipeline" {
            while command.data.active.as_ref().unwrap().pipeline_cursor < ready.len()
                && command
                    .data
                    .active
                    .as_ref()
                    .unwrap()
                    .pipeline_failed
                    .is_none()
                && !command
                    .data
                    .active
                    .as_ref()
                    .unwrap()
                    .pipeline_results
                    .iter()
                    .any(|(_, result)| result.is_err())
            {
                let cursor = command.data.active.as_ref().unwrap().pipeline_cursor;
                let id = &ready[cursor];
                if trace.is_cancelled() {
                    break;
                }
                if crate::session_recovery::suspension_requested() {
                    command.save().await?;
                    return Err("suspended".into());
                }
                if command
                    .data
                    .active
                    .as_ref()
                    .unwrap()
                    .pipeline_task
                    .is_none()
                {
                    match task_package(store, id, gs) {
                        Ok(task) => {
                            command.data.active.as_mut().unwrap().pipeline_task = Some(task);
                            command.save().await?;
                        }
                        Err(error) => {
                            let active = command.data.active.as_mut().unwrap();
                            active.pipeline_results.push((id.clone(), Err(error)));
                            active.pipeline_cursor += 1;
                            command.save().await?;
                            continue;
                        }
                    }
                }
                let task = command
                    .data
                    .active
                    .as_ref()
                    .unwrap()
                    .pipeline_task
                    .as_ref()
                    .unwrap()
                    .clone();
                let outcome = pipeline::run_report(store, runner, gs, id, &task).await?;
                let result = outcome.result;
                if let Err(reason) = &result {
                    if crate::session_recovery::is_interruption(reason) {
                        return Err(reason.clone());
                    }
                }
                let active = command.data.active.as_mut().unwrap();
                active.pipeline_failed = outcome.failed;
                active.pipeline_results.push((id.clone(), result));
                active.pipeline_cursor += 1;
                active.pipeline_task = None;
                command.save().await?;
            }
            GenSessionOutcome {
                results: command
                    .data
                    .active
                    .as_ref()
                    .unwrap()
                    .pipeline_results
                    .clone(),
                tokens: 0,
                failed: command
                    .data
                    .active
                    .as_ref()
                    .unwrap()
                    .pipeline_failed
                    .clone(),
            }
        } else {
            gen_session(store, runner, &admitted, &root, &ready, trace).await
        };
        if let Some(reason) = outcome
            .failed
            .as_ref()
            .filter(|reason| crate::session_recovery::is_interruption(reason))
        {
            return Err(reason.clone());
        }
        let (next, _) = crate::reconcile::breaker_step(
            command.data.streak,
            outcome.tokens,
            outcome.failed.as_deref(),
        );
        command.data.streak = next;
        let incomplete: Vec<_> = ready
            .iter()
            .filter_map(|id| {
                match outcome
                    .results
                    .iter()
                    .find(|(target, _)| target == id)
                    .map(|(_, result)| result)
                {
                    Some(Ok(_)) => None,
                    Some(Err(error)) => Some((id.clone(), error.clone())),
                    None => Some((
                        id.clone(),
                        outcome.failed.clone().unwrap_or_else(|| {
                            "The group ended before completing this target".into()
                        }),
                    )),
                }
            })
            .collect();
        for (id, result) in outcome.results {
            match result {
                Ok(files) => {
                    trace.event(TraceEvent::GenEntityDone { entity: id, files });
                    command.data.regenerated += 1;
                }
                Err(error) => {
                    if outcome.failed.is_none() {
                        command.data.first_error.get_or_insert_with(|| {
                            format!(
                                "{} was not completed: {error}",
                                store
                                    .graph
                                    .entities
                                    .get(&id)
                                    .map(|entity| entity.name.as_str())
                                    .filter(|name| !name.is_empty())
                                    .unwrap_or("This part of the project")
                            )
                        });
                    }
                    trace.event(TraceEvent::GenEntityFailed {
                        entity: id,
                        stage: "generate".into(),
                        error,
                    });
                    command.data.failures += 1;
                }
            }
        }
        command.data.active = None;
        command.data.cursor += 1;
        crate::session_recovery::remove_frame(&crate::session_recovery::batch_key(
            &store.out,
            &format!("g:generate:{root}"),
        ));
        if !incomplete.is_empty() {
            let name = |id: &str| {
                store
                    .graph
                    .entities
                    .get(id)
                    .map(|entity| entity.name.as_str())
                    .filter(|name| !name.is_empty())
                    .unwrap_or(id)
                    .to_owned()
            };
            let owed = incomplete
                .iter()
                .map(|(id, _)| name(id))
                .collect::<Vec<_>>()
                .join(", ");
            let reason = format!(
                "Generation group {} is incomplete. Still owed: {owed}. {}",
                name(&root),
                command
                    .data
                    .first_error
                    .as_deref()
                    .unwrap_or(&incomplete[0].1)
            );
            command.data.first_error = Some(reason.clone());
            command.data.stopped = Some(outcome.failed.unwrap_or(reason));
        } else if outcome.failed.is_some() {
            command.data.stopped = outcome.failed;
        }
        command.save().await?;
        if command.data.stopped.is_some() {
            break;
        }
    }
    let build = if !build_product || trace.is_cancelled() {
        json!({"state":"not-selected"})
    } else if command.data.failures > 0 || command.data.stopped.is_some() {
        json!({"state":"not-run", "reason":"generation is incomplete"})
    } else if crate::vfs::is_scoped() {
        json!({"state":"pending-ci", "pendingCi": Ledger::load(&store.out).build.is_some(), "reason":"build commands run through delivery CI"})
    } else {
        match run_build(&store.out, gs, trace, "gen") {
            Ok(()) => {
                let produced = Ledger::load(&store.out)
                    .build
                    .map(|build| build.produces.join(", "))
                    .filter(|paths| !paths.is_empty());
                if let Some(paths) = &produced {
                    trace.line("gen", &format!("built {paths}"));
                }
                json!({"ok": true, "produced": produced})
            }
            Err(error) => {
                if crate::session_recovery::is_interruption(&error) {
                    return Err(error);
                }
                trace.line("gen", &error);
                json!({"ok": false, "error": error})
            }
        }
    };
    let mut result = json!({"regenerated":command.data.regenerated,"skipped":command.data.skipped,
        "failures":command.data.failures,"stopped":command.data.stopped,"build":build});
    if let Some(error) = &command.data.first_error {
        result["error"] = json!(error);
    }
    command.data.result = Some(result.clone());
    command.save().await?;
    Ok(result)
}

// The agentic worker: one component group as one generation session on the harness,
// with the file and command tools. The batch carries one generate goal per ready
// member, so the parts of one component are written together under one set of
// conventions (docs/consumers/gen.md#grouping-by-component). Success is the ledger's
// word, never the model's: the session must have left record_generation's mark with
// current facts and existing files for each member.
// Mirrors docs/compiler/sessions.md#generation-sessions.
#[derive(Serialize, Deserialize)]
pub(super) struct GenSessionState {
    session: progress::BatchProgress,
    before: Vec<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) before_sequences: Option<Vec<Option<u64>>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    completed: std::collections::BTreeSet<usize>,
    #[serde(default, skip_serializing_if = "record_sequence_is_zero")]
    prior_tokens: u64,
    result: Option<GenSessionOutcome>,
}

impl GenSessionState {
    pub(super) fn recorded(&self, index: usize, row: Option<&EntityGen>) -> bool {
        if self.completed.contains(&index) {
            return true;
        }
        match &self.before_sequences {
            Some(sequences) => sequences.get(index).is_some_and(|before| {
                row.is_some_and(|row| row.record_sequence > before.unwrap_or(0))
            }),
            None => row.and_then(|row| row.recorded_at.as_ref()) != self.before[index].as_ref(),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub(super) async fn gen_session(
    store: &Store,
    runner: &dyn crate::runner::SessionRunner,
    gs: &GenSettings,
    root: &str,
    ids: &[String],
    trace: &crate::session::Trace,
) -> GenSessionOutcome {
    let error = |reason: String| GenSessionOutcome {
        results: Vec::new(),
        tokens: 0,
        failed: Some(reason),
    };
    // The admitted group keeps the pre-input fingerprint identity. Referenced
    // bytes can change while a run is suspended, or become dependencies after a
    // release. They belong to its current gate, never its recovery lookup key.
    let signature = json!({"settings":gs,"root":root,"ids":ids,
        "facts":ids.iter().map(|id| fact_hash_base(store,id,&reqs_of_sorted(store,id))).collect::<Vec<_>>()});
    let key = progress::command_key("generation-session", &store.out, &signature);
    let mut saved = match progress::Progress::load(key, signature, || {
        let batch = crate::runner::BatchRun {
            id: format!("g:generate:{root}"),
            executor: None,
            goals: ids
                .iter()
                .map(|id| crate::model::Goal {
                    id: format!("g:generate:{id}"),
                    kind: "generate".into(),
                    class: "compile".into(),
                    mandatory: true,
                    target: id.clone(),
                    unit: "entity".into(),
                    change: json!({"goal":"generate"}),
                    ..Default::default()
                })
                .collect(),
        };
        let ledger = Ledger::load(&store.out);
        GenSessionState {
            session: progress::BatchProgress::new(batch, &store.out),
            before: ids
                .iter()
                .map(|id| {
                    ledger
                        .entities
                        .get(crate::ids::entity_slug(id))
                        .and_then(|row| row.recorded_at.clone())
                })
                .collect(),
            before_sequences: Some(
                ids.iter()
                    .map(|id| {
                        ledger
                            .entities
                            .get(crate::ids::entity_slug(id))
                            .map(|row| row.record_sequence)
                    })
                    .collect(),
            ),
            completed: Default::default(),
            prior_tokens: 0,
            result: None,
        }
    }) {
        Ok(saved) => saved,
        Err(reason) => return error(reason),
    };
    if let Some(result) = &saved.data.result {
        return result.clone();
    }
    if let Err(reason) = saved.save().await {
        return error(reason);
    }
    loop {
        let report = match saved.data.session.report.clone() {
            Some(report) => report,
            None => match saved.data.session.committed_report(&store.out) {
                Some(report) => report,
                None => runner.run_item(&saved.data.session.batch, trace).await,
            },
        };
        if let Some(reason) = progress::interruption(&report) {
            return error(reason);
        }
        saved.data.session.report = Some(report.clone());
        if let Err(reason) = saved.save().await {
            return error(reason);
        }
        let after = Ledger::load(&store.out);
        let current_settings = gs.current_for_store(store);
        let results = ids
            .iter()
            .zip(&saved.data.before)
            .enumerate()
            .map(|(index, (id, _))| {
                let result = if !saved
                    .data
                    .recorded(index, after.entities.get(crate::ids::entity_slug(id)))
                {
                    Err(report
                        .failed
                        .clone()
                        .or_else(|| report.stopped.clone())
                        .unwrap_or_else(|| {
                            "The agent ended its session before recording completion".into()
                        }))
                } else {
                    gate(store, &current_settings, id, false)
                        .map_err(|error| report.failed.clone().unwrap_or(error))
                };
                (id.clone(), result)
            })
            .collect::<Vec<_>>();
        // A pre-input-fingerprint worker may have committed just before suspension.
        // Its receipt survives, but its old base hash cannot prove the supplied input
        // was used. Continue only those newly stale members in a fresh current batch.
        // Save the transition before admitting any replacement tool/model operation.
        if saved.data.before_sequences.is_none()
            && report.failed.is_none()
            && report.stopped.is_none()
        {
            let failed: Vec<usize> = results
                .iter()
                .enumerate()
                .filter_map(|(index, (_, result))| result.is_err().then_some(index))
                .collect();
            let refresh: Vec<usize> = failed
                .iter()
                .copied()
                .filter(|index| {
                    let id = &ids[*index];
                    let rids = reqs_of_sorted(store, id);
                    let base = fact_hash_base(store, id, &rids);
                    after
                        .entities
                        .get(crate::ids::entity_slug(id))
                        .is_some_and(|row| row.fact_hash == base)
                        && assets::referenced_assets(store, id, &rids)
                            .iter()
                            .any(|asset| asset.is_input())
                        && fact_hash(store, id) != base
                })
                .collect();
            if !refresh.is_empty() && refresh.len() == failed.len() {
                saved.data.completed.extend(
                    results
                        .iter()
                        .enumerate()
                        .filter_map(|(index, (_, result))| result.is_ok().then_some(index)),
                );
                let targets: std::collections::BTreeSet<&str> =
                    refresh.iter().map(|index| ids[*index].as_str()).collect();
                let mut batch = saved.data.session.batch.clone();
                batch
                    .goals
                    .retain(|goal| targets.contains(goal.target.as_str()));
                saved.data.before = ids
                    .iter()
                    .map(|id| {
                        after
                            .entities
                            .get(crate::ids::entity_slug(id))
                            .and_then(|row| row.recorded_at.clone())
                    })
                    .collect();
                saved.data.before_sequences = Some(
                    ids.iter()
                        .map(|id| {
                            after
                                .entities
                                .get(crate::ids::entity_slug(id))
                                .map(|row| row.record_sequence)
                        })
                        .collect(),
                );
                saved.data.prior_tokens = saved.data.prior_tokens.saturating_add(report.tokens);
                saved.data.session = progress::BatchProgress::new(batch, &store.out);
                crate::session_recovery::remove_frame(&crate::session_recovery::batch_key(
                    &store.out,
                    &saved.data.session.batch.id,
                ));
                if let Err(reason) = saved.save().await {
                    return error(reason);
                }
                continue;
            }
        }
        let failed = if results.iter().any(|(_, result)| result.is_err()) {
            report.failed
        } else {
            None
        };
        let outcome = GenSessionOutcome {
            results,
            tokens: saved.data.prior_tokens.saturating_add(report.tokens),
            failed,
        };
        saved.data.result = Some(outcome.clone());
        if let Err(reason) = saved.save().await {
            return error(reason);
        }
        return outcome;
    }
}

// What one generate session came to: the per-entity results, and what the session
// spent and how it failed, for the run's dead-endpoint breaker.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct GenSessionOutcome {
    pub(super) results: Vec<(String, Result<usize, String>)>,
    tokens: u64,
    pub(super) failed: Option<String>,
}
