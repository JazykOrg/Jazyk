use super::*;

impl McpServer {
    pub(super) fn handle_local(
        &self,
        method: &str,
        params: &Value,
    ) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                self.watch_edits();
                if let Some(name) = params["clientInfo"]["name"].as_str() {
                    *self.client.lock().unwrap() = Some(name.to_string());
                }
                // A task-lifecycle serving is a worker among workers: register it and
                // heartbeat while the process lives. An ephemeral serving is part of a
                // run that already answers for itself, so it never registers. Mirrors
                // docs/frontends/mcp.md#the-control-plane-over-mcp.
                if !self.bridge.ephemeral
                    && !crate::vfs::is_scoped()
                    && self.modes.iter().any(|m| {
                        m == "compile" || m == "generate" || m == "verify" || m == "decompile"
                    })
                {
                    let client = self
                        .client
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(|| "agent".into());
                    let mut w = self.worker.lock().unwrap();
                    match w.as_mut() {
                        Some(h) => h.set_client(&client),
                        None => {
                            *w = Some(crate::control::register(&self.out, "agent", &client));
                            // The heartbeat keeps the worker registration fresh so
                            // the sweep does not call it stale. A browser tab has one
                            // backend and no thread to spare, and nothing sweeps it
                            // (docs/frontends/wasm.md#the-seams).
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                // A weak handle: the thread ends when the serving
                                // that registered the worker is gone, so a replaced
                                // child serving stops heartbeating as a live worker.
                                let wref = std::sync::Arc::downgrade(&self.worker);
                                std::thread::spawn(move || loop {
                                    std::thread::sleep(std::time::Duration::from_secs(30));
                                    let Some(w) = wref.upgrade() else { break };
                                    let mut guard = w.lock().unwrap_or_else(|p| p.into_inner());
                                    match guard.as_mut() {
                                        Some(h) => h.refresh(None),
                                        None => break,
                                    }
                                });
                            }
                        }
                    }
                }
                // Echoing a supported requested version keeps strict clients (rmcp)
                // from treating the reply as a downgrade; an unknown one gets the
                // latest the serving speaks (docs/frontends/mcp.md#mcp-into-acp-sessions).
                Ok(json!({
                    "protocolVersion": negotiate_protocol(&params["protocolVersion"]),
                    "capabilities": self.capabilities(),
                    "serverInfo": {"name": "jazyk", "version": crate::version::VERSION},
                    "instructions": self.initialize_instructions(self.initialized())?
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => {
                let enabled = self.enabled_tools();
                let mut tools: Vec<Value> = catalog()
                    .iter()
                    .filter(|t| enabled.contains(&t.name))
                    .map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.parameters}))
                    .collect();
                tools.push(json!({"name":"project_context","description":include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/read-this-project-s-repository-url-selected-96d02.md")),"inputSchema":{"type":"object","properties":{},"additionalProperties":false}}));
                if self.modes.len() == 1 && self.modes[0] == "runtime" {
                    return Ok(json!({"tools": tools}));
                }
                tools.extend(crate::agent_workflows::tools());
                #[cfg(not(target_arch = "wasm32"))]
                if !self.bridge.reader && self.modes.iter().any(|m| m == "chat") {
                    tools.push(json!({
                        "name": "release_work",
                        "description": "Approve the selected set's current compile or generate stage only when the user explicitly requested that work. Leaves manual workflow settings unchanged. Does not approve human decisions, credentials, or publication.",
                        "inputSchema": {"type":"object","properties":{"stage":{"type":"string","enum":["compile","generate"]}},"required":["stage"],"additionalProperties":false}
                    }));
                }
                // The compilation lifecycle: the compile serving's own, and the chat
                // serving carries it too (docs/compiler/tools.md#toolsets). The
                // catalog's own `done` (the goal tools) yields to the lifecycle entry:
                // a client that forwards the list as is (Claude Code) is refused by
                // its API when a name repeats.
                if !self.bridge.reader && self.modes.iter().any(|m| m == "compile" || m == "chat") {
                    tools.retain(|t| !LIFECYCLE.contains(&t["name"].as_str().unwrap_or_default()));
                    tools.push(json!({
                        "name": "goals",
                        "description": "The goal board, compact: the counts, the batches the scheduler would form (each with its goals), the ready goals outside a batch, and the goals not ready summarized by kind and blocking reason with their ids. full: true lists every goal with its hints, change, cause, readiness, gated and claimedBy. Zero open goals carries the build verdict with its counts. Next: begin_goals.",
                        "inputSchema": {"type": "object", "properties": {"full": {"type": "boolean", "description": "every goal whole, hints included"}}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "begin_goals",
                        "description": "Claim the named batch (batch, an id from goals), the named goals as one batch (goals, when one scheduled batch holds them all), or the next ready batch. Opens the changeset and returns the assembled session prompt as instructions and the initially loaded set as package. One batch open at a time.",
                        "inputSchema": {"type": "object", "properties": {"batch": {"type": "string", "description": "a batch id from goals, e.g. b412-1"}, "goals": {"type": "array", "items": {"type": "string"}, "description": "goal ids sharing one locality and one executor"}, "full": {"type": "boolean", "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/repeat-the-agent-contract-and-the-skills-8b332.md"))}}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "done",
                        "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/resolve-final-goals-and-finish-the-open-70cf2.md")),
                        "inputSchema": crate::tools::done_parameters(true)
                    }));
                    tools.push(json!({
                        "name": "abandon_goals",
                        "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/drop-the-open-changeset-without-committing-the-fc970.md")),
                        "inputSchema": {"type": "object", "properties": {"reason": {"type": "string"}}, "additionalProperties": false}
                    }));
                }
                // A ledger session's serving answers the protocol line's lifecycle
                // calls with acknowledgements; the ledger records are the commit.
                // Mirrors docs/compiler/tools.md#compilation-tools.
                if !self.modes.iter().any(|m| m == "compile")
                    && self.bridge.only.is_some()
                    && self.modes.iter().any(|m| m == "generate" || m == "verify")
                {
                    tools.push(json!({
                        "name": "begin_goals",
                        "description": "Acknowledge the batch this serving already holds. The work runs through the ledger tools; finish with done.",
                        "inputSchema": {"type": "object", "properties": {"batch": {"type": "string"}}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "done",
                        "description": "Acknowledge the end of the batch. The ledger records (record_binding, record_generation, record_verdict) are the commit; a row recorded resolves its goal.",
                        "inputSchema": {"type": "object", "properties": {"summary": {"type": "string"}}, "additionalProperties": false}
                    }));
                }
                if self.modes.iter().any(|m| m == "benchmark") {
                    tools.push(json!({
                        "name": "benchmark_cases",
                        "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/the-benchmark-case-list-with-each-case-1dc00.md")),
                        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "begin_case",
                        "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/claim-the-named-case-or-the-first-7014a.md")),
                        "inputSchema": {"type": "object", "properties": {"case": {"type": "string"}}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "finish_case",
                        "description": "Grade the open case with its deterministic checks and discard the sandbox. Turn cases face the done gates first (a rejection keeps the case open; repair and finish again). Verification cases pass verdict (pass|fail) and evidence.",
                        "inputSchema": {"type": "object", "properties": {"summary": {"type": "string"}, "verdict": {"type": "string", "enum": ["pass", "fail"]}, "evidence": {"type": "string"}}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "benchmark_report",
                        "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/close-the-run-tier-scores-workflow-verdicts-cbcac.md")),
                        "inputSchema": {"type": "object", "properties": {"model": {"type": "string"}}, "additionalProperties": false}
                    }));
                }
                if self.modes.iter().any(|m| m == "decompile") {
                    tools.push(json!({
                        "name": "decompile_tasks",
                        "description": "Draft tasks derived from the unclaimed report (deliverable files no binding names), grouped by scope, each ready or gated behind a decompile release (`jazyk decompile` or the GUI records one). Next: begin_decompile on a released scope.",
                        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "begin_decompile",
                        "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/the-draft-package-for-one-scope-the-6b78c.md")),
                        "inputSchema": {"type": "object", "properties": {"scope": {"type": "string"}}, "required": ["scope"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "submit_draft",
                        "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/validate-and-land-the-draft-in-the-3dba3.md")),
                        "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}, "scope": {"type": "string"}}, "required": ["path", "content"], "additionalProperties": false}
                    }));
                }
                if !self.bridge.reader && self.modes.iter().any(|m| m == "chat") {
                    tools.push(json!({
                        "name": "revise_requirement",
                        "description": "Change one requirement: the new prose replaces the old verbatim quote in its source document, and the graph node updates in the same atomic commit. Optional statement carries the new statement (defaults to keeping the old one).",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "new_text": {"type": "string", "description": "the new prose sentence, written into the document"}, "statement": {"type": "string"}}, "required": ["id", "new_text"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "add_requirement",
                        "description": "Add one requirement: the prose sentence is inserted into the named section (after after_quote when given, else at the section's end) and the requirement lands in the same atomic commit.",
                        "inputSchema": {"type": "object", "properties": {"doc": {"type": "string"}, "section": {"type": "string"}, "text": {"type": "string", "description": "the prose sentence inserted into the document"}, "statement": {"type": "string"}, "entities": {"type": "array", "items": {"type": "string"}}, "after_quote": {"type": "string"}}, "required": ["doc", "section", "text", "statement", "entities"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "retract_requirement",
                        "description": "Remove one requirement: its sentence leaves the prose and the node leaves the graph, one atomic commit. The deletion writes its change records, so a view or instance that referenced it gets a retrace goal.",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "reason": {"type": "string"}}, "required": ["id", "reason"], "additionalProperties": false}
                    }));
                    if let Some(t) = crate::tools::catalog()
                        .into_iter()
                        .find(|t| t.name == "edit_fact")
                    {
                        tools.push(json!({"name": t.name, "description": t.description, "inputSchema": t.parameters}));
                    }
                    // The two decree tools beside edit_fact: undo one, raise one
                    // threshold. Mirrors docs/compiler/tools.md#chat-tools.
                    tools.push(json!({
                        "name": "retract_decree",
                        "description": "Undo one decree: a node created by decree is deleted; a field decreed over a quoted fact returns to the prior value and source its journal entry recorded. The open ratify goal closes. One atomic commit.",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "reason": {"type": "string"}}, "required": ["id", "reason"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "bump_limit",
                        "description": "Raise one entity's or view's own threshold above the built-in limit (limits: {<limit>: value}), recorded with decree provenance. limit is a registry name that applies to the node's kind, value positive. This dismisses a size goal: it stops deriving until the count crosses the raised threshold.",
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "limit": {"type": "string"}, "value": {"type": "integer", "minimum": 1}, "note": {"type": "string"}}, "required": ["id", "limit", "value"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "answer_diagnostic",
                        "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/record-a-human-answer-to-a-diagnostic-770a7.md")),
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "option": {"type": "integer", "minimum": 0}, "text": {"type": "string"}}, "required": ["id"], "additionalProperties": false}
                    }));
                    tools.push(json!({
                        "name": "update_diagnostic",
                        "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/replace-the-question-attached-to-an-open-bae21.md")),
                        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "prompt": crate::tools::prompt_schema()}, "required": ["id"], "additionalProperties": false}
                    }));
                    // Scaffolding is offered only where there is something to scaffold,
                    // and settings only where a jazyk.toml holds them. Listing both
                    // everywhere spends a call to earn a refusal.
                    // Mirrors docs/frontends/acp.md#project-tools.
                    if !self.initialized() {
                        tools.push(json!({
                            "name": "init_project",
                            "description": "Scaffold a jazyk project in this directory: jazyk.toml, docs/ with a placeholder root document, and deliverable/. This directory has none yet.",
                            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
                        }));
                    } else {
                        tools.push(json!({
                            "name": "update_project_settings",
                            "description": "Edit jazyk.toml keys as minimal line edits. Supported keys: workflow.compile and workflow.generate (manual; auto needs a human and is refused), workflow.worker (internal|agent|any), gen.worker (agentic|pipeline). baseHash is the configuration hash from list_document_sets; refused while a build runs.",
                            "inputSchema": {"type": "object", "properties": {"settings": {"type": "object", "additionalProperties": {"type": "string"}}, "baseHash": {"type": "string", "description": "configuration hash from list_document_sets"}}, "required": ["settings", "baseHash"], "additionalProperties": false}
                        }));
                    }
                }
                // An ephemeral serving exists for one task; a long poll there is a
                // stall wearing a tool's name, so it is not offered.
                // Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
                if !self.bridge.ephemeral {
                    tools.push(Self::recent_edits_tool());
                    tools.push(json!({
                        "name": "await_changes",
                        "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/long-poll-returns-when-the-graph-moves-1512b.md")),
                        "inputSchema": {"type": "object", "properties": {"timeout_seconds": {"type": "integer", "description": "seconds before returning unchanged, at most 3600; 0 = wait 3600"}}, "additionalProperties": false}
                    }));
                }
                // One name, one tool, whatever the modes: the union of servings must
                // not list a shared tool twice.
                let mut seen: BTreeSet<String> = BTreeSet::new();
                tools.retain(|t| seen.insert(t["name"].as_str().unwrap_or_default().to_string()));
                self.scope_catalog(&mut tools);
                Ok(json!({"tools": tools}))
            }
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default().to_string();
                let previous_config = (name == "update_tasks")
                    .then(|| {
                        crate::vfs::read_to_string(
                            self.project.root.join(crate::document_sets::CONFIG),
                        )
                        .ok()
                        .map(|source| crate::model::hash_hex(&source))
                    })
                    .flatten();
                // The transcript row: the call under its task's label, condensed the
                // same way a turn's rows are. Mirrors docs/frontends/mcp.md#transcripts.
                let label = match self.open.lock().unwrap().as_ref() {
                    Some(o) => o.id.clone(),
                    None => format!("mcp {}", self.modes.join(",")),
                };
                self.trace.event(crate::session::TraceEvent::ToolCall {
                    activity: crate::live_preview::activity(&name, &params["arguments"], None),
                    label: label.clone(),
                    name: name.clone(),
                    summary: crate::session::condense(&params["arguments"], 160),
                    full: crate::session::full_payload(&params["arguments"]),
                });
                let reply = self.tool_call(&name, params, &label).map(|reply| {
                    if name == "update_tasks" {
                        self.accept_task_revision(&reply, previous_config.as_deref());
                    }
                    self.worker_reply(&name, reply)
                });
                if let Ok(v) = &reply {
                    let is_err = v["isError"] == true;
                    let text = v["content"][0]["text"].as_str().unwrap_or_default();
                    let parsed: Value = serde_json::from_str(text).unwrap_or_else(|_| json!(text));
                    if is_err {
                        let (rule, message) = crate::session::refusal(text, "error");
                        self.trace.event(crate::session::TraceEvent::ToolError {
                            label: label.clone(),
                            rule,
                            message,
                        });
                    } else {
                        self.trace.event(crate::session::TraceEvent::ToolResult {
                            activity: crate::live_preview::activity(
                                &name,
                                &params["arguments"],
                                Some(&parsed),
                            ),
                            label: label.clone(),
                            name: name.clone(),
                            summary: crate::session::condense(&parsed, 160),
                            full: crate::session::full_payload(&parsed),
                        });
                    }
                }
                self.update_preview(
                    &name,
                    &label,
                    reply.as_ref().is_ok_and(|v| v["isError"] != true),
                );
                reply
            }
            // Not part of MCP (clients end a serving by closing stdin), but agents
            // hand-driving the transport send it out of LSP habit; answering it is
            // cheaper than teaching every agent the difference.
            "shutdown" => Ok(Value::Null),
            _ => Err((-32601, format!("method not found: {}", method))),
        }
    }

    fn tool_call(&self, name: &str, params: &Value, _label: &str) -> Result<Value, (i64, String)> {
        // A reader serving answers reads alone, whatever its mode lists elsewhere: the
        // chat tools, the lifecycles, and every write are not served there
        // (docs/frontends/wasm.md#chat).
        if self.bridge.reader
            && !crate::tools::READ_TOOLS.contains(&name)
            && !matches!(name, "await_changes" | "recent_edits")
        {
            return Err((-32602, format!("{UNKNOWN_TOOL} `{}`", name)));
        }
        {
            {
                let name = name.to_string();
                match name.as_str() {
                    #[cfg(not(target_arch = "wasm32"))]
                    "release_work"
                        if !self.bridge.reader && self.modes.iter().any(|m| m == "chat") =>
                    {
                        let stage = params["arguments"]["stage"].as_str().unwrap_or_default();
                        if !matches!(stage, "compile" | "generate") {
                            return Ok(text_result(
                                json!({"error":{"rule":"bad-argument","message":"stage must be compile or generate"}}),
                                true,
                            ));
                        }
                        if self.open.lock().unwrap().is_some()
                            || crate::control::build_lease(&self.out).is_some()
                        {
                            return Ok(text_result(
                                json!({"error":{"rule":"work-running","message":"Finish the open batch or wait for the running build before releasing work."}}),
                                true,
                            ));
                        }
                        let _lock = crate::document_sets::ProjectLock::acquire(
                            &self.project.root,
                            "project.lock",
                        )
                        .map_err(|e| (-32602, e.1))?;
                        crate::control::release(&self.project, &self.out, Some(stage));
                        return Ok(text_result(
                            json!({"released":stage,"generation":Store::load(&self.out).status.generation,"next":SERVING.get(if stage == "generate" {"released-generate"} else {"released-compile"})}),
                            false,
                        ));
                    }
                    "await_changes" => {
                        // Waiting makes no sense mid-task, and on an ephemeral
                        // serving there is no between-task to wait for.
                        if self.bridge.ephemeral {
                            return Ok(text_result(
                                json!({"error": {"rule": "not-served",
                                "message": "this serving exists for one task; there is nothing to await. Finish the open task with done."}}),
                                true,
                            ));
                        }
                        if self.open.lock().unwrap().is_some() {
                            return Ok(text_result(
                                json!({"error": {"rule": "batch-open",
                                "message": "a batch is open; awaiting changes now is a stall. Finish it with done (or abandon_goals), then await."}}),
                                true,
                            ));
                        }
                        return Ok(text_result(self.await_changes(params), false));
                    }
                    "goals" if self.modes.iter().any(|m| m == "compile" || m == "chat") => {
                        let full = params["arguments"]["full"].as_bool() == Some(true);
                        let v = self.goals_answer(full);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    "begin_goals" if self.modes.iter().any(|m| m == "compile" || m == "chat") => {
                        let v = self.begin_goals(params);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    // `done` is what every batch's instructions say; on a compile
                    // serving it is the finish. One verb everywhere.
                    "done"
                        if self.modes.iter().any(|m| m == "compile" || m == "chat")
                            && self.bench.lock().unwrap().open.is_none() =>
                    {
                        if let Some(v) = self.session_complete() {
                            return Ok(text_result(v, true));
                        }
                        if self.open.lock().unwrap().is_none() {
                            return Ok(text_result(
                                json!({"error": {"rule": "no-open-batch", "message": "no batch is open; begin_goals first"}}),
                                true,
                            ));
                        }
                        let v = self.done_batch(params);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    "abandon_goals" if self.modes.iter().any(|m| m == "compile" || m == "chat") => {
                        let v = self.abandon_goals(params);
                        let is_err = !v["error"].is_null();
                        return Ok(text_result(v, is_err));
                    }
                    // A ledger session's serving already holds its batch: the
                    // lifecycle calls answer with acknowledgements, and the ledger
                    // records are the commit.
                    // Mirrors docs/compiler/tools.md#compilation-tools.
                    "goals" | "begin_goals" | "done" | "abandon_goals"
                        if self.bridge.only.is_some()
                            && self.modes.iter().any(|m| m == "generate" || m == "verify") =>
                    {
                        let only = self.bridge.only.clone().unwrap_or_default();
                        let v = match name.as_str() {
                            "begin_goals" => json!({"batch": only,
                                "note": "this serving already holds its batch; work through the ledger tools, then done"}),
                            "done" => json!({"ok": true, "batch": only,
                                "note": "the ledger records are the commit for this batch; a row recorded resolves its goal"}),
                            "abandon_goals" => json!({"abandoned": only,
                                "note": "nothing was staged here; unrecorded rows stay owed"}),
                            _ => crate::board::Board::compute(&self.current_project(), &self.out)
                                .answer(),
                        };
                        return Ok(text_result(v, false));
                    }
                    "benchmark_cases" if self.modes.iter().any(|m| m == "benchmark") => {
                        return Ok(text_result(self.benchmark_cases(), false))
                    }
                    "begin_case" if self.modes.iter().any(|m| m == "benchmark") => {
                        return Ok(text_result(self.begin_case(params), false))
                    }
                    "finish_case" if self.modes.iter().any(|m| m == "benchmark") => {
                        return Ok(text_result(self.finish_case(params), false))
                    }
                    "benchmark_report" if self.modes.iter().any(|m| m == "benchmark") => {
                        return Ok(text_result(self.benchmark_report(params), false))
                    }
                    "decompile_tasks" if self.modes.iter().any(|m| m == "decompile") => {
                        let store = Store::load(&self.out);
                        let gs = self.current_gen_settings();
                        let control = crate::control::Control::load(&self.project, &self.out);
                        let tasks = crate::decompile::pending(&self.project, &store, &gs, &control);
                        let reply = if tasks.is_empty() {
                            json!({"tasks": [], "note": SERVING.get("decompile-none")})
                        } else {
                            json!({"tasks": tasks, "next": SERVING.get("decompile-next")})
                        };
                        return Ok(text_result(reply, false));
                    }
                    "begin_decompile" if self.modes.iter().any(|m| m == "decompile") => {
                        let Some(scope) = params["arguments"]["scope"].as_str() else {
                            return Ok(text_result(
                                json!({"error": {"rule": "missing-argument", "message": "scope is required; decompile_tasks lists the scopes"}}),
                                true,
                            ));
                        };
                        let store = Store::load(&self.out);
                        let gs = self.current_gen_settings();
                        let control = crate::control::Control::load(&self.project, &self.out);
                        let released = control
                            .released
                            .decompile
                            .iter()
                            .any(|s| s == scope || s == ".");
                        if !released {
                            return Ok(text_result(
                                json!({"error": {"rule": "awaiting-release", "message": format!(
                                "scope `{}` is not released for decompilation; `jazyk decompile {}` or the GUI's decompile action approves it", scope, scope)}}),
                                true,
                            ));
                        }
                        let reply = match crate::decompile::task(&self.project, &store, &gs, scope)
                        {
                            Ok(v) => v,
                            Err(e) => json!({"error": {"rule": "unknown-scope", "message": e}}),
                        };
                        let is_err = !reply["error"].is_null();
                        return Ok(text_result(reply, is_err));
                    }
                    "revise_requirement" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.revise_requirement(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "add_requirement" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.add_requirement(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "retract_requirement" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.retract_requirement(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "update_criterion"
                        if !self.bridge.reader
                            && self.modes.iter().any(|m| m == "chat")
                            && self.open.lock().unwrap().is_none() =>
                    {
                        let r = self.update_criterion_chat(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "edit_fact" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.edit_fact(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "retract_decree" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.retract_decree(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "bump_limit" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.bump_limit(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "answer_diagnostic" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.answer_diagnostic(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "update_diagnostic" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.update_diagnostic_chat(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "init_project" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.init_project();
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "update_project_settings" if self.modes.iter().any(|m| m == "chat") => {
                        let r = self.update_project_settings(&params["arguments"]);
                        let is_err = !r["error"].is_null();
                        return Ok(text_result(r, is_err));
                    }
                    "submit_draft" if self.modes.iter().any(|m| m == "decompile") => {
                        let path = params["arguments"]["path"].as_str().unwrap_or_default();
                        let content = params["arguments"]["content"].as_str().unwrap_or_default();
                        let scope = params["arguments"]["scope"].as_str();
                        let reply = match crate::decompile::submit(
                            &self.project,
                            &self.out,
                            path,
                            content,
                            scope,
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                return Ok(text_result(
                                    json!({"error": {"rule": "bad-draft", "message": e}}),
                                    true,
                                ))
                            }
                        };
                        return Ok(text_result(reply, false));
                    }
                    _ => {}
                }
                let args = params["arguments"].clone();
                let enabled = self.enabled_tools();
                if !enabled.contains(&name.as_str()) {
                    return Err((-32602, format!("{UNKNOWN_TOOL} `{}`", name)));
                }
                let is_write = !crate::tools::READ_TOOLS.contains(&name.as_str())
                    && name != crate::tools::FEEDBACK_TOOL;
                let is_graph_write = is_write
                    && !crate::tools::GEN_TOOLS.contains(&name.as_str())
                    && !crate::tools::BIND_TOOLS.contains(&name.as_str())
                    && !crate::tools::VERIFY_TOOLS.contains(&name.as_str())
                    && !crate::tools::FILE_TOOLS.contains(&name.as_str())
                    && !crate::task_runtime::TOOLS.contains(&name.as_str());

                // An open benchmark case takes every call first: the sandbox is the
                // world under test. Mirrors docs/benchmark/benchmark.md#agent-run-benchmarks.
                {
                    let mut b = self.bench.lock().unwrap();
                    if let Some(o) = b.open.as_mut() {
                        let allowed = {
                            let mut t = toolset(&o.item.task);
                            t.extend(crate::tools::GEN_TOOLS);
                            t.push("run_tests");
                            t
                        };
                        if !allowed.contains(&name.as_str()) && name != crate::tools::FEEDBACK_TOOL
                        {
                            return Ok(text_result(
                                json!({"error": {"rule": "wrong-toolset", "message": format!(
                                    "`{}` is not part of a {} case", name, o.item.task)}}),
                                true,
                            ));
                        }
                        o.calls += 1;
                        return match o.session.dispatch(&name, &args) {
                            Ok(v) => Ok(text_result(v, false)),
                            Err(e) => Ok(text_result(e.to_value(), true)),
                        };
                    }
                }
                // Graph writes and goal tools stage into the open batch's changeset,
                // narrowed to the union of the batch's goal kinds' toolsets. Mirrors
                // docs/frontends/mcp.md#compilation-over-mcp.
                let mut open = self.open.lock().unwrap();
                if let Some(o) = open.as_mut() {
                    let kinds = o.session.scope.kinds();
                    if kinds.iter().all(|kind| kind == "verify")
                        && matches!(
                            name.as_str(),
                            "write_text_file" | "link_asset" | "remove_asset_link" | "run_command"
                        )
                    {
                        return Ok(text_result(
                            json!({"error":{"rule":"wrong-toolset","message":"verification may read implementing files but cannot write them"}}),
                            true,
                        ));
                    }
                    let kind_refs: Vec<&str> = kinds.iter().map(String::as_str).collect();
                    let allowed = toolset_for_kinds(&kind_refs);
                    if is_graph_write && !allowed.contains(&name.as_str()) {
                        return Ok(text_result(
                            json!({"error": {"rule": "wrong-toolset", "message": format!(
                                "`{}` is not part of a {} batch; this batch's write tools: {}",
                                name, kinds.join("+"),
                                allowed.iter().filter(|t| !crate::tools::READ_TOOLS.contains(t) && !crate::tools::GOAL_TOOLS.contains(t) && **t != crate::tools::FEEDBACK_TOOL && **t != crate::tools::SPEND_TOOL).cloned().collect::<Vec<_>>().join(", "))}}),
                            true,
                        ));
                    }
                    o.rounds += 1;
                    // Activity on the open batch keeps its lease alive.
                    crate::control::refresh_lease(&self.out, &o.id);
                    return match o.session.dispatch(&name, &args) {
                        Ok(v) => Ok(text_result(v, false)),
                        Err(e) => Ok(text_result(e.to_value(), true)),
                    };
                }
                if name == "classify_input"
                    && !self.bridge.reader
                    && self.modes.iter().any(|m| m == "chat")
                {
                    if let Some(value) = self.session_complete_closed() {
                        return Ok(text_result(value, true));
                    }
                    drop(open);
                    let reply = self.classify_input_chat(&args);
                    let is_err = !reply["error"].is_null();
                    return Ok(text_result(reply, is_err));
                }
                if is_graph_write && self.modes.iter().any(|m| m == "compile" || m == "chat") {
                    if let Some(v) = self.session_complete_closed() {
                        return Ok(text_result(v, true));
                    }
                    // Binding and generation have no batch: a contradiction the tests
                    // expose reaches the author through the failing-binding check
                    // (docs/frontends/mcp.md#generation-and-verification-over-mcp).
                    let lifecycle = self.modes.iter().any(|m| m == "generate" || m == "bind");
                    let message = if lifecycle {
                        include_str!(concat!(
                            env!("OUT_DIR"),
                            "/payloads/goals/prompts/inline/mcp-dispatch/no-batch-is-open-begin-goals-first-82da8.md"
                        ))
                    } else {
                        "no batch is open; begin_goals first, then stage writes into it"
                    };
                    return Ok(text_result(
                        json!({"error": {"rule": "no-open-batch", "message": message}}),
                        true,
                    ));
                }
                drop(open);

                // The ledger task tools answer from the board: the open bind,
                // generate, and verify goals, which the ledger lifecycles claim and
                // recording resolves. Mirrors
                // docs/frontends/mcp.md#generation-and-verification-over-mcp.
                if matches!(
                    name.as_str(),
                    "binding_tasks" | "generation_tasks" | "verification_tasks"
                ) {
                    let kind = match name.as_str() {
                        "binding_tasks" => "bind",
                        "generation_tasks" => "generate",
                        _ => "verify",
                    };
                    let board = crate::board::Board::compute(&self.current_project(), &self.out);
                    let rows: Vec<Value> = board
                        .goals
                        .iter()
                        .filter(|g| g.kind == kind)
                        .map(|g| {
                            let mut v = json!({
                                "goal": g.id,
                                "reason": g.change["reason"],
                                "state": g.state,
                                "gated": board.gated.contains(&g.id),
                                "ready": board.is_ready(&g.id),
                            });
                            if let Some(r) = board.readiness.get(&g.id).and_then(|r| r.reason()) {
                                v["blockedBy"] = json!(r);
                            }
                            if kind == "generate" {
                                v["entity"] = json!(g.target);
                            } else {
                                v["requirement"] = json!(g.target);
                                v["entity"] = g.change["entity"].clone();
                            }
                            v
                        })
                        .collect();
                    let begin = match kind {
                        "bind" => "begin_binding",
                        "generate" => "begin_generation",
                        _ => "begin_verification (or run_tests for programmatic rows)",
                    };
                    // Rows that went stale under a generation (a shared test file, an
                    // implementation file) hold no generate goal until run_tests
                    // re-baselines them; say so instead of letting the list read as
                    // complete (docs/frontends/mcp.md#generation-and-verification-over-mcp).
                    let stale_note = if kind == "generate" {
                        let gs = self.current_gen_settings();
                        let ledger = crate::gen::Ledger::load(&self.out);
                        let store = Store::load(&self.out);
                        let stale = ledger
                            .requirements
                            .iter()
                            .filter(|(rid, row)| {
                                store.graph.requirements.contains_key(*rid)
                                    && matches!(
                                        crate::verify::status_of(&store, rid, row, &gs).0.as_str(),
                                        "stale-test" | "stale-code"
                                    )
                            })
                            .count();
                        (stale > 0)
                            .then(|| SERVING.fill("ledger-stale", &[("count", &stale.to_string())]))
                    } else {
                        None
                    };
                    let binding = if kind == "generate" {
                        board.goals.iter().filter(|g| g.kind == "bind").count()
                    } else {
                        0
                    };
                    let mut reply = json!({
                        "tasks": rows,
                        "count": rows.len(),
                        "next": if rows.is_empty() {
                            SERVING.get("ledger-nothing-owed").to_string()
                        } else if rows.iter().all(|r| r["gated"] == json!(true)) {
                            SERVING.fill("ledger-gated", &[("begin", begin)])
                        } else {
                            SERVING.fill("ledger-ready", &[("begin", begin)])
                        },
                    });
                    if kind == "generate" {
                        reply["bindingWork"] = json!(binding);
                    }
                    if matches!(kind, "bind" | "generate")
                        && !self.current_gen_settings().configured
                    {
                        reply["prerequisites"]["sourceMapping"] = json!(false);
                        reply["next"] = json!(SERVING.get("source-mapping"));
                    }
                    if let Some(n) = stale_note {
                        reply["staleNote"] = json!(n);
                    }
                    return Ok(text_result(reply, false));
                }

                // The control plane over the stateless generation lifecycle: manual
                // mode gates begins behind a release, begin claims the entity's
                // lease, record frees it. Mirrors docs/frontends/mcp.md#the-control-plane-over-mcp.
                if name == "begin_generation" {
                    let c = crate::control::Control::load(&self.project, &self.out);
                    if self.bridge.build_token.is_none() {
                        if c.generate == "manual"
                            && c.released.generate != Store::load(&self.out).status.generation
                        {
                            return Ok(text_result(
                                json!({"error": {"rule": "awaiting-release", "message":
                                    "generation is gated: the workflow is manual and the graph's changes are not released yet; `jazyk release generate` or the GUI's generate action approves them"}}),
                                true,
                            ));
                        }
                        if let Some(l) = crate::control::build_lease(&self.out) {
                            return Ok(text_result(
                                json!({"error": {"rule": "build-running", "message": format!(
                                    "an internal build is running (lease `{}`, expires {}s after its last 30s heartbeat); await_changes returns when it clears", l.worker, crate::control::LEASE_TTL_SECS)}}),
                                true,
                            ));
                        }
                    }
                    if let Some(ent) = args["entity"].as_str() {
                        // A generate goal that is not ready (its requirements still owe
                        // a binding, a compile goal open in its cone) is refused with
                        // the board's reason: a Codex session once bound one requirement
                        // and generated the whole entity next.
                        // Mirrors docs/compiler/goals/generate.md#readiness.
                        let mut board =
                            crate::board::Board::compute(&self.current_project(), &self.out);
                        board.mark_own(&self.worker_id());
                        let gid = format!("g:generate:{}", ent);
                        // A lease (this serving's own retry, or another worker's) is the
                        // claim below's to answer.
                        // An internal run's session is the approval: the release gate
                        // never refuses it, only real readiness does, since the run's own
                        // commits advance the generation past the release it stamped
                        // (docs/compiler/control-plane.md#modes-and-releases).
                        let only_gated =
                            self.bridge.build_token.is_some() && board.gated.contains(&gid);
                        if board.goals.iter().any(|g| g.id == gid)
                            && !board.is_ready(&gid)
                            && !board.claimed.contains_key(&gid)
                            && !only_gated
                        {
                            if let Some(r) = board.readiness.get(&gid).and_then(|r| r.reason()) {
                                return Ok(text_result(
                                    json!({"error": {"rule": "not-ready", "message": format!(
                                        "`{}` is not ready to generate: {}; generation_tasks lists the rows that are", ent, r)}}),
                                    true,
                                ));
                            }
                        }
                        if let Err(holder) =
                            crate::control::claim(&self.out, ent, &self.worker_id())
                        {
                            return Ok(text_result(
                                json!({"error": {"rule": "claimed", "message": format!(
                                    "`{}` is claimed by worker `{}`; pick another entity or wait for the lease to lapse ({}s after its last activity)", ent, holder, crate::control::LEASE_TTL_SECS)}}),
                                true,
                            ));
                        }
                    }
                }
                // Binding rides the same control plane: it writes test files into the
                // deliverable, so the generate release gates it, and the requirement
                // lease makes the claim exclusive.
                // Mirrors docs/consumers/bind.md#when-binding-runs.
                if name == "begin_binding" {
                    let c = crate::control::Control::load(&self.project, &self.out);
                    if self.bridge.build_token.is_none() {
                        if c.generate == "manual"
                            && c.released.generate != Store::load(&self.out).status.generation
                        {
                            return Ok(text_result(
                                json!({"error": {"rule": "awaiting-release", "message":
                                    "binding is gated: the workflow is manual and the graph's changes are not released yet; `jazyk release generate` or the GUI's generate action approves them"}}),
                                true,
                            ));
                        }
                        if let Some(l) = crate::control::build_lease(&self.out) {
                            return Ok(text_result(
                                json!({"error": {"rule": "build-running", "message": format!(
                                    "an internal build is running (lease `{}`, expires {}s after its last 30s heartbeat); await_changes returns when it clears", l.worker, crate::control::LEASE_TTL_SECS)}}),
                                true,
                            ));
                        }
                    }
                    if let Some(rid) = args["requirement"].as_str() {
                        if let Err(holder) =
                            crate::control::claim(&self.out, rid, &self.worker_id())
                        {
                            return Ok(text_result(
                                json!({"error": {"rule": "claimed", "message": format!(
                                    "`{}` is claimed by worker `{}`; pick another requirement or wait for the lease to lapse ({}s after its last activity)", rid, holder, crate::control::LEASE_TTL_SECS)}}),
                                true,
                            ));
                        }
                    }
                }

                if matches!(name.as_str(), "record_generation" | "record_binding") {
                    if let Some(target) = args[if name == "record_generation" {
                        "entity"
                    } else {
                        "requirement"
                    }]
                    .as_str()
                    {
                        if let Some(lease) = crate::control::leases(&self.out).get(target) {
                            if lease.worker != self.worker_id() {
                                return Ok(text_result(
                                    json!({"error":{"rule":"claimed","message":format!("{target} is claimed by worker {}; wait for that claim to finish.", lease.worker)}}),
                                    true,
                                ));
                            }
                        }
                    }
                }
                if crate::task_runtime::TOOLS.contains(&name.as_str()) {
                    // A generation or verification serving owns the tasks it adds; a
                    // chat adds the author's. Mirrors docs/consumers/gen.md#session-tasks-are-swept-when-unused.
                    // A worker holding a binding or generation claim is a ledger session
                    // whatever its serving (the default `all` includes chat).
                    let claimed = crate::control::leases(&self.out)
                        .values()
                        .any(|l| l.worker == self.worker_id());
                    let owner = (name == "update_tasks")
                        .then(|| {
                            if claimed {
                                return Some(format!("mcp:{}", self.worker_id()));
                            }
                            if self.modes.iter().any(|m| m == "chat") {
                                return None;
                            }
                            self.modes
                                .iter()
                                .find(|m| matches!(m.as_str(), "generate" | "verify"))
                                .map(|m| format!("mcp-{m}"))
                        })
                        .flatten();
                    let before = owner
                        .as_ref()
                        .and_then(|_| crate::task_runtime::load(&self.project.root).ok())
                        .map(|(_, t)| t);
                    return match crate::task_runtime::call(&self.project.root, &name, &args) {
                        Ok(mut value) => {
                            if let (Some(owner), Some(before)) = (owner, before) {
                                let after: Vec<crate::task_runtime::Task> =
                                    serde_json::from_value(value["tasks"].clone())
                                        .unwrap_or_default();
                                if let Err(e) = crate::task_runtime::record_owned(
                                    &self.out, &before, &after, &owner,
                                ) {
                                    value["ownershipNote"] = json!(format!("Tasks saved; ownership was not recorded, so they are kept: {e}"));
                                }
                            }
                            Ok(text_result(value, false))
                        }
                        Err(error) => Ok(text_result(
                            json!({"error":{"rule":"task-runtime","message":error}}),
                            true,
                        )),
                    };
                }
                let store = Store::load(&self.out);
                if store.docs.is_empty()
                    && store.graph.entities.is_empty()
                    && !self.modes.iter().any(|m| m == "compile")
                    && !matches!(
                        name.as_str(),
                        "inspect_input" | "classify_input" | "report_feedback"
                    )
                    && name != crate::tools::SPEND_TOOL
                {
                    return Ok(text_result(
                        json!({"error": {"rule": "no-build", "message": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-dispatch/no-graph-found-run-jazyk-compile-first-62ec5.md"))}}),
                        true,
                    ));
                }
                let mut scope = WorkScope::serving(if is_write { "mcp-write" } else { "mcp-read" });
                // A ledger session's serving holds its batch by id: the batch target is
                // whose component group the file tools write for, so a group session
                // extends its members' files and a regeneration rewrites its own.
                // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
                if let Some((_, target)) = self
                    .bridge
                    .only
                    .as_deref()
                    .and_then(crate::goals::parse_goal_id)
                {
                    scope.target = target.to_string();
                } else if let Some(target) = self.ledger_target.lock().unwrap().clone() {
                    if matches!(
                        name.as_str(),
                        "write_text_file" | "link_asset" | "remove_asset_link"
                    ) {
                        if let Err(holder) =
                            crate::control::claim(&self.out, &target, &self.worker_id())
                        {
                            return Ok(text_result(
                                json!({"error":{"rule":"claimed","message":format!("{target} is claimed by worker {holder}; wait or begin another entity.")}}),
                                true,
                            ));
                        }
                    }
                    scope.target = target;
                }
                let unit = (!scope.target.is_empty()).then(|| scope.target.clone());
                let mut session =
                    ToolSession::new(store, scope, self.mutation_limit, self.context_budget);
                session.gen = self.current_gen_settings();
                session.lint = self.project.linting.clone();
                session.caller =
                    self.caller(if self.write { "mcp-write" } else { "mcp-read" }, &name);
                // One session per call, one guard per unit: the serving's ledger claim,
                // or its calls outside any claim.
                {
                    let mut memory = self.call_memory.lock().unwrap();
                    if memory.0 != unit {
                        *memory = (unit.clone(), Default::default());
                    }
                    session.adopt_memory(memory.1.clone());
                }
                let reply = session.dispatch(&name, &args);
                *self.call_memory.lock().unwrap() = (unit, session.memory());
                match reply {
                    Ok(v) => {
                        if matches!(name.as_str(), "begin_generation" | "begin_binding") {
                            if let Some(target) = args[if name == "begin_generation" {
                                "entity"
                            } else {
                                "requirement"
                            }]
                            .as_str()
                            {
                                let previous = self
                                    .ledger_target
                                    .lock()
                                    .unwrap()
                                    .replace(target.to_owned());
                                if let Some(previous) = previous.filter(|p| p != target) {
                                    self.release_own_ledger_claim(&previous);
                                }
                            }
                        } else if matches!(name.as_str(), "record_generation" | "record_binding") {
                            if let Some(target) = args[if name == "record_generation" {
                                "entity"
                            } else {
                                "requirement"
                            }]
                            .as_str()
                            {
                                self.release_own_ledger_claim(target);
                                let mut current = self.ledger_target.lock().unwrap();
                                if current.as_deref() == Some(target) {
                                    *current = None;
                                }
                            }
                        }
                        if is_graph_write && !session.staged.is_empty() {
                            // graph --write: each call commits as its own changeset.
                            // The commit writes its change records, so the next
                            // build derives the goals the write opened.
                            let mut s = Store::load(&self.out);
                            let report = s.apply(
                                session.staged,
                                &crate::store::Commit::session(Vec::new(), 1, None),
                            );
                            let mut v = v;
                            v["committed"] = json!(report.applied);
                            if !report.skipped.is_empty() {
                                v["skipped"] = json!(report.skipped);
                            }
                            return Ok(text_result(v, false));
                        }
                        Ok(text_result(v, false))
                    }
                    Err(e) => {
                        if matches!(name.as_str(), "begin_generation" | "begin_binding") {
                            if let Some(target) = args[if name == "begin_generation" {
                                "entity"
                            } else {
                                "requirement"
                            }]
                            .as_str()
                            {
                                self.release_own_ledger_claim(target);
                            }
                        }
                        Ok(text_result(e.to_value(), true))
                    }
                }
            }
        }
    }
}
