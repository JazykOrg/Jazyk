use super::*;

impl McpServer {
    // Advertising follows the open batch, while enabled_tools remains the serving's
    // authorization boundary so cached or guessed writes still reach scope gates.
    fn active_tools(&self) -> Option<BTreeSet<&'static str>> {
        let mut active = None;
        if let Some(open) = self.open.lock().unwrap().as_ref() {
            let kinds = open.session.scope.kinds();
            let refs = kinds.iter().map(String::as_str).collect::<Vec<_>>();
            let mut names = toolset_for_kinds(&refs)
                .into_iter()
                .collect::<BTreeSet<_>>();
            names.extend(LIFECYCLE);
            if kinds
                .iter()
                .any(|kind| matches!(kind.as_str(), "bind" | "generate" | "verify"))
            {
                names.extend(["read_text_file", "list_files", "inspect_asset_link"]);
                if kinds
                    .iter()
                    .any(|kind| matches!(kind.as_str(), "bind" | "generate"))
                {
                    names.extend(["write_text_file", "link_asset", "remove_asset_link"]);
                }
            }
            if kinds
                .iter()
                .any(|kind| matches!(kind.as_str(), "bind" | "generate"))
            {
                names.extend([
                    "list_document_sets",
                    "preview_document_set",
                    "update_document_set",
                ]);
            }
            active = Some(names);
        }
        for child in self.children.lock().unwrap().values() {
            if let Some(names) = child.active_tools() {
                active.get_or_insert_with(BTreeSet::new).extend(names);
            }
        }
        active
    }

    pub(super) fn scope_catalog(&self, tools: &mut Vec<Value>) {
        if let Some(names) = self.active_tools() {
            tools.retain(|tool| {
                tool["name"] == "project_context"
                    || crate::agent_workflows::TOOLS
                        .contains(&tool["name"].as_str().unwrap_or_default())
                    || names.contains(tool["name"].as_str().unwrap_or_default())
            });
        }
        if tools.iter().any(|tool| {
            matches!(
                tool["name"].as_str(),
                Some("read_text_file" | "write_text_file" | "list_files")
            )
        }) {
            let root = self.current_gen_settings().deliverable;
            let base = if root == self.project.root.join(".") {
                "project root"
            } else {
                "deliverable directory"
            };
            for tool in tools {
                let action = match tool["name"].as_str() {
                    Some("read_text_file") => include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/mcp-catalog/read-an-eligible-file-capped-at-kib-6dc40.md"
                    )),
                    Some("write_text_file") => include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/mcp-catalog/write-a-source-file-creating-parents-read-a03a7.md"
                    )),
                    Some("list_files") => include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/mcp-catalog/list-eligible-files-path-narrows-to-a-461c3.md"
                    )),
                    _ => continue,
                };
                tool["description"] = json!(format!("{action} Paths are relative to the {base} `{}`. Keep Source subdirectory names. Markdown hrefs are document-relative; use referencedAssets.path for supplied inputs.", root.display()));
                tool["inputSchema"]["properties"]["path"]["description"] = json!(format!(
                    include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/mcp-catalog/relative-to-the-base-use-paths-from-5898c.md"
                    )),
                    root.display(),
                    base = base
                ));
            }
        }
    }

    pub(super) fn current_project(&self) -> crate::project::Project {
        match crate::document_sets::Workspace::load(&self.project.root) {
            Ok(workspace) if workspace.exists => match self.project.set_id.as_deref() {
                Some(id) => match workspace.resolve(Some(id)) {
                    Ok(project) => project,
                    Err((_, error)) => {
                        let mut project = self.project.clone();
                        project.config_error = Some(error);
                        project
                    }
                },
                None => workspace.files_project(),
            },
            Ok(_) => self.project.clone(),
            Err((_, error)) => {
                let mut project = self.project.clone();
                project.config_error = Some(error);
                project
            }
        }
    }

    pub(super) fn current_gen_settings(&self) -> crate::gen::GenSettings {
        let project = self.current_project();
        let mut settings = crate::gen::GenSettings::resolve(&project);
        if project.config_error.is_some() {
            settings.configured = false;
            settings.code.clear();
        }
        settings
    }

    pub(super) fn serves_matcher_repairs(&self) -> bool {
        !self.bridge.reader
            && (self.modes.iter().any(|mode| mode == "generate")
                || (self.modes.iter().any(|mode| mode == "chat")
                    && self.open.lock().unwrap().as_ref().is_some_and(|batch| {
                        batch
                            .session
                            .scope
                            .kinds()
                            .iter()
                            .any(|kind| matches!(kind.as_str(), "bind" | "generate"))
                    })))
    }

    pub(super) fn serves_project_writes(&self) -> bool {
        !self.bridge.reader
            && (self.write
                || self
                    .modes
                    .iter()
                    .any(|mode| matches!(mode.as_str(), "compile" | "generate" | "chat")))
    }

    pub(super) fn accept_task_revision(&self, reply: &Value, previous: Option<&str>) {
        let Some(previous) = previous else { return };
        if reply["isError"] == true {
            return;
        }
        let Some(text) = reply["content"][0]["text"].as_str() else {
            return;
        };
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return;
        };
        let Ok(source) =
            crate::vfs::read_to_string(self.project.root.join(crate::document_sets::CONFIG))
        else {
            return;
        };
        if value["revision"].as_str()
            != Some(crate::task_runtime::digest(source.as_bytes()).as_str())
        {
            return;
        }
        self.accept_config_revision(previous, &crate::model::hash_hex(&source));
    }

    fn accept_config_revision(&self, previous: &str, hash: &str) {
        let mut revision = self.config_revision.lock().unwrap();
        if revision.as_str() == previous {
            *revision = hash.into();
        }
        drop(revision);
        for child in self.children.lock().unwrap().values() {
            child.accept_config_revision(previous, hash);
        }
    }

    pub(super) fn matcher_catalog() -> Vec<Value> {
        let schema = json!({"type":"object","properties":{
            "baseHash":{"type":"string"},
            "roleIntents":{"type":"array","items":{"type":"object","properties":{
                "role":{"type":"string","enum":["docs","source"]},"patterns":{"type":"array","items":{"type":"string"}}},
                "required":["role","patterns"],"additionalProperties":false}},
            "set":{"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string"},
                "docs":{"type":"array","items":{"type":"string"}},"roots":{"type":"array","items":{"type":"string"}},
                "source":{"type":"array","items":{"type":"string"}}},"required":["id"],"additionalProperties":false}},
            "required":["baseHash","set"],"additionalProperties":false});
        let mut tools = vec![
            json!({"name":"list_document_sets","description":include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-catalog/inspect-document-set-selections-and-their-current-e9a33.md")),
            "inputSchema":{"type":"object","properties":{},"additionalProperties":false}}),
        ];
        for (name, description) in [
            (
                "preview_document_set",
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/mcp-catalog/preview-matcher-changes-to-this-worker-s-5703b.md"
                )),
            ),
            (
                "update_document_set",
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/mcp-catalog/repair-docs-source-or-roots-matchers-on-d7ca5.md"
                )),
            ),
        ] {
            tools.push(json!({"name":name,"description":description,"inputSchema":schema}));
        }
        tools
    }

    pub(super) fn scoped_matcher_call(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<Value, (i64, String)> {
        // Not listed here, so not served: an unknown tool, like any other.
        if !self.serves_matcher_repairs() {
            return Err((-32602, format!("{UNKNOWN_TOOL} `{}`", name)));
        }
        let workspace =
            crate::document_sets::Workspace::load(&self.project.root).map_err(|e| (-32602, e.1))?;
        if name == "list_document_sets" {
            return Ok(result_value(workspace.summary()));
        }
        if self.finished.lock().unwrap().is_some() {
            return Err((-32602, "this worker session is complete".into()));
        }
        let project = workspace
            .resolve(self.project.set_id.as_deref())
            .map_err(|e| (-32602, e.1))?;
        let id = project
            .set_id
            .as_deref()
            .ok_or((-32602, "this worker has no selected document set".into()))?;
        if args["set"]["id"].as_str() != Some(id) {
            return Err((
                -32602,
                "this worker can update only its current document set".into(),
            ));
        }
        let allowed = ["baseHash", "set", "roleIntents"];
        let set_fields = ["id", "name", "docs", "source", "roots"];
        if !args
            .as_object()
            .is_some_and(|fields| fields.keys().all(|key| allowed.contains(&key.as_str())))
            || !args["set"]
                .as_object()
                .is_some_and(|fields| fields.keys().all(|key| set_fields.contains(&key.as_str())))
        {
            return Err((
                -32602,
                "a scoped matcher edit changes only the current set's name, docs, source and roots"
                    .into(),
            ));
        }
        let preview = name == "preview_document_set";
        let result = if let Some(token) = self.bridge.build_token.as_deref() {
            crate::document_sets::edit_in_build(
                &workspace.root,
                &self.out,
                token,
                args,
                preview,
                id,
            )
        } else {
            if crate::control::build_lease(&self.out).is_some() {
                return Err((-32602, "a running build owns this document set".into()));
            }
            let open = self.open.lock().unwrap();
            if let Some(lock) = open.as_ref().and_then(|batch| batch._project_lock.as_ref()) {
                crate::document_sets::edit_current_set_with_lock(
                    &workspace.root,
                    args,
                    preview,
                    id,
                    lock,
                )
            } else {
                crate::document_sets::edit_current_set(&workspace.root, args, preview, id)
            }
        }
        .map_err(|e| (-32602, e.1))?;
        if !preview {
            *self.config_revision.lock().unwrap() = self.current_project().config_hash;
            if let Some(open) = self.open.lock().unwrap().as_mut() {
                open.session.gen = self.current_gen_settings();
            }
        }
        Ok(result_value(result))
    }

    pub(super) fn enabled_tools(&self) -> Vec<&'static str> {
        // The reader serving carries the read tools and nothing else, not even
        // report_feedback: every schema travels with every call of a small model.
        if self.bridge.reader {
            return crate::tools::READ_TOOLS.to_vec();
        }
        if self.modes.len() == 1 && self.modes[0] == "runtime" {
            return crate::task_runtime::TOOLS.to_vec();
        }
        let mut v: Vec<&'static str> = Vec::new();
        for m in &self.modes {
            let set = match m.as_str() {
                "runtime" => crate::task_runtime::TOOLS.to_vec(),
                "compile" => {
                    let mut t = toolset("mcp-compile");
                    t.extend(LIFECYCLE);
                    t
                }
                "generate" => {
                    let mut t = toolset("mcp-generate");
                    // Agents with no editor of their own get the sandboxed file and
                    // command tools (the embedded agent's profile sets serve_files).
                    // Mirrors docs/frontends/mcp.md#mcp-into-acp-sessions.
                    if self.bridge.serve_files {
                        t.extend(
                            crate::tools::FILE_TOOLS
                                .into_iter()
                                .filter(|t| *t != "run_command"),
                        );
                    }
                    t
                }
                "benchmark" => {
                    let mut t = toolset("mcp-compile");
                    t.extend(crate::tools::GEN_TOOLS);
                    t.push("run_tests");
                    t.extend([
                        "benchmark_cases",
                        "begin_case",
                        "finish_case",
                        "benchmark_report",
                    ]);
                    t
                }
                "verify" => {
                    let mut t = toolset("mcp-verify");
                    // The read-only file tools for agents with no editor of their
                    // own: a verify session reads and runs, never writes.
                    // Mirrors docs/compiler/goals/verify.md#tools.
                    if self.bridge.serve_files {
                        t.extend(crate::tools::FILE_TOOLS.iter().copied().filter(|t| {
                            !matches!(
                                *t,
                                "write_text_file"
                                    | "link_asset"
                                    | "remove_asset_link"
                                    | "run_command"
                            )
                        }));
                    }
                    t
                }
                // The chat serving: reads, every lifecycle, and the server-implemented
                // chat tools; compiler writes require a batch (docs/frontends/mcp.md#toolsets).
                "chat" => {
                    let mut t = toolset("mcp-compile");
                    t.extend(LIFECYCLE);
                    // A tab has no shell, so binding, generation, and verification
                    // are not served there (docs/frontends/wasm.md#chat).
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        t.extend(crate::tools::GEN_TOOLS);
                        t.extend(crate::tools::BIND_TOOLS);
                        t.extend(crate::tools::VERIFY_TOOLS);
                        t.extend(crate::task_runtime::TOOLS);
                        t.extend(
                            crate::tools::FILE_TOOLS
                                .into_iter()
                                .filter(|t| *t != "run_command"),
                        );
                    }
                    t
                }
                // The decompile serving: read tools only from the catalog; its own
                // lifecycle tools are server-implemented (they need the project).
                "decompile" => toolset("mcp-read"),
                _ => toolset(if self.write { "mcp-write" } else { "mcp-read" }),
            };
            for t in set {
                if !v.contains(&t) {
                    v.push(t);
                }
            }
        }
        if !v.contains(&crate::tools::FEEDBACK_TOOL) {
            v.push(crate::tools::FEEDBACK_TOOL);
        }
        v
    }
}
