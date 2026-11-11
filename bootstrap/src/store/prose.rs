// Prose edits and ratification: dual writes, quote rewrites, re-anchoring, and
// ratification proposals. Mirrors docs/compiler/compilation.md#edit-paths.
use super::*;

impl Store {
    // Absorb a dual-write prose edit: replace the document's stored section tree and
    // content hash with the post-edit text, keeping coverage marks for sections whose
    // references survive. Mirrors docs/compiler/graph.md#mutations.
    pub fn absorb_doc_edit(&mut self, doc: &str, text: &str) {
        let sections = crate::md::parse_sections(text);
        let rec = self.docs.entry(doc.to_string()).or_default();
        rec.content_hash = crate::model::hash_hex(text);
        rec.coverage.retain(|r, _| sections.contains_key(r));
        rec.sections = sections;
    }

    // Commit a prose replacement with the graph mutations it carries as one changeset.
    // The file is written first (through `write` when a frontend delegates it), then the
    // edit and the mutations apply together; a commit that skips anything puts the
    // prose back, so neither side moved. Every quoted anchor in the section whose quote
    // contains the replaced text is re-anchored mechanically (a requirement's statement
    // updated when the text appears in it verbatim), unless an op in the changeset
    // already re-anchors, ratifies, or deletes it. Mirrors
    // docs/compiler/compilation.md#edit-paths.
    pub fn dual_write(
        &mut self,
        root: &Path,
        edit: &ProseEdit,
        ops: Vec<Op>,
        commit: &Commit,
        write: Option<WriteEdit>,
    ) -> Result<CommitReport, String> {
        if ops.is_empty() {
            return Err(
                "edit-needs-mutation: a prose edit rides with the graph mutation it carries"
                    .to_string(),
            );
        }
        let path = root.join(&edit.doc);
        let put = |text: &str, old: &str, new: &str| -> Result<(), String> {
            match write {
                Some(w) => w(&edit.doc, old, new, text),
                None => crate::vfs::write(&path, text)
                    .map_err(|e| format!("write {}: {}", path.display(), e)),
            }
        };
        put(&edit.full, &edit.old_text, &edit.new_text)?;
        let mut all = vec![Op::EditDocProse {
            doc: edit.doc.clone(),
            section: edit.section.clone(),
            old_text: edit.old_text.clone(),
            new_text: edit.new_text.clone(),
            text: edit.full.clone(),
        }];
        all.extend(self.reanchor_ops(edit, &ops));
        all.extend(ops);
        let report = self.apply(all, commit);
        if !report.skipped.is_empty() {
            let _ = put(&edit.old_full, &edit.new_text, &edit.old_text);
            return Err(report.skipped.join("; "));
        }
        crate::control::carry_release(&self.out, &edit.doc, &edit.old_full, &edit.full);
        Ok(report)
    }

    // The requirements a prose replacement touches: those quoted from the edited
    // section whose quote contains the replaced text, each with its rewritten quote.
    pub fn quote_rewrites(&self, edit: &ProseEdit) -> Vec<(String, String)> {
        if edit.old_text.trim().is_empty() {
            return Vec::new();
        }
        self.graph
            .requirements
            .iter()
            .filter_map(|(qid, r)| {
                let src = r.source.as_ref()?;
                if src.doc != edit.doc || src.section != edit.section {
                    return None;
                }
                edit.splice(&src.quote).map(|q| (qid.clone(), q))
            })
            .collect()
    }

