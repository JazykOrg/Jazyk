use super::*;

impl ToolSession {
    // upsert_criterion: the observable evidence of one requirement, staged with the
    // same natural-key resolution the store applies (requirement plus statement).
    // Mirrors docs/compiler/tools.md#write-tools and docs/compiler/model/criterion.md.
    pub(super) fn upsert_criterion(&mut self, args: &Value) -> Result<Value, ToolError> {
        let statement = Self::str_arg(args, "statement")?;
        statement_present(&statement)?;
        let rid = self.canon_req_id(&Self::str_arg(args, "requirement")?)?;
        let kind = Self::str_arg(args, "kind")?;
        if !crate::model::CRITERION_KINDS.contains(&kind.as_str()) {
            return Err(ToolError::new(
                "bad-kind",
                format!(
                    "kind `{}`; one of: {}",
                    kind,
                    crate::model::CRITERION_KINDS.join(", ")
                ),
            ));
        }
        let has_source = Self::present(&args["section"]) || Self::present(&args["quote"]);
        let has_derived = Self::present(&args["provenance"]);
        if has_source == has_derived {
            return Err(ToolError::new(
                if has_source { "bad-provenance" } else { "provenance-required" },
                "a criterion enters with exactly one provenance: section plus quote (the case, example, or bullet copied verbatim from its own section), or provenance {derived: {from, reasoning}} for evidence the documents do not state".into(),
            ));
        }
        if let Some(req_statement) = self.requirement_statement(&rid) {
            if crate::store::normalize_statement(&req_statement)
                == crate::store::normalize_statement(&statement)
            {
                return Err(ToolError::new(
                    "criterion-restates",
                    format!(
                        "the criterion restates {} (\"{}\"); a criterion is one concrete condition and its observable outcome, not the rule again",
                        rid, req_statement
                    ),
                ));
            }
        }
        // Another requirement's sentence is that requirement, not evidence for this
        // one (docs/compiler/model/criterion.md#what-a-good-criterion-is).
        let norm = crate::store::normalize_statement(&statement);
        let other = self
            .snapshot
            .graph
            .requirements
            .iter()
            .filter(|(id, _)| id.as_str() != rid)
            .map(|(id, r)| (id.clone(), r.statement.clone()))
            .chain(self.staged.iter().filter_map(|o| match o {
                Op::CreateRequirement { id, requirement } if id != &rid => {
                    Some((id.clone(), requirement.statement.clone()))
                }
                _ => None,
            }))
            .find(|(_, st)| crate::store::normalize_statement(st) == norm);
        if let Some((oid, ost)) = other {
            return Err(ToolError::new(
                "criterion-restates",
                format!(
                    "the criterion restates {} (\"{}\"), another requirement: it is that requirement, not evidence for {}; state one concrete situation of {} and its observable outcome",
                    oid, ost, rid, rid
                ),
            ));
        }
        // The id follows the requirement's document, so a requirement's criteria read
        // as one family whatever section each quotes (criterion.md#identity).
        let req_source: Option<SourceRef> = self
            .snapshot
            .graph
            .requirements
            .get(&rid)
            .and_then(|r| r.source.clone())
            .or_else(|| {
                self.staged.iter().find_map(|o| match o {
                    Op::CreateRequirement { id, requirement } if *id == rid => {
                        requirement.source.clone()
                    }
                    _ => None,
                })
            });
        let req_doc = req_source.as_ref().map(|s| s.doc.clone());
        let (source, provenance, _stem_doc) = if has_source {
            let section = Self::str_arg(args, "section")?;
            let quote = Self::str_arg(args, "quote")?;
            let (doc, sec) = self.resolve_section(&section)?;
            let quote = self.check_quote(&doc, &sec, &quote)?;
            (
                Some(SourceRef {
                    doc: doc.clone(),
                    section: sec,
                    quote: quote.trim().to_string(),
                }),
                None,
                req_doc.clone().unwrap_or(doc),
            )
        } else {
            let derived = &args["provenance"]["derived"];
            let reasoning = derived["reasoning"]
                .as_str()
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .ok_or_else(|| {
                    ToolError::new(
                        "bad-provenance",
                        "provenance.derived carries reasoning: which part of the statement this criterion covers (the bound, the negative, the measure)".into(),
                    )
                })?
                .to_string();
            let mut from: Vec<String> = derived["from"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if from.is_empty() {
                from.push(rid.clone());
            }
            (
                None,
                Some(Provenance::Derived { from, reasoning }),
                req_doc.unwrap_or_else(|| "x".to_string()),
            )
        };
        let mut entities: Vec<String> = Vec::new();
        for e in Self::str_list(args, "entities") {
            match self.canon_entity_id(&e) {
                Some(id) => {
                    if !entities.contains(&id) {
                        entities.push(id);
                    }
                }
                None => return Err(self.unknown_entity_error(&e)),
            }
        }
        let criterion = Criterion {
            requirement: rid.clone(),
            statement: statement.clone(),
            given: Self::opt_str(args, "given"),
            when: Self::opt_str(args, "when"),
            then: Self::opt_str(args, "then"),
            kind,
            measure: Self::opt_str(args, "measure"),
            entities,
            source,
            provenance,
            reasoning: Self::opt_str(args, "reasoning"),
            ..Default::default()
        };
        let key = crate::store::normalize_statement(&statement);
        // Staged this turn: refresh in place.
        let staged_pos = self.staged.iter().position(|o| {
            matches!(o, Op::CreateCriterion { criterion: c, .. }
                if c.requirement == rid && crate::store::normalize_statement(&c.statement) == key)
        });
        if let Some(pos) = staged_pos {
            let Op::CreateCriterion { id, criterion: c } = &mut self.staged[pos] else {
                unreachable!()
            };
            let id = id.clone();
            *c = criterion;
            return Ok(json!({"id": id, "updated": true}));
        }
        // Recorded: the store folds onto the existing id.
        let existing = self
            .snapshot
            .graph
            .criteria
            .iter()
            .find(|(_, c)| {
                c.requirement == rid && crate::store::normalize_statement(&c.statement) == key
            })
            .map(|(cid, _)| cid.clone());
        if let Some(cid) = existing {
            self.stage(Op::CreateCriterion {
                id: cid.clone(),
                criterion,
            })?;
            return Ok(json!({"id": cid, "updated": true}));
        }
        let taken: BTreeSet<String> = self
            .staged
            .iter()
            .filter_map(|o| match o {
                Op::CreateCriterion { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        let id = self
            .snapshot
            .mint_crit_id(&criterion, req_source.as_ref(), &taken);
        self.stage(Op::CreateCriterion {
            id: id.clone(),
            criterion,
        })?;
        Ok(json!({"id": id, "created": true}))
    }

    pub(super) fn update_criterion_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let id = Self::str_arg(args, "id")?;
        if ![
            "statement",
            "kind",
            "measure",
            "given",
            "when",
            "then",
            "entities",
            "section",
            "quote",
        ]
        .iter()
        .any(|field| args.get(*field).is_some_and(|v| !v.is_null()))
        {
            return Err(ToolError::new("bad-args", "Pass id and at least one criterion field to change: statement, kind, measure, given, when, then, entities, or section plus quote.".into()));
        }

        if !self.criterion_known(&id) {
            return Err(ToolError::new(
                "unknown-id",
                format!("unknown criterion id `{}`; the requirement's criteria are listed under it in the loaded set", id),
            ));
        }
        if let Some(k) = Self::opt_str(args, "kind") {
            if !crate::model::CRITERION_KINDS.contains(&k.as_str()) {
                return Err(ToolError::new(
                    "bad-kind",
                    format!(
                        "kind `{}`; one of: {}",
                        k,
                        crate::model::CRITERION_KINDS.join(", ")
                    ),
                ));
            }
        }
        let entities = match args["entities"].as_array() {
            Some(_) => {
                let mut es = Vec::new();
                for e in Self::str_list(args, "entities") {
                    match self.canon_entity_id(&e) {
                        Some(id) => es.push(id),
                        None => return Err(self.unknown_entity_error(&e)),
                    }
                }
                Some(es)
            }
            None => None,
        };
        let source = if Self::present(&args["section"]) || Self::present(&args["quote"]) {
            let criterion = self
                .staged
                .iter()
                .rev()
                .find_map(|op| match op {
                    Op::CreateCriterion { id: cid, criterion } if cid == &id => Some(criterion),
                    _ => None,
                })
                .or_else(|| self.snapshot.graph.criteria.get(&id));
            if criterion.is_some_and(|c| c.source.is_some())
                || self
                    .staged
                    .iter()
                    .any(|op| matches!(op, Op::RatifyProvenance { id: cid, .. } if cid == &id))
            {
                return Err(ToolError::new("already-quoted", "This criterion already has a source quote; use upsert_criterion to re-source it.".into()));
            }
            if self.staged.len() + 2 > self.mutation_limit {
                return Err(ToolError::new("mutation-budget", "Linking a criterion needs room for its update and provenance change; finish this batch first.".into()));
            }
            let section = Self::str_arg(args, "section")?;
            let quote = Self::str_arg(args, "quote")?;
            let (doc, section) = self.resolve_section(&section)?;
            let quote = self.check_quote(&doc, &section, &quote)?;
            Some(SourceRef {
                doc,
                section,
                quote: quote.trim().into(),
            })
        } else {
            None
        };
        self.stage(Op::UpdateCriterion {
            id: id.clone(),
            statement: Self::opt_str(args, "statement"),
            kind: Self::opt_str(args, "kind"),
            measure: Self::opt_str(args, "measure"),
            given: Self::opt_str(args, "given"),
            when: Self::opt_str(args, "when"),
            then: Self::opt_str(args, "then"),
            entities,
        })?;
        if let Some(source) = source {
            self.stage(Op::RatifyProvenance {
                id: id.clone(),
                source,
            })?;
        }
        Ok(json!({"id": id, "updated": true}))
    }

    pub(super) fn delete_criterion_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let id = Self::str_arg(args, "id")?;
        let reason = Self::str_arg(args, "reason")?;
        if !self.criterion_known(&id) {
            return Err(ToolError::new(
                "unknown-id",
                format!("unknown criterion id `{}`", id),
            ));
        }
        self.stage(Op::DeleteCriterion {
            id: id.clone(),
            reason,
        })?;
        Ok(json!({"id": id, "deleted": true}))
    }

    pub(super) fn upsert_requirement_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let statement = Self::str_arg(args, "statement")?;
        statement_present(&statement)?;
        // Exactly one provenance: the source sentence, or a derivation. Empty
        // fields count as absent (docs/compiler/tools.md#validation-and-errors).
        let has_source = Self::present(&args["section"]) || Self::present(&args["quote"]);
        let has_derived = Self::present(&args["provenance"]);
        if has_source == has_derived {
            return Err(ToolError::new(
                if has_source { "bad-provenance" } else { "provenance-required" },
                "a requirement enters with exactly one provenance: section plus quote (the verbatim source sentence), or provenance {derived: {from, reasoning}} for a statement the documents do not state".into(),
            ));
        }
        if has_derived {
            return self.upsert_derived_requirement(args, statement);
        }
        // Provenance is validated first: a quote that does not locate is the
        // clearest signal a statement was invented, and it must not be masked
        // by an entity-id error the model would keep retrying around.
        let section = Self::str_arg(args, "section")?;
        let quote = Self::str_arg(args, "quote")?;
        let (doc, sec) = self.resolve_section(&section)?;
        if let Some(wd) = self.scope.reconcile_doc() {
            if doc != wd {
                return Err(ToolError::new(
                    "wrong-document",
                    format!(
                        "source cites {} but this batch reconciles {}; quote the sentence from {}'s own sections (text this document merely links to cannot anchor a requirement here)",
                        doc, wd, wd
                    ),
                ));
            }
        }
        let quote = self.check_quote(&doc, &sec, &quote)?;
        self.one_sentence(&doc, &sec, &quote)?;
        let raw_entities = Self::str_list(args, "entities");
        if raw_entities.is_empty() {
            return Err(ToolError::new(
                "no-entities",
                "a requirement must reference at least one entity id".into(),
            ));
        }
        let mut entities: Vec<String> = Vec::new();
        for e in &raw_entities {
            match self.canon_entity_id(e) {
                Some(id) => {
                    if !entities.contains(&id) {
                        entities.push(id);
                    }
                }
                None => return Err(self.unknown_entity_error(e)),
            }
        }
        let edges = match args["edges"].as_array() {
            Some(arr) => parse_edges(self, arr, Some(&entities))?,
            None => Vec::new(),
        };
        let transition = if !Self::present(&args["transition"]) {
            None
        } else {
            Some(self.parse_transition(&args["transition"], &entities)?)
        };
        // Every requirement carries at least one facet: the four kinds are
        // exhaustive, and a facetless statement joins no flow, gives no
        // branch, and shapes no test
        // (docs/compiler/model/requirement.md#facets).
        let facets = if !Self::present(&args["facets"]) {
            return Err(ToolError::new(
                "facets-missing",
                "a requirement carries at least one facet: behavior (the subject does something), constraint (an invariant it holds), failure-mode (what happens when something goes wrong), or quality (a measured bound, with measure); a plain stated fact is a constraint".into(),
            ));
        } else {
            parse_facets(&args["facets"])?
        };
        let source = SourceRef {
            doc: doc.clone(),
            section: sec.clone(),
            quote: quote.trim().to_string(),
        };
        let requirement = Requirement {
            statement,
            entities,
            edges,
            transition,
            facets,
            source: Some(source.clone()),
            reasoning: Self::opt_str(args, "reasoning"),
            ..Default::default()
        };
        // Stage-time natural-key resolution: the model sees the resolved id,
        // never a misleading fresh one. Same predicate as the commit-time fold.
        let norm_statement = crate::store::normalize_statement(&requirement.statement);
        let norm_quote = crate::store::normalize_statement(&source.quote);
        let same_key = |r: &Requirement| {
            let Some(rs) = r.source.as_ref() else {
                return false;
            };
            rs.doc == source.doc
                && rs.section == source.section
                && (crate::store::normalize_statement(&r.statement) == norm_statement
                    || (crate::store::normalize_statement(&rs.quote) == norm_quote
                        && crate::store::statement_rewords(&r.statement, &requirement.statement)))
        };
        // A statement this turn already staged: refresh that staged call in
        // place, so a repeated upsert is idempotent within the turn.
        let staged_pos = self.staged.iter().position(
            |op| matches!(op, Op::CreateRequirement { requirement: r, .. } if same_key(r)),
        );
        if let Some(pos) = staged_pos {
            let Op::CreateRequirement { id, requirement: r } = &mut self.staged[pos] else {
                unreachable!()
            };
            for e in &requirement.entities {
                if !r.entities.contains(e) {
                    r.entities.push(e.clone());
                }
            }
            for edge in requirement.edges {
                if !r
                    .edges
                    .iter()
                    .any(|x| x.a == edge.a && x.b == edge.b && x.rel_type == edge.rel_type)
                {
                    r.edges.push(edge);
                }
            }
            if requirement.transition.is_some() {
                r.transition = requirement.transition;
            }
            if !requirement.facets.is_empty() {
                r.facets = requirement.facets;
            }
            let matched = (crate::store::normalize_statement(&r.statement) != norm_statement)
                .then(|| r.statement.clone());
            r.statement = requirement.statement;
            r.source = requirement.source;
            let mut v = json!({"id": id.clone(), "updated": true});
            if let Some(m) = matched {
                v["matched"] = json!(m);
            }
            return Ok(v);
        }
        // A statement the store already holds updates in place. A stale anchor
        // in the same section whose statement subsumes (or is subsumed by) the
        // new one is that statement reworded: it resolves to the anchor's id.
        let existing = self
            .snapshot
            .graph
            .requirements
            .iter()
            .find(|(_, r)| same_key(r))
            .map(|(rid, _)| rid.clone())
            .or_else(|| {
                self.scope
                    .stale_anchors()
                    .iter()
                    .find(|a| {
                        self.snapshot.graph.requirements.get(*a).is_some_and(|r| {
                            r.source.as_ref().is_some_and(|rs| {
                                rs.doc == source.doc
                                    && rs.section == source.section
                                    && !self.snapshot.quote_locates(&rs.doc, &rs.section, &rs.quote)
                            }) && crate::store::statement_subsumes(
                                &r.statement,
                                &requirement.statement,
                            )
                        })
                    })
                    .cloned()
            });
        if let Some(rid) = existing {
            // The reply names the statement the call landed on when it differs,
            // so a fold is never silent. Mirrors docs/compiler/tools.md#write-tools.
            let matched = self
                .snapshot
                .graph
                .requirements
                .get(&rid)
                .filter(|r| crate::store::normalize_statement(&r.statement) != norm_statement)
                .map(|r| r.statement.clone());
            self.staged_reqs.insert(rid.clone());
            self.taken_ids.insert(rid.clone());
            self.stage(Op::CreateRequirement {
                id: rid.clone(),
                requirement,
            })?;
            let mut v = json!({"id": rid, "updated": true});
            if let Some(m) = matched {
                v["matched"] = json!(m);
            }
            return Ok(v);
        }
        // A new statement: the store mints the id; a supplied id is ignored.
        let mut taken = self.taken_ids.clone();
        taken.extend(self.staged_reqs.iter().cloned());
        let id = self.snapshot.mint_req_id(&requirement, &taken);
        self.staged_reqs.insert(id.clone());
        self.taken_ids.insert(id.clone());
        self.stage(Op::CreateRequirement {
            id: id.clone(),
            requirement,
        })?;
        Ok(json!({"id": id, "created": true}))
    }

    pub(super) fn update_requirement_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let id = self.canon_req_id(&Self::str_arg(args, "id")?)?;
        // An empty statement counts as absent: statement unchanged.
        let statement = Self::opt_str(args, "statement");
        // Empty means absent: [] (or [""]) leaves the entity list unchanged.
        let entities = if !Self::present(&args["entities"]) {
            None
        } else {
            match args["entities"].as_array() {
                Some(_) => {
                    let mut canon: Vec<String> = Vec::new();
                    for e in Self::str_list(args, "entities") {
                        match self.canon_entity_id(&e) {
                            Some(id) => {
                                if !canon.contains(&id) {
                                    canon.push(id);
                                }
                            }
                            None => return Err(self.unknown_entity_error(&e)),
                        }
                    }
                    Some(canon)
                }
                None => None,
            }
        };
        // Under an abstract-entity goal a re-point is a swap: the goal's target
        // may leave the list for a sub-entity, and no other entity the
        // statement names may be dropped. Mirrors
        // docs/compiler/goals/abstract-entity.md#gate.
        if let Some(new) = &entities {
            let splits: Vec<&str> = self
                .scope
                .goals
                .iter()
                .filter(|g| g.kind == "abstract-entity")
                .map(|g| g.target.as_str())
                .collect();
            let stored: Vec<String> = self
                .snapshot
                .graph
                .requirements
                .get(&id)
                .map(|r| {
                    r.entities
                        .iter()
                        .map(|e| self.snapshot.resolve_id(e).to_string())
                        .collect()
                })
                .unwrap_or_default();
            let dropped: Vec<&String> = stored
                .iter()
                .filter(|e| !new.contains(e) && !splits.contains(&e.as_str()))
                .collect();
            if !splits.is_empty() && !dropped.is_empty() {
                let added: Vec<&String> = new.iter().filter(|e| !stored.contains(e)).collect();
                let mut send: Vec<String> = Vec::new();
                for e in &stored {
                    if splits.contains(&e.as_str()) {
                        for a in &added {
                            if !send.contains(a) {
                                send.push((*a).clone());
                            }
                        }
                        if added.is_empty() && !send.contains(e) {
                            send.push(e.clone());
                        }
                    } else if !send.contains(e) {
                        send.push(e.clone());
                    }
                }
                return Err(ToolError::new(
                    "entities-dropped",
                    format!(
                        "{} would lose {}; a re-point under an abstract-entity goal swaps the target for the sub-entity and keeps every other entity the statement names: send entities [{}]",
                        id,
                        dropped.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
                        send.join(", ")
                    ),
                ));
            }
        }
        // The entities the edges and the transition are checked against: the
        // revised list, or the stored one.
        let listed: Vec<String> = entities.clone().unwrap_or_else(|| {
            self.snapshot
                .graph
                .requirements
                .get(&id)
                .map(|r| r.entities.clone())
                .unwrap_or_default()
        });
        // On the update path an empty edges list is a filled-in blank,
        // not "replace the judged edges with nothing".
        let edges = if !Self::present(&args["edges"]) {
            None
        } else {
            args["edges"]
                .as_array()
                .map(|arr| parse_edges(self, arr, Some(&listed)))
                .transpose()?
        };
        let transition = if !Self::present(&args["transition"]) {
            None
        } else {
            Some(self.parse_transition(&args["transition"], &listed)?)
        };
        let facets = if !Self::present(&args["facets"]) {
            None
        } else {
            Some(parse_facets(&args["facets"])?)
        };
        // A revision may re-anchor its provenance: section plus quote, both or
        // neither. The quote must locate, same gate as a fresh upsert.
        // The common miscall is passing the statement as the quote while only
        // meaning to change `entities`; name the existing anchor so the repair
        // is to drop the two fields, not to guess another sentence.
        let anchor_hint = || {
            match self.snapshot.graph.requirements.get(&id).and_then(|r| r.source.as_ref()) {
            Some(src) => format!(
                "{} is anchored at {}#{} quoting \"{}\". `quote` is the document's own sentence, never the statement. To change only the entities or edges, omit `section` and `quote`",
                id,
                src.doc,
                src.section,
                crate::llm::truncate(&src.quote, 120)
            ),
            None => "`quote` is the document's own sentence, never the statement. To change only the entities or edges, omit `section` and `quote`".to_string(),
        }
        };
        let source = match (Self::opt_str(args, "section"), Self::opt_str(args, "quote")) {
            (Some(section), Some(q)) => {
                let (doc, sec) = self
                    .resolve_section(&section)
                    .map_err(|e| ToolError::new(&e.rule, format!("{}; {}", e.message, anchor_hint())))?;
                if let Some(wd) = self.scope.reconcile_doc() {
                    let reference_repair = crate::input::is_blob(&self.snapshot, &wd, "/")
                        && self.snapshot.graph.requirements.get(&id)
                            .and_then(|r| r.source.as_ref())
                            .is_some_and(|source| source.doc == doc && source.section == sec)
                        && crate::input::staged_requirement_links(
                            &self.snapshot,
                            &wd,
                            &[Op::UpdateRequirement {
                                id: id.clone(),
                                source: Some(SourceRef { doc: doc.clone(), section: sec.clone(), quote: q.clone() }),
                                statement: None, entities: None, edges: None,
                                transition: None, facets: None, provenance: None,
                            }],
                        ).contains(&id);
                    if doc != wd && !reference_repair {
                        return Err(ToolError::new(
                            "wrong-document",
                            format!(
                                "source cites {} but this batch reconciles {}; quote the sentence from {}'s own sections",
                                doc, wd, wd
                            ),
                        ));
                    }
                }
                let q = self
                    .check_quote(&doc, &sec, &q)
                    .map_err(|e| ToolError::new(&e.rule, format!("{}; {}", e.message, anchor_hint())))?;
                self.one_sentence(&doc, &sec, &q)?;
                Some(SourceRef { doc, section: sec, quote: q.trim().to_string() })
            }
            (None, None) => None,
            _ => {
                return Err(ToolError::new(
                    "bad-argument",
                    format!("re-anchoring needs both section and quote; pass the two together or neither. {}", anchor_hint()),
                ))
            }
        };
        self.stage(Op::UpdateRequirement {
            id: id.clone(),
            statement,
            entities,
            edges,
            transition,
            facets,
            source,
            provenance: None,
        })?;
        Ok(json!({"id": id, "updated": true}))
    }

