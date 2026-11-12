use super::*;

impl McpServer {
    // A user conversation multiplexes set-local sessions. There is no ambient selection.
    pub(super) fn cancelled(&self) -> bool {
        self.bridge
            .cancel_file
            .as_ref()
            .is_some_and(|p| crate::vfs::exists(p))
    }

    pub async fn handle_async(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        if method != "tools/call" || params["name"] != "compile_document_sets" {
            return self.handle(method, params);
        }
        if self.cancelled() {
            return Err((-32602, "the build was stopped".into()));
        }
        // Neither a read-only nor a scoped serving lists the tool, so neither serves it.
        if !self.serves_project_writes()
            || self.project.set_id.is_some()
            || self.bridge.only.is_some()
            || self.bridge.build_token.is_some()
        {
            return Err((-32602, format!("{UNKNOWN_TOOL} `compile_document_sets`")));
        }
        let workspace =
            crate::document_sets::Workspace::load(&self.project.root).map_err(|e| (-32602, e.1))?;
        let response = crate::webapi::dispatch::request_at_async(
            &workspace.root,
            "POST",
            "/api/compile",
            &params["arguments"].to_string(),
        )
        .await;
        let response: Value =
            serde_json::from_str(&response).map_err(|e| (-32603, e.to_string()))?;
        if let Some(reason) = compile_interruption(&response) {
            return Err((-32603, reason));
        }
        Ok(result_value(response))
    }

    // A refusal inside a tool call is a tool result the agent can read and repair; a
    // JSON-RPC error stays for an unknown method or tool, a call without a name, and a
    // stopped or suspended run. Mirrors docs/frontends/mcp.md#toolsets.
    pub fn handle(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        let reply = self.route(method, params);
        if method != "tools/call" || !params["name"].is_string() {
            return reply;
        }
        match reply {
            Err((code, message))
                if code != -32601
                    && !message.starts_with(UNKNOWN_TOOL)
                    && !crate::session_recovery::is_interruption(&message)
                    && message != "execution suspended"
                    && !self.cancelled() =>
            {
                Ok(text_result(
                    json!({"error": {"rule": "refused", "message": message}}),
                    true,
                ))
            }
            other => other,
        }
    }

