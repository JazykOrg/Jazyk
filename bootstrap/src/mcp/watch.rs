use super::*;

impl McpServer {
    // The server's own long poll: returns when the graph's generation counter moves, a
    // documentation file changes on disk, or the ledger or a watched deliverable file
    // changes, or at the timeout, capped at an hour (0 waits for the cap); the default
    // returns because most MCP clients bound a tool call with their own timeout.
    // Mirrors docs/frontends/mcp.md#the-work-loop.
    // Watch from the first request. Mirrors docs/frontends/mcp.md#watching-edits.
    pub(super) fn watch_edits(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.bridge.ephemeral {
            crate::edit_watch::for_root(&self.project.root);
        }
    }

    pub(super) fn recent_edits_tool() -> Value {
        json!({
            "name": "recent_edits",
            "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-watch/project-files-edited-since-the-cursor-since-b8c62.md")),
            "inputSchema": {"type": "object", "properties": {
                "since": {"type": "integer", "minimum": 0, "description": "cursor of a previous reply"},
                "setId": {"type": "string", "description": "narrow the rows to one document set"}
            }, "additionalProperties": false}
        })
    }

    // An alignment or reconcile goal still to run, whether ready or awaiting a release.
    fn dirtied(g: &crate::model::Goal) -> bool {
        matches!(g.kind.as_str(), "place-anchors" | "reconcile-section")
            && !matches!(g.state, crate::model::GoalState::Failed { .. })
    }

    // Mirrors docs/frontends/mcp.md#watching-edits.
    pub(super) fn recent_edits(&self, args: &Value) -> Value {
        let only = args["setId"].as_str().or(self.project.set_id.as_deref());
        #[cfg(not(target_arch = "wasm32"))]
        let (watching, cursor, edits) = {
            let log = crate::edit_watch::for_root(&self.project.root);
            // A rescan answers an edit landed inside the debounce window too.
            log.rescan();
            (
                log.watching(),
                log.cursor(),
                log.since(args["since"].as_u64().unwrap_or(0)),
            )
        };
        #[cfg(target_arch = "wasm32")]
        let (watching, cursor, edits): (bool, u64, Vec<crate::edit_watch::Edit>) =
            (false, 0, Vec::new());
        let Ok(w) = crate::document_sets::Workspace::load(&self.project.root) else {
            return json!({"cursor": cursor, "watching": watching, "graphStale": false, "edits": []});
        };
        let mut boards: BTreeMap<
            String,
            (crate::board::Board, crate::store::Store, crate::gen::Ledger),
        > = BTreeMap::new();
        let mut stale = false;
        let mut rows = Vec::new();
        for e in edits {
            let sets: Vec<Value> = w
                .memberships(&e.path)
                .into_iter()
                .filter(|m| only.is_none_or(|id| m["setId"] == id))
                .map(|m| json!({"setId": m["setId"], "role": m["role"]}))
                .collect();
            // Under setId, only that set's files; otherwise also the project file and
            // removed files, which no set selects any more.
            if sets.is_empty()
                && (only.is_some() || !(e.removed || e.path == crate::document_sets::CONFIG))
            {
                continue;
            }
            let mut row = json!({"path": e.path, "removed": e.removed, "sets": sets});
            let mut goals: Vec<String> = Vec::new();
            for m in row["sets"].as_array().cloned().unwrap_or_default() {
                let id = m["setId"].as_str().unwrap_or_default().to_string();
                if !boards.contains_key(&id) {
                    let Ok(p) = w.resolve(Some(&id)) else {
                        continue;
                    };
                    let board = crate::board::Board::compute(&p, &p.out);
                    stale |= board.goals.iter().any(Self::dirtied);
                    boards.insert(
                        id.clone(),
                        (board, Store::load(&p.out), crate::gen::Ledger::load(&p.out)),
                    );
                }
                let (board, store, ledger) = &boards[&id];
                if m["role"] == "docs" {
                    goals.extend(
                        board
                            .goals
                            .iter()
                            .filter(|g| {
                                Self::dirtied(g)
                                    && crate::board::target_doc(&g.target).as_deref()
                                        == Some(e.path.as_str())
                            })
                            .map(|g| g.id.clone()),
                    );
                } else if row["owner"].is_null() {
                    let deliverable = w
                        .resolve(Some(&id))
                        .map(|p| crate::gen::GenSettings::resolve(&p).deliverable)
                        .unwrap_or_default();
                    let base = deliverable
                        .strip_prefix(&self.project.root)
                        .unwrap_or(std::path::Path::new(""))
                        .to_path_buf();
                    if let Ok(rel) = std::path::Path::new(&e.path).strip_prefix(&base) {
                        if let Some(owner) = crate::gen::file_owner(
                            store,
                            ledger,
                            "",
                            &rel.to_string_lossy().replace('\\', "/"),
                        ) {
                            row["owner"] = json!(format!("ent:{owner}"));
                        }
                    }
                }
            }
            if !goals.is_empty() {
                goals.sort();
                goals.dedup();
                row["goals"] = json!(goals);
            }
            rows.push(row);
        }
        json!({"cursor": cursor, "watching": watching, "graphStale": stale, "edits": rows})
    }

    pub(super) fn await_changes(&self, params: &Value) -> Value {
        // Capped, and 0 waits for the cap: a call the client forgot still ends.
        let timeout = match params["arguments"]["timeout_seconds"]
            .as_u64()
            .unwrap_or(300)
        {
            0 => AWAIT_CAP_SECS,
            t => t.min(AWAIT_CAP_SECS),
        };
        let gs = self.current_gen_settings();
        let fingerprint = |path: &std::path::Path| -> String {
            crate::vfs::entry_metadata(path)
                .map(|m| format!("{}:{:?}", m.len(), m.modified().ok()))
                .unwrap_or_default()
        };
        // Watched surfaces: docs, the ledger, every file the ledger names, and the
        // control file, so a release or mode change wakes the poll.
        let watched = |gs: &crate::gen::GenSettings| -> Vec<std::path::PathBuf> {
            let mut v = self.current_project().doc_files();
            v.extend(crate::gen::Ledger::files(&self.out));
            v.push(crate::control::Control::path(&self.out));
            let ledger = crate::gen::Ledger::load(&self.out);
            for row in ledger.requirements.values() {
                for f in &row.files {
                    v.push(gs.deliverable.join(f));
                }
                v.push(crate::gen::artifact_path(&self.out, gs, &row.test));
            }
            v.sort();
            v.dedup();
            v
        };
        let snapshot: std::collections::BTreeMap<std::path::PathBuf, String> = watched(&gs)
            .into_iter()
            .map(|f| (f.clone(), fingerprint(&f)))
            .collect();
        let start_gen = Store::load(&self.out).status.generation;
        let deadline = Some(crate::sys::Instant::now() + std::time::Duration::from_secs(timeout));
        let mut changed_docs: Vec<String> = Vec::new();
        let mut changed = false;
        // A tab has one thread and nothing else that could move the graph while it
        // waits, so the poll answers at once with what it sees now.
        #[cfg(target_arch = "wasm32")]
        let deadline = deadline.map(|_| crate::sys::Instant::now());
        #[cfg(not(target_arch = "wasm32"))]
        let edits = crate::edit_watch::for_root(&self.project.root);
        #[cfg(not(target_arch = "wasm32"))]
        let edit_cursor = edits.cursor();
        while deadline.is_none_or(|d| crate::sys::Instant::now() < d)
            && !self.interrupts.stop_current()
        {
            // A recorded edit wakes the poll at once; the 2 s tick still catches the
            // generation, ledger, and control surfaces.
            #[cfg(not(target_arch = "wasm32"))]
            {
                if edits.watching() {
                    edits.wait(edit_cursor, std::time::Duration::from_secs(2));
                } else {
                    crate::sys::sleep(std::time::Duration::from_secs(2));
                }
                for e in edits.since(edit_cursor) {
                    if e.path != crate::document_sets::CONFIG && !changed_docs.contains(&e.path) {
                        changed_docs.push(e.path);
                    }
                    changed = true;
                }
            }
            #[cfg(target_arch = "wasm32")]
            crate::sys::sleep(std::time::Duration::from_secs(2));
            let store = Store::load(&self.out);
            if store.status.generation != start_gen {
                changed = true;
            }
            for f in watched(&gs) {
                if snapshot.get(&f).map(|s| s.as_str()) != Some(fingerprint(&f).as_str()) {
                    let rel = f
                        .strip_prefix(&self.project.root)
                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|_| f.to_string_lossy().to_string());
                    if !changed_docs.contains(&rel) {
                        changed_docs.push(rel);
                    }
                    changed = true;
                }
            }
            if changed {
                break;
            }
        }
        let board = crate::board::Board::compute(&self.current_project(), &self.out);
        let c = crate::control::Control::load(&self.current_project(), &self.out);
        let graph_kinds = crate::board::Board::graph_kinds();
        let (c_act, b_act, g_act, v_act) = (
            board.ready_of(&graph_kinds),
            board.ready_of(&["bind"]),
            board.ready_of(&["generate"]),
            board.ready_of(&["verify"]),
        );
        let gated = board.gated.len();
        let counts = board.counts();
        let store = Store::load(&self.out);
        // Stale: documents changed but not yet reconciled, visible as open tier 0/1
        // goals on the derived board (docs/frontends/mcp.md#the-work-loop).
        let graph_stale = board.open_of(&["place-anchors", "reconcile-section"]) > 0;
        json!({
            "changed": changed,
            "changedDocs": changed_docs,
            "graphStale": graph_stale,
            "board": {
                "open": counts.open,
                "ready": counts.ready,
                "blocked": counts.blocked,
                "parked": counts.parked,
                "failed": counts.failed,
                "optional": counts.optional,
                "byClass": counts.by_class,
            },
            "compilationTasks": board.open_of(&graph_kinds),
            "bindingTasks": board.open_of(&["bind"]),
            "generationTasks": board.open_of(&["generate"]),
            "verificationTasks": crate::verify::pending_counts(&store, &gs),
            "workflow": {"compile": c.compile, "generate": c.generate},
            "gatedTasks": gated,
            "verdict": board.verdict.to_string(),
            "openDiagnostics": board.open_diags,
            "next": SERVING.get(if c_act > 0 {
                "status-next-goals"
            } else if b_act > 0 {
                "status-next-binding"
            } else if g_act > 0 {
                "status-next-generation"
            } else if v_act > 0 {
                "status-next-verification"
            } else if gated > 0 {
                "status-next-gated"
            } else {
                "status-next-none"
            }),
        })
    }
}
