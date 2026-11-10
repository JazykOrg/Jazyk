use super::*;

impl ToolSession {
    // Every tool call enters here. Its arguments are normalized first, so a model
    // that fills every schema field with an empty value makes the same call as one
    // that omits them, and a fact the documents state keeps its quote as its one
    // provenance. Mirrors docs/compiler/tools.md#validation-and-errors.
    pub fn dispatch(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        let supplied = args;
        let mut args = normalize_args(args);
        // Literal file data and supplied guards must not disappear as filler.
        // In particular, blank old_text must never become a full overwrite.
        if name == "write_text_file" {
            for key in ["content", "old_text", "expectedHash"] {
                if let Some(value) = supplied.get(key) {
                    args[key] = value.clone();
                }
            }
        }
        let note = prefer_quote(name, &mut args);
        let result = self.dispatch_normalized(name, &args);
        match (result, note) {
            (Ok(mut v), Some(note)) => {
                if v.is_object() {
                    v["note"] = json!(note);
                }
                Ok(v)
            }
            (r, _) => r,
        }
    }

    fn dispatch_normalized(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        self.loaded.next_round();
        // Targets a call names count as referenced, for the unload suggestions.
        for key in ["target", "id", "goal", "section", "ref"] {
            if let Some(v) = args[key].as_str() {
                self.loaded.note_reference(v);
            }
        }
        if let Some(h) = args["handle"].as_str() {
            if let Ok((t, _, _)) = crate::context::parse_handle(h) {
                self.loaded.note_reference(&t);
            }
        }
        // The repeated-call guard, keyed per open batch: the second identical call
        // answers with a repeat marker, the third is refused. done, mark_goal_done,
        // mark_goal_failed, record_generation, and record_binding are exempt: repairing
        // files after a rejected claim legitimately repeats its arguments. A load of an already loaded target counts as a repeat whatever
        // its depth. settle_pairs is exempt as a whole and guards per entry.
        // Mirrors docs/compiler/sessions.md#repeated-calls.
        let exempt = matches!(
            name,
            "done"
                | "mark_goal_done"
                | "mark_goal_failed"
                | "record_generation"
                | "record_binding"
                | PAIR_SETTLE_TOOL
        ) || crate::task_runtime::TOOLS.contains(&name);
        if !exempt {
            // A load keys on the target it resolves to, so an alias and its id are one
            // call, and counts only while that target is loaded: a refused load or an
            // evicted target starts again.
            let key = if name == "load" {
                let raw = args["target"].as_str().unwrap_or_default();
                let id = self.canon_entity_id(raw).unwrap_or_else(|| raw.to_string());
                let id = self.snapshot.resolve_id(&id).to_string();
                let loaded = self
                    .loaded
                    .items
                    .iter()
                    .any(|i| i.target == id && i.what != "stub");
                let key = format!("load|{}", id);
                if !loaded {
                    self.repeats.remove(&key);
                }
                key
            } else {
                format!("{}|{}", name, args)
            };
            let seen = {
                let c = self.repeats.entry(key.clone()).or_insert(0);
                *c += 1;
                *c
            };
            if seen >= 3 {
                // The state may have moved since the last answer: a pure read answers
                // again and is compared, any other call compares the generation and the
                // staged mutations it was answered under.
                let staged = self.state_mark();
                let moved = if REANSWERABLE_TOOLS.contains(&name) {
                    let result = self.dispatch_gated(name, args);
                    let answer = Self::answer_text(&result);
                    if self.last_answers.get(&key).map(|(a, _)| a) != Some(&answer) {
                        self.last_answers
                            .insert(key.clone(), (answer, self.state_mark()));
                        self.repeats.insert(key, 1);
                        return result;
                    }
                    false
                } else {
                    self.last_answers.get(&key).map(|(_, n)| *n) != Some(staged)
                };
                if moved {
                    self.repeats.insert(key.clone(), 1);
                    let result = self.dispatch_gated(name, args);
                    let answer = Self::answer_text(&result);
                    self.last_answers.insert(key, (answer, self.state_mark()));
                    return result;
                }
                self.refusals += 1;
                // Past eight refusals in one batch, the serving finishes the session
                // implicitly: the staged work commits under the same gates the budget
                // path uses.
                if self.refusals > 8 && self.done.is_none() {
                    self.finish_implicit("(implicit: the session repeated itself past the guard)");
                }
                return Err(ToolError::new(
                    "repeated-call",
                    format!(
                        "this is call {} to `{}` with identical arguments; the answer has not changed. Act on the answer you already have, or finish with done. If this refusal seems wrong, say so with report_feedback.",
                        seen, name
                    ),
                ));
            }
            // A repeat is marked only when its answer matches the previous one; a
            // changed answer means the state moved, and the count starts again.
            let result = self.dispatch_gated(name, args);
            let answer = Self::answer_text(&result);
            // A load of a loaded target is a repeat by target, whatever it answers.
            let same =
                name == "load" || self.last_answers.get(&key).map(|(a, _)| a) == Some(&answer);
            self.last_answers
                .insert(key.clone(), (answer, self.state_mark()));
            if seen == 2 {
                if !same {
                    self.repeats.insert(key, 1);
                    return result;
                }
                let mut v = result?;
                if v.is_object() {
                    v["repeat"] = json!(
                        "you already made this exact call; the answer is unchanged. Act on it."
                    );
                }
                return Ok(v);
            }
            return result;
        }
        self.dispatch_gated(name, args)
    }