    // The mechanical re-anchoring a prose replacement owes: requirement sources and
    // entity mentions in the edited section whose quote contains the replaced text.
    fn reanchor_ops(&self, edit: &ProseEdit, ops: &[Op]) -> Vec<Op> {
        let mut out = Vec::new();
        if edit.old_text.trim().is_empty() {
            return out;
        }
        let handled = |id: &str| {
            ops.iter().any(|o| match o {
                Op::UpdateRequirement {
                    id: x,
                    source: Some(_),
                    ..
                }
                | Op::RatifyProvenance { id: x, .. }
                | Op::DeleteRequirement { id: x, .. }
                | Op::RetractDecree { id: x, .. }
                | Op::PlaceAnchor { id: x, .. } => x == id,
                _ => false,
            })
        };
        let splice = |text: &str| edit.splice(text);
        for (qid, new_quote) in self.quote_rewrites(edit) {
            if handled(&qid) {
                continue;
            }
            let r = &self.graph.requirements[&qid];
            out.push(Op::UpdateRequirement {
                id: qid.clone(),
                statement: splice(&r.statement),
                entities: None,
                edges: None,
                transition: None,
                facets: None,
                source: Some(SourceRef {
                    doc: edit.doc.clone(),
                    section: edit.section.clone(),
                    quote: new_quote,
                }),
                provenance: None,
            });
        }
        for (eid, e) in &self.graph.entities {
            if handled(eid) {
                continue;
            }
            for m in &e.mentions {
                if m.doc != edit.doc || m.section != edit.section {
                    continue;
                }
                let Some(new_quote) = splice(&m.quote) else {
                    continue;
                };
                out.push(Op::PlaceAnchor {
                    id: eid.clone(),
                    from: m.clone(),
                    to: SourceRef {
                        doc: edit.doc.clone(),
                        section: edit.section.clone(),
                        quote: new_quote,
                    },
                    reevaluate: false,
                });
            }
        }
        out
    }

    // The ratification proposal for a fact that stands with derived or decree
    // provenance: a `ratification-pending` diagnostic whose prompt carries one `edit`
    // option inserting the proposed sentence and one `answer` option that retracts.
    // `sentence` is the fact's own text. A decree over a quoted fact (`former`) targets
    // the former source and overwrites the former quote; otherwise the target is the
    // section sourcing most of a derivation's `from` nodes, or the first entity's first
    // mention, and the sentence is appended. `attribute` names the attribute when the
    // fact is one. Mirrors docs/compiler/model/diagnostic.md#ratification-proposals.
    pub fn ratification_proposal(
        &self,
        subject: &str,
        sentence: &str,
        provenance: &Provenance,
        former: Option<&SourceRef>,
        entities: &[String],
        attribute: Option<&str>,
    ) -> Diagnostic {
        let anchor_of = |id: &str| -> Option<(String, String)> {
            let id = self.resolve_id(id);
            if let Some(r) = self.graph.requirements.get(id) {
                return r
                    .source
                    .as_ref()
                    .map(|s| (s.doc.clone(), s.section.clone()));
            }
            self.graph
                .entities
                .get(id)
                .and_then(|e| e.mentions.first())
                .map(|m| (m.doc.clone(), m.section.clone()))
        };
        let (kind, reasoning) = match provenance {
            Provenance::Derived { from, reasoning } => (
                "derived",
                format!("{} (from {})", reasoning, from.join(", ")),
            ),
            Provenance::Decree { author, at, note } => (
                "decreed",
                format!(
                    "decreed by {} at {}{}",
                    author,
                    at,
                    note.as_ref()
                        .map(|n| format!(": {}", n))
                        .unwrap_or_default()
                ),
            ),
            Provenance::Quote(s) => ("quoted", format!("{}#{}", s.doc, s.section)),
        };
        let target: Option<(String, String, String)> = match (former, provenance) {
            (Some(f), _) => Some((f.doc.clone(), f.section.clone(), f.quote.clone())),
            (None, Provenance::Derived { from, .. }) => {
                let mut counts: Vec<((String, String), usize)> = Vec::new();
                for id in from {
                    if let Some(anchor) = anchor_of(id) {
                        match counts.iter_mut().find(|(a, _)| *a == anchor) {
                            Some((_, n)) => *n += 1,
                            None => counts.push((anchor, 1)),
                        }
                    }
                }
                counts
                    .iter()
                    .max_by_key(|(_, n)| *n)
                    .map(|((d, s), _)| (d.clone(), s.clone(), String::new()))
            }
            _ => None,
        }
        .or_else(|| {
            std::iter::once(subject)
                .chain(entities.iter().map(String::as_str))
                .find_map(anchor_of)
                .map(|(d, s)| (d, s, String::new()))
        });
        let what = match attribute {
            Some(a) => format!("{}'s attribute `{}`", subject, a),
            None => subject.to_string(),
        };
        let mut options = Vec::new();
        let question = match &target {
            Some((doc, section, old)) => {
                options.push(PromptOption {
                    label: if old.is_empty() {
                        format!("Insert into {} {}", doc, section)
                    } else {
                        format!("Replace the sentence in {} {}", doc, section)
                    },
                    edit: Some(SuggestedEdit {
                        doc: doc.clone(),
                        section: section.clone(),
                        old_text: old.clone(),
                        new_text: sentence.to_string(),
                        facts: Vec::new(),
                    }),
                    answer: None,
                });
                format!("Should {} state it? \"{}\"", doc, sentence)
            }
            None => format!("Should the documents state it? \"{}\"", sentence),
        };
        options.push(PromptOption {
            label: "Retract".to_string(),
            edit: None,
            answer: Some("retract".to_string()),
        });
        Diagnostic {
            goal: None,
            rule: "ratification-pending".to_string(),
            severity: "info".to_string(),
            subjects: vec![subject.to_string()],
            message: format!("{} is {} and no document states it.", what, kind),
            reasoning: Some(reasoning),
            lifecycle: "open".to_string(),
            triage: None,
            prompt: Some(DiagnosticPrompt {
                question,
                options,
                freeform: true,
            }),
            answer: None,
            created: None,
            updated: None,
        }
    }

