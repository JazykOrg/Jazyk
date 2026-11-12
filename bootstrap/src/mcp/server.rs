use super::*;

pub(super) fn instructions_for(modes: &[String], write: bool, initialized: bool) -> String {
    instructions_in(modes, write, initialized, cfg!(target_arch = "wasm32"))
}

// The serving's own instructions and its longer `next` guidance.
// Mirrors docs/compiler/sessions.md#payload-files.
pub(crate) const SERVING: crate::payload::Fragments =
    crate::payload::Fragments(include_str!(concat!(
        env!("OUT_DIR"),
        "/payloads/goals/prompts/serving/serving-instructions.md.mustache"
    )));

// Capability context only. Shared policy comes from agent_workflows::render. A chat
// in a tab has no build path of its own: the page's Build button runs builds, so a
// model asked to build says so instead of asking for prose
// (docs/frontends/wasm.md#chat).
pub(super) fn instructions_in(
    modes: &[String],
    write: bool,
    initialized: bool,
    tab: bool,
) -> String {
    if modes.len() == 1 && modes[0] == "runtime" {
        return crate::task_runtime::PROMPT.to_owned();
    }
    let mut parts = vec![SERVING.fill("toolsets", &[("modes", &modes.join(", "))])];
    parts.push(
        SERVING
            .get(if initialized {
                "initialized"
            } else {
                "uninitialized"
            })
            .to_string(),
    );
    if tab {
        parts.push(SERVING.get("tab").to_string());
        if modes.iter().any(|m| m == "chat") {
            parts.push(SERVING.get("writer-build-note").to_string());
        }
    }
    if modes.iter().any(|m| m == "graph") && write {
        parts.push(SERVING.get("graph-write").to_string());
    }
    parts.push(SERVING.get("workflow").to_string());
    parts.iter().map(|p| format!("{} ", p)).collect()
}

// Restrictions for a reader profile, following the same main policy.
pub fn reader_instructions() -> String {
    format!(
        "{} {}",
        SERVING.get("reader"),
        SERVING.get("tab-build-note")
    )
}

impl McpServer {
    pub(super) fn initialize_instructions(
        &self,
        initialized: bool,
    ) -> Result<String, (i64, String)> {
        let integration = crate::project_context::instructions(&self.project.root);
        if self.modes.len() == 1 && self.modes[0] == "runtime" {
            return Ok(format!("{}{}", crate::task_runtime::PROMPT, integration));
        }
        // A project file that does not load still lets the agent connect and repair it
        // (docs/frontends/mcp.md#mcp-into-acp-sessions).
        let policy = crate::agent_workflows::render(&self.project.root).unwrap_or_else(|e| {
            SERVING.fill(
                "project-unloadable",
                &[
                    ("config", crate::document_sets::CONFIG),
                    ("error", &e.to_string()),
                ],
            )
        });
        let capabilities = if self.bridge.reader {
            reader_instructions()
        } else {
            instructions_for(&self.modes, self.write, initialized)
        };
        let text = SERVING.fill(
            "initialize",
            &[("policy", &policy), ("capabilities", &capabilities)],
        );
        Ok(format!("{text}\n{integration}"))
    }

    // Ledger workers carry the execution contract in their opening prompt. A
    // manually claimed ledger batch receives it from begin_goals instead.
    pub(super) fn has_execution_contract(&self) -> bool {
        (self.bridge.ephemeral
            && self.bridge.only.is_some()
            && self
                .modes
                .iter()
                .any(|mode| matches!(mode.as_str(), "generate" | "verify")))
            || self.open.lock().unwrap().as_ref().is_some_and(|batch| {
                batch
                    .goals
                    .iter()
                    .any(|goal| matches!(goal.kind.as_str(), "bind" | "generate" | "verify"))
            })
    }

    // This receipt is fixed before mutations can change or renumber the board.
    // gdk3 owned generation openings carry the contract, including restored ones.
    pub(super) fn has_generation_contract(&self) -> bool {
        self.delivered.lock().unwrap().generation_contract
    }