    // The state a call is answered under, for the repeated-call guard: the committed
    // generation (per-call servings commit between calls) and the staged mutations.
    fn state_mark(&self) -> (u64, usize) {
        (self.snapshot.status.generation, self.staged.len())
    }

    // The comparable text of an answer, for the repeated-call guard.
    fn answer_text(result: &Result<Value, ToolError>) -> String {
        match result {
            Ok(v) => v.to_string(),
            Err(e) => format!("{}: {}", e.rule, e.message),
        }
    }

    fn dispatch_gated(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        // Reads see the session's snapshot, not its staged mutations. Saying so on
        // every read while writes are staged stops the caller from concluding a
        // staged delete or update was lost.
        if READ_TOOLS.contains(&name) && !self.staged.is_empty() {
            return self.dispatch_inner(name, args).map(|mut v| {
                if v.is_object() {
                    v["note"] = json!("reads show the graph as the session began; this session's staged mutations apply at commit");
                }
                v
            });
        }
        let before = self.staged.len();
        // A graph write is all or nothing: a refused one leaves neither staged ops
        // nor lookup entries behind. Mirrors docs/compiler/tools.md#write-tools.
        let saved = GRAPH_WRITE_TOOLS.contains(&name).then(|| {
            (
                self.staged_entities.clone(),
                self.staged_diags.clone(),
                self.staged_reqs.clone(),
                self.staged_views.clone(),
                self.staged_parents.clone(),
                self.taken_ids.clone(),
            )
        });
        let result = self.dispatch_inner(name, args);
        if let (Err(_), Some(saved)) = (&result, saved) {
            self.staged.truncate(before);
            (
                self.staged_entities,
                self.staged_diags,
                self.staged_reqs,
                self.staged_views,
                self.staged_parents,
                self.taken_ids,
            ) = saved;
        }
        let mut v = result?;
        // A mutating reply previews the goals the mutation will open at commit and
        // re-renders the condensed status block.
        // Mirrors docs/compiler/reconciler.md#bubbling.
        if self.staged.len() > before && v.is_object() {
            let mut opens = crate::board::staged_opens(&self.snapshot, &self.staged[before..]);
            // A grouping or a dissolve moves children between levels, so the levels it
            // touches face the fan-out limit in the same preview.
            for line in self.level_opens(&self.staged[before..]) {
                if !opens.contains(&line) {
                    opens.push(line);
                }
            }
            // A line previewed earlier this session is counted, not repeated: an
            // extraction batch once read the same review line on every upsert.
            let mut fresh: Vec<String> = Vec::new();
            let mut repeated = 0usize;
            for line in opens {
                if self.previewed.insert(line.clone()) {
                    fresh.push(line);
                } else {
                    repeated += 1;
                }
            }
            if !fresh.is_empty() {
                v["opens"] = json!(fresh);
            }
            if repeated > 0 {
                v["opensRepeated"] = json!(format!(
                    "{} goal line(s) previewed earlier in this session stand as before",
                    repeated
                ));
            }
            v["status"] = json!(self.condensed_status());
        }
        Ok(v)
    }

