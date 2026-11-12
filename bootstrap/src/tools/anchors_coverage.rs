use super::*;

impl ToolSession {
    // Validates the quote and returns the form that locates in the source, so the
    // stored provenance stays verbatim to the document. A text-codec model often
    // backslash-escapes markdown inside JSON (\` for `); the source never carries
    // the backslashes, so the unescaped form is tried as a fallback and stored.
    // Mirrors docs/compiler/graph.md#validation-gates.
    // A requirement quotes one sentence: past a leading list number, at most one of its
    // sentences runs to four words (a fact line "Age 58." stands). A code block or a
    // diagram quotes its own line. Mirrors docs/compiler/graph.md#validation-gates.
    pub(super) fn one_sentence(&self, doc: &str, sec: &str, quote: &str) -> Result<(), ToolError> {
        let kind = self
            .snapshot
            .docs
            .get(doc)
            .and_then(|r| r.sections.get(sec))
            .map(|s| s.kind.clone())
            .unwrap_or_default();
        if kind == "code-block" || kind == "diagram" {
            return Ok(());
        }
        let body = quote.trim_start();
        let digits = body.chars().take_while(|c| c.is_ascii_digit()).count();
        let body = if digits > 0 && body[digits..].starts_with(". ") {
            &body[digits + 2..]
        } else {
            body
        };
        let chars: Vec<char> = body.chars().collect();
        let mut sentences: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            cur.push(c);
            if matches!(c, '.' | '!' | '?') {
                let mut j = i + 1;
                while j < chars.len() && matches!(chars[j], '.' | '!' | '?') {
                    cur.push(chars[j]);
                    j += 1;
                }
                let mut k = j;
                while k < chars.len() && chars[k].is_whitespace() {
                    k += 1;
                }
                let boundary = j < chars.len()
                    && chars[j].is_whitespace()
                    && k < chars.len()
                    && (chars[k].is_uppercase()
                        || chars[k].is_ascii_digit()
                        || matches!(chars[k], '"' | '\'' | '(' | '[' | '`' | '«'));
                if boundary {
                    sentences.push(std::mem::take(&mut cur));
                    i = k;
                    continue;
                }
                i = j;
                continue;
            }
            i += 1;
        }
        sentences.push(cur);
        let full: Vec<String> = sentences
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| {
                s.split(|c: char| !c.is_alphanumeric())
                    .filter(|w| !w.is_empty())
                    .count()
                    >= 4
            })
            .collect();
        if full.len() > 1 {
            return Err(ToolError::new(
                "quote-multi-sentence",
                format!(
                    "the quote spans {} sentences ({}); a requirement quotes one sentence: record one requirement per sentence, each quoting its own",
                    full.len(),
                    full.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(", ")
                ),
            ));
        }
        Ok(())
    }

    pub(super) fn check_quote(
        &self,
        doc: &str,
        sec: &str,
        quote: &str,
    ) -> Result<String, ToolError> {
        let q = quote.trim();
        if q.is_empty() {
            return Err(ToolError::new(
                "bad-quote",
                include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-anchors_coverage/quote-is-empty-copy-the-sentence-verbatim-cf779.md")).into(),
            ));
        }
        // The heading is the section's title, not its body: a model that quotes it
        // hears why (docs/compiler/graph.md#validation-gates).
        let norm = |s: &str| {
            s.trim_start_matches('#')
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        let title = self
            .snapshot
            .docs
            .get(doc)
            .and_then(|r| r.sections.get(sec))
            .map(|s| s.title.clone())
            .unwrap_or_default();
        if !title.trim().is_empty() && norm(q) == norm(&title) {
            return Err(ToolError::new(
                "quote-not-found",
                format!(
                    "`{}` is the heading of {}#{}, a title rather than body text, and headings are not quotable; quote a sentence of the section's body (a fact the heading names is stated in the body or nowhere)",
                    title, doc, sec
                ),
            ));
        }
        if self.snapshot.quote_locates(doc, sec, q) {
            return Ok(q.to_string());
        }
        let mut unescaped = String::with_capacity(q.len());
        let mut chars = q.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(&n) = chars.peek() {
                    if n.is_ascii_punctuation() {
                        continue;
                    }
                }
            }
            unescaped.push(c);
        }
        if unescaped != q && self.snapshot.quote_locates(doc, sec, &unescaped) {
            return Ok(unescaped);
        }
        Err(ToolError::new(
            "quote-not-found",
            format!(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/tools-anchors_coverage/quote-not-found-in-copy-the-sentence-35663.md"
                )),
                doc, sec
            ),
        ))
    }

    pub(super) fn anchor_tool(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        let raw = Self::str_arg(args, "id")?;
        let scope_proposals = self.scope.proposals();
        let id = self
            .canon_entity_id(&raw)
            .filter(|i| scope_proposals.contains(i))
            .or_else(|| {
                self.canon_req_id(&raw)
                    .ok()
                    .filter(|i| scope_proposals.contains(i))
            })
            .ok_or_else(|| {
                ToolError::new(
                    "unknown-anchor",
                    format!(
                        "`{}` is not one of this batch's proposals ({}); decide only the anchors the goal lists",
                        raw,
                        scope_proposals.join(", ")
                    ),
                )
            })?;
        // The proposals name the anchor's old location and quote; the op carries
        // them so the store can tell one entity mention from another. An entity
        // with several proposed mentions is decided in one call: every one of
        // them goes to the same section.
        let pdoc = self.scope.place_doc().or_else(|| self.scope.doc());
        let proposals: Vec<AnchorProposal> = self
            .snapshot
            .status
            .alignment
            .iter()
            .filter(|b| Some(&b.doc) == pdoc.as_ref())
            .flat_map(|b| b.proposals.iter())
            .filter(|p| p.anchor == id)
            .cloned()
            .collect();
        if proposals.is_empty() {
            return Err(ToolError::new(
                "unknown-anchor",
                format!("no pending proposal for `{}`", id),
            ));
        }
        let mut froms: Vec<SourceRef> = Vec::new();
        for p in &proposals {
            let (from_doc, from_sec) = split_section_ref(&p.from).ok_or_else(|| {
                ToolError::new("bad-section", format!("bad proposal location `{}`", p.from))
            })?;
            froms.push(SourceRef {
                doc: from_doc,
                section: from_sec,
                quote: p.quote.clone(),
            });
        }
        if name == "orphan_anchor" {
            for from in froms {
                self.stage(Op::OrphanAnchor {
                    id: id.clone(),
                    from,
                })?;
            }
            return Ok(json!({"id": id, "orphaned": true}));
        }
        let (doc, sec) = self.resolve_section(&Self::str_arg(args, "section")?)?;
        let given = match Self::opt_str(args, "quote") {
            Some(_) if froms.len() > 1 => {
                return Err(ToolError::new(
                    "bad-argument",
                    format!(
                        include_str!(concat!(
                            env!("OUT_DIR"),
                            "/payloads/goals/prompts/inline/tools-anchors_coverage/has-proposed-mentions-omit-quote-so-each-61d84.md"
                        )),
                        id,
                        froms.len()
                    ),
                ))
            }
            Some(q) => Some(self.check_quote(&doc, &sec, &q)?),
            None => None,
        };
        let reevaluate = args["reevaluate"].as_bool().unwrap_or(false);
        let mut all_locate = true;
        for from in froms {
            let quote = given.clone().unwrap_or_else(|| from.quote.clone());
            all_locate &= self.snapshot.quote_locates(&doc, &sec, &quote);
            self.stage(Op::PlaceAnchor {
                id: id.clone(),
                from,
                to: SourceRef {
                    doc: doc.clone(),
                    section: sec.clone(),
                    quote,
                },
                reevaluate,
            })?;
        }
        Ok(json!({
            "id": id,
            "placed": true,
            "reevaluate": reevaluate || !all_locate,
            "note": if all_locate { "quote locates in the new section" } else { "the stored quote does not locate there; the extraction turn will re-anchor or delete it" },
        }))
    }

    pub(super) fn set_coverage_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let section = Self::str_arg(args, "section")?;
        let state = Self::str_arg(args, "state")?;
        if !SETTABLE_COVERAGE.contains(&state.as_str()) {
            return Err(ToolError::new(
                "bad-state",
                format!(
                    include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-anchors_coverage/state-must-be-one-of-90d0c.md")),
                    state,
                    SETTABLE_COVERAGE.join(", ")
                ),
            ));
        }
        // A placeholder note counts as absent; weak models emit these literally.
        let note = Self::opt_str(args, "note").filter(|n| {
            !matches!(
                n.trim().to_lowercase().as_str(),
                "<nil>" | "nil" | "null" | "none" | "n/a" | "na" | "-"
            )
        });
        if state == "non-normative" && note.is_none() {
            return Err(ToolError::new(
                "note-required",
                "non-normative requires a note saying why the section states no requirements"
                    .into(),
            ));
        }
        let (doc, sec) = self.resolve_section(&section)?;
        // The mark must land on a section a reconcile-section goal of this
        // batch owns; the rejection names the owning goals' sections.
        let owned = self.scope.reconcile_scopes();
        if !owned.is_empty()
            && !owned
                .iter()
                .any(|g| g.doc.as_deref() == Some(doc.as_str()) && g.sections.contains(&sec))
        {
            let listing: Vec<String> = owned
                .iter()
                .flat_map(|g| {
                    g.sections
                        .iter()
                        .map(|s| format!("{} ({})", s, g.goal))
                        .collect::<Vec<_>>()
                })
                .collect();
            return Err(ToolError::new(
                "wrong-section",
                format!(
                    "{} is owned by no goal of this batch; the batch's sections: {}",
                    sec,
                    listing.join(", ")
                ),
            ));
        }
        // A section that yielded a statement is not non-normative. The two claims
        // contradict, and the harness settles it deterministically rather than
        // journaling a mark the graph itself refutes.
        if state == "non-normative" {
            let mut yielded: Vec<String> = self
                .snapshot
                .graph
                .requirements
                .iter()
                .filter(|(_, r)| r.anchored_at(&doc, &sec))
                .map(|(id, _)| id.clone())
                .collect();
            for op in &self.staged {
                if let Op::CreateRequirement { id, requirement } = op {
                    if requirement.anchored_at(&doc, &sec) {
                        yielded.push(id.clone());
                    }
                }
            }
            if !yielded.is_empty() {
                yielded.sort();
                yielded.dedup();
                return Err(ToolError::new(
                    "contradicts-extraction",
                    format!(
                        "{}#{} already yielded {}; a section that states a requirement is covered, not non-normative. Mark it covered, or delete the statement first if it was a mistake",
                        doc,
                        sec,
                        yielded.join(", ")
                    ),
                ));
            }
        }
        if crate::input::is_blob(&self.snapshot, &doc, &sec) {
            use crate::goals::GoalKind;
            let candidate = Op::SetCoverage {
                doc: doc.clone(),
                section: sec.clone(),
                state: state.clone(),
                note: note.clone(),
            };
            let mut projected = self.staged.clone();
            projected.retain(|op| !matches!(op, Op::SetCoverage { .. }));
            projected.push(candidate);
            if let Some(violation) = crate::goals::ReconcileSection
                .gates(&self.snapshot, &projected)
                .first()
            {
                return Err(ToolError::new(&violation.rule, violation.message.clone()));
            }
        }
        // One coverage mark per section per changeset: restaging replaces the
        // earlier mark instead of journaling contradictory states.
        self.staged
            .retain(|op| !matches!(op, Op::SetCoverage { doc: d, section: s, .. } if d == &doc && s == &sec));
        self.stage(Op::SetCoverage {
            doc,
            section: sec,
            state,
            note,
        })?;
        Ok(json!({"set": true}))
    }
}

