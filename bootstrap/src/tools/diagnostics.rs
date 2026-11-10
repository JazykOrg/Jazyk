use super::*;

impl ToolSession {
    // Parse an edit option's `facts` argument: a list of {id, field, value}. Absent or
    // empty is no facts. Mirrors docs/compiler/model/diagnostic.md#prompts.
    fn parse_facts(&self, v: &Value) -> Result<Vec<crate::model::FactChange>, ToolError> {
        if !Self::present(v) {
            return Ok(Vec::new());
        }
        let Some(arr) = v.as_array() else {
            return Err(ToolError::new(
                "bad-fact",
                "edit.facts is a list of {id, field, value}".into(),
            ));
        };
        let mut out = Vec::new();
        for f in arr {
            if !Self::present(f) {
                continue;
            }
            let (Some(id), Some(field)) = (
                f["id"].as_str().filter(|s| !s.trim().is_empty()),
                f["field"].as_str().filter(|s| !s.trim().is_empty()),
            ) else {
                return Err(ToolError::new(
                    "bad-fact",
                    "each of edit.facts needs an id and a field: {id, field, value}".into(),
                ));
            };
            out.push(crate::model::FactChange {
                id: id.trim().to_string(),
                field: field.trim().to_string(),
                value: f["value"].clone(),
            });
        }
        Ok(out)
    }

    // Gate an edit option's fact changes and turn each into the one-field
    // update_requirement it applies as: the requirement is quoted from the edit's
    // section, the field is editable, and the value passes the gates that tool runs on
    // it. Facts apply in order, so a transition or an edge after an `entities` change
    // is judged against the new list. Mirrors docs/compiler/model/diagnostic.md#prompts.
    pub(crate) fn fact_ops(
        &self,
        doc: &str,
        section: &str,
        facts: &[crate::model::FactChange],
    ) -> Result<Vec<Op>, ToolError> {
        let mut listed: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut ops = Vec::new();
        for (k, f) in facts.iter().enumerate() {
            let fail =
                |rule: &str, msg: String| ToolError::new(rule, format!("fact {}: {}", k, msg));
            let rid = self
                .canon_req_id(&f.id)
                .map_err(|e| fail(&e.rule, e.message))?;
            let Some(r) = self.snapshot.graph.requirements.get(&rid) else {
                return Err(fail(
                    "unknown-id",
                    format!(
                        "`{}` is not committed yet; a fact names a requirement the graph holds",
                        rid
                    ),
                ));
            };
            let here = r
                .source
                .as_ref()
                .is_some_and(|q| q.doc == doc && q.section == section);
            if !here {
                return Err(fail(
                    "bad-fact",
                    format!(
                        "{} is not quoted from {}#{}; a fact names a requirement the rewrite touches",
                        rid, doc, section
                    ),
                ));
            }
            let ents = listed
                .entry(rid.clone())
                .or_insert_with(|| r.entities.clone())
                .clone();
            let (mut statement, mut entities, mut edges, mut transition, mut facets) =
                (None, None, None, None, None);
            match f.field.as_str() {
                "statement" => {
                    let v = f.value.as_str().map(str::trim).unwrap_or_default();
                    statement_present(v).map_err(|e| fail(&e.rule, e.message))?;
                    statement = Some(v.to_string());
                }
                "entities" => {
                    let Some(arr) = f.value.as_array() else {
                        return Err(fail(
                            "bad-fact",
                            "entities takes a list of entity ids".into(),
                        ));
                    };
                    let mut canon: Vec<String> = Vec::new();
                    for e in arr {
                        let raw = e.as_str().unwrap_or_default().trim();
                        if raw.is_empty() {
                            continue;
                        }
                        match self.canon_entity_id(raw) {
                            Some(id) if !canon.contains(&id) => canon.push(id),
                            Some(_) => {}
                            None => {
                                let e = self.unknown_entity_error(raw);
                                return Err(fail(&e.rule, e.message));
                            }
                        }
                    }
                    if canon.is_empty() {
                        return Err(fail(
                            "bad-fact",
                            "entities names at least one entity".into(),
                        ));
                    }
                    listed.insert(rid.clone(), canon.clone());
                    entities = Some(canon);
                }
                "edges" => {
                    let Some(arr) = f.value.as_array() else {
                        return Err(fail(
                            "bad-fact",
                            "edges takes a list of {a, b, type?, cardinality?}".into(),
                        ));
                    };
                    edges = Some(
                        parse_edges(self, arr, Some(&ents))
                            .map_err(|e| fail(&e.rule, e.message))?,
                    );
                }
                "transition" => {
                    transition = Some(
                        self.parse_transition(&f.value, &ents)
                            .map_err(|e| fail(&e.rule, e.message))?,
                    );
                }
                "facets" => {
                    facets = Some(parse_facets(&f.value).map_err(|e| fail(&e.rule, e.message))?);
                }
                other => {
                    return Err(fail(
                        "bad-fact",
                        format!(
                            "field `{}` is not a fact an edit sets; one of: {}",
                            other,
                            crate::model::FACT_FIELDS.join(", ")
                        ),
                    ))
                }
            }
            ops.push(Op::UpdateRequirement {
                id: rid,
                statement,
                entities,
                edges,
                transition,
                facets,
                source: None,
                provenance: None,
            });
        }
        Ok(ops)
    }

