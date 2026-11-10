use super::*;

impl ToolSession {
    // The author a decree records: the MCP client when one is known, else the user.
    fn decree_author(&self) -> String {
        self.caller
            .client
            .clone()
            .filter(|c| !c.trim().is_empty())
            .or_else(|| crate::sys::env_var("USER").ok().filter(|u| !u.is_empty()))
            .unwrap_or_else(|| "human".to_string())
    }

    pub(super) fn decree(&self, note: Option<String>) -> Provenance {
        Provenance::Decree {
            author: self.decree_author(),
            at: crate::verify::now_iso(),
            note,
        }
    }

    // The parent of an entity as this session sees it: staged first, then the snapshot.
    // The parent an entity holds once this session's staged work applies: a staged
    // move wins, a staged create carries its own, and a child of a staged dissolve
    // lands on the dissolved grouping's parent (recursively, when that parent
    // dissolves in the same changeset). Mirrors docs/compiler/tools.md#grouping-tools.
    pub(super) fn parent_of(&self, id: &str) -> Option<String> {
        if let Some(p) = self.staged_parents.get(id) {
            return Some(p.clone());
        }
        if let Some(e) = self.staged_entities.get(id) {
            return e.parent.clone();
        }
        let stored = self
            .snapshot
            .graph
            .entities
            .get(self.snapshot.resolve_id(id))
            .and_then(|e| e.parent.clone())?;
        if self.staged_dissolved(&stored) {
            return self.parent_of(&stored);
        }
        Some(stored)
    }

    // Whether this session staged a dissolve of the entity.
    fn staged_dissolved(&self, id: &str) -> bool {
        self.staged
            .iter()
            .any(|o| matches!(o, Op::DissolveEntity { id: d, .. } if d == id))
    }