impl ToolSession {
    pub(super) fn classify_input_tool(&mut self, args: &Value) -> Result<Value, ToolError> {
        let doc = Self::str_arg(args, "path")?;
        let decision = InputDecision {
            mode: Self::str_arg(args, "mode")?,
            sha256: Self::str_arg(args, "sha256")?,
            reason: Self::str_arg(args, "reason")?,
        };
        if !self.scope.goals.is_empty() {
            let owned = self.scope.reconcile_scopes().iter().any(|goal| {
                let Some(source) = goal.doc.as_deref() else { return false };
                source == doc || self.snapshot.docs.get(source).is_some_and(|record| {
                    let text = crate::input::projection_text(&record.sections);
                    crate::input::reference_links(source,&text).iter().any(|(href,_)| {
                        matches!(crate::gen::assets::target_input(source,href),crate::gen::assets::Target::Local(path) if path == doc)
                    })
                })
            });
            if !owned {
                return Err(ToolError::new(
                    "out-of-scope",
                    "classify the current reconciliation document or an input it references".into(),
                ));
            }
        }
        let preview = &self.snapshot;
        let (_, sections) = preview
            .classified_input(&doc, &decision, false)
            .map_err(|e| ToolError::new("input-classification", e))?;
        self.stage(Op::ClassifyInput {
            doc: doc.clone(),
            decision: decision.clone(),
        })?;
        Ok(
            json!({"path":doc,"classification":decision,"sections":sections.keys().collect::<Vec<_>>(),"notice":"Classification replaces section coverage. Reconcile the replacement input after commit."}),
        )
    }
}