    // Parse and gate a diagnostic prompt argument: at most 4 options, label
    // required, exactly one of edit or answer per option, and an edit's old_text
    // must locate in its section. Mirrors docs/compiler/model/diagnostic.md#prompts.
    pub(super) fn parse_prompt(
        &self,
        v: &Value,
    ) -> Result<Option<crate::model::DiagnosticPrompt>, ToolError> {
        use crate::model::{DiagnosticPrompt, PromptOption, SuggestedEdit};
        // Empty means absent (docs/compiler/tools.md#validation-and-errors): a
        // hollow prompt (no question, no options) was filled in, not asked. The
        // content fields decide; `freeform: false` is a filler's default, not content.
        if v.is_null() || (!Self::present(&v["question"]) && !Self::present(&v["options"])) {
            return Ok(None);
        }
        let Some(question) = v["question"].as_str().filter(|s| !s.trim().is_empty()) else {
            return Err(ToolError::new(
                "bad-prompt",
                "prompt.question is required: one sentence addressed to a person".into(),
            ));
        };
        let mut options = Vec::new();
        if let Some(arr) = v["options"].as_array() {
            if arr.len() > 4 {
                return Err(ToolError::new(
                    "bad-prompt",
                    "a prompt carries at most 4 options".into(),
                ));
            }
            for (i, o) in arr.iter().enumerate() {
                // An all-empty option item is a filled-in blank; drop it.
                if !Self::present(o) {
                    continue;
                }
                let Some(label) = o["label"].as_str().filter(|s| !s.trim().is_empty()) else {
                    return Err(ToolError::new(
                        "bad-prompt",
                        format!("option {} needs a label", i),
                    ));
                };
                let has_edit = Self::present(&o["edit"]);
                let answer = o["answer"].as_str().filter(|s| !s.trim().is_empty());
                if has_edit && answer.is_some() {
                    return Err(ToolError::new(
                        "bad-prompt",
                        format!("option {} carries both edit and answer; keep one: `edit` when the option changes the document (the edit is its answer), `answer` when it replies in words; drop the other and send the call again", i),
                    ));
                }
                if !has_edit && answer.is_none() {
                    return Err(ToolError::new(
                        "bad-prompt",
                        format!("option {} carries neither edit nor answer; give it one (`edit` to change the document, `answer` to reply in words); an option with neither is a filled-in blank, and a diagnostic that asks nothing of a human omits `prompt` entirely", i),
                    ));
                }
                let edit = if has_edit {
                    let e = &o["edit"];
                    let get = |k: &str| -> Result<String, ToolError> {
                        e[k].as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(|s| s.to_string())
                            .ok_or_else(|| {
                                ToolError::new(
                                    "bad-prompt",
                                    format!("option {}: edit.{} is required", i, k),
                                )
                            })
                    };
                    let (doc, mut section) = (get("doc")?, get("section")?);
                    // Both reference forms are accepted: `/ref` and the full
                    // `doc.md#/ref` the packs display.
                    if let Some((sd, sr)) = crate::model::split_section_ref(&section) {
                        if sd == doc {
                            section = sr;
                        } else {
                            return Err(ToolError::new(
                                "bad-prompt",
                                format!("option {}: edit.section names document `{}` but edit.doc says `{}`", i, sd, doc),
                            ));
                        }
                    }
                    // An empty old_text appends new_text to the section's body.
                    let old_text = e["old_text"].as_str().unwrap_or_default().to_string();
                    let new_text = get("new_text")?;
                    let Some(rec) = self.snapshot.docs.get(&doc) else {
                        return Err(ToolError::new(
                            "bad-prompt",
                            format!("option {}: unknown document `{}`", i, doc),
                        ));
                    };
                    let Some(sec) = rec.sections.get(&section) else {
                        return Err(ToolError::new(
                            "bad-prompt",
                            format!("option {}: unknown section `{}#{}`", i, doc, section),
                        ));
                    };
                    if !old_text.trim().is_empty()
                        && crate::md::locate_bytes(&sec.raw, &old_text).is_none()
                    {
                        return Err(ToolError::new(
                            "bad-prompt",
                            format!("option {}: old_text does not locate in {}#{}; copy it verbatim from the section", i, doc, section),
                        ));
                    }
                    let facts = self.parse_facts(&e["facts"]).map_err(|err| {
                        ToolError::new(&err.rule, format!("option {}: {}", i, err.message))
                    })?;
                    self.fact_ops(&doc, &section, &facts).map_err(|err| {
                        ToolError::new(&err.rule, format!("option {}: {}", i, err.message))
                    })?;
                    Some(SuggestedEdit {
                        doc,
                        section,
                        old_text,
                        new_text,
                        facts,
                    })
                } else {
                    None
                };
                options.push(PromptOption {
                    label: label.to_string(),
                    edit,
                    answer: answer.map(|s| s.to_string()),
                });
            }
        }
        Ok(Some(DiagnosticPrompt {
            question: question.to_string(),
            options,
            freeform: v["freeform"].as_bool().unwrap_or(false),
        }))
    }

