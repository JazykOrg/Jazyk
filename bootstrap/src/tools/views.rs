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
                    if let Some(v) = self.staged_views.get_mut(&id) {
                        if !members.is_empty() {
                            v.members = members.clone();
                        }
                        if !collapse.is_empty() {
                            v.collapse = collapse.clone();
                        }
                    }
                    self.stage(Op::UpdateView {
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
                    return Ok(json!({"id": id, "created": false}));
                }
                let id = self.snapshot.mint_view_id(&kind, &title, &self.taken_ids);
                self.taken_ids.insert(id.clone());
                let view = View {
                    kind,
                    title,
                    members,
                    excluded,
                    query,
                    collapse,
                    provenance: Some(Provenance::Derived { from, reasoning }),
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
                let title = Self::opt_str(args, "title");
                // Empty means absent: [] (or [""]) is a filled-in blank, not
                // "replace the membership with nothing".
                let members = if Self::present(&args["members"]) {
                    Some(self.canon_members(&Self::str_list(args, "members"), "member")?)
                } else {
                    None
                };
                let add_members =
                    self.canon_members(&Self::str_list(args, "add_members"), "member")?;
                let remove_members =
                    self.canon_members(&Self::str_list(args, "remove_members"), "member")?;
                let collapse = if Self::present(&args["collapse"]) {
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
                let reasoning = Self::opt_str(args, "reasoning");
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
                if let Some(v) = self.staged_views.get_mut(&id) {
                    v.members = result;
                    v.collapse = coll;
                    if let Some(t) = &title {
                        v.title = t.clone();
                    }
                }
                self.stage(Op::UpdateView {
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
                Ok(json!({"id": id, "updated": true}))
            }
            _ => {
                let raw = Self::str_arg(args, "id")?;
                let reason = Self::str_arg(args, "reason")?;
                let Some((id, current)) = self.view_known(&raw) else {
                    return Err(ToolError::new(
                        "unknown-id",
                        format!("unknown view id `{}`", raw),
                    ));
                };
                if current.default {
                    return Err(ToolError::new(
                        "default-view",
                        format!(
                            "{} is a default view; the next commit would derive it again. Exclude its members or collapse them (update_view), which makes it curated, then delete",
                            id
                        ),
                    ));
                }
                self.staged_views.remove(&id);
                self.stage(Op::DeleteView { id, reason })?;
                Ok(json!({"deleted": true}))
            }
        }
    }
}