    fn route(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        let _runtime_owner =
            crate::task_runtime::CancellationScope::enter(self.bridge.cancel_file.as_deref());
        if method == "tools/call"
            && crate::agent_workflows::TOOLS.contains(&params["name"].as_str().unwrap_or_default())
        {
            if self.modes.len() == 1 && self.modes[0] == "runtime" {
                return Err((
                    -32601,
                    "workflow skills are not served by the dedicated task runtime".into(),
                ));
            }
            let value = if params["name"] == "list_jazyk_skills" {
                crate::agent_workflows::catalog().map_err(|e| (-32603, e))?
            } else {
                let name = params["arguments"]["name"]
                    .as_str()
                    .ok_or((-32602, "name is required".into()))?;
                crate::agent_workflows::read(name).map_err(|e| (-32602, e))?
            };
            return Ok(text_result(value, false));
        }
        if method == "tools/call" {
            if let Some(error) = self.current_project().config_error {
                return Err((-32602, error));
            }
        }
        if self.cancelled()
            && method == "tools/call"
            && !matches!(
                params["name"].as_str(),
                Some(
                    "project_context"
                        | "abandon_goals"
                        | "cancel_run"
                        | "inspect_run"
                        | "wait_run"
                        | "read_run_log"
                        | "search_run_log"
                        | "read_run_changes"
                        | "task_context"
                )
            )
        {
            return Err((
                -32602,
                "the build was stopped; this session cannot commit".into(),
            ));
        }
        if method == "tools/call" && params["name"] == "recent_edits" && !self.bridge.ephemeral {
            return Ok(text_result(self.recent_edits(&params["arguments"]), false));
        }
        if method == "tools/call" && params["name"] == "project_context" {
            return Ok(text_result(
                crate::project_context::read(&self.project.root),
                false,
            ));
        }
        if self.project.set_id.is_some()
            || self.bridge.only.is_some()
            || self.bridge.build_token.is_some()
        {
            if params["arguments"]["setId"]
                .as_str()
                .is_some_and(|id| Some(id) != self.project.set_id.as_deref())
            {
                return Err((
                    -32602,
                    "this worker is scoped to another document set".into(),
                ));
            }
            if method == "tools/list" {
                let mut result = self.handle_local(method, params)?;
                if self.serves_matcher_repairs() {
                    result["tools"]
                        .as_array_mut()
                        .unwrap()
                        .extend(Self::matcher_catalog());
                }
                return Ok(result);
            }
            if method == "tools/call"
                && matches!(
                    params["name"].as_str(),
                    Some("list_document_sets" | "preview_document_set" | "update_document_set")
                )
            {
                return self
                    .scoped_matcher_call(params["name"].as_str().unwrap(), &params["arguments"]);
            }
            return self.handle_local(method, params);
        }
        if method == "tools/call"
            && crate::task_runtime::TOOLS.contains(&params["name"].as_str().unwrap_or_default())
        {
            return self.handle_local(method, params);
        }
        if self.modes.len() == 1 && self.modes[0] == "runtime" {
            return self.handle_local(method, params);
        }
        let w = match crate::document_sets::Workspace::load(&self.project.root) {
            Ok(w) => w,
            // A broken project file refuses tool calls, not the connection: initialize,
            // ping, and tools/list still answer (docs/frontends/mcp.md#mcp-into-acp-sessions).
            Err(e)
                if method == "tools/call"
                    || !matches!(method, "initialize" | "ping" | "tools/list") =>
            {
                return Err((-32602, e.1))
            }
            Err(_) if method == "initialize" => {
                self.watch_edits();
                if let Some(name) = params["clientInfo"]["name"].as_str() {
                    *self.client.lock().unwrap() = Some(name.into());
                }
                return Ok(
                    json!({"protocolVersion":negotiate_protocol(&params["protocolVersion"]),
                    "capabilities":self.capabilities(),"serverInfo":{"name":"jazyk","version":crate::version::VERSION},
                    "instructions":self.initialize_instructions(self.initialized())?}),
                );
            }
            Err(_) => return self.handle_local(method, params),
        };
        if method == "initialize" {
            self.watch_edits();
            // Registration belongs to each set-local child, created lazily.
            if let Some(name) = params["clientInfo"]["name"].as_str() {
                *self.client.lock().unwrap() = Some(name.into());
            }
            return Ok(
                json!({"protocolVersion":negotiate_protocol(&params["protocolVersion"]),
                "capabilities":self.capabilities(),"serverInfo":{"name":"jazyk","version":crate::version::VERSION},
                "instructions":self.initialize_instructions(w.exists)?}),
            );
        }
        let writable = self.serves_project_writes();
        if method == "tools/list" {
            let mut result = self.handle_local(method, params)?;
            let tools = result["tools"].as_array_mut().unwrap();
            for t in tools.iter_mut() {
                // Project-wide tools take no set: project_context says so in its description.
                if crate::agent_workflows::TOOLS.contains(&t["name"].as_str().unwrap_or_default())
                    || t["name"] == "project_context"
                {
                    continue;
                }
                t["inputSchema"]["properties"]["setId"] = json!({"type":"string","description":"Document set ID from list_document_sets. Required when multiple sets exist."});
            }
            tools.push(json!({"name":"list_document_sets","description":"List every document set, its file selections, state and configuration hash.","inputSchema":{"type":"object","properties":{}}}));
            tools.push(json!({"name":"read_document","description":include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-router/read-a-project-document-as-compiler-text-716d3.md")),"inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}}));
            if writable && self.modes.iter().any(|m| m == "chat") {
                tools.push(json!({"name":"write_document","description":include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-router/create-or-replace-one-prose-document-without-92210.md")),"inputSchema":{"type":"object","properties":{"path":{"type":"string"},"text":{"type":"string"},"baseHash":{"type":"string"}},"required":["path","text"],"additionalProperties":false}}));
            }
            if writable {
                for (name,description) in [("preview_document_set","Preview a document-set edit without writing."),("save_document_set","Create or edit one set, preserving its identity and graph."),("create_document_set","Create a named document set."),("update_document_set","Update a document set while preserving its identity."),("remove_document_set","Remove a set from the project. Documents and its retained graph remain on disk.")] {
                    tools.push(json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":{
                        "baseHash":{"type":"string"},"remove":{"type":"string"},
                        "roleIntents":{"type":"array","items":{"type":"object","properties":{"role":{"type":"string","enum":["docs","source"]},"patterns":{"type":"array","items":{"type":"string"}}},"required":["role","patterns"],"additionalProperties":false}},
                        "set":{"type":"object","properties":{
                            "id":{"type":"string"},"name":{"type":"string"},"docs":{"type":"array","items":{"type":"string"}},
                            "roots":{"type":"array","items":{"type":"string"}},"source":{"type":"array","items":{"type":"string"}}}}},"required":["baseHash"]}}));
                }
            }
            if writable {
                tools.push(json!({"name":"compile_document_sets","description":"Compile selected set IDs sequentially, or all dirty sets when setIds is absent.","inputSchema":{"type":"object","properties":{"setIds":{"type":"array","items":{"type":"string"}}}}}));
            }
            self.scope_catalog(result["tools"].as_array_mut().unwrap());
            return Ok(result);
        }
        if method != "tools/call" {
            return self.handle_local(method, params);
        }
        let name = params["name"].as_str().unwrap_or_default();
        if crate::task_runtime::TOOLS.contains(&name) {
            return self.handle_local(method, params);
        }
        if name == "list_document_sets" {
            return Ok(result_value(w.summary()));
        }
        if name == "write_document" {
            if !writable || !self.modes.iter().any(|m| m == "chat") {
                // Listed only on a writable chat serving: elsewhere it is not served.
                return Err((-32602, format!("{UNKNOWN_TOOL} `write_document`")));
            }
            let args = &params["arguments"];
            let path = args["path"]
                .as_str()
                .ok_or((-32602, "path is required".into()))?;
            let text = args["text"]
                .as_str()
                .ok_or((-32602, "text is required".into()))?;
            let project = w.files_project();
            let base = args["baseHash"].as_str();
            // Validate and version-check before asking an attached editor to write.
            if let Some(sink) = &self.bridge.edit_sink {
                let abs = crate::webapi::docs::safe_doc_path_why(&project, path)
                    .map_err(|m| (-32602, m))?;
                if std::path::Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| {
                        e.eq_ignore_ascii_case("docx") || e.eq_ignore_ascii_case("pdf")
                    })
                {
                    return Err((
                        -32602,
                        "binary documents require a format-specific editor".into(),
                    ));
                }
                let old = match crate::vfs::read_to_string(&abs) {
                    Ok(text) => Some(text),
                    Err(error)
                        if error.kind() == std::io::ErrorKind::NotFound
                            && !crate::vfs::exists(&abs) =>
                    {
                        None
                    }
                    Err(error) => {
                        return Err((
                            -32602,
                            format!(
                                "cannot read existing document before editor delegation: {error}"
                            ),
                        ))
                    }
                };
                let hash = old.as_deref().map(crate::model::hash_hex);
                if base != hash.as_deref() || (base.is_none() && old.is_some()) {
                    return Ok(text_result(
                        json!({"error":{"rule":"stale-document","message":"document changed; read_document before retrying","diskHash":hash}}),
                        true,
                    ));
                }
                match sink_write(sink, &abs, old.as_deref().unwrap_or_default(), text, text) {
                    Ok(()) => {
                        let mut v = json!({"path":path,"hash":crate::model::hash_hex(text),"delegated":true});
                        if let Some(f) =
                            crate::effects::file_effect(&project.root, path, old.as_deref(), text)
                        {
                            v["effects"] = json!({"files": [f]});
                        }
                        return Ok(result_value(v));
                    }
                    // Nothing listening: the direct write below, as for any serving.
                    Err(SinkError::Unavailable(reason)) => self.note(&format!(
                        "edit sink unavailable ({reason}); writing {path} directly"
                    )),
                    Err(SinkError::Failed(m)) => {
                        return Ok(text_result(
                            json!({"error":{"rule":"editor-refused","message":format!("the editor did not apply the edit: {m}")}}),
                            true,
                        ))
                    }
                }
            }
            let before = crate::webapi::docs::safe_doc_path_why(&project, path)
                .ok()
                .and_then(|abs| crate::vfs::read_to_string(&abs).ok());
            return match crate::webapi::docs::doc_write(&project, path, text, base) {
                crate::webapi::docs::DocWrite::Written(mut v) => {
                    if let Some(f) =
                        crate::effects::file_effect(&project.root, path, before.as_deref(), text)
                    {
                        v["effects"] = json!({"files": [f]});
                    }
                    Ok(result_value(v))
                }
                crate::webapi::docs::DocWrite::Conflict(v) => Ok(text_result(
                    json!({"error":{"rule":"stale-document","message":v}}),
                    true,
                )),
                crate::webapi::docs::DocWrite::Invalid(m)
                | crate::webapi::docs::DocWrite::Failed(m) => Err((-32602, m)),
            };
        }
        if name == "read_document" {
            let path = params["arguments"]["path"]
                .as_str()
                .ok_or((-32602, "path is required".into()))?;
            let project = w.files_project();
            let binary = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("docx") || e.eq_ignore_ascii_case("pdf"));
            let mut value = if binary {
                crate::webapi::binary_documents::read(&project, path)
                    .map_err(|(_, message)| (-32602, message))?
            } else {
                match crate::webapi::docs::doc_content(&project, path) {
                    crate::webapi::docs::DocRead::Found(value) => value,
                    crate::webapi::docs::DocRead::Missing(message)
                    | crate::webapi::docs::DocRead::Invalid(message) => {
                        return Err((-32602, message))
                    }
                }
            };
            if let Some(object) = value.as_object_mut() {
                object.remove("base64");
            }
            return Ok(result_value(value));
        }
        if name == "compile_document_sets" {
            if !writable {
                return Err((-32602, format!("{UNKNOWN_TOOL} `{name}`")));
            }
            #[cfg(target_arch = "wasm32")]
            return Err((
                -32602,
                "compilation requires the asynchronous MCP entry point".into(),
            ));
            #[cfg(not(target_arch = "wasm32"))]
            {
                let response = crate::webapi::dispatch::request_at(
                    &w.root,
                    "POST",
                    "/api/compile",
                    &params["arguments"].to_string(),
                );
                let response: Value =
                    serde_json::from_str(&response).map_err(|e| (-32603, e.to_string()))?;
                if let Some(reason) = compile_interruption(&response) {
                    return Err((-32603, reason));
                }
                return Ok(result_value(response));
            }
        }
        if matches!(
            name,
            "preview_document_set"
                | "save_document_set"
                | "create_document_set"
                | "update_document_set"
                | "remove_document_set"
        ) {
            if !writable {
                return Err((-32602, format!("{UNKNOWN_TOOL} `{name}`")));
            }
            if matches!(name, "preview_document_set" | "update_document_set") {
                if let Some(id) = params["arguments"]["set"]["id"].as_str() {
                    let children = self.children.lock().unwrap();
                    if let Some(child) = children.get(id).filter(|child| {
                        child.serves_matcher_repairs() && child.open.lock().unwrap().is_some()
                    }) {
                        return child.scoped_matcher_call(name, &params["arguments"]);
                    }
                }
            }
            return crate::document_sets::edit(
                &w.root,
                &params["arguments"],
                name == "preview_document_set",
            )
            .map(result_value)
            .map_err(|e| (-32602, e.1));
        }
        if matches!(name, "init_project" | "update_project_settings") {
            return self.handle_local(method, params);
        }
        let routed = crate::webapi::sets::route("/", params["arguments"].clone())
            .map_err(|e| (-32602, e.1))?;
        let ids = if let Some(id) = routed.selector.as_deref() {
            vec![w.select(id).map_err(|e| (-32602, e.1))?.id.clone()]
        } else if w.sets.len() == 1 {
            vec![w.sets[0].id.clone()]
        } else if matches!(
            name,
            "search"
                | "goals"
                | "graph_status"
                | "diagnostics"
                | "binding_tasks"
                | "generation_tasks"
                | "verification_tasks"
                | "decompile_tasks"
        ) {
            w.sets.iter().map(|s| s.id.clone()).collect()
        } else {
            return Err((
                -32602,
                "pass setId from list_document_sets for this operation".into(),
            ));
        };
        let mut replies = Vec::new();
        let mut children = self.children.lock().unwrap();
        for id in ids {
            if name != "abandon_goals"
                && children
                    .get(&id)
                    .is_some_and(|s| *s.config_revision.lock().unwrap() != w.hash)
            {
                if children[&id].open.lock().unwrap().is_some() {
                    return Err((
                        -32602,
                        "configuration changed during the open batch; abandon it before continuing"
                            .into(),
                    ));
                }
                children.remove(&id);
            }
            if !children.contains_key(&id) {
                let p = w.resolve(Some(&id)).map_err(|e| (-32602, e.1))?;
                let mut child = Self::with_bridge(
                    p.clone(),
                    p.out.clone(),
                    self.modes.clone(),
                    self.write,
                    self.bridge.clone(),
                );
                // A set's call runs under the transport's request: one interrupt record.
                child.interrupts = self.interrupts.clone();
                child.handle_local("initialize",&json!({"clientInfo":{"name":self.client.lock().unwrap().clone().unwrap_or_else(||"agent".into())}}))?;
                children.insert(id.clone(), child);
            }
            let mut local = params.clone();
            local["arguments"] = routed.body.clone();
            if let Some(o) = local["arguments"].as_object_mut() {
                o.remove("setId");
            }
            let reply = children[&id].handle_local(method, &local)?;
            let text = reply["content"][0]["text"].as_str().unwrap_or_default();
            let result = serde_json::from_str(text).unwrap_or_else(|_| json!(text));
            replies.push(json!({"setId":id,"name":w.select(&id).unwrap().name,"result":crate::webapi::sets::qualify_value(result,&id),"isError":reply["isError"]}));
        }
        if replies.len() == 1 {
            let mut reply = result_value(replies[0]["result"].clone());
            reply["isError"] = replies[0]["isError"].clone();
            Ok(reply)
        } else {
            Ok(result_value(json!({"sets":replies})))
        }
    }
}
