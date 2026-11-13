use super::*;

impl ToolSession {
    fn task_entity_filter(&self, args: &Value) -> Result<Option<String>, ToolError> {
        Self::opt_str(args, "entity")
            .map(|id| {
                self.canon_entity_id(&id)
                    .ok_or_else(|| self.unknown_entity_error(&id))
            })
            .transpose()
    }

    pub(super) fn generation_tasks_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let gs = self.gen_settings();
        let entity = self.task_entity_filter(args)?;
        let tasks = crate::gen::pending(&self.snapshot, &gs)
            .into_iter()
            .filter(|row| {
                entity.as_deref().is_none_or(|id| {
                    row["entity"]
                        .as_str()
                        .is_some_and(|owner| self.snapshot.resolve_id(owner) == id)
                })
            })
            .collect::<Vec<_>>();
        if tasks.is_empty() {
            return Ok(
                json!({"tasks": [], "note": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-ledger/no-generation-work-is-currently-derived-inspect-0c52f.md"))}),
            );
        }
        Ok(json!({"tasks": tasks, "next": "begin_generation on one entity"}))
    }

    pub(super) fn begin_generation_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let entity = Self::str_arg(args, "entity")?;
        let gs = self.gen_settings();
        let id = self.snapshot.resolve_id(&entity).to_string();
        let mut v = crate::gen::task_package(&self.snapshot, &id, &gs)
            .map_err(|e| ToolError::new("unknown-id", e))?;
        if self.opening_contracts {
            let in_prompt = self.scope.goals.iter().any(|g| g.kind == "generate");
            opened_package("begin_generation", &mut v, in_prompt);
        }
        Ok(v)
    }

