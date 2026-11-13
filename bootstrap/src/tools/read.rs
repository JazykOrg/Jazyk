use super::*;

impl ToolSession {
    // Search across the snapshot plus this turn's staged creates.
    pub(super) fn search_all(&self, query: &str) -> Vec<(String, String, String)> {
        let mut hits: Vec<(String, String, String)> = Vec::new();
        let q = query.trim().to_lowercase();
        for (id, e) in &self.staged_entities {
            if e.name.to_lowercase().contains(&q) || q.contains(&e.name.to_lowercase()) {
                hits.push((
                    id.clone(),
                    e.name.clone(),
                    e.definition.clone().unwrap_or_default(),
                ));
            }
        }
        hits.extend(self.snapshot.search(query));
        hits.truncate(8);
        hits
    }

    // Search view titles across the snapshot plus this turn's staged creates. Same
    // tiering as entity search (exact, then substring, then token overlap); views have
    // no aliases. Mirrors docs/compiler/tools.md#read-tools.
    fn search_views(&self, query: &str) -> Vec<(String, String)> {
        let q = crate::store::normalize(query);
        let q_tokens: std::collections::BTreeSet<&str> = q.split(' ').collect();
        let mut scored: Vec<(u32, String, String)> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (id, v) in self
            .staged_views
            .iter()
            .chain(self.snapshot.graph.views.iter())
        {
            if !seen.insert(id.clone()) {
                continue;
            }
            let n = crate::store::normalize(&v.title);
            let tier = if n == q {
                0
            } else if n.contains(&q) || q.contains(n.as_str()) {
                1
            } else {
                let n_tokens: std::collections::BTreeSet<&str> = n.split(' ').collect();
                if q_tokens.intersection(&n_tokens).count() > 0 {
                    2
                } else {
                    continue;
                }
            };
            scored.push((tier, id.clone(), v.title.clone()));
        }
        scored.sort();
        scored
            .into_iter()
            .take(8)
            .map(|(_, id, title)| (id, title))
            .collect()
    }

