use super::*;

impl ToolSession {
    // An entity or requirement id, snapshot or staged, canonicalized.
    pub(super) fn node_known(&self, raw: &str) -> Option<String> {
        if let Some(id) = self.canon_entity_id(raw) {
            return Some(id);
        }
        self.canon_req_id(raw).ok()
    }

    // A view, snapshot or staged, by id.
    pub(super) fn view_known(&self, raw: &str) -> Option<(String, View)> {
        let id = raw.trim();
        if let Some(v) = self.staged_views.get(id) {
            return Some((id.to_string(), v.clone()));
        }
        self.snapshot
            .graph
            .views
            .get(id)
            .map(|v| (id.to_string(), v.clone()))
    }

    // The staged view a natural key lands on: kind plus normalized title.
    fn staged_view_by_key(&self, kind: &str, title: &str) -> Option<String> {
        let norm = |s: &str| {
            s.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        let want = norm(title);
        self.staged_views
            .iter()
            .find(|(_, v)| v.kind == kind && norm(&v.title) == want)
            .map(|(id, _)| id.clone())
    }

    // Canonicalize a list of member ids, each an existing entity or requirement.
    pub(super) fn canon_members(
        &self,
        raw: &[String],
        what: &str,
    ) -> Result<Vec<String>, ToolError> {
        let mut out = Vec::new();
        for r in raw {
            let Some(id) = self.node_known(r) else {
                return Err(ToolError::new(
                    "unknown-id",
                    format!(
                        "{} `{}` does not exist; every view member is an existing entity or requirement id",
                        what, r
                    ),
                ));
            };
            out.push(id);
        }
        Ok(out)
    }

    fn is_entity_id(&self, id: &str) -> bool {
        self.known_entity(id)
    }

    // Gate a view's membership: unique ordered members that follow the kind's rule,
    // and collapse ids that are members or ancestors of members.
    // Mirrors docs/compiler/graph.md#validation-gates.
    pub(super) fn check_view_members(
        &self,
        kind: &str,
        members: &[String],
        collapse: &[String],
    ) -> Result<(), ToolError> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for m in members {
            if !seen.insert(m.as_str()) {
                return Err(ToolError::new(
                    "duplicate-member",
                    format!(
                        "member {} is listed twice; members are unique and ordered",
                        m
                    ),
                ));
            }
        }
        let entities = members.iter().filter(|m| self.is_entity_id(m)).count();
        let requirements = members.len() - entities;
        let rule = member_rule(kind);
        let ok = match rule {
            "entities" => requirements == 0,
            "one entity" => members.is_empty() || (entities == 1 && requirements == 0),
            "one entity then requirements" => {
                members.is_empty() || (entities == 1 && self.is_entity_id(&members[0]))
            }
            _ => entities == 0,
        };
        if !ok {
            return Err(ToolError::new(
                "bad-member",
                format!(
                    "a {} view's members are {}; got {} entities and {} requirements",
                    kind, rule, entities, requirements
                ),
            ));
        }
        for c in collapse {
            let covers = members.iter().any(|m| {
                if m == c {
                    return true;
                }
                let mut cur = m.clone();
                for _ in 0..64 {
                    match self.parent_of(&cur) {
                        Some(p) if &p == c => return true,
                        Some(p) => cur = p,
                        None => return false,
                    }
                }
                false
            });
            if !covers {
                return Err(ToolError::new(
                    "bad-collapse",
                    format!(
                        "collapse names {}, which is neither a member nor an ancestor of one",
                        c
                    ),
                ));
            }
        }
        Ok(())
    }

