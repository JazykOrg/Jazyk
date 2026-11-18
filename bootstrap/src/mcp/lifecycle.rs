use super::*;

impl McpServer {
    // ---- the compilation lifecycle ----

    // The serving's identity in leases: the running build's token when this serving
    // is part of one, its registration id otherwise, or a pid-scoped fallback when
    // the client never sent initialize.
    pub(super) fn worker_id(&self) -> String {
        if let Some(t) = &self.bridge.build_token {
            return t.clone();
        }
        self.worker
            .lock()
            .unwrap()
            .as_ref()
            .map(|h| h.id().to_string())
            .unwrap_or_else(|| format!("agent-{}-{}", crate::sys::process_id(), self.serving_seq))
    }

    // The `goals` tool: the board with readiness sentences, gated and claimedBy, the
    // batches the scheduler would form, and the verdict with its counts when nothing
    // is open. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    pub(super) fn goals_answer(&self, full: bool) -> Value {
        let mut board = crate::board::Board::compute(&self.current_project(), &self.out);
        // An empty board with a stale `incomplete` verdict settles here: the tail is
        // deterministic and idempotent, and a lister that finds nothing to do may as
        // well say so truthfully. A dangling judged diagnostic settles the same way.
        if board.open_mandatory() == 0
            && (!board.verdict.converged() || board.dangling_diags)
            && self.open.lock().unwrap().is_none()
        {
            let mut s = Store::load(&self.out);
            let quiet = crate::session::Trace::stderr(crate::session::TraceLevel::Quiet);
            crate::reconcile::finalize(
                &mut s,
                &self.current_project(),
                Vec::new(),
                &crate::model::Costs::default(),
                &quiet,
            );
            board = crate::board::Board::compute(&self.current_project(), &self.out);
        }
        board.mark_own(&self.worker_id());
        let mut v = if full {
            board.answer()
        } else {
            board.answer_compact()
        };
        if self.bridge.only.is_some() {
            if let Some(fin) = self.finished.lock().unwrap().as_ref() {
                v["note"] = json!(format!(
                    "this session is complete (batch `{}` committed); goals listed here belong to the next session",
                    fin
                ));
            }
        }
        if let Some(o) = self.open.lock().unwrap().as_ref() {
            v["openBatch"] = json!(format!(
                "batch `{}` is already open; done or abandon_goals first",
                o.id
            ));
        }
        v
    }

    // Claim a goal batch: the named batch, the named goals when one batch holds them
    // all (one locality, one executor), the --only scope, or the next ready batch.
    // Opens the changeset and returns the assembled session prompt as `instructions`
    // and the initially loaded set as `package`.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    // A scoped serving whose batch already committed: the session's work is done, and
    // every later write, goal tool, done, or abandon says so instead of the
    // `no-open-batch` refusal that sends an agent back to begin_goals.
    // Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
    pub(super) fn session_complete(&self) -> Option<Value> {
        if self.open.lock().unwrap().is_some() {
            return None;
        }
        self.session_complete_closed()
    }

    // The same answer for a caller that already holds the `open` lock and found no
    // batch: locking it again would deadlock.
    pub(super) fn session_complete_closed(&self) -> Option<Value> {
        self.bridge.only.as_ref()?;
        let fin = self.finished.lock().unwrap().clone()?;
        Some(
            json!({"error": {"rule": "session-complete", "message": format!(
            "this session is complete: batch `{}` committed at generation {}. Nothing is left to stage, mark, or commit in this session; end your turn now",
            fin, crate::store::read_generation(&self.out))}}),
        )
    }

    pub(super) fn begin_goals(&self, params: &Value) -> Value {
        if let Some(o) = self.open.lock().unwrap().as_ref() {
            return json!({"error": {"rule": "batch-open", "message": format!(
                "batch `{}` is already open with {} staged mutation(s); done or abandon_goals first",
                o.id, o.session.staged.len())}});
        }
        // A scoped serving serves exactly one batch. Once it has committed, the goals
        // now open on the board belong to the next session; say so instead of the
        // wrong-target/stale refusal pair a confused agent bounces between.
        if self.bridge.only.is_some() {
            if let Some(fin) = self.finished.lock().unwrap().as_ref() {
                return json!({"error": {"rule": "session-complete", "message": format!(
                    "this session is complete: batch `{}` committed. Goals reopened since belong to the next session; finish without claiming more", fin)}});
            }
        }
        // The snapshot and the board derive from the same synced store.
        let mut store = Store::load(&self.out);
        let project = self.current_project();
        let (parsed, _) = crate::reconcile::parse_all(&project);
        store.sync_docs(&parsed);
        let control = crate::control::Control::load(&project, &self.out);
        let board = crate::board::Board::derive(&store, &project, &control);
        let args = &params["arguments"];
        let wanted_batch = args["batch"].as_str();
        let wanted_goals: Vec<String> = args["goals"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        // A scoped serving of a concurrent build's session takes the batch the build
        // opened under its id, with the goals the build registered that are still
        // open: a board re-derived since another session's commit numbers its
        // batches anew. Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
        let registered: Option<crate::board::Batch> = self.bridge.only.as_ref().and_then(|only| {
            if wanted_batch.is_some_and(|b| b != only) {
                return None;
            }
            let entry = crate::control::leases(&self.out)
                .into_iter()
                .find(|l| &l.task == only && !l.goals.is_empty())?;
            if !wanted_goals.iter().all(|g| entry.goals.contains(g)) {
                return None;
            }
            let goals: Vec<String> = entry.goals.into_iter().filter(|g| board.open(g)).collect();
            let first = board.goal(goals.first()?)?;
            Some(crate::board::Batch {
                id: only.clone(),
                class: crate::goals::Class::parse(&first.class),
                tier: crate::goals::tier(&first.kind),
                executor: project
                    .executors
                    .resolve(&first.kind, &first.class)
                    .map(String::from),
                locality: String::new(),
                goals,
            })
        });
        let selected = if let Some(b) = registered.as_ref() {
            Some(b)
        } else if let Some(bid) = wanted_batch {
            board.batches.iter().find(|b| b.id == bid)
        } else if !wanted_goals.is_empty() {
            // Named goals form one batch only when one scheduled batch already holds
            // them all: same locality, same tier, executors agreeing.
            board
                .batches
                .iter()
                .find(|b| wanted_goals.iter().all(|g| b.goals.contains(g)))
        } else if let Some(only) = &self.bridge.only {
            board
                .batches
                .iter()
                .find(|b| b.id == *only || b.goals.iter().any(|g| g == only))
        } else {
            board.batches.first()
        };
        let Some(batch) = selected else {
            let mut v = board.answer();
            v["error"] = json!({"rule": "no-ready-batch", "message": match (wanted_batch, wanted_goals.is_empty()) {
                (Some(b), _) => format!("`{}` is not a batch on the current board; pick from the batches above", b),
                (None, false) => include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-lifecycle/the-named-goals-do-not-share-one-86eca.md")).to_string(),
                _ => "no batch is ready; the board above says why".to_string(),
            }});
            return v;
        };
        let batch = batch.clone();
        let enabled = self.enabled_tools();
        if let Some(kind) = batch
            .goals
            .iter()
            .filter_map(|id| board.goal(id))
            .find_map(|goal| {
                crate::goals::kind(&goal.kind)
                    .filter(|kind| kind.toolset().iter().any(|tool| !enabled.contains(tool)))
                    .map(|_| goal.kind.as_str())
            })
        {
            return json!({"error":{"rule":"wrong-toolset","message":format!(
                "this serving cannot execute the {kind} batch; connect the generate, verify or all toolset for that work")}});
        }
        // A bridge serving scoped to one batch (or one of its goals) refuses
        // everything else, so a confused agent cannot claim the rest of the board.
        if let Some(only) = &self.bridge.only {
            if batch.id != *only && !batch.goals.iter().any(|g| g == only) {
                return json!({"error": {"rule": "wrong-target", "message": format!(
                    "this serving is scoped to `{}`; batch `{}` belongs to another session", only, batch.id)}});
            }
        }
        // A gated batch awaits its release; a serving carrying the build's own token is
        // approved by the build's command. Nothing else refuses: another worker's open
        // batch or a running build never blocks opening work, and commits synchronize
        // them. Mirrors docs/compiler/control-plane.md#workers-and-open-work.
        if self.bridge.build_token.is_none()
            && batch.goals.iter().any(|id| board.gated.contains(id))
        {
            return json!({"error": {"rule": "awaiting-release", "message": format!(
                "batch `{}` is awaiting release: `jazyk release compile` (or the GUI) approves it", batch.id)}});
        }
        if let Err(why) =
            crate::control::claim_goals(&self.out, &batch.id, &self.worker_id(), &batch.goals)
        {
            return json!({"error": {"rule": "store-newer", "message": why}});
        }
        // The graph as this batch sees it, for replays (docs/compiler/sessions.md#snapshots).
        crate::store::snapshot_graph(&self.out, &batch.id);
        if let Some(h) = self.worker.lock().unwrap().as_mut() {
            h.refresh(Some(&batch.id));
        }
        let goals: Vec<Goal> = batch
            .goals
            .iter()
            .filter_map(|id| board.goal(id))
            .cloned()
            .collect();
        let records: BTreeMap<String, Vec<String>> = goals
            .iter()
            .map(|g| (g.id.clone(), board.records_of(&g.id)))
            .collect();
        let known_open: BTreeSet<String> =
            board.open_goals().iter().map(|g| g.id.clone()).collect();
        // The assembled prompt and the loaded set, before the store moves into the
        // session. The first batch of a serving ships the agent contract and the
        // skills in full; later batches elide what the agent already saw.
        let (loaded, mut skills) = crate::session::initial_loaded(&store, &goals);
        let mut pb = crate::session::ProjectBlock::from_goals(
            &store,
            &board.goals,
            &goals,
            &control.compile,
        )
        .with_lint(&self.project.linting);
        pb.batch = batch.id.clone();
        let full = args["full"].as_bool() == Some(true);
        let (include_contract, skip_skills) = {
            let d = self.delivered.lock().unwrap();
            let include = full || !d.contract;
            let skip: BTreeSet<String> = if full {
                BTreeSet::new()
            } else {
                skills
                    .rendered_names()
                    .into_iter()
                    .filter(|n| d.skills.contains(n))
                    .collect()
            };
            (include, skip)
        };
        let mut instructions = crate::session::session_prompt_elided(
            &store,
            &goals,
            &loaded,
            &skills,
            &pb,
            include_contract,
            &skip_skills,
            crate::session::Protocol::Serving,
        );
        // A first reply over the client's inline limit ships the skills as pointers
        // (docs/frontends/mcp.md#compilation-over-mcp).
        const INLINE_LIMIT: usize = 45_000;
        let inline_skills: Vec<String> = skills
            .rendered_names()
            .into_iter()
            .filter(|n| !skip_skills.contains(n))
            .collect();
        if instructions.len() > INLINE_LIMIT && !inline_skills.is_empty() {
            // An omitted payload has not entered this serving's model context.
            for name in &inline_skills {
                skills.forget(name);
            }
            let body = crate::session::session_prompt_elided(
                &store,
                &goals,
                &loaded,
                &skills,
                &pb,
                include_contract,
                &skip_skills,
                crate::session::Protocol::Serving,
            );
            instructions = format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/mcp-lifecycle/skills-not-inlined-the-reply-would-pass-cf6f3.md"
                )),
                inline_skills.join(", "),
                body
            );
        }
        {
            let mut delivered = self.delivered.lock().unwrap();
            delivered.contract = true;
            delivered.skills.extend(skills.rendered_names());
        }
        let pinned: BTreeSet<String> = goals.iter().map(|g| g.target.clone()).collect();
        let package = loaded.render_status(&skills.index_line(), skills.rendered_chars(), &pinned);
        let scope = WorkScope::for_batch(&batch.id, &goals);
        let kinds = scope.kinds();
        let kind_refs: Vec<&str> = kinds.iter().map(String::as_str).collect();
        let write_tools: Vec<&str> = toolset_for_kinds(&kind_refs)
            .into_iter()
            .filter(|t| {
                !crate::tools::READ_TOOLS.contains(t)
                    && !crate::tools::GOAL_TOOLS.contains(t)
                    && *t != crate::tools::FEEDBACK_TOOL
                    && *t != crate::tools::SPEND_TOOL
            })
            .collect();
        let mut session = ToolSession::new(store, scope, self.mutation_limit, self.context_budget);
        session.loaded = loaded;
        session.skills = skills;
        session.gen = self.current_gen_settings();
        session.lint = self.project.linting.clone();
        session.caller = self.caller(&kinds.join("+"), &batch.id);
        let goal_rows: Vec<Value> = goals
            .iter()
            .map(|g| {
                json!({"id": g.id, "kind": g.kind, "target": g.target, "mandatory": g.mandatory})
            })
            .collect();
        let reply = if self.bridge.packaged {
            // The bridge already delivered the contract as the session prompt.
            json!({
                "batch": batch.id,
                "goals": goals.iter().map(|g| g.id.clone()).collect::<Vec<_>>(),
                "note": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-lifecycle/changeset-open-stage-findings-with-the-write-a81e7.md")),
            })
        } else {
            json!({
                "batch": batch.id,
                "goals": goal_rows,
                "instructions": instructions,
                "package": package,
                "writeTools": write_tools,
                "readTools": crate::tools::READ_TOOLS.to_vec(),
                "goalTools": crate::tools::GOAL_TOOLS.to_vec(),
                "finishTool": "done",
                "next": SERVING.get("begin-goals-next"),
            })
        };
        *self.open.lock().unwrap() = Some(OpenBatch {
            work: None,
            heartbeat: None,
            config_hash: project.config_hash.clone(),
            id: batch.id,
            goals,
            records,
            known_open,
            session,
            rounds: 0,
        });
        reply
    }

    // The `done` tool: run the batch gates, commit atomically, re-derive the board,
    // and run the deterministic tail when the board empties.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    pub(super) fn done_batch(&self, params: &Value) -> Value {
        if let Some(v) = self.session_complete() {
            return v;
        }
        let mut open = self.open.lock().unwrap();
        let Some(mut o) = open.take() else {
            return json!({"error": {"rule": "no-open-batch", "message": "no batch is open; begin_goals first"}});
        };
        // The same batch gates an in-process session faces: every goal resolved or
        // failed, coverage contract, stale anchors, undecided proposals.
        if let Err(e) = o.session.dispatch("done", &params["arguments"]) {
            let v = e.to_value();
            crate::control::refresh_lease(&self.out, &o.id, &self.worker_id());
            *open = Some(o); // the changeset stays open; repair and finish again
            return v;
        }
        let batch_id = o.id.clone();
        let mut reply = self.commit_open(&mut o);
        if !reply["error"].is_null() {
            *open = Some(o);
            return reply;
        }
        *self.finished.lock().unwrap() = Some(batch_id);
        drop(open);
        // The consumer that empties the board of mandatory work runs the deterministic
        // tail; optional goals stand aside, as blocked ones do.
        // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
        let board = crate::board::Board::compute(&self.current_project(), &self.out);
        let graph_kinds = crate::board::Board::graph_kinds();
        let mandatory_ready = |b: &crate::board::Board| {
            b.ready_goals()
                .iter()
                .filter(|g| g.mandatory && graph_kinds.contains(&g.kind.as_str()))
                .count()
        };
        if mandatory_ready(&board) == 0 {
            let mut s2 = Store::load(&self.out);
            let quiet = crate::session::Trace::stderr(crate::session::TraceLevel::Quiet);
            let report = crate::reconcile::finalize(
                &mut s2,
                &self.current_project(),
                Vec::new(),
                &crate::model::Costs::default(),
                &quiet,
            );
            reply["verdict"] = json!(report.verdict);
            if let Some(why) = &report.unsaved {
                reply["unsaved"] = json!(why);
            }
            if report.verdict.to_string().contains("blocked") {
                // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
                reply["verdictNote"] = json!(include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/mcp-lifecycle/blocked-goals-wait-on-a-release-jazyk-39ebc.md"
                )));
            }
            reply["coveragePct"] = json!(report.coverage_pct);
            // The verdict never travels alone (docs/compiler/compilation.md#convergence).
            let counts = s2.open_diag_counts();
            if !counts.is_empty() {
                reply["openDiagnostics"] = json!(counts);
                reply["diagnosticsNote"] = json!(include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/mcp-lifecycle/open-diagnostics-stand-in-the-graph-the-f6b3c.md"
                )));
            }
            let board2 = crate::board::Board::compute(&self.current_project(), &self.out);
            if mandatory_ready(&board2) == 0 {
                // The generation rows that became ready ride under `ready`
                // (docs/frontends/mcp.md#compilation-over-mcp).
                let generate: Vec<String> = board2
                    .ready_goals()
                    .iter()
                    .filter(|g| g.kind == "generate")
                    .map(|g| g.target.clone())
                    .collect();
                // Optional goals stand aside for the verdict but stay claimable:
                // say so, or "nothing pending" contradicts the next goals reply.
                let optional_ready = board2
                    .ready_goals()
                    .iter()
                    .filter(|g| !g.mandatory && graph_kinds.contains(&g.kind.as_str()))
                    .count();
                // Failed goals stand between the board and convergence: say so
                // instead of "converged" (docs/frontends/mcp.md#compilation-over-mcp).
                let failed = s2.status.failed.len();
                let mut parts: Vec<String> = vec![if failed > 0 {
                    SERVING.fill("compile-incomplete", &[("failed", &failed.to_string())])
                } else {
                    SERVING.get("compile-converged").to_string()
                }];
                if optional_ready > 0 {
                    parts.push(
                        SERVING.fill("optional-ready", &[("count", &optional_ready.to_string())]),
                    );
                }
                if !generate.is_empty() {
                    parts.push(SERVING.fill(
                        "generation-ready",
                        &[("count", &generate.len().to_string())],
                    ));
                }
                if parts.len() == 1 {
                    parts.push(SERVING.get("nothing-pending").to_string());
                }
                reply["next"] = json!(parts.join("; "));
                reply["ready"] = json!({"generate": generate});
                // beginNext stops at convergence: an agent told to converge once held
                // an optional batch it had not asked for, and the note above says how
                // to take it (docs/frontends/mcp.md#compilation-over-mcp).
                return reply;
            }
            // The checks can surface new work (rare); name its first batch, never the
            // whole board (a novel's board of 205 bind goals once came back as a
            // 508k-character reply the agent could not read).
            reply["next"] = match board2.batches.first() {
                Some(b) => {
                    let mut v = board2.batch_json(b);
                    v["note"] = json!(SERVING.fill("next-batch", &[("batch", &b.id)]));
                    v
                }
                None => {
                    json!({"note": SERVING.get("no-batch-ready")})
                }
            };
            return reply;
        }
        // beginNext claims the next ready batch in the same call, saving a round
        // trip per batch. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
        if params["arguments"]["beginNext"].as_bool() == Some(true) {
            let began = self.begin_goals(&json!({"arguments": {}}));
            if began["error"].is_null() {
                reply["began"] = began;
                // A session that reached its own stopping point after the claim
                // releases the batch untouched (docs/frontends/mcp.md#compilation-over-mcp).
                reply["beganNote"] = json!(SERVING.get("began-note"));
                return reply;
            }
        }
        // `next` names the next ready batch by id with its goals, never the whole
        // board: a board reply carries every open goal with its contract paragraph
        // and once ran to 169k characters. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
        reply["next"] = match board.batches.first() {
            Some(b) => {
                let mut v = board.batch_json(b);
                v["note"] = json!(SERVING.fill("next-batch", &[("batch", &b.id)]));
                v
            }
            None => {
                json!({"note": SERVING.get("no-batch-ready")})
            }
        };
        reply
    }

    // Land a batch whose gates passed: release the lease, apply the staged work with
    // the resolutions on the journal entry, clear the resolved goals' records,
    // persist the failed goals, un-park, and record the goals the commit opened.
    // Shared by `done` and the ephemeral end-of-input finish.
    fn commit_open(&self, o: &mut OpenBatch) -> Value {
        let current = Store::load(&self.out);
        if !current.load_errors.is_empty() {
            return json!({"error":{"rule":"store-unreadable","message":current.load_errors.join("; ")}});
        }
        let _work_lock = match crate::open_work::Lock::acquire(&self.out) {
            Ok(lock) => lock,
            Err(error) => return json!({"error":{"rule":"open-work-busy","message":error}}),
        };
        if let Some(receipt) = &o.work {
            match crate::open_work::read(&self.out, &receipt.id) {
                Ok(record)
                    if record.state == "committed"
                        && record.receipt.owner == receipt.owner
                        && record.receipt.epoch == receipt.epoch =>
                {
                    return record.result
                }
                Ok(record) => {
                    if let Err(error) = record.check(receipt) {
                        return json!({"error":{"rule":"open-work-owner","message":error}});
                    }
                }
                Err(error) => return json!({"error":{"rule":"open-work-owner","message":error}}),
            }
        }
        // A configuration write since the batch opened: refuse once, so the agent checks
        // its work against the new document sets; the next finish commits.
        let config = self.current_project().config_hash;
        if !o.config_hash.is_empty() && o.config_hash != config {
            o.config_hash = config;
            return json!({"error": {"rule": "config-moved", "message": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-lifecycle/config-moved-4b2d9.md"))}});
        }
        if let Some(receipt) = &o.work {
            let generation = crate::store::read_generation(&self.out);
            for g in o.session.snapshot.status.generation + 1..=generation {
                if let Some(e) =
                    crate::goals::read_journal_entry(&crate::layout::journal_file(&self.out, g), g)
                {
                    if e.work_id.as_deref() == Some(&receipt.id) {
                        return json!({"committed":true,"workId":receipt.id,"generation":g,"overwrites":e.overwrites,"replayed":true});
                    }
                }
            }
        }
        if self.bridge.build_token.is_none() {
            let board = crate::board::Board::compute(&self.current_project(), &self.out);
            let gated: Vec<_> = o
                .goals
                .iter()
                .filter(|g| board.gated.contains(&g.id))
                .map(|g| g.id.clone())
                .collect();
            if !gated.is_empty() {
                return json!({"error":{"rule":"awaiting-release","message":include_str!(concat!(env!("OUT_DIR"),"/payloads/goals/prompts/inline/open-work/approval-required.md")),"goals":gated}});
            }
        }
        let staged = o.session.staged.clone();
        let mut commit = o.session.commit(o.rounds, None);
        commit.work_id = o.work.as_ref().map(|r| r.id.clone());
        commit.actor = o
            .work
            .as_ref()
            .and_then(|r| crate::open_work::read(&self.out, &r.id).ok())
            .map(|r| r.identity);
        commit.validation = serde_json::to_value(&o.session).ok();
        let resolved_ids: Vec<String> = commit.resolved.iter().map(|r| r.goal.clone()).collect();
        let failed = o.session.failed_goals();
        commit.progress = Some(crate::store::CommitProgress {
            clear_records: resolved_ids
                .iter()
                .flat_map(|id| o.records.get(id).cloned().unwrap_or_default())
                .collect(),
            failed: failed
                .iter()
                .filter_map(|(id, reason)| {
                    o.goals
                        .iter()
                        .find(|g| g.id == *id)
                        .map(|g| crate::model::FailedGoal {
                            goal: g.clone(),
                            reason: reason.clone(),
                        })
                })
                .collect(),
            finished: o.goals.iter().map(|g| g.id.clone()).collect(),
        });
        let since = o.session.snapshot.status.generation;
        let (parsed, _) = crate::reconcile::parse_all(&self.current_project());
        let mut reply = json!({"committed": true, "batch": o.id, "applied": 0});
        let mut generation = None;
        // Commits are the only synchronization: a changeset staged against an older
        // generation rebases onto what landed since, or reports the conflicts and
        // stays open on the current generation. A writer landing between the check
        // and the commit makes the store refuse `store-moved`, and the check runs
        // again. Mirrors docs/compiler/graph.md#commit-conflicts.
        let mut s = Store::load(&self.out);
        for attempt in 0.. {
            if s.status.generation != since {
                commit.rebased_from = Some(since);
            }
            // A zero-mutation batch with resolutions still commits: the journal is where
            // a resolution's justification lives (docs/compiler/graph.md#journal).
            if staged.is_empty() && commit.resolved.is_empty() && failed.is_empty() {
                break;
            }
            let report = s.apply_with_docs(staged.clone(), &commit, &parsed);
            if report.skipped.len() == 1
                && report.skipped[0].starts_with("store-moved")
                && attempt < 4
            {
                s = Store::load(&self.out);
                continue;
            }
            if !report.skipped.is_empty() {
                return json!({"error":{"rule":"commit-skipped","message":report.skipped.join("; ")}});
            }
            generation = Some(report.generation);
            reply["applied"] = json!(report.applied);
            reply["generation"] = json!(report.generation);
            if let Some(from) = commit.rebased_from {
                reply["rebasedFrom"] = json!(from);
            }
            break;
        }
        if let Some(g) = generation {
            if let Some(entry) =
                crate::goals::read_journal_entry(&crate::layout::journal_file(&self.out, g), g)
            {
                reply["overwrites"] = json!(entry.overwrites);
            }
        }
        if let Some(receipt) = &o.work {
            match crate::open_work::read(&self.out, &receipt.id) {
                Ok(mut record) => {
                    reply["workId"] = json!(receipt.id);
                    reply["conflictNotices"] = json!(self.work_notices(&record));
                    record.state = "committed".into();
                    record.result = reply.clone();
                    record.payload = Value::Null;
                    record.mutations.clear();
                    #[cfg(not(target_arch = "wasm32"))]
                    if crate::vfs::is_scoped() && crate::hosted::has_execution() {
                        record.payload = serde_json::to_value(&*o).unwrap_or(Value::Null);
                        crate::open_work::update_targets(&mut record);
                    }
                    let seq = record.sequence;
                    if let Err(error) = crate::open_work::save(&self.out, &mut record, Some(seq)) {
                        reply["recoveryWarning"] = json!(error);
                    }
                }
                Err(error) => reply["recoveryWarning"] = json!(error),
            }
        }
        o.session.staged.clear();
        crate::control::release_lease(&self.out, &o.id, &self.worker_id());
        if let Some(h) = self.worker.lock().unwrap().as_mut() {
            h.refresh(Some(""));
        }
        // The counter rides every done reply, mutations or not
        // (docs/frontends/mcp.md#compilation-over-mcp).
        if generation.is_none() {
            reply["generation"] = json!(s.status.generation);
        }
        if !resolved_ids.is_empty() {
            reply["resolved"] = json!(resolved_ids);
        }
        if !failed.is_empty() {
            reply["failed"] = json!(failed.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>());
        }
        let board = crate::board::Board::compute(&self.current_project(), &self.out);
        let opened: Vec<OpenedGoal> = board
            .open_goals()
            .iter()
            .filter(|g| !o.known_open.contains(&g.id))
            .map(|g| OpenedGoal {
                goal: g.id.clone(),
                cause: g.cause.clone().unwrap_or_default(),
            })
            .collect();
        // Derived facts this commit landed carry ratification proposals: the ratify goals
        // they open wait on a human and swell the blocked count, so the reply says so
        // (docs/frontends/mcp.md#compilation-over-mcp).
        let ratify_new = generation
            .map(|g| {
                let build = format!("g{}", g);
                Store::load(&self.out)
                    .graph
                    .diagnostics
                    .values()
                    .filter(|d| {
                        d.rule == "ratification-pending"
                            && d.created.as_deref() == Some(build.as_str())
                    })
                    .count()
            })
            .unwrap_or(0);
        if ratify_new > 0 {
            reply["ratificationNote"] = json!(format!(
                "{} derived fact(s) this commit landed (criteria, groupings, sub-entities) carry ratification proposals: {} ratify goal(s) wait on a human and join the blocked count; nothing is wrong with them",
                ratify_new, ratify_new
            ));
        }
        if !opened.is_empty() {
            reply["opened"] = json!(opened.iter().map(|x| x.goal.clone()).collect::<Vec<_>>());
            if let Some(g) = generation {
                let mut s2 = Store::load(&self.out);
                s2.record_opened_goals(g, opened);
            }
        }
        reply
    }

    // End of input with an open batch: the agent's session ended without the
    // finishing call. Valid staged work still lands, under the same gates the budget
    // path uses. Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
    pub(super) fn eof_finish(&self) {
        // Every open batch finishes, the parked ones first.
        let parked: Vec<String> = self.parked.lock().unwrap().keys().cloned().collect();
        for id in parked {
            let batch = self.parked.lock().unwrap().remove(&id);
            if let Some(batch) = batch {
                let previous = self.open.lock().unwrap().replace(batch);
                self.eof_finish_active();
                if let Some(previous) = previous {
                    *self.open.lock().unwrap() = Some(previous);
                }
            }
        }
        self.eof_finish_active();
    }

    fn eof_finish_active(&self) {
        if let Some(o) = self.open.lock().unwrap().take() {
            crate::control::release_lease(&self.out, &o.id, &self.worker_id());
            // Accepted calls are already durable. Dropping the heartbeat unlocks
            // this retained work after its TTL without committing it.
        }
    }

    pub(super) fn abandon_goals(&self, params: &Value) -> Value {
        if let Some(v) = self.session_complete() {
            return v;
        }
        let mut open = self.open.lock().unwrap();
        let Some(o) = open.take() else {
            return json!({"error": {"rule": "no-open-batch", "message": "no batch is open"}});
        };
        if let Some(receipt) = &o.work {
            let saved = (|| -> Result<(), String> {
                let mut r = crate::open_work::read(&self.out, &receipt.id)?;
                r.check(receipt)?;
                let seq = r.sequence;
                r.state = "discarded".into();
                r.payload = Value::Null;
                r.mutations.clear();
                r.targets.clear();
                crate::open_work::save(&self.out, &mut r, Some(seq))
            })();
            if let Err(error) = saved {
                *open = Some(o);
                return json!({"error":{"rule":"open-work","message":error}});
            }
        }
        crate::control::release_lease(&self.out, &o.id, &self.worker_id());
        if let Some(h) = self.worker.lock().unwrap().as_mut() {
            h.refresh(Some(""));
        }
        let reason = params["arguments"]["reason"].as_str().unwrap_or("");
        json!({
            "abandoned": o.id,
            "goals": o.goals.iter().map(|g| g.id.clone()).collect::<Vec<_>>(),
            "dropped": o.session.staged.len(),
            "reason": reason,
            "note": "the staged changeset is gone; the goals return to open",
        })
    }
}