    fn search_view_miss(&self, query: &str) -> Value {
        let known: std::collections::BTreeMap<_, _> = self
            .snapshot
            .graph
            .views
            .iter()
            .chain(self.staged_views.iter())
            .collect();
        let total = known.len();
        let views: Vec<_> = known
            .into_iter()
            .take(25)
            .map(|(id, view)| format!("{} ({})", id, view.title))
            .collect();
        let mut next = format!(
            include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/inline/tools-read/no-view-matches-of-the-graph-s-3cb84.md"
            )),
            query,
            views.len(),
            total
        );
        if total > views.len() {
            next.push_str(" Refine the query to find an unlisted view title.");
        }
        json!({"hits": [], "viewCount": total, "views": views, "next": next})
    }

    // A miss is an answer, not a dead end. A bare empty list reads as "ask again";
    // models loop on it. Say what the graph holds and what to do next.
    fn search_miss(&self, query: &str) -> Value {
        let mut known: Vec<String> = self
            .staged_entities
            .iter()
            .map(|(id, e)| format!("{} ({})", id, e.name))
            .collect();
        for (id, e) in &self.snapshot.graph.entities {
            if !self.staged_entities.contains_key(id) {
                known.push(format!("{} ({})", id, e.name));
            }
        }
        let total = known.len();
        known.truncate(25);
        let shown = known.len();
        json!({
            "hits": [],
            "entityCount": total,
            "entities": known,
            "next": if total == 0 {
                format!(
                    include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-read/no-match-for-the-graph-holds-no-6bd09.md")),
                    query
                )
            } else {
                format!(
                    include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-read/no-match-for-of-the-graph-s-f53e8.md")),
                    query, shown, total
                )
            }
        })
    }

    // Resolve a section argument: either "doc.md#/ref" or a bare "/ref" against the
    // batch's document.
    pub(super) fn resolve_section(&self, section: &str) -> Result<(String, String), ToolError> {
        // A bare document name (`orders.md`, `orders.md#`) is its root section.
        // Mirrors docs/compiler/tools.md#read-tools.
        let bare = section
            .strip_suffix("#/")
            .or_else(|| section.strip_suffix('#'))
            .unwrap_or(section);
        if !bare.contains('#') {
            if let Some(rec) = self.snapshot.docs.get(bare) {
                let mut roots: Vec<&String> = rec
                    .sections
                    .iter()
                    .filter(|(_, c)| c.parent.is_none())
                    .map(|(r, _)| r)
                    .collect();
                roots.sort_by_key(|r| r.len());
                if let Some(root) = roots.first() {
                    return Ok((bare.to_string(), (*root).clone()));
                }
            }
        }
        let full = if section.starts_with('/') {
            match self.scope.doc() {
                Some(d) => format!("{}#{}", d, section),
                None => {
                    return Err(ToolError::new(
                        "bad-section",
                        format!(
                            include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-read/bare-section-reference-needs-a-document-use-2c15e.md")),
                            section, section
                        ),
                    ))
                }
            }
        } else {
            section.to_string()
        };
        let (doc, sec) = split_section_ref(&full).ok_or_else(|| {
            // Repair-oriented: name the sections this batch is actually working on.
            let owned: Vec<String> = self
                .scope
                .reconcile_scopes()
                .iter()
                .flat_map(|g| {
                    let d = g.doc.clone().unwrap_or_default();
                    g.sections
                        .iter()
                        .map(move |s| format!("{}#{}", d, s))
                        .collect::<Vec<_>>()
                })
                .collect();
            let hint = if owned.is_empty() {
                String::new()
            } else {
                format!("; this batch's sections: {}", owned.join(", "))
            };
            ToolError::new(
                "bad-section",
                format!(
                    "bad section reference `{}`; expected doc.md#/ref{}",
                    section, hint
                ),
            )
        })?;
        if !self
            .snapshot
            .docs
            .get(&doc)
            .map(|d| d.sections.contains_key(&sec))
            .unwrap_or(false)
        {
            return Err(ToolError::new(
                "unknown-section",
                format!("unknown section `{}#{}`", doc, sec),
            ));
        }
        // A fenced block resolves to the section holding it: the block owns no mark
        // and no goal, and its parent keeps the whole body.
        // Mirrors docs/compiler/tools.md#write-tools.
        let sec = crate::goals::owning_section(&self.snapshot, &doc, &sec);
        Ok((doc, sec))
    }

    // One page of a diagnostics listing: at most DIAGNOSTICS_PAGE entries from
    // `offset`, `count` the number that match. Mirrors docs/compiler/tools.md#read-tools.
    fn diagnostics_page(list: Vec<Value>, args: &Value) -> Value {
        let count = list.len();
        let offset = args["offset"].as_u64().map(|o| o as usize).unwrap_or(0);
        let page: Vec<Value> = list
            .into_iter()
            .skip(offset)
            .take(DIAGNOSTICS_PAGE)
            .collect();
        let next = offset.saturating_add(page.len());
        let mut v = json!({"diagnostics": page, "count": count});
        if next < count {
            v["truncated"] = json!(true);
            v["nextOffset"] = json!(next);
        }
        v
    }

    pub(super) fn load_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let target = Self::str_arg(args, "target")?;
        // A depth past u32 saturates instead of wrapping to a shallow load.
        let depth = u32::try_from(args["depth"].as_u64().unwrap_or(1).max(1)).unwrap_or(u32::MAX);
        let id = self.canon_entity_id(&target).unwrap_or(target);
        self.refuse_past_high_water("load", Some(&id))?;
        let text = self.loaded.load(&self.snapshot, &id, depth).map_err(|e| {
            // A node staged this session is not in the committed snapshot
            // the loaded set renders. Mirrors docs/compiler/tools.md#read-tools.
            if self.staged_reqs.contains(&id) || self.staged_entities.contains_key(&id) {
                ToolError::new(
                    "staged-target",
                    format!(
                        include_str!(concat!(
                            env!("OUT_DIR"),
                            "/payloads/goals/prompts/inline/tools-read/is-staged-in-this-session-and-not-f3b1b.md"
                        )),
                        id
                    ),
                )
            } else {
                ToolError::new("bad-target", e)
            }
        })?;
        let mut v = json!({"pack": text});
        // In a serving, the first node of a kind loaded brings the kind's
        // skill, once; a batch works under its goal kinds' skills and reads
        // other nodes as reference. Mirrors docs/compiler/sessions.md#skills.
        if self.scope.goals.is_empty() && self.scope.target.is_empty() && !self.skills.has_pinned()
        {
            if let Some(skill) = crate::session::skill_for_target(&self.snapshot, &id) {
                if let Ok(SkillLoad::Rendered(payload)) = self.skills.activate(skill) {
                    v["skill"] = json!(format!("[skill: {} (active)]\n{}", skill, payload));
                }
            }
        }
        v["status"] = json!(self.condensed_status());
        Ok(v)
    }

    pub(super) fn expand_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let handle = Self::str_arg(args, "handle")?;
        self.refuse_past_high_water("expand", None)?;
        let text = self
            .loaded
            .expand(&self.snapshot, &handle)
            .map_err(|e| ToolError::new("unknown-handle", e))?;
        Ok(json!({"pack": text, "status": self.condensed_status()}))
    }

    pub(super) fn unload_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let target = Self::str_arg(args, "target")?;
        if !self.loaded.unload(&target) {
            return Err(ToolError::new(
                "bad-target",
                format!(
                    "`{}` is not loaded; graph_status lists the loaded set",
                    target
                ),
            ));
        }
        // A reload after an unload genuinely re-renders: clear the load count
        // for this target so the budget-forced unload->reload cycle is not
        // refused as a repeat.
        self.repeats.remove(&format!("load|{}", target));
        // Unloading the last node of a kind marks the kind's skill inactive.
        if let Some(skill) = crate::session::skill_for_target(&self.snapshot, &target) {
            let another = self.loaded.items.iter().any(|i| {
                crate::session::skill_for_target(&self.snapshot, &i.target) == Some(skill)
            });
            if !another {
                self.skills.deactivate(skill);
            }
        }
        Ok(json!({"unloaded": target, "status": self.condensed_status()}))
    }

    pub(super) fn get_view_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let id = Self::str_arg(args, "id")?;
        let Some(v) = self.snapshot.graph.views.get(&id) else {
            return Err(ToolError::new(
                "unknown-id",
                format!(
                    "unknown view id `{}`; search with kind view lists titles",
                    id
                ),
            ));
        };
        let slug = id.rsplit('/').next().unwrap_or_default();
        // The drill-down links: computed here from the containment tree and
        // the level views, never stored. Mirrors docs/compiler/tools.md#read-tools.
        let children = self.view_children(v);
        // The relationships among the members, lifted and collapsed as the
        // rendering draws them. Mirrors docs/compiler/tools.md#read-tools.
        let relationships: Vec<Value> = crate::render::view_relationships(&self.snapshot, &id, v)
            .into_iter()
            .map(|(a, b, ty, cardinality, count)| {
                let mut r = json!({"a": a, "b": b, "type": ty});
                if let Some(c) = cardinality {
                    r["cardinality"] = json!(c);
                }
                if count > 1 {
                    r["count"] = json!(count);
                }
                r
            })
            .collect();
        let reply = json!({
            "id": id, "kind": v.kind, "title": v.title, "members": v.members,
            "excluded": v.excluded.iter().map(|x| json!({"id": x.id, "note": x.note})).collect::<Vec<_>>(),
            "collapse": v.collapse, "query": v.query, "default": v.default,
            "rendering": format!("derived/diagrams/{}/{}.svg", v.kind, slug),
            "children": children,
            "relationships": relationships,
        });
        self.loaded.load_stub(&self.snapshot, &id);
        Ok(reply)
    }

    pub(super) fn search_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        // Several names in one call: one result per query, in order, so
        // search-before-create costs one round per section.
        // Mirrors docs/compiler/tools.md#read-tools.
        if let Some(qs) = args.get("queries").and_then(Value::as_array) {
            let mut results = Vec::new();
            for q in qs {
                let Some(q) = q.as_str().map(str::trim).filter(|q| !q.is_empty()) else {
                    return Err(ToolError::new(
                        "bad-args",
                        "queries takes a list of non-empty strings".to_string(),
                    ));
                };
                let mut one = json!({"query": q});
                if let Some(k) = args.get("kind") {
                    one["kind"] = k.clone();
                }
                // Straight to the handler: one queries call is one call
                // under the repeated-call guard, never one per name.
                let mut r = self.dispatch_inner("search", &one)?;
                r["query"] = json!(q);
                results.push(r);
            }
            return Ok(json!({"results": results}));
        }
        let query = Self::str_arg(args, "query")?;
        let kind = Self::opt_str(args, "kind").unwrap_or_else(|| "entity".to_string());
        if kind != "entity" && kind != "view" {
            return Err(ToolError::new(
                "bad-args",
                format!("unknown search kind `{}`; expected entity or view", kind),
            ));
        }
        if kind == "view" {
            let hits = self.search_views(&query);
            if !hits.is_empty() {
                // A read's subject joins the loaded set as a stub.
                for (id, _) in &hits {
                    self.loaded.load_stub(&self.snapshot, id);
                }
                return Ok(json!({
                    "hits": hits
                        .iter()
                        .map(|(id, title)| json!({"id": id, "name": title}))
                        .collect::<Vec<_>>()
                }));
            }
            return Ok(self.search_view_miss(&query));
        }
        let hits = self.search_all(&query);
        if !hits.is_empty() {
            // A read's subject joins the loaded set as a stub.
            for (id, _, _) in &hits {
                self.loaded.load_stub(&self.snapshot, id);
            }
            return Ok(json!({
                "hits": hits
                    .iter()
                    .map(|(id, name, def)| json!({"id": id, "name": name, "definition": def}))
                    .collect::<Vec<_>>()
            }));
        }
        Ok(self.search_miss(&query))
    }

    pub(super) fn read_section_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let r = Self::str_arg(args, "ref")?;
        let (doc, sec) = self.resolve_section(&r)?;
        let rec = &self.snapshot.docs[&doc];
        let s = &rec.sections[&sec];
        let children: Vec<String> = rec
            .sections
            .iter()
            .filter(|(_, c)| c.parent.as_deref() == Some(sec.as_str()))
            .map(|(r, c)| format!("{}#{} ({})", doc, r, c.title))
            .collect();
        // Bounded by lines and bytes from `line`, as file reads are.
        // Mirrors docs/compiler/tools.md#read-tools.
        let start = args["line"]
            .as_u64()
            .map(|l| (l as usize).saturating_sub(1))
            .unwrap_or(0);
        let total = s.raw.split_inclusive('\n').count();
        let mut body = String::new();
        let mut taken = 0usize;
        for line in s
            .raw
            .split_inclusive('\n')
            .skip(start)
            .take(READ_SECTION_LINES)
        {
            if body.len() + line.len() > READ_SECTION_BYTES {
                if taken == 0 {
                    let mut end = READ_SECTION_BYTES.min(line.len());
                    while !line.is_char_boundary(end) {
                        end -= 1;
                    }
                    body.push_str(&line[..end]);
                    taken = 1;
                }
                break;
            }
            body.push_str(line);
            taken += 1;
        }
        let mut reply = json!({"title": s.title, "raw": body, "children": children});
        if start > 0 || start + taken < total || body.len() < s.raw.len() {
            reply["totalLines"] = json!(total);
        }
        if start + taken < total {
            reply["truncated"] = json!(true);
            reply["nextLine"] = json!(start + taken + 1);
        }
        self.loaded
            .load_stub(&self.snapshot, &format!("{}#{}", doc, sec));
        Ok(reply)
    }

    pub(super) fn get_entity_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let id = Self::str_arg(args, "id")?;
        let rid = self
            .canon_entity_id(&id)
            .ok_or_else(|| self.unknown_entity_error(&id))?;
        // Staged first, then the snapshot: canon_entity_id resolves the
        // session's own staged entities, so the lookup sees them too.
        // Read-your-writes: staged updates replay over the base, so the
        // session reads back what it just wrote.
        let mut e = self
            .staged_entities
            .get(&rid)
            .or_else(|| self.snapshot.graph.entities.get(&rid))
            .ok_or_else(|| self.unknown_entity_error(&id))?
            .clone();
        for op in &self.staged {
            let Op::UpdateEntity {
                id: uid,
                name,
                definition,
                add_aliases,
                remove_aliases,
                add_mention,
                stereotype,
                parent,
                clear_parent,
                set_attributes,
                add_attributes,
                ..
            } = op
            else {
                continue;
            };
            if *uid != rid && self.snapshot.resolve_id(uid) != rid {
                continue;
            }
            if let Some(n) = name {
                e.name = n.clone();
            }
            if let Some(d) = definition {
                e.definition = Some(d.clone());
            }
            for a in add_aliases {
                if !e.aliases.contains(a) {
                    e.aliases.push(a.clone());
                }
            }
            e.aliases.retain(|a| !remove_aliases.contains(a));
            if let Some(m) = add_mention {
                e.mentions.push(m.clone());
            }
            if let Some(s) = stereotype {
                e.stereotype = Some(s.clone());
            }
            if let Some(p) = parent {
                e.parent = Some(p.clone());
            }
            if *clear_parent {
                e.parent = None;
            }
            if let Some(set) = set_attributes {
                e.attributes = set.clone();
            }
            for a in add_attributes {
                match e.attributes.iter_mut().find(|x| x.name == a.name) {
                    Some(x) => *x = a.clone(),
                    None => e.attributes.push(a.clone()),
                }
            }
        }
        let e = &e;
        let reqs: Vec<Value> = self
            .snapshot
            .requirements_referencing(&rid)
            .iter()
            .filter_map(|r| {
                self.snapshot
                    .graph
                    .requirements
                    .get(r)
                    .map(|req| json!({"id": r, "statement": req.statement}))
            })
            .collect();
        let rels: Vec<Value> = self
            .snapshot
            .graph
            .relationships
            .iter()
            .filter(|(_, rel)| rel.members.contains(&rid))
            .map(|(id, rel)| json!({"id": id, "type": rel.strongest(), "members": rel.members, "contributions": rel.contributions}))
            .collect();
        let mut v = json!({
            "id": rid, "name": e.name, "definition": e.definition, "aliases": e.aliases,
            "mentions": e.mentions.iter().map(|m| json!({"doc": m.doc, "section": m.section, "quote": m.quote})).collect::<Vec<_>>(),
            "requirements": reqs, "relationships": rels
        });
        // The default scope is noise; showing it reads as a value someone set.
        if e.scope != "public" {
            v["scope"] = json!(e.scope);
        }
        // The containment and label fields a review judges: absent means absent.
        // Mirrors docs/compiler/tools.md#read-tools.
        if let Some(p) = &e.parent {
            v["parent"] = json!(p);
        }
        if let Some(s) = &e.stereotype {
            v["stereotype"] = json!(s);
        }
        if !e.attributes.is_empty() {
            v["attributes"] = json!(e.attributes);
        }
        let children: Vec<&String> = self
            .snapshot
            .graph
            .entities
            .iter()
            .filter(|(_, c)| c.parent.as_deref() == Some(rid.as_str()))
            .map(|(cid, _)| cid)
            .collect();
        if !children.is_empty() {
            v["children"] = json!(children);
        }
        self.loaded.load_stub(&self.snapshot, &rid);
        Ok(v)
    }

    pub(super) fn diagnostics_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let lifecycle = Self::opt_str(args, "lifecycle").unwrap_or_else(|| "open".to_string());
        let rule = Self::opt_str(args, "rule");
        let subject = Self::opt_str(args, "subject");
        // Read-your-writes: the session's own staged findings ride beside
        // the snapshot, staged content winning on a shared id.
        let mut merged: std::collections::BTreeMap<&String, &Diagnostic> =
            self.snapshot.graph.diagnostics.iter().collect();
        for (i, d) in &self.staged_diags {
            merged.insert(i, d);
        }
        let list: Vec<Value> = merged
            .into_iter()
            .filter(|(_, d)| lifecycle == "all" || d.lifecycle == lifecycle)
            .filter(|(_, d)| rule.as_deref().is_none_or(|r| d.rule == r))
            .filter(|(_, d)| {
                // An entity subject covers its requirements and criteria, as the
                // review hints count them (docs/compiler/tools.md#read-tools).
                subject.as_deref().is_none_or(|s| {
                    d.subjects.iter().any(|x| x == s)
                        || d.subjects.iter().any(|x| {
                            self.snapshot.graph.requirements.get(x).is_some_and(|r| {
                                r.entities.iter().any(|e| self.snapshot.resolve_id(e) == s)
                            }) || self.snapshot.graph.criteria.get(x).is_some_and(|c| {
                                self.snapshot
                                    .graph
                                    .requirements
                                    .get(&c.requirement)
                                    .is_some_and(|r| {
                                        r.entities.iter().any(|e| self.snapshot.resolve_id(e) == s)
                                    })
                            })
                        })
                })
            })
            .map(|(id, d)| {
                let mut v = json!({
                    "id": id, "rule": d.rule, "severity": d.severity, "lifecycle": d.lifecycle,
                    "triage": d.triage, "subjects": d.subjects, "message": d.message,
                });
                // The standing question rides along, so verifying a prompt
                // never requires reading the shard files.
                if let Some(p) = &d.prompt {
                    v["question"] = json!(p.question);
                    v["options"] = json!(p
                        .options
                        .iter()
                        .map(|o| o.label.clone())
                        .collect::<Vec<_>>());
                }
                if let Some(a) = &d.answer {
                    v["answered"] = json!(a.status);
                }
                // The build's own bookkeeping: never a session's to resolve.
                // Mirrors docs/compiler/model/diagnostic.md#lifecycle-and-triage.
                if harness_owned_rule(&d.rule) {
                    v["owner"] = json!("harness");
                }
                v
            })
            .collect();
        // The subjects of the listed findings join the loaded set as stubs.
        let subjects: Vec<String> = list
            .iter()
            .filter(|d| rule.is_some() || d["owner"] != "harness")
            .flat_map(|d| d["subjects"].as_array().cloned().unwrap_or_default())
            .filter_map(|s| s.as_str().map(String::from))
            .take(8)
            .collect();
        for s in subjects {
            self.loaded.load_stub(&self.snapshot, &s);
        }
        // Unfiltered, the build's bookkeeping is counted per rule, not listed:
        // one proposal per derived criterion buried the judgment findings.
        // Mirrors docs/compiler/tools.md#read-tools.
        if rule.is_none() {
            let mut bookkeeping: std::collections::BTreeMap<String, usize> = Default::default();
            let list: Vec<Value> = list
                .into_iter()
                .filter(|d| {
                    if d["owner"] == "harness" {
                        *bookkeeping
                            .entry(d["rule"].as_str().unwrap_or("").to_string())
                            .or_insert(0) += 1;
                        false
                    } else {
                        true
                    }
                })
                .collect();
            let mut v = Self::diagnostics_page(list, args);
            if !bookkeeping.is_empty() {
                v["bookkeeping"] = json!(bookkeeping);
                v["bookkeepingNote"] = json!(include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/tools-read/the-build-s-own-records-never-a-9b5c0.md"
                )));
            }
            return Ok(v);
        }
        Ok(Self::diagnostics_page(list, args))
    }
}