    // Parse a view query argument. Mirrors docs/compiler/model/view.md#fields.
    // An all-empty query object is a filled-in blank, not a match-everything rule:
    // empty means absent (docs/compiler/tools.md#validation-and-errors), so a query
    // naming no scope, parent, or stereotype parses as no query at all.
    fn parse_query(&self, v: &Value) -> Result<Option<ViewQuery>, ToolError> {
        if !v.is_object() {
            return Err(ToolError::new(
                "bad-args",
                "query is {scope?, parent?, stereotype?, depth?}".into(),
            ));
        }
        let parent = match Self::opt_str(v, "parent") {
            Some(p) => Some(self.canon_entity_id(&p).ok_or_else(|| {
                let e = self.unknown_entity_error(&p);
                ToolError::new("unknown-id", format!("query.parent: {}", e.message))
            })?),
            None => None,
        };
        let q = ViewQuery {
            scope: Self::opt_str(v, "scope"),
            parent,
            stereotype: Self::opt_str(v, "stereotype"),
            depth: v["depth"]
                .as_u64()
                .map(|d| u32::try_from(d).unwrap_or(u32::MAX)),
        };
        if q.scope.is_none() && q.parent.is_none() && q.stereotype.is_none() {
            return Ok(None);
        }
        Ok(Some(q))
    }