    // Every entity id alive once the staged work applies: the snapshot's minus the
    // ones this session deletes, absorbs, or dissolves, plus this session's creates.
    fn entities_after(&self) -> Vec<String> {
        let gone: BTreeSet<&str> = self
            .staged
            .iter()
            .filter_map(|o| match o {
                Op::DeleteEntity { id, .. }
                | Op::MergeEntities { absorb: id, .. }
                | Op::DissolveEntity { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        self.snapshot
            .graph
            .entities
            .keys()
            .filter(|id| !gone.contains(id.as_str()))
            .chain(self.staged_entities.keys())
            .cloned()
            .collect()
    }

    // An entity's scope, snapshot or staged.
    fn scope_of(&self, id: &str) -> Option<String> {
        self.staged_entities
            .get(id)
            .or_else(|| {
                self.snapshot
                    .graph
                    .entities
                    .get(self.snapshot.resolve_id(id))
            })
            .map(|e| e.scope.clone())
    }

    // The level an entity sits in, as a target: its parent's id, or `scope:<scope>`
    // for a parentless entity. Mirrors docs/compiler/concepts/levels.md#the-scope-root.
    fn level_target_of(&self, id: &str) -> String {
        match self.parent_of(id) {
            Some(p) => p,
            None => crate::store::scope_root_target(
                &self.scope_of(id).unwrap_or_else(|| "public".to_string()),
            ),
        }
    }

    // The direct children of a level once the staged work applies, in id order: the
    // entities whose parent is the node, or the parentless entities of the scope for
    // `scope:<scope>`. Mirrors docs/compiler/concepts/levels.md#levels.
    pub(super) fn level_after(&self, target: &str) -> Vec<String> {
        let mut ids = self.entities_after();
        ids.sort();
        ids.retain(|id| self.level_target_of(id) == target);
        ids
    }

    // The level view of one entity: `view:component/<slug>` or `view:class/<slug>`
    // when the snapshot or this session's staged views hold it, or the id the kind
    // rule mints at commit when the entity holds two or more children after the
    // staged work (component when the node or any child carries a structural
    // stereotype, class otherwise). Mirrors docs/compiler/model/view.md#level-views.
    pub(super) fn level_view_of(&self, id: &str) -> Option<String> {
        let slug = crate::ids::entity_slug(id);
        for kind in ["component", "class"] {
            let vid = format!("view:{}/{}", kind, slug);
            if self.view_known(&vid).is_some() {
                return Some(vid);
            }
        }
        let children = self.level_after(id);
        // One child with attributes holds a level too (view.md#level-views).
        let lone_with_attributes = children.len() == 1
            && self
                .staged_entities
                .get(&children[0])
                .or_else(|| self.snapshot.graph.entities.get(&children[0]))
                .is_some_and(|e| !e.attributes.is_empty());
        if children.len() < 2 && !lone_with_attributes {
            return None;
        }
        let structural = |x: &str| {
            self.staged_entities
                .get(x)
                .or_else(|| self.snapshot.graph.entities.get(x))
                .and_then(|e| e.stereotype.as_deref())
                .is_some_and(|s| {
                    matches!(
                        s.trim().to_lowercase().as_str(),
                        "system" | "component" | "service" | "interface" | "actor"
                    )
                })
        };
        let kind = if structural(id) || children.iter().any(|c| structural(c)) {
            "component"
        } else {
            "class"
        };
        Some(format!("view:{}/{}", kind, slug))
    }

    // The entities a view draws: a structural view's entity members, a flow view's
    // participants (its member requirements' entities), in member order, deduped.
    fn drawn_entities(&self, v: &View) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for m in &v.members {
            let rid = self.snapshot.resolve_id(m).to_string();
            if self.snapshot.graph.entities.contains_key(&rid) {
                if !out.contains(&rid) {
                    out.push(rid);
                }
            } else if let Some(r) = self.snapshot.graph.requirements.get(&rid) {
                for e in &r.entities {
                    let e = self.snapshot.resolve_id(e).to_string();
                    if !out.contains(&e) {
                        out.push(e);
                    }
                }
            }
        }
        out
    }

    // The drill-down links of a view: for every entity it draws that has a level
    // view, `{member, view}`. Computed at call time from the containment tree and the
    // level views, never stored. Mirrors docs/compiler/model/view.md#fields.
    pub(super) fn view_children(&self, v: &View) -> Vec<Value> {
        self.drawn_entities(v)
            .into_iter()
            .filter_map(|m| {
                self.level_view_of(&m)
                    .map(|view| json!({"member": m, "view": view}))
            })
            .collect()
    }

    // The fan-out goals a grouping or a dissolve will open at commit: every level the
    // staged ops touch (the parent a grouping joins, the parents a move leaves and
    // joins, the parent a dissolve's children land on, the grouping's own level) is
    // counted after the changeset against `children-per-entity`, and a level past
    // the soft or hard threshold previews `abstract-entity` on its target.
    // Mirrors docs/compiler/reconciler.md#bubbling and #fan-out.
    pub(super) fn level_opens(&self, ops: &[Op]) -> Vec<String> {
        use crate::limits::{threshold, CHILDREN_PER_ENTITY};
        let mut targets: Vec<String> = Vec::new();
        let mut touch = |t: String| {
            if !targets.contains(&t) {
                targets.push(t);
            }
        };
        for op in ops {
            match op {
                Op::CreateEntity { id, entity } => {
                    touch(match &entity.parent {
                        Some(p) => p.clone(),
                        None => crate::store::scope_root_target(&entity.scope),
                    });
                    touch(id.clone());
                }
                Op::UpdateEntity {
                    id,
                    parent: Some(p),
                    ..
                } => {
                    touch(p.clone());
                    if let Some(prior) = self
                        .snapshot
                        .graph
                        .entities
                        .get(id)
                        .and_then(|e| e.parent.clone())
                    {
                        touch(prior);
                    }
                }
                Op::DissolveEntity { id, .. } => {
                    if let Some(e) = self.snapshot.graph.entities.get(id) {
                        touch(match &e.parent {
                            Some(p) => p.clone(),
                            None => crate::store::scope_root_target(&e.scope),
                        });
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for t in targets {
            if self.staged_dissolved(&t) {
                continue;
            }
            let count = self.level_after(&t).len() as u64;
            let bump = self
                .snapshot
                .graph
                .entities
                .get(&t)
                .and_then(|e| e.limits.get(CHILDREN_PER_ENTITY))
                .map(|b| b.value);
            let Some((soft, hard)) = threshold(CHILDREN_PER_ENTITY, bump) else {
                continue;
            };
            if count > hard {
                out.push(format!(
                    "abstract-entity {} (fan-out {} over hard {}, mandatory)",
                    t, count, hard
                ));
            } else if count > soft {
                out.push(format!(
                    "abstract-entity {} (fan-out {} over soft {}, optional)",
                    t, count, soft
                ));
            }
        }
        out
    }

    // Gate a `parent` argument: it must resolve, and the containment tree stays
    // acyclic (an entity is never its own ancestor). `child` is the entity being
    // parented, absent for a create. Mirrors docs/compiler/graph.md#validation-gates.
    pub(super) fn check_parent(&self, child: Option<&str>, raw: &str) -> Result<String, ToolError> {
        let Some(p) = self.canon_entity_id(raw) else {
            let e = self.unknown_entity_error(raw);
            return Err(ToolError::new(
                "unknown-parent",
                format!("parent {}", e.message),
            ));
        };
        if let Some(c) = child {
            let mut chain = vec![p.clone()];
            let mut cur = p.clone();
            for _ in 0..64 {
                if cur == c {
                    return Err(ToolError::new(
                        "parent-cycle",
                        format!(
                            "{} cannot be under {}: the chain {} leads back to it; an entity is never its own ancestor",
                            c,
                            p,
                            chain.join(" > ")
                        ),
                    ));
                }
                match self.parent_of(&cur) {
                    Some(next) => {
                        chain.push(next.clone());
                        cur = next;
                    }
                    None => break,
                }
            }
        }
        Ok(p)
    }

    // An existing or staged entity whose name tokens contain, or are contained by, the
    // candidate's tokens (same scope). "backend" vs "backend system" is one concept;
    // single generic tokens are exempt to keep "id" from matching "user id".
    // The name-variant rule. Mirrors docs/compiler/graph.md#validation-gates.
    // `compound_passes`: a compound that extends an existing single name is a different
    // concept for an entity (`Employee Handbook` beside `Employee`); for a grouping it
    // is a lookalike of the area, which reuses it.
    pub(super) fn near_name(
        &self,
        name: &str,
        scope: &str,
        compound_passes: bool,
    ) -> Option<(String, String)> {
        let raw = |s: &str| -> BTreeSet<String> {
            s.to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|t| !t.is_empty())
                .map(String::from)
                .collect()
        };
        // A plural folds to its singular so `Departments` meets `Department`.
        let fold = |t: &str| -> String {
            if t.len() > 4 && t.ends_with("ies") {
                format!("{}y", &t[..t.len() - 3])
            } else if t.len() > 3 && t.ends_with('s') && !t.ends_with("ss") {
                t[..t.len() - 1].to_string()
            } else {
                t.to_string()
            }
        };
        // The plural folds for a grouping only: an area named `Orders` and the
        // concept `Order` are two things, a grouping `Departments` beside a stated
        // `Department` is a twin.
        let tokens = |s: &str| -> BTreeSet<String> {
            if compound_passes {
                raw(s)
            } else {
                raw(s).iter().map(|t| fold(t)).collect()
            }
        };
        let cand_raw = raw(name);
        let cand = tokens(name);
        if cand.is_empty() {
            return None;
        }
        // For an entity, a variant differs by generic classifiers alone; a concrete
        // noun added ("Employee Handbook" beside "Employee") is another concept. A
        // grouping keeps the whole containment rule: a lookalike of an existing area
        // reuses it (docs/compiler/concepts/levels.md#naming).
        // Mirrors docs/compiler/model/entity.md#what-is-an-entity.
        const GENERIC: [&str; 16] = [
            "system",
            "service",
            "module",
            "component",
            "department",
            "team",
            "tool",
            "layer",
            "platform",
            "app",
            "application",
            "api",
            "process",
            "engine",
            "subsystem",
            "hub",
        ];
        let check = |ename: &str| -> bool {
            if raw(ename) == cand_raw {
                return false; // exact natural-key match is the upsert path, not a twin
            }
            let ex = tokens(ename);
            if ex == cand {
                return true; // the same tokens up to a plural
            }
            let (small, big) = if ex.len() <= cand.len() {
                (&ex, &cand)
            } else {
                (&cand, &ex)
            };
            if !small.is_subset(big) {
                return false;
            }
            let generic_only = !compound_passes
                || big
                    .difference(small)
                    .all(|t| GENERIC.contains(&fold(t).as_str()));
            if !generic_only {
                return false;
            }
            if small.len() > 1 {
                return true; // a multi-token name contained in the other
            }
            // One token inside a longer name: a variant only when the candidate is
            // the shorter one (`Billing` beside `Billing System`); a compound that
            // extends an existing single name is a different concept.
            (cand.len() == 1 || !compound_passes)
                && small.iter().next().map(|t| t.len() >= 5).unwrap_or(false)
        };
        for (id, e) in &self.snapshot.graph.entities {
            if e.scope == scope && check(&e.name) {
                return Some((id.clone(), e.name.clone()));
            }
        }
        for (id, e) in &self.staged_entities {
            if e.scope == scope && check(&e.name) {
                return Some((id.clone(), e.name.clone()));
            }
        }
        None
    }

    pub(super) fn known_entity(&self, id: &str) -> bool {
        let rid = self.snapshot.resolve_id(id);
        self.snapshot.graph.entities.contains_key(rid) || self.staged_entities.contains_key(id)
    }

    // Lenient reference resolution. Models, small ones especially, drop the `ent:`
    // prefix or pass the display name; when exactly one node matches, the intent is
    // unambiguous, so resolve it instead of bouncing the call. Mirrors
    // docs/compiler/graph.md#validation-gates.
    pub(super) fn canon_entity_id(&self, raw: &str) -> Option<String> {
        if self.known_entity(raw) {
            return Some(self.snapshot.resolve_id(raw).to_string());
        }
        let raw = raw.trim();
        if !raw.starts_with("ent:") {
            let prefixed = format!("ent:{}", raw);
            if self.known_entity(&prefixed) {
                return Some(self.snapshot.resolve_id(&prefixed).to_string());
            }
            let slug = format!(
                "ent:{}",
                raw.to_lowercase()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join("-")
            );
            if self.known_entity(&slug) {
                return Some(self.snapshot.resolve_id(&slug).to_string());
            }
        } else if let Some(rest) = raw.strip_prefix("ent:") {
            // A case or spacing variant of an existing id (`ent:factHash`) resolves to it.
            let slug = format!(
                "ent:{}",
                rest.to_lowercase()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join("-")
            );
            if self.known_entity(&slug) {
                return Some(self.snapshot.resolve_id(&slug).to_string());
            }
        }
        // Exact display name or alias, snapshot plus staged; unique match only.
        let want = raw
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if want.is_empty() {
            return None;
        }
        let norm = |n: &str| {
            n.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        let mut hits: Vec<String> = Vec::new();
        let all = self
            .snapshot
            .graph
            .entities
            .iter()
            .map(|(i, e)| (i.clone(), e))
            .chain(self.staged_entities.iter().map(|(i, e)| (i.clone(), e)));
        for (id, e) in all {
            if (norm(&e.name) == want || e.aliases.iter().any(|a| norm(a) == want))
                && !hits.contains(&id)
            {
                hits.push(id);
            }
        }
        if hits.len() == 1 {
            return Some(hits.remove(0));
        }
        None
    }

    // Requirement-id counterpart of canon_entity_id: forgive a missing `req:` prefix.
    pub(super) fn canon_req_id(&self, raw: &str) -> Result<String, ToolError> {
        let known = |id: &str| {
            self.snapshot.graph.requirements.contains_key(id) || self.staged_reqs.contains(id)
        };
        if known(raw) {
            return Ok(raw.to_string());
        }
        if !raw.starts_with("req:") {
            let prefixed = format!("req:{}", raw.trim());
            if known(&prefixed) {
                return Ok(prefixed);
            }
        }
        Err(ToolError::new(
            "unknown-id",
            format!("unknown requirement id `{}`", raw),
        ))
    }

    pub(super) fn unknown_entity_error(&self, id: &str) -> ToolError {
        // A non-entity node id is a wrong-tool call, not a missing entity: say which
        // tool reads that kind instead of suggesting a create.
        for (prefix, kind) in [
            ("req:", "a requirement"),
            ("view:", "a view; get_view also reads it"),
            ("diag:", "a diagnostic; diagnostics lists it"),
            ("sm:", "a state machine"),
        ] {
            if id.starts_with(prefix) {
                return ToolError::new(
                    "unknown-id",
                    format!(
                        "`{}` is {}, not an entity; call load with target `{}`",
                        id, kind, id
                    ),
                );
            }
        }
        let bare = id.strip_prefix("ent:").unwrap_or(id).replace('-', " ");
        let hits = self.search_all(&bare);
        let hint = if hits.is_empty() {
            "search for it, or create it with upsert_entity first".to_string()
        } else {
            format!(
                "nearest existing: {}; use one of those, or create it with upsert_entity first",
                hits.iter()
                    .take(3)
                    .map(|(id, _, _)| id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        ToolError::new(
            "unknown-id",
            format!("unknown entity id `{}`; {}", id, hint),
        )
    }

    pub(super) fn upsert_entity_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let name_arg = Self::str_arg(args, "name")?;
        let scope = Self::opt_str(args, "scope").unwrap_or_else(|| "public".to_string());
        let note = Self::opt_str(args, "note");
        if let Some(why) = junk_name(&name_arg) {
            if note.is_none() && !self.proper_name_in_docs(&name_arg) {
                return Err(ToolError::new(
                    "junk-name",
                    format!(
                        "`{}` {}; entities are domain concepts. If it truly is one, repeat the call with a `note` explaining why",
                        name_arg, why
                    ),
                ));
            }
        }
        // Near-name gate: a qualifier variant of an existing entity is almost
        // always the same concept. Reuse it and record the wording as an alias
        // instead of minting a twin; a note overrides when genuinely distinct.
        // Mirrors docs/compiler/model/entity.md#what-is-an-entity.
        // A child declared under its lookalike is a child concept, not a twin.
        let parent_is_lookalike = |eid: &str| {
            args["parent"]
                .as_str()
                .map(|p| self.snapshot.resolve_id(p) == eid || p == eid)
                .unwrap_or(false)
        };
        let lookalike = self
            .near_name(&name_arg, &scope, true)
            .filter(|(eid, _)| !parent_is_lookalike(eid));
        if note.is_none() {
            if let Some((eid, ename)) = &lookalike {
                return Err(ToolError::new(
                    "near-duplicate",
                    format!(
                        "`{}` looks like a name variant of existing `{}` ({}); if it is the same concept, reuse that id and add your wording with update_entity add_aliases. A field, part, state, or child concept of it IS a different concept: repeat the call with a `note` naming that relation",
                        name_arg, eid, ename
                    ),
                ));
            }
        }
        // The note that let the name through is the separate verdict on the
        // pair; it rides on the entity so the lookalike index never asks again.
        // Mirrors docs/compiler/model/entity.md#fields.
        let distinct_from: std::collections::BTreeMap<String, String> = match (&note, &lookalike) {
            (Some(n), Some((eid, _))) => [(eid.clone(), n.clone())].into_iter().collect(),
            _ => std::collections::BTreeMap::new(),
        };
        // Exactly one provenance: a located mention, or a derivation. Empty
        // objects count as absent (docs/compiler/tools.md#validation-and-errors).
        let mention = &args["mention"];
        let has_mention = Self::present(mention);
        let has_derived = Self::present(&args["provenance"]);
        if has_mention == has_derived {
            return Err(ToolError::new(
                if has_mention { "bad-provenance" } else { "provenance-required" },
                "an entity enters with exactly one provenance: mention {section, quote} (the sentence that talks about it), or provenance {derived: {from, reasoning}} for structure the documents do not state".into(),
            ));
        }
        let mention_ref = if has_mention {
            let section = mention["section"].as_str().unwrap_or_default();
            let quote = mention["quote"].as_str().unwrap_or_default();
            let (doc, sec) = self.resolve_section(section)?;
            if let Some(wd) = self.scope.reconcile_doc() {
                if doc != wd {
                    return Err(ToolError::new(
                        "wrong-document",
                        format!(
                            "mention cites {} but this batch reconciles {}; quote a sentence from {}'s own sections (text this document merely links to cannot anchor a mention here)",
                            doc, wd, wd
                        ),
                    ));
                }
            }
            let quote = self.check_quote(&doc, &sec, quote).map_err(|e| {
                // An entity named only by a heading enters from nothing: say so,
                // so the model stops looking for another way to mint it.
                // Mirrors docs/compiler/graph.md#validation-gates.
                if e.message.contains("headings are not quotable") {
                    ToolError::new(&e.rule, format!("{}. An entity no body sentence names is not minted from its heading: reference the entities the body names, and leave \"the system\" unreferenced when no «system» exists", e.message))
                } else {
                    e
                }
            })?;
            Some(SourceRef {
                doc,
                section: sec,
                quote,
            })
        } else {
            None
        };
        let provenance = if has_derived {
            Some(self.parse_derived(&args["provenance"])?)
        } else {
            None
        };
        let stereotype = Self::opt_str(args, "stereotype");
        let parent = match Self::opt_str(args, "parent") {
            Some(p) => Some(self.check_parent(None, &p)?),
            None => None,
        };
        let attributes = if args["attributes"].is_null() {
            Vec::new()
        } else {
            self.parse_attributes(&args["attributes"], mention_ref.as_ref())?
        };

        // Lookup before create: the natural key may already exist in the graph or in
        // this turn's own staged creates.
        let existing = match self
            .snapshot
            .find_natural(&name_arg, &scope, parent.as_deref())
        {
            Ok(found) => found,
            Err(candidates) => {
                return Err(ToolError::new(
                    "ambiguous-name",
                    format!(
                        "`{}` names several entities ({}); pass `parent` to say which one you mean, or update_entity on the id",
                        name_arg,
                        candidates.join(", ")
                    ),
                ))
            }
        }
        .or_else(|| {
            self.staged_entities
                .iter()
                .find(|(_, e)| {
                    e.scope == scope
                        && e.name.trim().to_lowercase() == name_arg.trim().to_lowercase()
                        && parent.as_ref().is_none_or(|p| e.parent.as_ref() == Some(p))
                })
                .map(|(id, _)| id.clone())
        });
        // A new entity enters with a sentence that names it: a word of its name
        // or an alias in the quote. Mirrors docs/compiler/graph.md#validation-gates.
        if existing.is_none() {
            if let Some(m) = &mention_ref {
                let words = |s: &str| -> BTreeSet<String> {
                    s.to_lowercase()
                        .split(|c: char| !c.is_alphanumeric())
                        .filter(|w| {
                            w.len() >= 3 && !["the", "and", "for", "with", "system"].contains(w)
                        })
                        .map(|w| {
                            if w.len() > 4 && w.ends_with("ies") {
                                format!("{}y", &w[..w.len() - 3])
                            } else if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") {
                                w[..w.len() - 1].to_string()
                            } else {
                                w.to_string()
                            }
                        })
                        .collect()
                };
                let mut named = words(&name_arg);
                for a in Self::str_list(args, "aliases") {
                    named.extend(words(&a));
                }
                if !named.is_empty() && named.is_disjoint(&words(&m.quote)) {
                    return Err(ToolError::new(
                        "mention-unnamed",
                        format!(
                            "the quote never names `{}`; an entity enters with a sentence that names it (a word of its name or an alias). An entity the documents never name enters with provenance {{derived: {{from, reasoning}}}}, or not at all: \"the system\" is a placeholder, never a new entity",
                            name_arg
                        ),
                    ));
                }
            }
        }
        // A parentless twin: the same name in the same scope with no parent is
        // almost always the concept whose whole this section states, so placing
        // it is update_entity with parent; a second one under the parent needs a
        // note. Mirrors docs/compiler/concepts/identity.md#the-natural-key-under-containment.
        if existing.is_none() && parent.is_some() && note.is_none() {
            let key = name_arg.trim().to_lowercase();
            let twin = self
                .snapshot
                .graph
                .entities
                .iter()
                .filter(|(id, _)| !self.staged_entities.contains_key(*id))
                .map(|(id, e)| (id.clone(), e))
                .chain(self.staged_entities.iter().map(|(id, e)| (id.clone(), e)))
                .find(|(_, e)| {
                    e.scope == scope && e.parent.is_none() && e.name.trim().to_lowercase() == key
                })
                .map(|(id, _)| id);
            if let Some(tid) = twin {
                return Err(ToolError::new(
                    "parentless-twin",
                    format!(
                        "`{}` already exists without a parent as `{}`; to place it under `{}` call update_entity {{id: {}, parent: {}}}. A second `{}` under that parent is a different concept only with a `note` saying what tells them apart",
                        name_arg, tid, parent.as_deref().unwrap_or(""), tid, parent.as_deref().unwrap_or(""), name_arg
                    ),
                ));
            }
        }
        if let Some(id) = existing {
            if let Some(p) = &parent {
                self.staged_parents.insert(id.clone(), p.clone());
            }
            self.stage(Op::UpdateEntity {
                id: id.clone(),
                name: None,
                definition: Self::opt_str(args, "definition"),
                add_aliases: Self::str_list(args, "aliases"),
                remove_aliases: Vec::new(),
                clear_parent: false,
                not_about_add: Vec::new(),
                add_mention: mention_ref,
                stereotype,
                parent,
                set_attributes: None,
                add_attributes: attributes,
                provenance: None,
            })?;
            return Ok(json!({"id": id, "created": false}));
        }
        let id = self.snapshot.mint_entity_id(&name_arg, &self.taken_ids);
        self.taken_ids.insert(id.clone());
        let entity = Entity {
            name: name_arg,
            aliases: Self::str_list(args, "aliases"),
            distinct_from,
            definition: Self::opt_str(args, "definition"),
            scope,
            stereotype,
            parent,
            attributes,
            mentions: mention_ref.into_iter().collect(),
            provenance,
            reasoning: note,
            ..Default::default()
        };
        self.staged_entities.insert(id.clone(), entity.clone());
        self.stage(Op::CreateEntity {
            id: id.clone(),
            entity,
        })?;
        Ok(json!({"id": id, "created": true}))
    }

    pub(super) fn update_entity_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let id = Self::str_arg(args, "id")?;
        let Some(rid) = self.canon_entity_id(&id) else {
            return Err(self.unknown_entity_error(&id));
        };
        let name = Self::opt_str(args, "name");
        if let Some(n) = &name {
            if let Some(why) = junk_name(n) {
                if !self.proper_name_in_docs(n) {
                    return Err(ToolError::new("junk-name", format!("`{}` {}", n, why)));
                }
            }
        }
        let parent = match Self::opt_str(args, "parent") {
            Some(p) => Some(self.check_parent(Some(&rid), &p)?),
            None => None,
        };
        // An attribute without its own provenance takes the entity's first mention.
        let first_mention = self
            .snapshot
            .graph
            .entities
            .get(&rid)
            .and_then(|e| e.mentions.first().cloned())
            .or_else(|| {
                self.staged_entities
                    .get(&rid)
                    .and_then(|e| e.mentions.first().cloned())
            });
        let attributes = if args["attributes"].is_null() {
            Vec::new()
        } else {
            self.parse_attributes(&args["attributes"], first_mention.as_ref())?
        };
        if let Some(p) = &parent {
            self.staged_parents.insert(rid.clone(), p.clone());
        }
        self.stage(Op::UpdateEntity {
            id: rid.clone(),
            name,
            definition: Self::opt_str(args, "definition"),
            add_aliases: Self::str_list(args, "add_aliases"),
            remove_aliases: Self::str_list(args, "remove_aliases"),
            clear_parent: args["remove_parent"].as_bool() == Some(true),
            not_about_add: Vec::new(),
            add_mention: None,
            stereotype: Self::opt_str(args, "stereotype"),
            parent,
            set_attributes: None,
            add_attributes: attributes,
            provenance: None,
        })?;
        Ok(json!({"id": rid, "updated": true}))
    }

    pub(super) fn delete_entity_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let id = Self::str_arg(args, "id")?;
        let reason = Self::str_arg(args, "reason")?;
        let Some(rid) = self.canon_entity_id(&id) else {
            return Err(self.unknown_entity_error(&id));
        };
        let mut refs = self.snapshot.requirements_referencing(&rid);
        for op in &self.staged {
            if let Op::CreateRequirement {
                id: qid,
                requirement,
            } = op
            {
                if requirement.entities.contains(&rid) || requirement.entities.contains(&id) {
                    refs.push(qid.clone());
                }
            }
        }
        if !refs.is_empty() {
            return Err(ToolError::new(
                "still-referenced",
                format!(
                    "cannot delete {}; requirements still reference it: {}",
                    rid,
                    refs.join(", ")
                ),
            ));
        }
        // The parent gate holds at staging like every other: the children
        // are counted as this session's staged work leaves them (a staged
        // move away frees the parent, a staged create under it binds it).
        // Mirrors docs/compiler/tools.md#write-tools.
        let children: Vec<String> = self
            .entities_after()
            .into_iter()
            .filter(|e| e != &rid && self.parent_of(e).as_deref() == Some(rid.as_str()))
            .collect();
        if !children.is_empty() {
            return Err(ToolError::new(
                "still-a-parent",
                format!(
                    "cannot delete {}; entities still name it as parent: {}; move them (update_entity with parent) or delete them first",
                    rid,
                    children.join(", ")
                ),
            ));
        }
        self.stage(Op::DeleteEntity { id: rid, reason })?;
        Ok(json!({"deleted": true}))
    }

    pub(super) fn merge_entities_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let keep_arg = Self::str_arg(args, "keep")?;
        let absorb_arg = Self::str_arg(args, "absorb")?;
        let reason = Self::str_arg(args, "reason")?;
        let Some(keep) = self.canon_entity_id(&keep_arg) else {
            return Err(self.unknown_entity_error(&keep_arg));
        };
        let Some(absorb) = self.canon_entity_id(&absorb_arg) else {
            return Err(self.unknown_entity_error(&absorb_arg));
        };
        if keep == absorb {
            return Err(ToolError::new(
                "bad-merge",
                "keep and absorb are the same entity".into(),
            ));
        }
        // A merge never makes the survivor its own ancestor: the absorbed
        // entity's children move under the survivor, so the survivor must not
        // sit under the absorbed one, staged moves counted.
        // Mirrors docs/compiler/tools.md#write-tools.
        let mut chain = vec![keep.clone()];
        let mut cur = keep.clone();
        for _ in 0..64 {
            match self.parent_of(&cur) {
                Some(p) if p == absorb => {
                    chain.push(p);
                    return Err(ToolError::new(
                        "bad-merge",
                        format!(
                            "{} is contained in {} ({}); merging would make it its own ancestor. Keep the container, or move {} out first",
                            keep,
                            absorb,
                            chain.join(" > "),
                            keep
                        ),
                    ));
                }
                Some(p) => {
                    chain.push(p.clone());
                    cur = p;
                }
                None => break,
            }
        }
        self.stage(Op::MergeEntities {
            keep: keep.clone(),
            absorb,
            reason,
        })?;
        Ok(json!({"kept": keep}))
    }

    // Build one level: one derived entity from the members, every member moved
    // under it, as one changeset. Composed from CreateEntity and one
    // UpdateEntity parent move per member, so the journal shows the create and
    // each move with its prior parent. Mirrors docs/compiler/tools.md#grouping-tools.
    pub(super) fn group_entities_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let name_arg = Self::str_arg(args, "name")?;
        let definition = Self::opt_str(args, "definition").unwrap_or_default();
        if definition.trim().is_empty() {
            return Err(ToolError::new(
                "definition-required",
                "definition is one sentence stating the grouping's responsibility; it is the sentence the documents should gain".into(),
            ));
        }
        let reasoning = Self::opt_str(args, "reasoning").unwrap_or_default();
        if reasoning.trim().is_empty() {
            return Err(ToolError::new(
                "reasoning-required",
                "reasoning says why the domain would recognize the members as one thing; it becomes the grouping's derived provenance".into(),
            ));
        }
        // Every member resolves, once.
        let mut members: Vec<String> = Vec::new();
        for raw in Self::str_list(args, "members") {
            let Some(id) = self.canon_entity_id(&raw) else {
                let e = self.unknown_entity_error(&raw);
                return Err(ToolError::new(
                    "unknown-id",
                    format!("member {}", e.message),
                ));
            };
            if !members.contains(&id) {
                members.push(id);
            }
        }
        if members.len() < 2 {
            return Err(ToolError::new(
                "too-few-members",
                format!(
                    "a grouping holds at least two members ({} given); below two there is nothing to judge. To move one child, use update_entity with parent",
                    members.len()
                ),
            ));
        }
        // All members share one current parent, staged moves counted: a
        // grouping never crosses levels, and it takes that parent.
        let parent = self.parent_of(&members[0]);
        if let Some(other) = members.iter().find(|m| self.parent_of(m) != parent) {
            let level =
                |p: &Option<String>| p.clone().unwrap_or_else(|| "the scope root".to_string());
            return Err(ToolError::new(
                "cross-level",
                format!(
                    "a grouping never crosses levels: {} sits under {} while {} sits under {}; group members that share one parent, or move one first with update_entity parent",
                    members[0],
                    level(&parent),
                    other,
                    level(&self.parent_of(other))
                ),
            ));
        }
        let scope = self
            .scope_of(&members[0])
            .unwrap_or_else(|| "public".to_string());
        if let Some(other) = members
            .iter()
            .find(|m| self.scope_of(m).as_deref() != Some(scope.as_str()))
        {
            return Err(ToolError::new(
                "scope-mismatch",
                format!(
                    "a grouping never crosses a scope: {} is in {}, {} is not",
                    members[0], scope, other
                ),
            ));
        }
        // The near-duplicate gate, as upsert_entity: a lookalike of an existing
        // area reuses that entity, and an exact name is that entity already.
        // Mirrors docs/compiler/concepts/levels.md#naming.
        if let Ok(Some(existing)) = self.snapshot.find_natural(&name_arg, &scope, None) {
            return Err(ToolError::new(
                "near-duplicate",
                format!(
                    "`{}` already exists as {}; a grouping's name is judged like an entity name, so reparent the members under it with update_entity parent instead of minting a twin",
                    name_arg, existing
                ),
            ));
        }
        if let Some((eid, ename)) = self.near_name(&name_arg, &scope, false) {
            // A lookalike that is a peer of the members (a member of the
            // same level) carries the area's word without being the area:
            // the members never nest under it. Mirrors
            // docs/compiler/concepts/levels.md#naming.
            let sibling = !members.contains(&eid) && self.parent_of(&eid) == parent;
            let holds_children = self
                .snapshot
                .graph
                .entities
                .keys()
                .chain(self.staged_entities.keys())
                .any(|c| c != &eid && self.parent_of(c).as_deref() == Some(eid.as_str()));
            let message = if sibling && !holds_children {
                format!(
                    "`{}` looks like a name variant of `{}` ({}), a peer of the members at this level; a peer that carries the area's word is not the area and never becomes the members' parent: name the grouping from the heading or document that lists the members (or qualify the name), and let {} join the grouping of its own heading",
                    name_arg, eid, ename, eid
                )
            } else if sibling {
                format!(
                    "`{}` looks like a name variant of existing `{}` ({}), a sibling at this level that already holds children; a lookalike of an existing area reuses it: reparent the members under {} with update_entity parent, or pick the name the documents use for this area",
                    name_arg, eid, ename, eid
                )
            } else {
                format!(
                    "`{}` looks like a name variant of existing `{}` ({}), which sits at another level of the tree and names that level's concept; a move under it would cross levels, so qualify the grouping's name (the heading or document that lists the members, plus what they are) instead",
                    name_arg, eid, ename
                )
            };
            return Err(ToolError::new("near-duplicate", message));
        }
        // One changeset: the create and every move, or nothing.
        if self.staged.len() + members.len() + 1 > self.mutation_limit {
            return Err(ToolError::new(
                "mutation-budget",
                format!(
                    "the changeset's mutation budget ({}) cannot fit a grouping of {} members; call done",
                    self.mutation_limit,
                    members.len()
                ),
            ));
        }
        let id = self.snapshot.mint_entity_id(&name_arg, &self.taken_ids);
        self.taken_ids.insert(id.clone());
        let entity = Entity {
            name: name_arg,
            definition: Some(definition),
            scope,
            stereotype: Self::opt_str(args, "stereotype"),
            parent,
            provenance: Some(Provenance::Derived {
                from: members.clone(),
                reasoning,
            }),
            ..Default::default()
        };
        self.staged_entities.insert(id.clone(), entity.clone());
        self.stage(Op::CreateEntity {
            id: id.clone(),
            entity,
        })?;
        for m in &members {
            self.staged_parents.insert(m.clone(), id.clone());
            self.stage(Op::UpdateEntity {
                id: m.clone(),
                name: None,
                definition: None,
                add_aliases: Vec::new(),
                remove_aliases: Vec::new(),
                clear_parent: false,
                not_about_add: Vec::new(),
                add_mention: None,
                stereotype: None,
                parent: Some(id.clone()),
                set_attributes: None,
                add_attributes: Vec::new(),
                provenance: None,
            })?;
        }
        Ok(json!({"id": id, "moved": members}))
    }

    // Unbuild one level: a derived grouping's children reparent to its parent
    // and it tombstones with a redirect there. A stated entity is refused
    // toward the documents. Mirrors docs/compiler/tools.md#grouping-tools.
    pub(super) fn dissolve_entity_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let id = Self::str_arg(args, "id")?;
        let reason = Self::str_arg(args, "reason")?;
        let Some(rid) = self.canon_entity_id(&id) else {
            return Err(self.unknown_entity_error(&id));
        };
        // An entity staged this session is not in the graph yet: the call
        // withdraws it from the changeset, the create and every move onto it
        // dropped, so a sub-entity found empty leaves nothing behind.
        // Mirrors docs/compiler/tools.md#grouping-tools.
        if self.staged_entities.contains_key(&rid) {
            let before = self.staged.len();
            self.staged.retain(|op| match op {
                Op::CreateEntity { id, .. } => id != &rid,
                Op::UpdateEntity {
                    id,
                    parent: Some(p),
                    ..
                } => id != &rid && p != &rid,
                Op::UpdateEntity { id, .. } => id != &rid,
                Op::UpdateRequirement {
                    entities: Some(e), ..
                } => !e.iter().any(|x| x == &rid),
                Op::CreateRequirement { requirement, .. } => {
                    !requirement.entities.iter().any(|x| x == &rid)
                }
                _ => true,
            });
            self.staged_entities.remove(&rid);
            self.staged_parents.retain(|_, p| p != &rid);
            return Ok(json!({
                "withdrawn": rid,
                "dropped": before - self.staged.len(),
                "reason": reason,
                "note": "the entity was staged this session and never landed; the create and every move onto it are out of the changeset",
            }));
        }
        if self.staged_dissolved(&rid) {
            return Err(ToolError::new(
                "already-dissolved",
                format!("{} is already dissolved in this changeset", rid),
            ));
        }
        let e = self.snapshot.graph.entities.get(&rid).cloned();
        let Some(e) = e else {
            return Err(self.unknown_entity_error(&id));
        };
        let derived = matches!(e.provenance, Some(Provenance::Derived { .. }));
        if !e.mentions.is_empty() || !derived {
            return Err(ToolError::new(
                "stated-entity",
                format!(
                    "{} is an entity the documents state ({}); it holds its children in role, not in provenance, and only a grouping with derived provenance and no mentions dissolves. Revise the documents instead",
                    rid,
                    if e.mentions.is_empty() {
                        "provenance is not derived".to_string()
                    } else {
                        format!("{} mention(s)", e.mentions.len())
                    }
                ),
            ));
        }
        let refs = self.snapshot.requirements_referencing(&rid);
        if !refs.is_empty() {
            return Err(ToolError::new(
                "not-a-grouping",
                format!(
                    "{} carries requirements of its own ({}); higher levels carry none, so it is a sub-entity, not a grouping. Re-point its requirements first, or leave it",
                    rid,
                    refs.join(", ")
                ),
            ));
        }
        let parent = self.parent_of(&rid);
        let children = self.level_after(&rid);
        // The store fills parent and children as applied; this session's own
        // gates read the children's moves through parent_of.
        self.stage(Op::DissolveEntity {
            id: rid.clone(),
            reason,
            parent: None,
            children: Vec::new(),
        })?;
        Ok(json!({
            "dissolved": rid,
            "parent": parent,
            "children": children,
            "note": "a redirect to the parent stays; anything holding the old id resolves there",
        }))
    }
}