impl ToolSession {
    pub(super) fn inspect_input_tool(&self, args: &Value) -> Result<Value, ToolError> {
        let path = Self::str_arg(args, "path")?;
        let project = crate::input::project_for_store(&self.snapshot).ok_or_else(|| {
            ToolError::new("no-project", "cannot resolve this document set".into())
        })?;
        if !crate::document_sets::safe_relative(&project.root, &path)
            || !project.doc_files().contains(&project.root.join(&path))
        {
            return Err(ToolError::new(
                "input-selection",
                "path must be a selected input".into(),
            ));
        }
        let bytes = crate::vfs::read(project.root.join(&path))
            .map_err(|e| ToolError::new("input-unavailable", e.to_string()))?;
        let sections = self
            .snapshot
            .docs
            .get(&path)
            .map(|record| record.sections.keys().collect::<Vec<_>>())
            .unwrap_or_default();
        Ok(
            json!({"path":path,"sha256":crate::task_runtime::digest(&bytes),"size":bytes.len(),"classification":self.snapshot.decisions.inputs.get(&path),"quoteReferences":self.snapshot.input_quote_references(&path),
                "sectionCount":sections.len(),"sectionRefs":sections.iter().take(24).map(|section| format!("{path}#{section}")).collect::<Vec<_>>(),"sectionRefsTruncated":sections.len()>24}),
        )
    }
}