    pub(super) fn worker_reply(&self, name: &str, mut reply: Value) -> Value {
        if reply["isError"] == true
            || (!crate::task_runtime::TOOLS.contains(&name)
                && !matches!(name, "begin_binding" | "begin_generation"))
            || !self.has_execution_contract()
        {
            return reply;
        }
        let Some(text) = reply["content"][0]["text"].as_str() else {
            return reply;
        };
        let Ok(mut value) = serde_json::from_str::<Value>(text) else {
            return reply;
        };
        if matches!(name, "begin_binding" | "begin_generation") {
            crate::tools::opened_package(name, &mut value, self.has_generation_contract());
        } else {
            value = crate::task_runtime::worker_response(name, value);
        }
        reply["content"][0]["text"] =
            json!(serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()));
        reply
    }

    pub fn live_preview(&self) -> Option<crate::live_preview::Preview> {
        self.preview.lock().unwrap().clone()
    }

    pub(super) fn update_preview(&self, name: &str, label: &str, accepted: bool) {
        if !accepted {
            return;
        }
        let open = self.open.lock().unwrap();
        let (phase, label, generation, graph) = if let Some(batch) = open.as_ref() {
            if name != "begin_goals" && crate::tools::READ_TOOLS.contains(&name) {
                return;
            }
            (
                if name == "begin_goals" {
                    "base"
                } else {
                    "staged"
                },
                batch.id.as_str(),
                batch.session.snapshot.status.generation,
                crate::live_preview::staged(&batch.session.snapshot.graph, &batch.session.staged),
            )
        } else if name == "done" || name == "abandon_goals" {
            let store = Store::load(&self.out);
            (
                if name == "done" { "committed" } else { "base" },
                label,
                store.status.generation,
                Some(crate::live_preview::authored(&store.graph)),
            )
        } else {
            return;
        };
        let mut prior = self.preview.lock().unwrap();
        if name != "begin_goals"
            && prior
                .as_ref()
                .is_some_and(|p| p.phase == phase && p.graph == graph)
        {
            return;
        }
        let attempt = if name == "begin_goals" {
            format!("{}-{}", crate::sys::process_id(), crate::sys::now_millis())
        } else {
            prior
                .as_ref()
                .map(|p| p.attempt.clone())
                .unwrap_or_default()
        };
        let preview = crate::live_preview::Preview {
            label: label.into(),
            attempt,
            revision: prior.as_ref().map_or(1, |p| p.revision + 1),
            phase: phase.into(),
            generation,
            reason: graph
                .is_none()
                .then(|| "Live preview waits for this compound change to commit.".into()),
            graph,
        }
        .bounded();
        crate::live_preview::write(&self.out, &preview);
        *prior = Some(preview);
    }

    // Whether this serving stands in a project or in a bare directory. Decides which
    // project tools exist. Mirrors docs/frontends/acp.md#project-tools.
    pub(super) fn initialized(&self) -> bool {
        self.project
            .root
            .join(crate::document_sets::CONFIG)
            .vexists()
    }

    pub fn new(
        project: crate::project::Project,
        out: PathBuf,
        modes: Vec<String>,
        write: bool,
    ) -> McpServer {
        Self::with_bridge(project, out, modes, write, BridgeFlags::default())
    }

    pub fn with_bridge(
        project: crate::project::Project,
        out: PathBuf,
        modes: Vec<String>,
        write: bool,
        bridge: BridgeFlags,
    ) -> McpServer {
        let out_for_trace = out.clone();
        let generation_contract = bridge.ephemeral
            && modes.iter().any(|mode| mode == "generate")
            && bridge.only.as_deref().is_some_and(|only| {
                if let Some((kind, _)) = crate::goals::parse_goal_id(only) {
                    kind == "generate"
                } else {
                    crate::board::Board::compute(&project, &out)
                        .batches
                        .iter()
                        .find(|batch| batch.id == only)
                        .is_some_and(|batch| {
                            batch.goals.iter().any(|id| {
                                crate::goals::parse_goal_id(id)
                                    .is_some_and(|(kind, _)| kind == "generate")
                            })
                        })
                }
            });
        McpServer {
            notifies: Default::default(),
            listed: Default::default(),
            children: Default::default(),
            config_revision: std::sync::Mutex::new(project.config_hash.clone()),
            mutation_limit: crate::limits::SESSION_MUTATIONS,
            context_budget: crate::limits::CONTEXT_BUDGET,
            project,
            out,
            modes,
            write,
            client: std::sync::Mutex::new(None),
            open: std::sync::Mutex::new(None),
            bench: std::sync::Mutex::new(BenchRun::default()),
            worker: std::sync::Arc::new(std::sync::Mutex::new(None)),
            preview: Default::default(),
            trace: crate::session::Trace::stderr(crate::session::TraceLevel::Quiet)
                .with_transcript(&out_for_trace, "mcp"),
            bridge,
            delivered: std::sync::Mutex::new(Delivered {
                generation_contract,
                ..Default::default()
            }),
            finished: std::sync::Mutex::new(None),
            ledger_target: std::sync::Mutex::new(None),
            interrupts: Default::default(),
            call_memory: Default::default(),
        }
    }

    /// Protected owned session state. It is not a project file or a tool result.
    pub(crate) fn checkpoint(&self) -> Result<Value, String> {
        let project = self.current_project();
        if let Some(error) = &project.config_error {
            return Err(error.clone());
        }
        let children = self
            .children
            .lock()
            .map_err(|_| "MCP child state poisoned")?
            .iter()
            .map(|(id, child)| Ok((id.clone(), child.checkpoint()?)))
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        Ok(json!({
            "version": 1,
            "setId": self.project.set_id,
            "configHash": project.config_hash,
            "open": &*self.open.lock().map_err(|_| "MCP batch state poisoned")?,
            "delivered": &*self.delivered.lock().map_err(|_| "MCP delivery state poisoned")?,
            "finished": &*self.finished.lock().map_err(|_| "MCP completion state poisoned")?,
            "ledgerTarget": &*self.ledger_target.lock().map_err(|_| "MCP ledger state poisoned")?,
            "client": &*self.client.lock().map_err(|_| "MCP client state poisoned")?,
            "children": children
        }))
    }

    pub(crate) fn restore(&self, value: Value) -> Result<(), String> {
        let project = self.current_project();
        if let Some(error) = &project.config_error {
            return Err(error.clone());
        }
        if value["version"] != 1
            || value["setId"] != json!(self.project.set_id)
            || value["configHash"] != json!(project.config_hash)
        {
            return Err("MCP checkpoint is incompatible with this project configuration.".into());
        }
        let mut open: Option<OpenBatch> =
            serde_json::from_value(value["open"].clone()).map_err(|e| e.to_string())?;
        if let Some(batch) = open.as_mut() {
            if batch.session.snapshot.out != self.out
                || self.bridge.only.as_ref().is_some_and(|id| id != &batch.id)
                || batch.session.scope.batch != batch.id
            {
                return Err("MCP checkpoint belongs to a different batch or workspace.".into());
            }
            let current = Store::load(&self.out);
            if current.status.generation != batch.session.snapshot.status.generation {
                return Err("Workspace generation changed while this batch was suspended.".into());
            }
            batch._project_lock = Some(
                crate::document_sets::ProjectLock::acquire(&self.project.root, "project.lock")
                    .map_err(|e| e.1)?,
            );
            let goals = batch
                .goals
                .iter()
                .map(|goal| goal.id.clone())
                .collect::<Vec<_>>();
            crate::control::claim_restored(&self.out, &batch.id, &self.worker_id(), &goals)
                .map_err(|worker| {
                    format!("Restored batch `{}` is held by worker `{worker}`", batch.id)
                })?;
        }
        *self.open.lock().map_err(|_| "MCP batch state poisoned")? = open;
        *self
            .delivered
            .lock()
            .map_err(|_| "MCP delivery state poisoned")? =
            serde_json::from_value(value["delivered"].clone()).map_err(|e| e.to_string())?;
        *self
            .finished
            .lock()
            .map_err(|_| "MCP completion state poisoned")? =
            serde_json::from_value(value["finished"].clone()).map_err(|e| e.to_string())?;
        let ledger_target: Option<String> =
            serde_json::from_value(value["ledgerTarget"].clone()).map_err(|e| e.to_string())?;
        if let Some(target) = &ledger_target {
            crate::control::claim_restored(&self.out, target, &self.worker_id(), &[]).map_err(
                |worker| format!("Restored ledger task `{target}` is held by worker `{worker}`"),
            )?;
        }
        *self
            .ledger_target
            .lock()
            .map_err(|_| "MCP ledger state poisoned")? = ledger_target;
        *self
            .client
            .lock()
            .map_err(|_| "MCP client state poisoned")? =
            serde_json::from_value(value["client"].clone()).map_err(|e| e.to_string())?;
        if let Some(children) = value["children"].as_object() {
            let workspace =
                crate::document_sets::Workspace::load(&self.project.root).map_err(|e| e.1)?;
            let mut restored = self
                .children
                .lock()
                .map_err(|_| "MCP child state poisoned")?;
            for (id, state) in children {
                let project = workspace.resolve(Some(id)).map_err(|e| e.1)?;
                let mut child = Self::with_bridge(
                    project.clone(),
                    project.out.clone(),
                    self.modes.clone(),
                    self.write,
                    self.bridge.clone(),
                );
                child.interrupts = self.interrupts.clone();
                child.restore(state.clone())?;
                restored.insert(id.clone(), child);
            }
        }
        Ok(())
    }
}