    pub(super) fn record_medium_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        // The external worker's decision, recorded once. Mirrors
        // docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
        let form = Self::str_arg(args, "form")?;
        let produced = Self::str_arg(args, "produced")?;
        if produced != "written" && produced != "built" {
            return Err(ToolError::new(
                "bad-args",
                "produced is written or built".into(),
            ));
        }
        let artifact = Self::opt_str(args, "artifact").unwrap_or_default();
        if produced == "built" && artifact.trim().is_empty() {
            return Err(ToolError::new(
                "bad-args",
                "a built deliverable names the artifact its build produces".into(),
            ));
        }
        let _lock = crate::gen::Ledger::lock(&self.snapshot.out);
        let mut ledger = crate::gen::Ledger::load(&self.snapshot.out);
        if ledger.medium.is_some() && !ledger.entities.is_empty() {
            return Err(ToolError::new(
                "medium-settled",
                format!(
                    "the medium is settled ({}) and entities already follow it; it is not re-decided",
                    ledger.medium.as_ref().map(|m| m.line()).unwrap_or_default()
                ),
            ));
        }
        let medium = crate::gen::Medium {
            form,
            produced,
            toolchain: Self::opt_str(args, "toolchain").unwrap_or_default(),
            artifact,
        };
        let line = medium.line();
        ledger.medium = Some(medium);
        ledger
            .save(&self.snapshot.out)
            .map_err(|e| ToolError::new("ledger-write", e))?;
        Ok(json!({"recorded": true, "medium": line}))
    }

    pub(super) fn record_generation_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let entity = Self::str_arg(args, "entity")?;
        let gs = self.gen_settings();
        let id = self.snapshot.resolve_id(&entity).to_string();
        let Some(seen) = Self::opt_str(args, "factHash") else {
            return Err(ToolError::new(
                "bad-argument",
                "factHash is required; pass the factHash from the begin_generation package".into(),
            ));
        };
        if !args["manifest"].is_object() {
            return Err(ToolError::new(
                "bad-argument",
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/tools-ledger/manifest-is-required-files-tests-requirement-kind-f0806.md"
                ))
                .into(),
            ));
        }
        // `choices` rides beside the manifest; fold it in so one validated
        // set feeds the record and the diagnostics.
        let mut manifest = args["manifest"].clone();
        if manifest["choices"].is_null() && args["choices"].is_array() {
            manifest["choices"] = args["choices"].clone();
        }
        let choices = crate::gen::parse_choices(&self.snapshot, &manifest)
            .map_err(|e| ToolError::new("bad-choices", e))?;
        let reply = crate::gen::mark(&self.snapshot, &id, Some(seen.as_str()), &manifest, &gs)
            .map_err(|e| ToolError::new("bad-manifest", e))?;
        // Invented choices land as diagnostics through the session path; the
        // ops also resolve open ones this record omits.
        // Mirrors docs/consumers/gen.md#invented-choices.
        let ledger = crate::gen::Ledger::load(&self.snapshot.out);
        let unattached = ledger
            .entities
            .get(crate::ids::entity_slug(&id))
            .and_then(|e| e.unattached.clone());
        for op in crate::gen::choice_ops(&self.snapshot, &id, &choices, unattached.as_ref()) {
            self.stage(op)?;
        }
        Ok(reply)
    }

    pub(super) fn binding_tasks_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let gs = self.gen_settings();
        let entity = self.task_entity_filter(args)?;
        let mut tasks = crate::bind::pending(&self.snapshot, &gs)
            .into_iter()
            .filter(|row| {
                entity.as_deref().is_none_or(|id| {
                    row["entity"]
                        .as_str()
                        .is_some_and(|owner| self.snapshot.resolve_id(owner) == id)
                })
            })
            .collect::<Vec<_>>();
        if tasks.is_empty() {
            return Ok(
                json!({"tasks": [], "note": "every requirement is bound; generation_tasks lists what binding left unimplemented"}),
            );
        }
        // Generation works per entity: the rows come grouped by the entity
        // they unblock (docs/compiler/tools.md#binding-tools).
        tasks.sort_by(|a, b| {
            a["entity"]
                .as_str()
                .cmp(&b["entity"].as_str())
                .then(a["requirement"].as_str().cmp(&b["requirement"].as_str()))
        });
        let mut by_entity: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for t in &tasks {
            *by_entity
                .entry(t["entity"].as_str().unwrap_or("").to_string())
                .or_insert(0) += 1;
        }
        Ok(json!({
            "tasks": tasks,
            "byEntity": by_entity,
            "next": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-ledger/begin-binding-on-one-requirement-an-entity-074f6.md")),
        }))
    }

    pub(super) fn begin_binding_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let rid = Self::str_arg(args, "requirement")?;
        let gs = self.gen_settings();
        let mut v = crate::bind::task(&self.snapshot, &rid, &gs)
            .map_err(|e| ToolError::new("unknown-id", e))?;
        if self.opening_contracts {
            opened_package("begin_binding", &mut v, false);
        }
        Ok(v)
    }

    pub(super) fn record_binding_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let rid = Self::str_arg(args, "requirement")?;
        let verdict = Self::str_arg(args, "verdict")?;
        let gs = self.gen_settings();
        let files = Self::str_list(args, "files");
        let evidence = Self::opt_str(args, "evidence");
        if !args["test"].is_object() && !args["tests"].is_array() {
            return Err(ToolError::new(
                "bad-argument",
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/tools-ledger/test-is-required-kind-programmatic-llm-label-0c799.md"
                ))
                .into(),
            ));
        }
        crate::bind::record(
            &self.snapshot,
            &rid,
            &files,
            &args["test"],
            &args["tests"],
            &verdict,
            evidence.as_deref(),
            &gs,
        )
        .map_err(|e| ToolError::new("bad-binding", e))
    }

    pub(super) fn verification_tasks_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let gs = self.gen_settings();
        let filter = Self::opt_str(args, "filter");
        let entity = self.task_entity_filter(args)?;
        let tasks =
            crate::verify::pending(&self.snapshot, &gs, filter.as_deref(), entity.as_deref());
        if tasks.is_empty() {
            return Ok(
                json!({"tasks": [], "note": "nothing pending; every targeted row is verified"}),
            );
        }
        Ok(
            json!({"tasks": tasks, "next": "run_tests for programmatic rows; begin_verification then record_verdict for llm rows"}),
        )
    }

    pub(super) fn begin_verification_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let rid = Self::str_arg(args, "requirement")?;
        let gs = self.gen_settings();
        crate::verify::task(&self.snapshot, &rid, &gs).map_err(|e| ToolError::new("unknown-id", e))
    }

    pub(super) fn run_tests_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let gs = self.gen_settings();
        let mut targets = Self::str_list(args, "requirements");
        // all: every programmatic row, verified ones included.
        // Mirrors docs/compiler/tools.md#verification-tools.
        if targets.is_empty() && args["all"].as_bool() == Some(true) {
            let ledger = crate::gen::Ledger::load(&self.snapshot.out);
            targets = ledger
                .requirements
                .iter()
                .filter(|(_, row)| {
                    row.test.kind == "programmatic"
                        || row.tests.values().any(|t| t.test.kind == "programmatic")
                })
                .map(|(rid, _)| rid.clone())
                .collect();
        }
        crate::verify::run_selected(&self.snapshot, &gs, &targets)
            .map_err(|e| ToolError::new("build-failed", e))
    }

    pub(super) fn record_verdict_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let rid = Self::str_arg(args, "requirement")?;
        let verdict = Self::str_arg(args, "verdict")?;
        let gs = self.gen_settings();
        let seen = Self::opt_str(args, "factHash");
        let evidence = Self::opt_str(args, "evidence");
        crate::verify::mark_llm(
            &self.snapshot,
            &rid,
            &verdict,
            seen.as_deref(),
            evidence.as_deref(),
            args.get("criteria"),
            &gs,
        )
        .map_err(|e| ToolError::new("bad-argument", e))
    }

    pub(super) fn task_runtime_tool(
        &mut self,
        name: &str,
        args: &Value,
    ) -> Result<Value, ToolError> {
        let root = crate::project::find_root(&self.gen.deliverable)
            .or_else(|| crate::project::find_root(&self.snapshot.out))
            .ok_or_else(|| {
                ToolError::new("runtime-unavailable", "Project root not found".into())
            })?;
        // A ledger session owns the tasks it adds; the build sweeps them once
        // unused. Mirrors docs/consumers/gen.md#session-tasks-are-swept-when-unused.
        let owner = if name == "update_tasks" {
            self.task_owner()
        } else {
            None
        };
        let before = match owner {
            Some(_) => Some(
                crate::task_runtime::load(&root)
                    .map_err(|e| ToolError::new("task-runtime", e))?
                    .1,
            ),
            None => None,
        };
        let mut reply = crate::task_runtime::call(&root, name, args)
            .map_err(|e| ToolError::new("task-runtime", e))?;
        if let (Some(owner), Some(before)) = (owner, before) {
            // The task list is already saved: an ownership failure leaves the
            // tasks the author's and says so, never a retryable error.
            let after: Vec<crate::task_runtime::Task> =
                serde_json::from_value(reply["tasks"].clone()).unwrap_or_default();
            if let Err(e) =
                crate::task_runtime::record_owned(&self.snapshot.out, &before, &after, &owner)
            {
                reply["ownershipNote"] = json!(format!(
                    "Tasks saved; ownership was not recorded, so they are kept: {e}"
                ));
            }
        }
        Ok(reply)
    }

    // An external agent's own report of what its run billed for the open batch.
    // It stages nothing; the commit takes the figure when the harness measured
    // none. Mirrors docs/compiler/tools.md#spend-tool.
    pub(super) fn report_spend_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        if self.scope.goals.is_empty() {
            return Err(ToolError::new(
                "no-open-batch",
                "report_spend records a spend on the open batch's journal entry and no batch is open; `jazyk spend <generation> <tokens> [--cached N]` stamps a committed entry instead".into(),
            ));
        }
        let tokens = args["tokens"].as_u64().unwrap_or(0);
        if tokens == 0 {
            return Err(ToolError::new(
                "invalid-argument",
                "report_spend needs tokens: the whole number of tokens your endpoint billed for this batch, above zero".into(),
            ));
        }
        let cached = args["cached"].as_u64().unwrap_or(0);
        if cached > tokens {
            return Err(ToolError::new(
                "invalid-argument",
                format!("cached ({}) cannot exceed tokens ({}): the cached input is a share of the bill, not an addition to it", cached, tokens),
            ));
        }
        let replaced = self.reported_spend.is_some();
        self.reported_spend = Some((tokens, cached));
        let mut v = json!({"recorded": true, "tokens": tokens, "cached": cached});
        if replaced {
            v["note"] = json!("replaced the earlier report for this batch");
        }
        Ok(v)
    }

    // Feedback about jazyk's own prompts and tools. It stages nothing, spends no
    // mutation budget, and passes no gate beyond a non-empty message: a model
    // asking for help is never bounced. Mirrors docs/compiler/tools.md#feedback-tool.
    pub(super) fn report_feedback_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let message = Self::opt_str(args, "message").unwrap_or_default();
        let message = message.trim();
        if message.is_empty() {
            return Err(ToolError::new(
                "missing-argument",
                "report_feedback needs a message saying what was unclear and what would have helped".into(),
            ));
        }
        if self.feedback_count >= FEEDBACK_LIMIT {
            return Ok(json!({
                "recorded": false,
                "note": format!(include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-ledger/already-recorded-feedback-entries-for-this-session-63e3a.md")), FEEDBACK_LIMIT)
            }));
        }
        self.feedback_count += 1;
        let c = self.caller.clone();
        crate::feedback::append(
            &self.snapshot.out,
            &crate::feedback::Entry {
                at: crate::verify::now_iso(),
                kind: crate::feedback::normalize_kind(
                    &Self::opt_str(args, "kind").unwrap_or_default(),
                ),
                subject: Self::opt_str(args, "subject")
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                message: crate::llm::truncate(message, 4_000).to_string(),
                source: c.source,
                // The caller fields carry the goal kinds and the batch id.
                task: if c.task.is_empty() {
                    self.scope.feedback_task()
                } else {
                    c.task
                },
                target: if c.target.is_empty() {
                    self.scope.batch.clone()
                } else {
                    c.target
                },
                batch: if c.batch.is_empty() {
                    self.scope.goal_ids()
                } else {
                    c.batch
                },
                model: c.model,
                codec: c.codec,
                generation: self.snapshot.status.generation,
                run: c.run,
                client: c.client,
            },
        );
        Ok(json!({
            "recorded": true,
            "note": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-ledger/logged-for-jazyk-s-developers-continue-the-ee918.md"))
        }))
    }
}