    fn parse_exclusions(&self, v: &Value) -> Result<Vec<Exclusion>, ToolError> {
        let items: Vec<&Value> = match v {
            Value::Array(a) => a.iter().collect(),
            Value::Object(_) => vec![v],
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        for x in items {
            // Empty means absent (docs/compiler/tools.md#validation-and-errors): a
            // hollow exclusion, item or singleton, is a filled-in blank.
            if !Self::present(x) {
                continue;
            }
            let raw = x["id"].as_str().unwrap_or_default().trim().to_string();
            let Some(id) = self.node_known(&raw) else {
                return Err(ToolError::new(
                    "unknown-id",
                    format!("excluded `{}` does not exist", raw),
                ));
            };
            let note = x["note"].as_str().unwrap_or_default().trim().to_string();
            if note.is_empty() {
                return Err(ToolError::new(
                    "bad-args",
                    format!("excluding {} needs a note saying why it stays out", id),
                ));
            }
            out.push(Exclusion { id, note });
        }
        Ok(out)
    }
}

// The view tools, the derived requirement path, and the chat edit path.
impl ToolSession {
    // upsert_view, update_view, delete_view. Mirrors docs/compiler/tools.md#view-tools.
    pub(super) fn view_tool(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        match name {
            "upsert_view" => {
                let kind = Self::str_arg(args, "kind")?;
                if !VIEW_KINDS.contains(&kind.as_str()) {
                    return Err(ToolError::new(
                        "bad-kind",
                        format!(
                            "unknown view kind `{}`; one of: {}",
                            kind,
                            VIEW_KINDS.join(", ")
                        ),
                    ));
                }
                let title = Self::str_arg(args, "title")?;
                let reasoning = Self::str_arg(args, "reasoning")?;
                let human = self.diagram_human_decision(args)?;
                let mut authored = Self::diagram_fields(args);
                authored
                    .entry("purpose".into())
                    .or_insert_with(|| json!(reasoning));
                crate::diagrams::validate_fields(&self.diagram_snapshot(), &kind, &authored)
                    .map_err(|message| ToolError::new("diagram-fields", message))?;
                let members = self.canon_members(&Self::str_list(args, "members"), "member")?;
                let collapse = self.canon_members(&Self::str_list(args, "collapse"), "collapse")?;
                let excluded = self.parse_exclusions(&args["excluded"])?;
                let query = if Self::present(&args["query"]) {
                    self.parse_query(&args["query"])?
                } else {
                    None
                };
                // A query matches entities; a flow view's members are requirements,
                // so a query on one would flood it with every entity in the graph.
                if query.is_some() && crate::derive::FLOW_KINDS.contains(&kind.as_str()) {
                    return Err(ToolError::new(
                        "bad-args",
                        format!(
                            include_str!(concat!(
                                env!("OUT_DIR"),
                                "/payloads/goals/prompts/inline/tools-views/a-view-s-members-are-requirements-an-9464c.md"
                            )),
                            kind
                        ),
                    ));
                }
                self.check_view_members(&kind, &members, &collapse)?;
                crate::diagrams::validate_members(&members, &authored)
                    .map_err(|m| ToolError::new("diagram-members", m))?;
                let mut from: Vec<String> = Vec::new();
                for id in members
                    .iter()
                    .chain(collapse.iter())
                    .chain(excluded.iter().map(|x| &x.id))
                    .chain(query.iter().filter_map(|q| q.parent.as_ref()))
                {
                    if !from.contains(id) {
                        from.push(id.clone());
                    }
                }
                let existing = self
                    .snapshot
                    .find_view(&kind, &title)
                    .or_else(|| self.staged_view_by_key(&kind, &title));
                if let Some(id) = existing {
                    if let Some(current) = self.snapshot.graph.views.get(&id) {
                        if let (Some(old), Some(new)) = (
                            crate::diagrams::area(current),
                            authored
                                .get("area")
                                .and_then(|a| a.get("target"))
                                .and_then(Value::as_str),
                        ) {
                            if old != new {
                                return Err(ToolError::new(
                                    "diagram-area-conflict",
                                    "This title already identifies a diagram in another area"
                                        .into(),
                                ));
                            }
                        }
                    }

                    if let Some(v) = self.staged_views.get_mut(&id) {
                        v.authored.extend(authored.clone());
                        if let Some(query) = &query {
                            v.query = Some(query.clone());
                        }
                        v.excluded.extend(excluded.clone());
                        if !members.is_empty() {
                            v.members = members.clone();
                        }
                        if !collapse.is_empty() {
                            v.collapse = collapse.clone();
                        }
                    }
                    self.stage(Op::UpdateView {
                        clear_query: false,
                        authored: authored.clone(),
                        id: id.clone(),
                        title: None,
                        members: (!members.is_empty()).then(|| members.clone()),
                        add_members: Vec::new(),
                        remove_members: Vec::new(),
                        query,
                        collapse: (!collapse.is_empty()).then(|| collapse.clone()),
                        exclude: excluded,
                        reasoning: Some(reasoning),
                    })?;
                    if let Some(section) = args["section"].as_str() {
                        let (doc, section) = self.resolve_section(section)?;
                        let quote =
                            self.check_quote(&doc, &section, &Self::str_arg(args, "quote")?)?;
                        self.stage(Op::RatifyProvenance {
                            id: id.clone(),
                            source: SourceRef {
                                doc,
                                section,
                                quote,
                            },
                        })?;
                    }
                    if let Some(provenance) = human {
                        self.stage(Op::SetViewProvenance {
                            id: id.clone(),
                            provenance,
                        })?;
                    }
                    return Ok(json!({"id": id, "created": false}));
                }
                if !authored.contains_key("area") {
                    let target = query
                        .as_ref()
                        .and_then(|q| q.parent.clone())
                        .or_else(|| {
                            members
                                .first()
                                .and_then(|id| self.snapshot.graph.entities.get(id))
                                .map(|e| {
                                    e.parent.clone().unwrap_or_else(|| {
                                        crate::store::scope_root_target(&e.scope)
                                    })
                                })
                        })
                        .unwrap_or_else(|| "scope:public".into());
                    authored.insert("area".into(), json!({"target":target,"extent":"selection"}));
                }
                let provenance = if let Some(section) = args["section"].as_str() {
                    let (doc, sec) = self.resolve_section(section)?;
                    let quote = self.check_quote(&doc, &sec, &Self::str_arg(args, "quote")?)?;
                    Provenance::Quote(SourceRef {
                        doc,
                        section: sec,
                        quote,
                    })
                } else if let Some(provenance) = human {
                    provenance
                } else {
                    Provenance::Derived { from, reasoning }
                };
                let id = self.snapshot.mint_view_id(&kind, &title, &self.taken_ids);
                self.taken_ids.insert(id.clone());
                let view = View {
                    kind,
                    title,
                    members,
                    excluded,
                    query,
                    collapse,
                    authored,
                    provenance: Some(provenance),
                    default: false,
                    ..Default::default()
                };
                self.staged_views.insert(id.clone(), view.clone());
                self.stage(Op::CreateView {
                    id: id.clone(),
                    view,
                })?;
                Ok(json!({"id": id, "created": true}))
            }
            "update_view" => {
                let human = self.diagram_human_decision(args)?;
                let raw = Self::str_arg(args, "id")?;
                let Some((id, current)) = self.view_known(&raw) else {
                    return Err(ToolError::new(
                        "unknown-id",
                        format!(
                            "unknown view id `{}`; search with kind view, or create it with upsert_view",
                            raw
                        ),
                    ));
                };
                if ["purpose", "area"]
                    .iter()
                    .any(|key| args.get(*key).is_some_and(Value::is_null))
                {
                    return Err(ToolError::new(
                        "diagram-fields",
                        "Purpose and area cannot be cleared".into(),
                    ));
                }
                let authored = Self::diagram_fields(args);
                let mut merged = current.authored.clone();
                for (key, value) in &authored {
                    if value.is_null() {
                        merged.remove(key);
                    } else {
                        merged.insert(key.clone(), value.clone());
                    }
                }
                crate::diagrams::validate_fields(&self.diagram_snapshot(), &current.kind, &merged)
                    .map_err(|message| ToolError::new("diagram-fields", message))?;
                let title = Self::opt_str(args, "title");
                // Empty means absent: [] (or [""]) is a filled-in blank, not
                // "replace the membership with nothing".
                let members = if args.get("members").is_some() {
                    Some(self.canon_members(&Self::str_list(args, "members"), "member")?)
                } else {
                    None
                };
                let add_members =
                    self.canon_members(&Self::str_list(args, "add_members"), "member")?;
                let remove_members =
                    self.canon_members(&Self::str_list(args, "remove_members"), "member")?;
                let collapse = if args.get("collapse").is_some() {
                    Some(self.canon_members(&Self::str_list(args, "collapse"), "collapse")?)
                } else {
                    None
                };
                let exclude = self.parse_exclusions(&args["exclude"])?;
                let query = if Self::present(&args["query"]) {
                    self.parse_query(&args["query"])?
                } else {
                    None
                };
                let reasoning = Some(Self::str_arg(args, "reasoning")?);
                if args.get("members").is_some()
                    && (args.get("add_members").is_some() || args.get("remove_members").is_some())
                {
                    return Err(ToolError::new(
                        "ambiguous-members",
                        "Cannot combine replacement and membership deltas".into(),
                    ));
                }
                // A query matches entities; a flow view's members are requirements,
                // so a query on one would flood it with every entity in the graph.
                if query.is_some() && crate::derive::FLOW_KINDS.contains(&current.kind.as_str()) {
                    return Err(ToolError::new(
                        "bad-args",
                        format!(
                            include_str!(concat!(
                                env!("OUT_DIR"),
                                "/payloads/goals/prompts/inline/tools-views/a-view-s-members-are-requirements-an-9464c.md"
                            )),
                            current.kind
                        ),
                    ));
                }
                // The membership the call leaves behind passes the kind's rule.
                let mut result = members.clone().unwrap_or_else(|| current.members.clone());
                for m in &add_members {
                    if !result.contains(m) {
                        result.push(m.clone());
                    }
                }
                result
                    .retain(|m| !remove_members.contains(m) && !exclude.iter().any(|x| &x.id == m));
                let coll = collapse.clone().unwrap_or_else(|| current.collapse.clone());
                self.check_view_members(&current.kind, &result, &coll)?;
                crate::diagrams::validate_members(&result, &merged)
                    .map_err(|m| ToolError::new("diagram-members", m))?;
                self.staged_views
                    .entry(id.clone())
                    .or_insert(current.clone());
                if let Some(v) = self.staged_views.get_mut(&id) {
                    v.authored = merged;
                    v.members = result;
                    v.collapse = coll;
                    if args.get("query").is_some_and(Value::is_null) {
                        v.query = None;
                    } else if let Some(query) = &query {
                        v.query = Some(query.clone());
                    }
                    for exclusion in &exclude {
                        v.excluded.retain(|x| x.id != exclusion.id);
                        v.excluded.push(exclusion.clone());
                    }
                    if let Some(t) = &title {
                        v.title = t.clone();
                    }
                }
                self.stage(Op::UpdateView {
                    clear_query: args.get("query").is_some_and(Value::is_null),
                    authored,
                    id: id.clone(),
                    title,
                    members,
                    add_members,
                    remove_members,
                    query,
                    collapse,
                    exclude,
                    reasoning,
                })?;
                if let Some(provenance) = human {
                    self.stage(Op::SetViewProvenance {
                        id: id.clone(),
                        provenance,
                    })?;
                }
                Ok(json!({"id": id, "updated": true}))
            }
            _ => {
                let raw = Self::str_arg(args, "id")?;
                let reason = Self::str_arg(args, "reason")?;
                let Some((id, _current)) = self.view_known(&raw) else {
                    return Err(ToolError::new(
                        "unknown-id",
                        format!("unknown view id `{}`", raw),
                    ));
                };
                self.staged_views.remove(&id);
                self.stage(Op::DeleteView { id, reason })?;
                Ok(json!({"deleted": true}))
            }
        }
    }
}

impl ToolSession {
    fn diagram_human_decision(&self, args: &Value) -> Result<Option<Provenance>, ToolError> {
        let Some(decision) = Self::opt_str(args, "decision") else {
            return Ok(None);
        };
        if self.scope.serving != "chat" {
            return Err(ToolError::new(
                "human-only",
                "A human decision is available only in writable chat".into(),
            ));
        }
        if args.get("section").is_some() {
            return Err(ToolError::new(
                "provenance-required",
                "A diagram has one provenance source".into(),
            ));
        }
        Ok(Some(self.decree(Some(decision))))
    }
    fn diagram_snapshot(&self) -> Store {
        let mut available = self.snapshot.clone();
        available
            .graph
            .entities
            .extend(self.staged_entities.clone());
        available.graph.views.extend(self.staged_views.clone());
        for op in &self.staged {
            if let Op::CreateRequirement { id, requirement } = op {
                available
                    .graph
                    .requirements
                    .insert(id.clone(), requirement.clone());
            }
        }
        crate::derive::recompute_relationships(&mut available);
        available
    }
    fn diagram_fields(args: &Value) -> BTreeMap<String, Value> {
        crate::diagrams::FIELDS
            .iter()
            .filter_map(|k| args.get(*k).map(|v| (k.to_string(), v.clone())))
            .collect()
    }
    pub(super) fn diagram_decision_tool(
        &mut self,
        name: &str,
        args: &Value,
    ) -> Result<Value, ToolError> {
        let id = Self::str_arg(args, "id")?;
        let reason = Self::str_arg(args, "reason")?;
        if ["none", "n/a", "-", "skip", "not needed"].contains(&reason.to_lowercase().as_str()) {
            return Err(ToolError::new(
                "diagram-reason",
                "Suppression reason is a placeholder".into(),
            ));
        }
        if !self.scope.goals.is_empty()
            && !self
                .scope
                .goals
                .iter()
                .any(|g| g.kind == "diagram-coverage" && g.target == id)
        {
            return Err(ToolError::new(
                "wrong-area",
                "Suppression is outside this diagram-review batch".into(),
            ));
        }
        if name == "unsuppress_diagram_coverage" {
            self.stage(Op::SetDiagramDecision {
                id: id.clone(),
                decision: None,
            })?;
            return Ok(json!({"id":id,"suppressed":false}));
        }
        let finding = crate::diagrams::findings(&self.snapshot)
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| ToolError::new("unknown-finding", id.clone()))?;
        if finding.family == "source" {
            return Err(ToolError::new(
                "explicit-diagram",
                "A source diagram obligation cannot be suppressed".into(),
            ));
        }
        if Self::str_arg(args, "expectedAreaHash")? != finding.hash() {
            return Err(ToolError::new("area-moved", "Diagram area changed".into()));
        }
        let human = self.diagram_human_decision(args)?;
        let mut decision = finding.value();
        decision["reason"] = json!(reason);
        decision["ruleVersion"] = json!(1);
        decision["provenance"] = human.map(|p|serde_json::to_value(p).unwrap_or_default()).unwrap_or_else(||json!({"derived":{"from":finding.candidates.keys().collect::<Vec<_>>(),"reasoning":reason}}));
        self.stage(Op::SetDiagramDecision {
            id: id.clone(),
            decision: Some(decision),
        })?;
        Ok(json!({"id":id,"suppressed":true}))
    }
}