    pub(super) fn dispatch_inner(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        match name {
            "load" => self.load_tool(args),
            "expand" => self.expand_tool(args),
            "unload" => self.unload_tool(args),
            "graph_status" => Ok(json!({"status": self.render_status_full()})),
            "load_skill" => self.load_skill_tool(args),
            "get_view" => self.get_view_tool(args),
            "mark_goal_done" => self.mark_goal_done(args),
            PAIR_SETTLE_TOOL => self.settle_pairs(args),
            "mark_goal_failed" => self.mark_goal_failed_tool(args),
            "search" => self.search_tool(args),
            "read_section" => self.read_section_tool(args),
            "get_entity" => self.get_entity_tool(args),
            "diagnostics" => self.diagnostics_tool(args),
            "upsert_entity" => self.upsert_entity_tool(args),
            "update_entity" => self.update_entity_tool(args),
            "delete_entity" => self.delete_entity_tool(args),
            "merge_entities" => self.merge_entities_tool(args),
            "group_entities" => self.group_entities_tool(args),
            "dissolve_entity" => self.dissolve_entity_tool(args),
            "upsert_criterion" => self.upsert_criterion(args),
            "update_criterion" => self.update_criterion_tool(args),
            "delete_criterion" => self.delete_criterion_tool(args),
            "upsert_requirement" => self.upsert_requirement_tool(args),
            "update_requirement" => self.update_requirement_tool(args),
            "delete_requirement" => self.delete_requirement_tool(args),
            "place_anchor" | "orphan_anchor" => self.anchor_tool(name, args),
            "report_diagnostic" => self.report_diagnostic_tool(args),
            "update_diagnostic" => self.update_diagnostic_tool(args),
            "resolve_diagnostic" => self.resolve_diagnostic_tool(args),
            "upsert_view" | "update_view" | "delete_view" => self.view_tool(name, args),
            "edit_fact" => self.edit_fact(args),
            "set_coverage" => self.set_coverage_tool(args),
            "generation_tasks" => self.generation_tasks_tool(args),
            "begin_generation" => self.begin_generation_tool(args),
            "record_medium" => self.record_medium_tool(args),
            "record_generation" => self.record_generation_tool(args),
            "binding_tasks" => self.binding_tasks_tool(args),
            "begin_binding" => self.begin_binding_tool(args),
            "record_binding" => self.record_binding_tool(args),
            "verification_tasks" => self.verification_tasks_tool(args),
            "begin_verification" => self.begin_verification_tool(args),
            "run_tests" => self.run_tests_tool(args),
            "record_verdict" => self.record_verdict_tool(args),
            name if crate::task_runtime::TOOLS.contains(&name) => {
                self.task_runtime_tool(name, args)
            }
            "read_text_file" | "write_text_file" | "list_files" => self.file_tool(name, args),
            "report_spend" => self.report_spend_tool(args),
            "report_feedback" => self.report_feedback_tool(args),
            "done" => self.done_tool(args),
            other => Err(ToolError::new(
                "unknown-tool",
                format!("unknown tool `{}`", other),
            )),
        }
    }
}