    pub(super) fn report_diagnostic_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let rule = Self::str_arg(args, "rule")?;
        if !REVIEW_RULES.contains(&rule.as_str()) {
            return Err(ToolError::new(
                "bad-rule",
                format!(
                    "rule `{}` is not in the catalog; use one of: {}",
                    rule,
                    REVIEW_RULES.join(", ")
                ),
            ));
        }
        let severity = Self::str_arg(args, "severity")?;
        if !SEVERITIES.contains(&severity.as_str()) {
            return Err(ToolError::new(
                "bad-severity",
                format!(
                    "severity `{}` must be one of: {}",
                    severity,
                    SEVERITIES.join(", ")
                ),
            ));
        }
        let subjects = Self::str_list(args, "subjects");
        if subjects.is_empty() {
            return Err(ToolError::new(
                "no-subjects",
                "a diagnostic needs at least one subject node id".into(),
            ));
        }
        // A pair finding names both sides: one subject leaves the other side
        // unfindable. Mirrors docs/compiler/model/diagnostic.md#prompts.
        if (rule == "contradiction" || rule == "duplicate-requirement") && subjects.len() < 2 {
            return Err(ToolError::new(
                "no-subjects",
                format!(
                    "a {} names both sides in subjects; `{}` alone leaves the other statement unfindable",
                    rule,
                    subjects.join(", ")
                ),
            ));
        }
        for s in &subjects {
            let ok = self.known_entity(s)
                || self.snapshot.graph.requirements.contains_key(s)
                || self.staged_reqs.contains(s)
                || self.criterion_known(s)
                || split_section_ref(s)
                    .map(|(d, r)| {
                        self.snapshot
                            .docs
                            .get(&d)
                            .map(|rec| rec.sections.contains_key(&r))
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
            if !ok {
                return Err(ToolError::new(
                    "unknown-id",
                    format!("diagnostic subject `{}` does not exist", s),
                ));
            }
        }
        let message = Self::str_arg(args, "message")?;
        let prompt = self.parse_prompt(&args["prompt"])?;
        // A decision is a question for the owner: it carries the prompt that
        // asks it. Mirrors docs/compiler/model/diagnostic.md#rules-catalog.
        if rule == "decision" && prompt.is_none() {
            return Err(ToolError::new(
                "decision-needs-prompt",
                "a decision diagnostic carries a prompt: the question and the options the documents leave open (each a label with an edit or an answer)".into(),
            ));
        }
        // Stage-time natural-key resolution, same predicate as the commit
        // fold: the reply carries the finding's id, and a re-report answers
        // with the id it will update instead of a bare acknowledgement.
        let subject_set: BTreeSet<&str> = subjects.iter().map(String::as_str).collect();
        let same_key = |d: &Diagnostic| {
            d.rule == rule
                && d.lifecycle == "open"
                && d.subjects
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<&str>>()
                    == subject_set
        };
        let existing = self
            .staged_diags
            .iter()
            .find(|(_, d)| same_key(d))
            .map(|(i, _)| i.clone())
            .or_else(|| {
                self.snapshot
                    .graph
                    .diagnostics
                    .iter()
                    .find(|(_, d)| same_key(d))
                    .map(|(i, _)| i.clone())
            });
        let (id, created) = match existing {
            Some(i) => (i, false),
            None => {
                let taken: BTreeSet<String> = self.staged_diags.keys().cloned().collect();
                (self.snapshot.mint_diag_id(&rule, &subjects, &taken), true)
            }
        };
        let diagnostic = Diagnostic {
            rule,
            severity,
            subjects,
            message,
            reasoning: Self::opt_str(args, "reasoning"),
            lifecycle: "open".to_string(),
            triage: None,
            prompt,
            answer: None,
            created: None,
            updated: None,
        };
        self.stage(Op::ReportDiagnostic {
            id: id.clone(),
            diagnostic: diagnostic.clone(),
        })?;
        self.staged_diags.insert(id.clone(), diagnostic);
        Ok(json!({"reported": true, "id": id, "created": created}))
    }

    pub(super) fn update_diagnostic_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let id = Self::str_arg(args, "id")?;
        if !self.snapshot.graph.diagnostics.contains_key(&id)
            && !self.staged_diags.contains_key(&id)
        {
            return Err(ToolError::new(
                "unknown-id",
                format!("unknown diagnostic id `{}`", id),
            ));
        }
        let prompt = self.parse_prompt(&args["prompt"])?;
        self.stage(Op::UpdateDiagnosticPrompt { id, prompt })?;
        Ok(json!({"updated": true}))
    }

