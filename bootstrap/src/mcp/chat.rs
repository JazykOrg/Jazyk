use super::*;

// ---- the chat tools: dual writes and project setup ----
// A requirement lives in the prose; a chat edit moves the document and the graph in
// one atomic commit. Mirrors docs/compiler/tools.md#chat-tools and
// docs/frontends/acp.md#dual-write-tools.
impl McpServer {
    pub(super) fn release_own_ledger_claim(&self, target: &str) {
        if let Some(claim) = crate::control::owned_lease(&self.out, target, &self.worker_id()) {
            let _ = crate::control::release_owned_lease(&claim);
        }
    }

    // Write a document (or jazyk.toml) edit: through the delegating sink when the
    // spawning proxy listens, straight to disk otherwise.
    // Mirrors docs/frontends/acp.md#doc-edit-delegation.
    pub(super) fn write_edit(
        &self,
        rel: &str,
        old_text: &str,
        new_text: &str,
        full: &str,
    ) -> Result<(), String> {
        let path = self.project.root.join(rel);
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("docx") || e.eq_ignore_ascii_case("pdf"))
        {
            return Err(include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/inline/mcp-chat/docx-and-pdf-cannot-be-changed-by-8eb2f.md"
            ))
            .into());
        }
        if let Some(sink) = &self.bridge.edit_sink {
            match sink_write(sink, &path, old_text, new_text, full) {
                Ok(()) => return Ok(()),
                // Nothing listening, or an IDE that cannot take edits: write directly.
                Err(SinkError::Unavailable(reason)) => self.note(&format!(
                    "edit sink unavailable ({reason}); writing {rel} directly"
                )),
                // A refusal or an unanswered edit is the call's failure; writing the
                // disk behind the IDE could race or override its answer.
                Err(SinkError::Failed(e)) => {
                    return Err(format!(
                        "the editor did not apply the edit to {}: {}",
                        rel, e
                    ))
                }
            }
        }
        crate::vfs::write(&path, full).map_err(|e| format!("write {}: {}", path.display(), e))
    }

    // A tool session for one chat call over a synced snapshot (the prose edit already
    // absorbed when one is given, so the usual gates validate the new quote).
    fn chat_session(
        &self,
        parsed: &std::collections::BTreeMap<
            String,
            (
                String,
                std::collections::BTreeMap<String, crate::model::Section>,
            ),
        >,
        edit: Option<&ProseEdit>,
        target: &str,
    ) -> ToolSession {
        let mut snapshot = Store::load(&self.out);
        snapshot.sync_docs(parsed);
        if let Some(e) = edit {
            snapshot.absorb_doc_edit(&e.doc, &e.full);
        }
        let mut scope = WorkScope::serving("mcp-write");
        scope.target = target.to_string();
        let mut session =
            ToolSession::new(snapshot, scope, self.mutation_limit, self.context_budget);
        session.gen = self.current_gen_settings();
        session.lint = self.project.linting.clone();
        session.caller = self.caller("chat", target);
        session
    }

    // The shared tail of every dual write: the staged graph mutations commit with
    // the prose replacement as one changeset through the store, the file written
    // first (through the delegating sink when the proxy listens) and put back when
    // the commit skips. Mirrors docs/compiler/compilation.md#edit-paths.
    fn commit_dual_write(&self, edit: &ProseEdit, ops: Vec<crate::store::Op>, kind: &str) -> Value {
        if ops.is_empty() {
            return json!({"error": {"rule": "edit-needs-mutation", "message": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-chat/the-call-staged-no-graph-mutation-a-3493c.md"))}});
        }
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        let mut s = Store::load(&self.out);
        s.sync_docs(&parsed);
        let write =
            |doc: &str, old: &str, new: &str, full: &str| self.write_edit(doc, old, new, full);
        let before = s.graph.clone();
        match s.dual_write(
            &self.project.root,
            edit,
            ops,
            &crate::store::Commit::store(kind),
            Some(&write),
        ) {
            Ok(report) => {
                let mut v = json!({"committed": true, "applied": report.applied, "doc": edit.doc,
                   "note": "the prose and the graph moved together; no recompile is owed for this edit"});
                self.attach_effects(
                    &mut v,
                    &before,
                    &s.graph,
                    &[(
                        edit.doc.as_str(),
                        Some(edit.old_full.as_str()),
                        edit.full.as_str(),
                    )],
                );
                v
            }
            Err(e) => json!({"error": {"rule": "commit-skipped", "message": e}}),
        }
    }

    // Validate one graph call through the session gates against a snapshot that
    // absorbed the edit, then commit the pair.
    fn dual_commit(&self, edit: ProseEdit, graph_call: (&str, Value), target: &str) -> Value {
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        let mut session = self.chat_session(&parsed, Some(&edit), target);
        let (name, args) = graph_call;
        if let Err(e) = session.dispatch(name, &args) {
            return e.to_value();
        }
        let ops = std::mem::take(&mut session.staged);
        self.commit_dual_write(&edit, ops, "dual-write")
    }

    // The section's stored body, for locating a prose edit inside it.
    fn section_raw(&self, doc: &str, section: &str) -> Option<String> {
        Store::load(&self.out)
            .docs
            .get(doc)
            .and_then(|d| d.sections.get(section))
            .map(|s| s.raw.clone())
    }

    // Classification outside a batch uses the same staging and commit gates.
    pub(super) fn classify_input_chat(&self, args: &Value) -> Value {
        let target = args["path"].as_str().unwrap_or_default();
        let mut store = Store::load(&self.out);
        let mut scope = WorkScope::serving("mcp-write");
        scope.target = target.to_string();
        let mut session = ToolSession::new(
            store.clone(),
            scope,
            self.mutation_limit,
            self.context_budget,
        );
        session.caller = self.caller("chat", target);
        let mut reply = match session.dispatch("classify_input", args) {
            Ok(value) => value,
            Err(error) => return error.to_value(),
        };
        let report = store.apply(
            std::mem::take(&mut session.staged),
            &crate::store::Commit {
                all_or_nothing: true,
                ..crate::store::Commit::store("chat-classification")
            },
        );
        if !report.skipped.is_empty() {
            return json!({"error":{"rule":"commit-skipped","message":report.skipped.join("; ")}});
        }
        reply["committed"] = json!(true);
        reply["generation"] = json!(report.generation);
        reply
    }

    // Conversational criterion repair uses the compiler validator and commits once.
    pub(super) fn update_criterion_chat(&self, args: &Value) -> Value {
        let target = args["id"].as_str().unwrap_or_default();
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        let mut session = self.chat_session(&parsed, None, target);
        if let Err(error) = session.dispatch("update_criterion", args) {
            return error.to_value();
        }
        let mut store = Store::load(&self.out);
        store.sync_docs(&parsed);
        let report = store.apply(
            std::mem::take(&mut session.staged),
            &crate::store::Commit {
                all_or_nothing: true,
                ..crate::store::Commit::store("chat-criterion")
            },
        );
        if !report.skipped.is_empty() {
            return json!({"error":{"rule":"commit-skipped","message":report.skipped.join("; ")}});
        }
        json!({"id":target,"committed":true,"generation":report.generation})
    }

    // One authored field on one node: a dual write when the fact is quoted and a
    // sentence rewrite was accepted, a decree with its ratification proposal
    // otherwise. Mirrors docs/compiler/tools.md#chat-tools.
    pub(super) fn edit_fact(&self, args: &Value) -> Value {
        let target = args["id"].as_str().unwrap_or_default().to_string();
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        let mut session = self.chat_session(&parsed, None, &target);
        let reply = match session.dispatch("edit_fact", args) {
            Ok(v) => v,
            Err(e) => return e.to_value(),
        };
        let ops = std::mem::take(&mut session.staged);
        if reply["prose"].is_object() {
            let p = &reply["prose"];
            let (doc, section) = (
                p["doc"].as_str().unwrap_or_default().to_string(),
                p["section"].as_str().unwrap_or_default().to_string(),
            );
            let path = self.project.root.join(&doc);
            let old_full = match crate::vfs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", doc, e)}})
                }
            };
            let edit = match ProseEdit::locate(
                &doc,
                &section,
                self.section_raw(&doc, &section).as_deref(),
                &old_full,
                p["old_text"].as_str().unwrap_or_default(),
                p["new_text"].as_str().unwrap_or_default(),
            ) {
                Ok(e) => e,
                Err(e) => return json!({"error": {"rule": "stale-anchor", "message": e}}),
            };
            let mut v = self.commit_dual_write(&edit, ops, "dual-write");
            if v["error"].is_null() {
                v["path"] = json!("dual-write");
            }
            return v;
        }
        let mut s = Store::load(&self.out);
        s.sync_docs(&parsed);
        let before = s.graph.clone();
        let report = s.apply(ops, &crate::store::Commit::store("decree"));
        if !report.skipped.is_empty() {
            return json!({"error": {"rule": "commit-skipped", "message": report.skipped.join("; ")}});
        }
        let mut v = reply;
        v["committed"] = json!(true);
        v["generation"] = json!(report.generation);
        self.attach_effects(&mut v, &before, &s.graph, &[]);
        v
    }

    // The reply's `effects`: the file hunks written and the graph as it differs from
    // `before` after the commit. Mirrors docs/frontends/acp.md#edit-effects.
    fn attach_effects(
        &self,
        v: &mut Value,
        before: &crate::model::Graph,
        after: &crate::model::Graph,
        files: &[(&str, Option<&str>, &str)],
    ) {
        let files = files
            .iter()
            .filter_map(|(rel, old, new)| {
                crate::effects::file_effect(&self.project.root, rel, *old, new)
            })
            .collect();
        if let Some(e) = crate::effects::record(files, crate::effects::graph_effects(before, after))
        {
            v["effects"] = e;
        }
    }

    // The decree's author: the connected client's name, else the login, else `human`,
    // the same ladder a chat ToolSession climbs for edit_fact.
    fn chat_decree_author(&self) -> String {
        self.client
            .lock()
            .unwrap()
            .clone()
            .filter(|c| !c.trim().is_empty())
            .or_else(|| crate::sys::env_var("USER").ok().filter(|u| !u.is_empty()))
            .unwrap_or_else(|| "human".to_string())
    }

    // Undo one decree as its own `decree` changeset. The store holds the gate (a
    // quoted or otherwise non-pending node is refused as `not-a-decree`) and the
    // commit closes the open ratify goal. Mirrors docs/compiler/tools.md#chat-tools.
    pub(super) fn retract_decree(&self, args: &Value) -> Value {
        let id = args["id"].as_str().unwrap_or_default().trim();
        let reason = args["reason"].as_str().unwrap_or_default().trim();
        if id.is_empty() || reason.is_empty() {
            return json!({"error": {"rule": "missing-argument", "message": "pass id (the decreed node) and reason (one line, journaled)"}});
        }
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        let mut s = Store::load(&self.out);
        s.sync_docs(&parsed);
        let rid = s.resolve_id(id).to_string();
        // The store's predicate, asked first so a refusal writes no generation: a
        // decree or derivation is any provenance but a quote, on a node with no
        // sentence of its own; a default view was never decreed.
        let pending = |p: &Option<crate::model::Provenance>| {
            p.as_ref()
                .is_some_and(|p| !matches!(p, crate::model::Provenance::Quote(_)))
        };
        let is_decree = if let Some(r) = s.graph.requirements.get(&rid) {
            r.source.is_none() && pending(&r.provenance)
        } else if let Some(e) = s.graph.entities.get(&rid) {
            pending(&e.provenance)
        } else if let Some(v) = s.graph.views.get(&rid) {
            !v.default && pending(&v.provenance)
        } else if let Some(c) = s.graph.criteria.get(&rid) {
            pending(&c.provenance)
        } else {
            return json!({"error": {"rule": "unknown-id", "message": format!("unknown node `{}`", rid)}});
        };
        if !is_decree {
            return json!({"error": {"rule": "not-a-decree", "message": format!(
                "{} is quoted from the documents, not decreed; retract_requirement or edit_fact changes it", rid)}});
        }
        let report = s.apply(
            vec![crate::store::Op::RetractDecree {
                id: rid.clone(),
                reason: reason.to_string(),
            }],
            &crate::store::Commit::store("decree"),
        );
        if !report.skipped.is_empty() {
            let msg = report.skipped.join("; ");
            let rule = if msg.contains("not a decree") {
                "not-a-decree"
            } else {
                "commit-skipped"
            };
            return json!({"error": {"rule": rule, "message": msg}});
        }
        json!({
            "id": rid, "retracted": true, "path": "decree",
            "generation": report.generation, "swept": report.swept,
        })
    }

    // Raise one node's own threshold, recorded with decree provenance, as its own
    // `decree` changeset. The store holds the gates (registry name, applies to the
    // kind, positive value). Mirrors docs/compiler/tools.md#chat-tools and
    // docs/compiler/graph.md#per-node-bumps.
    pub(super) fn bump_limit(&self, args: &Value) -> Value {
        let id = args["id"].as_str().unwrap_or_default().trim();
        let limit = args["limit"].as_str().unwrap_or_default().trim();
        let Some(value) = args["value"].as_u64().filter(|v| *v > 0) else {
            return json!({"error": {"rule": "bad-value", "message": "value is a positive integer: the node's new threshold"}});
        };
        if id.is_empty() || limit.is_empty() {
            return json!({"error": {"rule": "missing-argument", "message": format!(
                "pass id (an entity or view) and limit, one of: {}",
                crate::limits::LIMITS.iter().map(|l| l.name).collect::<Vec<_>>().join(", "))}});
        }
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        let mut s = Store::load(&self.out);
        s.sync_docs(&parsed);
        let rid = s.resolve_id(id).to_string();
        let is_entity = s.graph.entities.contains_key(&rid);
        let is_view = s.graph.views.contains_key(&rid);
        if !is_entity && !is_view {
            return json!({"error": {"rule": "unknown-id", "message": format!("no entity or view `{}`", rid)}});
        }
        // The store's gates, asked first so a refusal writes no generation: a
        // registry name that applies to the node's kind.
        let Some(l) = crate::limits::limit(limit) else {
            return json!({"error": {"rule": "bad-limit", "message": format!(
                "unknown limit `{}`; one of: {}", limit,
                crate::limits::LIMITS.iter().map(|l| l.name).collect::<Vec<_>>().join(", "))}});
        };
        let applies = (is_entity && crate::limits::ENTITY_LIMITS.contains(&l.name))
            || (is_view && crate::limits::VIEW_LIMITS.contains(&l.name));
        if !applies {
            return json!({"error": {"rule": "bad-limit", "message": format!(
                "`{}` does not apply to {}; {} limits: {}", limit, rid,
                if is_entity { "entity" } else { "view" },
                if is_entity { crate::limits::ENTITY_LIMITS.join(", ") } else { crate::limits::VIEW_LIMITS.join(", ") })}});
        }
        let note = args["note"]
            .as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("raised {} to {}", limit, value));
        let provenance = crate::model::Provenance::Decree {
            author: self.chat_decree_author(),
            at: crate::verify::now_iso(),
            note: Some(note),
        };
        let report = s.apply(
            vec![crate::store::Op::BumpLimit {
                id: rid.clone(),
                limit: limit.to_string(),
                value,
                provenance,
            }],
            &crate::store::Commit::store("decree"),
        );
        if !report.skipped.is_empty() {
            return json!({"error": {"rule": "bad-limit", "message": report.skipped.join("; ")}});
        }
        json!({
            "id": rid, "limit": limit, "value": value, "path": "bump",
            "generation": report.generation,
        })
    }

    // Record a human answer relayed from conversation. An edit option is applied by
    // the answer engine (through this serving's edit sink when the proxy listens);
    // any other answer is recorded and the handling contract returns to the calling
    // agent. Mirrors docs/frontends/acp.md#questions-in-chat.
    pub(super) fn answer_diagnostic(&self, args: &Value) -> Value {
        let id = args["id"].as_str().unwrap_or_default().to_string();
        let reply = if let Some(i) = args["option"].as_u64() {
            crate::answer::Reply::Choice(i as usize)
        } else if let Some(t) = args["text"].as_str() {
            crate::answer::Reply::Text(t.to_string())
        } else {
            return json!({"error": {"rule": "missing-argument", "message": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-chat/pass-option-an-index-into-the-prompt-a828d.md"))}});
        };
        let write =
            |doc: &str, old: &str, new: &str, full: &str| self.write_edit(doc, old, new, full);
        match crate::answer::answer(&self.project, &self.out, &id, reply, Some(&write)) {
            Ok(mut v) => {
                if v["status"] == "handling" {
                    if let Ok(p) = crate::answer::handling_prompt(&self.out, &id) {
                        v["next"] = json!(p);
                    }
                }
                v
            }
            Err(e) => json!({"error": {"rule": "answer-failed", "message": e}}),
        }
    }

    // Maintain the question on a finding, through a ToolSession so the same gates
    // validate the prompt, committed as its own changeset.
    pub(super) fn update_diagnostic_chat(&self, args: &Value) -> Value {
        let target = args["id"].as_str().unwrap_or_default().to_string();
        let mut snapshot = Store::load(&self.out);
        let (parsed, _) = crate::reconcile::parse_all(&self.project);
        snapshot.sync_docs(&parsed);
        let mut scope = WorkScope::serving("mcp-write");
        scope.target = target.clone();
        let mut session =
            ToolSession::new(snapshot, scope, self.mutation_limit, self.context_budget);
        session.gen = self.current_gen_settings();
        session.lint = self.project.linting.clone();
        session.caller = self.caller("chat", &target);
        if let Err(e) = session.dispatch("update_diagnostic", args) {
            return e.to_value();
        }
        let ops = std::mem::take(&mut session.staged);
        let mut s = Store::load(&self.out);
        s.sync_docs(&parsed);
        let item = crate::model::WorkItem {
            task: "chat".into(),
            target,
            dirty_sections: vec![],
            stale_anchors: vec![],
            proposals: Vec::new(),
        };
        let report = s.apply(ops, &item.commit(1, None));
        if !report.skipped.is_empty() {
            return json!({"error": {"rule": "commit-skipped", "message": report.skipped.join("; ")}});
        }
        json!({"updated": true})
    }

    pub(super) fn revise_requirement(&self, args: &Value) -> Value {
        let rid = args["id"].as_str().unwrap_or_default();
        let new_text = args["new_text"].as_str().unwrap_or_default().trim();
        if new_text.is_empty() {
            return json!({"error": {"rule": "missing-argument", "message": "new_text is required: the prose sentence that replaces the old quote"}});
        }
        let store = Store::load(&self.out);
        let rid = store.resolve_id(rid).to_string();
        let Some(r) = store.graph.requirements.get(&rid) else {
            return json!({"error": {"rule": "unknown-id", "message": format!("unknown requirement `{}`", rid)}});
        };
        let Some(src) = r.source.as_ref() else {
            return json!({"error": {"rule": "not-quoted", "message": format!(
                "{} has no sentence in the documents yet ({}); ratify or retract it instead of revising prose", rid, crate::session::provenance_line(r))}});
        };
        let (doc, section, old_quote) = (src.doc.clone(), src.section.clone(), src.quote.clone());
        let statement = args["statement"]
            .as_str()
            .unwrap_or(&r.statement)
            .to_string();
        let section_raw = store
            .docs
            .get(&doc)
            .and_then(|d| d.sections.get(&section))
            .map(|s| s.raw.clone());
        drop(store);
        let path = self.project.root.join(&doc);
        let old_full = match crate::vfs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", doc, e)}})
            }
        };
        let edit = match ProseEdit::locate(
            &doc,
            &section,
            section_raw.as_deref(),
            &old_full,
            &old_quote,
            new_text,
        ) {
            Ok(e) => e,
            Err(_) => {
                return json!({"error": {"rule": "stale-anchor", "message": format!(
                    "the requirement's quote no longer locates in {}#{}; compile first, then revise. The section reads:\n{}",
                    doc, section, section_raw.unwrap_or_default())}})
            }
        };
        self.dual_commit(
            edit,
            ("update_requirement", json!({"id": rid, "statement": statement, "section": format!("{}#{}", doc, section), "quote": new_text})),
            &rid,
        )
    }

    pub(super) fn add_requirement(&self, args: &Value) -> Value {
        let doc = args["doc"].as_str().unwrap_or_default().to_string();
        let section = args["section"]
            .as_str()
            .unwrap_or_default()
            .trim_start_matches(&format!("{}#", doc))
            .to_string();
        let text = args["text"].as_str().unwrap_or_default().trim().to_string();
        let statement = args["statement"].as_str().unwrap_or_default();
        if doc.is_empty() || section.is_empty() || text.is_empty() || statement.is_empty() {
            return json!({"error": {"rule": "missing-argument", "message": "doc, section, text, statement, and entities are required"}});
        }
        let Some(sec_raw) = self.section_raw(&doc, &section) else {
            return json!({"error": {"rule": "unknown-section", "message": format!(include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-chat/no-section-in-the-graph-for-a-64836.md")), doc, section)}});
        };
        let path = self.project.root.join(&doc);
        let old_full = match crate::vfs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", doc, e)}})
            }
        };
        // The insertion point: after the quote located inside the named section, or at
        // the section's end. A quote elsewhere in the document is not this section's.
        let window = Some(sec_raw.trim())
            .filter(|raw| !raw.is_empty())
            .and_then(|raw| old_full.find(raw).map(|b| (b, b + raw.len())));
        let edit = match args["after_quote"].as_str() {
            Some(_) if window.is_none() => {
                return json!({"error": {"rule": "stale-anchor", "message": format!(
                    "section {}#{} drifted from the document on disk; compile first", doc, section)}})
            }
            Some(q) => match window.and_then(|(wb, we)| {
                crate::md::locate_bytes(&old_full[wb..we], q).map(|(_, e)| wb + e)
            }) {
                Some(at) => ProseEdit {
                    doc: doc.clone(),
                    section: section.clone(),
                    old_text: String::new(),
                    new_text: text.clone(),
                    full: format!(
                        "{}\n\n{}{}",
                        old_full[..at].trim_end_matches('\n'),
                        text,
                        &old_full[at..]
                    ),
                    old_full,
                },
                None => {
                    return json!({"error": {"rule": "stale-anchor", "message": format!(
                        include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-chat/after-quote-does-not-locate-in-copy-ce5d1.md")), doc, section)}})
                }
            },
            None => match ProseEdit::locate(&doc, &section, Some(&sec_raw), &old_full, "", &text) {
                Ok(e) => e,
                Err(e) => return json!({"error": {"rule": "stale-anchor", "message": e}}),
            },
        };
        let entities: Vec<String> = args["entities"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        self.dual_commit(
            edit,
            // The author's own sentence is a stated fact: `constraint` is its honest
            // facet, and a review may sharpen it
            // (docs/compiler/model/requirement.md#facets).
            ("upsert_requirement", json!({"statement": statement, "entities": entities, "section": format!("{}#{}", doc, section), "quote": text, "facets": [{"facet": "constraint", "reasoning": "stated by the author in the document"}]})),
            &doc,
        )
    }

    pub(super) fn retract_requirement(&self, args: &Value) -> Value {
        let rid = args["id"].as_str().unwrap_or_default();
        let reason = args["reason"].as_str().unwrap_or_default();
        let store = Store::load(&self.out);
        let rid = store.resolve_id(rid).to_string();
        let Some(r) = store.graph.requirements.get(&rid) else {
            return json!({"error": {"rule": "unknown-id", "message": format!("unknown requirement `{}`", rid)}});
        };
        let Some(src) = r.source.as_ref() else {
            return json!({"error": {"rule": "not-quoted", "message": format!(
                "{} has no sentence in the documents ({}); retract the decree or derivation instead", rid, crate::session::provenance_line(r))}});
        };
        let (doc, section, old_quote) = (src.doc.clone(), src.section.clone(), src.quote.clone());
        drop(store);
        let path = self.project.root.join(&doc);
        let old_full = match crate::vfs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", doc, e)}})
            }
        };
        let Some((b, e)) = crate::md::locate_bytes(&old_full, &old_quote) else {
            return json!({"error": {"rule": "stale-anchor", "message": format!(
                "the requirement's quote no longer locates in {}; compile first", doc)}});
        };
        // Take the sentence out; a bullet line loses its marker and trailing newline too.
        let mut start = b;
        let line_start = old_full[..b].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let lead = &old_full[line_start..b];
        if lead.trim_start().starts_with("- ") || lead.trim().is_empty() {
            start = line_start;
        }
        let mut end = e;
        if old_full[e..].starts_with('\n') && start == line_start {
            end = e + 1;
        }
        let full = format!("{}{}", &old_full[..start], &old_full[end..]);
        let edit = ProseEdit {
            doc,
            section,
            old_text: old_quote,
            new_text: String::new(),
            full,
            old_full,
        };
        self.dual_commit(
            edit,
            ("delete_requirement", json!({"id": rid, "reason": reason})),
            &rid,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn init_project(&self) -> Value {
        let root = &self.project.root;
        if root.join(crate::document_sets::CONFIG).vexists() {
            return json!({"error": {"rule": "already-a-project", "message": "jazyk.toml already exists here"}});
        }
        let path = root.join(crate::document_sets::CONFIG);
        let written = path
            .parent()
            .map_or(Ok(()), crate::vfs::create_dir_all)
            .and_then(|_| crate::vfs::write(&path, crate::cli::INIT_TOML));
        if let Err(e) = written {
            return json!({"error": {"rule": "write-failed", "message": format!("{}: {}", path.display(), e)}});
        }
        let made = match crate::cli::init_scaffold(root) {
            Ok(made) => made,
            Err(e) => return json!({"error": {"rule": "write-failed", "message": e}}),
        };
        json!({"initialized": root.display().to_string(), "created": made,
               "next": SERVING.get("init-next")})
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn init_project(&self) -> Value {
        json!({"error": "the browser backend does not scaffold a project"})
    }

    // Edits the allowlisted keys under the project lock and a configuration hash, never
    // while a build runs. The agent, executor, and model settings stay out of reach.
    // Mirrors docs/frontends/acp.md#project-tools.
    pub(super) fn update_project_settings(&self, args: &Value) -> Value {
        const KEYS: [(&str, &[&str]); 4] = [
            ("workflow.compile", &["manual", "auto"]),
            ("workflow.generate", &["manual", "auto"]),
            ("workflow.worker", &["internal", "agent", "any"]),
            ("gen.worker", &["agentic", "pipeline"]),
        ];
        let supported = || KEYS.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ");
        let Some(settings) = args["settings"].as_object() else {
            return json!({"error": {"rule": "missing-argument", "message": "settings is required: a map of key to value"}});
        };
        if !self.initialized() {
            return json!({"error": {"rule": "not-a-project", "message":
                "no jazyk.toml here to edit; call init_project first"}});
        }
        for (key, value) in settings {
            let Some((_, allowed)) = KEYS.iter().find(|(k, _)| k == key) else {
                return json!({"error": {"rule": "unsupported-key", "message": format!(
                    "`{}` is not editable here; supported: {}", key, supported())}});
            };
            let Some(value) = value.as_str() else {
                return json!({"error": {"rule": "bad-value", "message": format!("`{}` takes a string value", key)}});
            };
            if !allowed.is_empty() && !allowed.contains(&value) {
                return json!({"error": {"rule": "bad-value", "message": format!(
                    "`{}` takes one of {}", key, allowed.join(", "))}});
            }
            // `auto` is a standing approval of all future work: only a human makes
            // it (docs/compiler/control-plane.md#modes-and-releases).
            if matches!(key.as_str(), "workflow.compile" | "workflow.generate") && value == "auto" {
                return json!({"error": {"rule": "needs-human", "message": format!(
                    include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-chat/key-auto-lets-work-run-with-no-dc569.md")), key = key)}});
            }
        }
        let Some(base) = args["baseHash"].as_str() else {
            return json!({"error": {"rule": "missing-argument", "message":
                "baseHash is required: the configuration hash list_document_sets returns"}});
        };
        let root = &self.project.root;
        let Ok(_project) = crate::document_sets::ProjectLock::acquire(root, "project.lock") else {
            return json!({"error": {"rule": "project-busy", "message":
                "a build or another project operation holds the project; retry when it finishes"}});
        };
        let before = match crate::document_sets::Workspace::load(root) {
            Ok(w) => w,
            Err((_, e)) => return json!({"error": {"rule": "bad-config", "message": e}}),
        };
        let running = std::iter::once(self.out.clone())
            .chain(
                before
                    .sets
                    .iter()
                    .filter_map(|s| before.resolve(Some(&s.id)).ok().map(|p| p.out)),
            )
            .find_map(|out| crate::control::build_lease(&out));
        if let Some(lease) = running {
            return json!({"error": {"rule": "build-running", "message": format!(
                "a build is running (lease `{}`); settings change after it finishes", lease.worker)}});
        }
        let path = root.join(crate::document_sets::CONFIG);
        let Ok(_config) = crate::document_sets::ProjectLock::acquire(root, "config.lock") else {
            return json!({"error": {"rule": "project-busy", "message": "another configuration edit is in progress; retry"}});
        };
        let old_full = match crate::vfs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                return json!({"error": {"rule": "read-failed", "message": format!("{}: {}", path.display(), e)}})
            }
        };
        if crate::model::hash_hex(&old_full) != base || before.hash != base {
            return json!({"error": {"rule": "stale-config", "message":
                include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/mcp-chat/configuration-changed-call-list-document-sets-for-c1210.md"))}});
        }
        let mut full = old_full.clone();
        for (key, value) in settings {
            let (section, k) = key.split_once('.').unwrap_or(("", key.as_str()));
            full = match try_toml_set(&full, section, k, value.as_str().unwrap_or_default()) {
                Ok(text) => text,
                Err(e) => return json!({"error": {"rule": "bad-config", "message": e}}),
            };
        }
        if let Err((_, e)) = crate::document_sets::Workspace::parse(root, &full, true) {
            return json!({"error": {"rule": "bad-config", "message": e}});
        }
        if let Err(e) = self.write_edit(crate::document_sets::CONFIG, &old_full, &full, &full) {
            return json!({"error": {"rule": "write-failed", "message": e}});
        }
        drop(_config);
        crate::agents_md::sync(root).report();
        json!({"updated": settings.keys().cloned().collect::<Vec<_>>(),
               "hash": crate::model::hash_hex(&full),
               "note": "jazyk.toml edited in place; a running GUI reloads it live"})
    }
}

// Minimal line edit on a TOML text: set `key = "value"` inside `[section]`, appending
// the section or the key when missing, touching nothing else. A text that does not
// parse, or a section path that names a non-table value, comes back unchanged.
pub fn toml_set(text: &str, section: &str, key: &str, value: &str) -> String {
    try_toml_set(text, section, key, value).unwrap_or_else(|_| text.to_string())
}

// The same edit, saying why it cannot apply instead of panicking or guessing.
pub fn try_toml_set(text: &str, section: &str, key: &str, value: &str) -> Result<String, String> {
    let mut document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("{} does not parse: {}", crate::document_sets::CONFIG, e))?;
    let mut table: &mut dyn toml_edit::TableLike = document.as_table_mut();
    let mut path = String::new();
    for part in section.split('.').filter(|p| !p.is_empty()) {
        path = if path.is_empty() {
            part.to_string()
        } else {
            format!("{path}.{part}")
        };
        table = table
            .entry(part)
            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_like_mut()
            .ok_or_else(|| {
                format!(
                    "`{path}` is not a table in {}",
                    crate::document_sets::CONFIG
                )
            })?;
    }
    if table.get(key).is_some_and(|item| item.is_table_like()) {
        return Err(format!(
            "`{key}` is a table in {}, not a value",
            crate::document_sets::CONFIG
        ));
    }
    table.insert(key, toml_edit::value(value));
    Ok(document.to_string())
}