    // Related evidence stays atomic in the graph but shares one author decision.
    pub(super) fn combine_criterion_proposals(&mut self, build: &str) -> Vec<Op> {
        let mut ops = Vec::new();
        let combined: Vec<_> = self
            .graph
            .diagnostics
            .iter()
            .filter(|(_, d)| {
                d.rule == "ratification-pending"
                    && d.lifecycle == "open"
                    && d.answer.is_none()
                    && d.subjects.len() > 1
                    && d.subjects.iter().all(|s| s.starts_with("crit:"))
            })
            .map(|(id, d)| (id.clone(), d.clone()))
            .collect();
        for (id, mut d) in combined {
            let before = serde_json::to_value(&d).unwrap();
            d.subjects.retain(|id| {
                self.graph.criteria.get(id).is_some_and(|c| {
                    c.source.is_none() && matches!(c.provenance, Some(Provenance::Derived { .. }))
                })
            });
            if d.subjects.is_empty() {
                d.lifecycle = "resolved".into();
                d.updated = Some(build.into());
                ops.push(Op::ResolveDiagnostic {
                    id: id.clone(),
                    reason: "no proposed criteria remain".into(),
                });
            } else {
                let updated = d.updated.clone();
                self.refresh_criterion_proposal(&mut d, build);
                d.updated = updated;
                if serde_json::to_value(&d).unwrap() == before {
                    continue;
                }
                d.updated = Some(build.into());
                ops.push(Op::ReportDiagnostic {
                    id: id.clone(),
                    diagnostic: d.clone(),
                });
            }
            self.graph.diagnostics.insert(id, d);
        }
        let mut groups: BTreeMap<(String, String, String), Vec<String>> = BTreeMap::new();
        for (id, d) in &self.graph.diagnostics {
            if d.rule != "ratification-pending"
                || d.lifecycle != "open"
                || d.answer.is_some()
                || d.triage.is_some()
            {
                continue;
            }
            let Some(p) = &d.prompt else { continue };
            let Some(e) = p.options.iter().find_map(|o| o.edit.as_ref()) else {
                continue;
            };
            if !e.old_text.is_empty() || !e.facts.is_empty() || d.subjects.is_empty() {
                continue;
            }
            let criteria: Option<Vec<_>> = d
                .subjects
                .iter()
                .map(|s| self.graph.criteria.get(s))
                .collect();
            let Some(criteria) = criteria else { continue };
            let owner = &criteria[0].requirement;
            if criteria.iter().any(|c| {
                c.requirement != *owner || !matches!(c.provenance, Some(Provenance::Derived { .. }))
            }) {
                continue;
            }
            groups
                .entry((owner.clone(), e.doc.clone(), e.section.clone()))
                .or_default()
                .push(id.clone());
        }
        for ids in groups.values_mut().filter(|ids| ids.len() > 1) {
            ids.sort_by_key(|id| {
                let d = &self.graph.diagnostics[id];
                (
                    d.created
                        .as_deref()
                        .and_then(|s| s.strip_prefix('g'))
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0),
                    id.clone(),
                )
            });
            let keep = &ids[0];
            let mut d = self.graph.diagnostics[keep].clone();
            for id in &ids[1..] {
                d.subjects
                    .extend(self.graph.diagnostics[id].subjects.clone());
                let old = self.graph.diagnostics.get_mut(id).unwrap();
                old.lifecycle = "resolved".into();
                old.updated = Some(build.into());
                ops.push(Op::ResolveDiagnostic {
                    id: id.clone(),
                    reason: format!("combined into {}", keep),
                });
            }
            d.subjects.sort();
            d.subjects.dedup();
            self.refresh_criterion_proposal(&mut d, build);
            self.graph.diagnostics.insert(keep.clone(), d.clone());
            ops.push(Op::ReportDiagnostic {
                id: keep.clone(),
                diagnostic: d,
            });
        }
        ops
    }

    fn refresh_criterion_proposal(&self, d: &mut Diagnostic, build: &str) {
        let criteria: Vec<_> = d
            .subjects
            .iter()
            .filter_map(|id| self.graph.criteria.get(id))
            .collect();
        let text = criteria
            .iter()
            .map(|c| c.statement.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let count = criteria.len();
        d.message = format!(
            "Document {} proposed acceptance {} for one requirement.",
            count,
            if count == 1 { "criterion" } else { "criteria" }
        );
        d.reasoning = Some(
            criteria
                .iter()
                .filter_map(|c| match &c.provenance {
                    Some(Provenance::Derived { reasoning, .. }) => {
                        Some(format!("{}: {}", c.statement, reasoning))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
        if let Some(p) = &mut d.prompt {
            p.question = "Add these acceptance criteria to the document? Review the proposed text before accepting.".into();
            p.freeform = true;
            if let Some(e) = p.options.iter_mut().find_map(|o| o.edit.as_mut()) {
                e.new_text = text;
            }
        }
        d.updated = Some(build.into());
    }

    // ---- commit ----

    // Resolve the open ratification-pending diagnostics on a fact, returning the
    // journaled resolutions.
    pub(super) fn resolve_ratification_diags(
        &mut self,
        subject: &str,
        build: &str,
        reason: &str,
    ) -> Vec<Op> {
        let mut out = Vec::new();
        let mut refresh = Vec::new();
        for (did, d) in self.graph.diagnostics.iter_mut() {
            if d.lifecycle == "open"
                && d.rule == "ratification-pending"
                && d.subjects.iter().any(|s| s == subject)
            {
                if d.subjects.len() > 1 {
                    d.subjects.retain(|s| s != subject);
                    refresh.push((did.clone(), d.clone()));
                } else {
                    d.lifecycle = "resolved".to_string();
                    out.push(Op::ResolveDiagnostic {
                        id: did.clone(),
                        reason: reason.to_string(),
                    });
                }
                d.updated = Some(build.to_string());
            }
        }
        for (id, mut d) in refresh {
            if d.subjects
                .iter()
                .all(|s| self.graph.criteria.contains_key(s))
            {
                self.refresh_criterion_proposal(&mut d, build);
                self.graph.diagnostics.insert(id.clone(), d.clone());
                out.push(Op::ReportDiagnostic { id, diagnostic: d });
            }
        }
        out
    }

    // ---- document sync (the dirty set) ----
}