    pub(super) fn delete_requirement_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let id = self.canon_req_id(&Self::str_arg(args, "id")?)?;
        let reason = Self::str_arg(args, "reason")?;
        self.stage(Op::DeleteRequirement { id, reason })?;
        Ok(json!({"deleted": true}))
    }

    // upsert_requirement with derived provenance: a statement the documents do not
    // state, keyed on the statement within its `from` set, minted as `req:x-<n>`.
    // Mirrors docs/compiler/model/requirement.md#identity.
    fn upsert_derived_requirement(
        &mut self,
        args: &Value,
        statement: String,
    ) -> Result<Value, ToolError> {
        let raw_entities = Self::str_list(args, "entities");
        if raw_entities.is_empty() {
            return Err(ToolError::new(
                "no-entities",
                "a requirement must reference at least one entity id".into(),
            ));
        }
        let mut entities: Vec<String> = Vec::new();
        for e in &raw_entities {
            match self.canon_entity_id(e) {
                Some(id) => {
                    if !entities.contains(&id) {
                        entities.push(id);
                    }
                }
                None => return Err(self.unknown_entity_error(e)),
            }
        }
        let edges = match args["edges"].as_array() {
            Some(arr) => parse_edges(self, arr, Some(&entities))?,
            None => Vec::new(),
        };
        let transition = if !Self::present(&args["transition"]) {
            None
        } else {
            Some(self.parse_transition(&args["transition"], &entities)?)
        };
        let facets = if !Self::present(&args["facets"]) {
            Vec::new()
        } else {
            parse_facets(&args["facets"])?
        };
        let provenance = self.parse_derived(&args["provenance"])?;
        let from_set: BTreeSet<String> = match &provenance {
            Provenance::Derived { from, .. } => from.iter().cloned().collect(),
            _ => BTreeSet::new(),
        };
        let norm = crate::store::normalize_statement(&statement);
        let same_key = |r: &Requirement| {
            r.source.is_none()
                && matches!(&r.provenance, Some(Provenance::Derived { from, .. })
                    if from.iter().cloned().collect::<BTreeSet<String>>() == from_set)
                && crate::store::normalize_statement(&r.statement) == norm
        };
        let requirement = Requirement {
            statement,
            entities,
            edges,
            transition,
            facets,
            provenance: Some(provenance),
            reasoning: Self::opt_str(args, "reasoning"),
            ..Default::default()
        };
        let staged_pos = self.staged.iter().position(
            |op| matches!(op, Op::CreateRequirement { requirement: r, .. } if same_key(r)),
        );
        if let Some(pos) = staged_pos {
            let Op::CreateRequirement { id, requirement: r } = &mut self.staged[pos] else {
                unreachable!()
            };
            for e in &requirement.entities {
                if !r.entities.contains(e) {
                    r.entities.push(e.clone());
                }
            }
            for edge in requirement.edges {
                if !r
                    .edges
                    .iter()
                    .any(|x| x.a == edge.a && x.b == edge.b && x.rel_type == edge.rel_type)
                {
                    r.edges.push(edge);
                }
            }
            if requirement.transition.is_some() {
                r.transition = requirement.transition;
            }
            if !requirement.facets.is_empty() {
                r.facets = requirement.facets;
            }
            r.statement = requirement.statement;
            return Ok(json!({"id": id.clone(), "updated": true}));
        }
        if let Some(rid) = self
            .snapshot
            .graph
            .requirements
            .iter()
            .find(|(_, r)| same_key(r))
            .map(|(rid, _)| rid.clone())
        {
            self.staged_reqs.insert(rid.clone());
            self.taken_ids.insert(rid.clone());
            self.stage(Op::CreateRequirement {
                id: rid.clone(),
                requirement,
            })?;
            return Ok(json!({"id": rid, "updated": true}));
        }
        let mut taken = self.taken_ids.clone();
        taken.extend(self.staged_reqs.iter().cloned());
        let id = self.snapshot.mint_req_id(&requirement, &taken);
        self.staged_reqs.insert(id.clone());
        self.taken_ids.insert(id.clone());
        self.stage(Op::CreateRequirement {
            id: id.clone(),
            requirement,
        })?;
        Ok(
            json!({"id": id, "created": true, "note": "a derived statement; a ratification proposal asks the owner to state it in the documents"}),
        )
    }
}