    pub(super) fn resolve_diagnostic_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let id = Self::str_arg(args, "id")?;
        let reason = Self::str_arg(args, "reason")?;
        let Some(d) = self
            .snapshot
            .graph
            .diagnostics
            .get(&id)
            .or_else(|| self.staged_diags.get(&id))
        else {
            return Err(ToolError::new(
                "unknown-id",
                format!("unknown diagnostic id `{}`", id),
            ));
        };
        // A ratification proposal closes only through accept or retract.
        // Mirrors docs/compiler/model/diagnostic.md#lifecycle-and-triage.
        if d.rule == "ratification-pending" {
            return Err(ToolError::new(
                "not-resolvable",
                format!(
                    "{} is a ratification proposal; it resolves when its edit option is applied or the fact is retracted, never through resolve_diagnostic",
                    id
                ),
            ));
        }
        // The build's own bookkeeping is the harness's to settle.
        if harness_owned_rule(&d.rule) {
            return Err(ToolError::new(
                "not-resolvable",
                format!(
                    "{} ({}) is owned by the harness: the build settles it itself (a parked goal resuming, a human's ruling), never a session; leave it and judge the entity",
                    id, d.rule
                ),
            ));
        }
        self.stage(Op::ResolveDiagnostic { id, reason })?;
        Ok(json!({"resolved": true}))
    }

    // The chat edit path: one authored field on one committed node. A quoted fact with
    // an accepted sentence rewrite (`note`) stages the mutation with its re-anchored
    // quote and reports the prose replacement for the serving to write; anything else
    // stages a decree with its ratification proposal.
    // Mirrors docs/compiler/tools.md#chat-tools and docs/compiler/compilation.md#edit-paths.
    pub(super) fn edit_fact(&mut self, args: &Value) -> Result<Value, ToolError> {
        let raw_id = Self::str_arg(args, "id")?;
        let field = Self::str_arg(args, "field")?;
        let value = &args["value"];
        // A hollow value ({}, [], "") is as absent as null: the requiredness
        // bounce names the repair instead of a deeper parser's misleading error.
        if !Self::present(value) {
            return Err(ToolError::new(
                "bad-args",
                "value is required: the field's new content".into(),
            ));
        }
        let note = Self::opt_str(args, "note");
        let text_value = || -> Result<String, ToolError> {
            value
                .as_str()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    ToolError::new(
                        "bad-args",
                        format!("field `{}` takes a non-empty string value", field),
                    )
                })
        };
        let uncommitted = |id: &str| {
            ToolError::new(
                "unknown-id",
                format!(
                    "`{}` is staged in this session and not committed; edit_fact works on committed facts",
                    id
                ),
            )
        };
        let bad_field = |kind: &str, fields: &str| {
            ToolError::new(
                "bad-field",
                format!(
                    "field `{}` is not editable on {}; one of: {}. Ids, created, updated, mentions, and provenance itself are never edited directly",
                    field, kind, fields
                ),
            )
        };

        if let Ok(rid) = self.canon_req_id(&raw_id) {
            let Some(r) = self.snapshot.graph.requirements.get(&rid).cloned() else {
                return Err(uncommitted(&rid));
            };
            let (mut statement, mut edges, mut transition, mut facets) = (None, None, None, None);
            match field.as_str() {
                "statement" => statement = Some(text_value()?),
                "edges" => {
                    let Some(arr) = value.as_array() else {
                        return Err(ToolError::new(
                            "bad-args",
                            "edges takes a list of {a, b, type?, cardinality?}".into(),
                        ));
                    };
                    edges = Some(parse_edges(self, arr, Some(&r.entities))?);
                }
                "transition" => transition = Some(self.parse_transition(value, &r.entities)?),
                "facets" => facets = Some(parse_facets(value)?),
                _ => {
                    return Err(bad_field(
                        "a requirement",
                        "statement, edges, transition, facets",
                    ))
                }
            }
            let sentence = statement.clone().unwrap_or_else(|| r.statement.clone());
            return match (r.source.as_ref(), note.as_deref()) {
                (Some(src), Some(rewrite)) => {
                    let rewrite = rewrite.trim().to_string();
                    self.stage(Op::UpdateRequirement {
                        id: rid.clone(),
                        statement,
                        entities: None,
                        edges,
                        transition,
                        facets,
                        source: Some(SourceRef {
                            doc: src.doc.clone(),
                            section: src.section.clone(),
                            quote: rewrite.clone(),
                        }),
                        provenance: None,
                    })?;
                    Ok(json!({
                        "id": rid, "field": field, "path": "dual-write",
                        "prose": {"doc": src.doc, "section": src.section, "old_text": src.quote, "new_text": rewrite}
                    }))
                }
                _ => {
                    let decree = self.decree(note);
                    let proposal = self.snapshot.ratification_proposal(
                        &rid,
                        &sentence,
                        &decree,
                        r.source.as_ref(),
                        &r.entities,
                        None,
                    );
                    self.stage(Op::UpdateRequirement {
                        id: rid.clone(),
                        statement,
                        entities: None,
                        edges,
                        transition,
                        facets,
                        source: None,
                        provenance: Some(decree),
                    })?;
                    let question = proposal.prompt.as_ref().map(|p| p.question.clone());
                    self.stage(Op::ReportDiagnostic {
                        id: String::new(),
                        diagnostic: proposal,
                    })?;
                    Ok(json!({
                        "id": rid, "field": field, "path": "decree", "proposal": question,
                        "note": "the edit landed graph-only with decree provenance; a ratification proposal asks the owner to state it in the documents"
                    }))
                }
            };
        }

        if let Some(eid) = self.canon_entity_id(&raw_id) {
            let Some(e) = self.snapshot.graph.entities.get(&eid).cloned() else {
                return Err(uncommitted(&eid));
            };
            let quoted_mention = if e.provenance.is_none() {
                e.mentions.first().cloned()
            } else {
                None
            };
            let (mut definition, mut stereotype, mut parent) = (None, None, None);
            let mut attribute: Option<Attribute> = None;
            let (sentence, former): (String, Option<SourceRef>) = match field.as_str() {
                "definition" => {
                    let v = text_value()?;
                    let s = format!("{}: {}", e.name, v);
                    definition = Some(v);
                    (s, quoted_mention.clone())
                }
                "stereotype" => {
                    let v = text_value()?;
                    let s = format!("{} is a {}.", e.name, v);
                    stereotype = Some(v);
                    (s, quoted_mention.clone())
                }
                "parent" => {
                    let p = self.check_parent(Some(&eid), &text_value()?)?;
                    let pname = self
                        .snapshot
                        .graph
                        .entities
                        .get(&p)
                        .map(|x| x.name.clone())
                        .unwrap_or_else(|| p.clone());
                    let s = format!("{} is part of {}.", e.name, pname);
                    parent = Some(p);
                    (s, quoted_mention.clone())
                }
                f if f.starts_with("attributes.") => {
                    let rest = &f["attributes.".len()..];
                    let Some((aname, sub)) = rest.rsplit_once('.') else {
                        return Err(bad_field(
                            "an entity",
                            "definition, stereotype, parent, attributes.<name>.type, attributes.<name>.value",
                        ));
                    };
                    if sub != "type" && sub != "value" {
                        return Err(bad_field(
                            "an entity",
                            "definition, stereotype, parent, attributes.<name>.type, attributes.<name>.value",
                        ));
                    }
                    let Some(mut a) = e.attributes.iter().find(|a| a.name == aname).cloned() else {
                        return Err(ToolError::new(
                            "unknown-attribute",
                            format!(
                                "{} has no attribute `{}`; its attributes: {}",
                                eid,
                                aname,
                                e.attributes
                                    .iter()
                                    .map(|a| a.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        ));
                    };
                    let v = text_value()?;
                    let s = if sub == "type" {
                        a.r#type = Some(v.clone());
                        format!("{} has an attribute {} of type {}.", e.name, aname, v)
                    } else {
                        a.value = Some(v.clone());
                        format!("{}'s {} is {}.", e.name, aname, v)
                    };
                    let former = match &a.provenance {
                        Provenance::Quote(q) => Some(q.clone()),
                        _ => None,
                    };
                    attribute = Some(a);
                    (s, former)
                }
                _ => {
                    return Err(bad_field(
                        "an entity",
                        "definition, stereotype, parent, attributes.<name>.type, attributes.<name>.value",
                    ))
                }
            };
            if let Some(p) = &parent {
                self.staged_parents.insert(eid.clone(), p.clone());
            }
            return match (former.as_ref(), note.as_deref()) {
                (Some(src), Some(rewrite)) => {
                    let to = SourceRef {
                        doc: src.doc.clone(),
                        section: src.section.clone(),
                        quote: rewrite.trim().to_string(),
                    };
                    let mut add_attributes = Vec::new();
                    match attribute {
                        Some(mut a) => {
                            a.provenance = Provenance::Quote(to.clone());
                            add_attributes.push(a);
                        }
                        None => self.stage(Op::PlaceAnchor {
                            id: eid.clone(),
                            from: src.clone(),
                            to: to.clone(),
                            reevaluate: false,
                        })?,
                    }
                    self.stage(Op::UpdateEntity {
                        id: eid.clone(),
                        name: None,
                        definition,
                        add_aliases: Vec::new(),
                        remove_aliases: Vec::new(),
                        clear_parent: false,
                        not_about_add: Vec::new(),
                        add_mention: None,
                        stereotype,
                        parent,
                        set_attributes: None,
                        add_attributes,
                        provenance: None,
                    })?;
                    Ok(json!({
                        "id": eid, "field": field, "path": "dual-write",
                        "prose": {"doc": src.doc, "section": src.section, "old_text": src.quote, "new_text": to.quote}
                    }))
                }
                _ => {
                    let decree = self.decree(note);
                    let attr_name = attribute.as_ref().map(|a| a.name.clone());
                    let proposal = self.snapshot.ratification_proposal(
                        &eid,
                        &sentence,
                        &decree,
                        former.as_ref(),
                        std::slice::from_ref(&eid),
                        attr_name.as_deref(),
                    );
                    let (add_attributes, provenance) = match attribute {
                        Some(mut a) => {
                            a.provenance = decree.clone();
                            (vec![a], None)
                        }
                        None => (Vec::new(), Some(decree)),
                    };
                    self.stage(Op::UpdateEntity {
                        id: eid.clone(),
                        name: None,
                        definition,
                        add_aliases: Vec::new(),
                        remove_aliases: Vec::new(),
                        clear_parent: false,
                        not_about_add: Vec::new(),
                        add_mention: None,
                        stereotype,
                        parent,
                        set_attributes: None,
                        add_attributes,
                        provenance,
                    })?;
                    let question = proposal.prompt.as_ref().map(|p| p.question.clone());
                    self.stage(Op::ReportDiagnostic {
                        id: String::new(),
                        diagnostic: proposal,
                    })?;
                    Ok(json!({
                        "id": eid, "field": field, "path": "decree", "proposal": question,
                        "note": "the edit landed graph-only with decree provenance; a ratification proposal asks the owner to state it in the documents"
                    }))
                }
            };
        }

        if let Some((vid, v)) = self.view_known(&raw_id) {
            if field != "members" {
                return Err(bad_field("a view", "members"));
            }
            let Some(arr) = value.as_array() else {
                return Err(ToolError::new(
                    "bad-args",
                    "members takes an ordered list of node ids".into(),
                ));
            };
            let raw: Vec<String> = arr
                .iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect();
            let members = self.canon_members(&raw, "member")?;
            self.check_view_members(&v.kind, &members, &v.collapse)?;
            self.stage(Op::UpdateView {
                id: vid.clone(),
                title: None,
                members: Some(members),
                add_members: Vec::new(),
                remove_members: Vec::new(),
                query: None,
                collapse: None,
                exclude: Vec::new(),
                reasoning: note,
            })?;
            return Ok(json!({
                "id": vid, "field": "members", "path": "decree",
                "note": "the view is curated from here on; views carry no ratification proposal"
            }));
        }

        Err(ToolError::new(
            "unknown-id",
            format!(
                "unknown id `{}`; edit_fact takes a requirement (req:), entity (ent:), or view (view:) id. For a criterion (crit:) use update_criterion with id and the fields to change; chat needs no begin_goals",
                raw_id
            ),
        ))
    }
}
