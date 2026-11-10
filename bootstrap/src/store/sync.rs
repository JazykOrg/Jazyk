// Document sync: hashing parsed documents against the stored records, alignment,
// and stale anchors. Mirrors docs/compiler/alignment.md.
use super::*;

impl Store {
    // An anchor-stale record on a section, its anchors merged with any earlier record
    // on the same section.
    pub(super) fn push_stale_anchor(
        &self,
        batch: &mut RecordBatch,
        mutation: usize,
        section: &str,
        anchor: &str,
    ) {
        let mut anchors: Vec<String> = self
            .status
            .changes
            .iter()
            .filter(|c| c.kind == CHANGE_ANCHOR_STALE && c.subject == section)
            .flat_map(|c| c.detail["anchors"].as_array().cloned().unwrap_or_default())
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if !anchors.contains(&anchor.to_string()) {
            anchors.push(anchor.to_string());
        }
        anchors.sort();
        if let Some(existing) = batch
            .records
            .iter_mut()
            .find(|c| c.kind == CHANGE_ANCHOR_STALE && c.subject == section)
        {
            let mut merged: Vec<String> = existing.detail["anchors"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            for a in anchors {
                if !merged.contains(&a) {
                    merged.push(a);
                }
            }
            merged.sort();
            existing.detail = serde_json::json!({ "anchors": merged });
            return;
        }
        batch.push(
            mutation,
            CHANGE_ANCHOR_STALE,
            section,
            "quote",
            serde_json::json!({ "anchors": anchors }),
        );
    }

    // Bring the stored document records in line with a fresh parse. Returns the dirty work.
    // Alignment matches the trees (docs/compiler/alignment.md): exact moves rewrite
    // anchored references mechanically and are not dirty; every other relocation is a
    // proposal persisted for the place-anchors session; an anchor with no candidate is
    // stale. Coverage carries over only for unchanged and exactly moved sections. A save
    // that dirtied sections journals an `edit` entry of its own; the moves and proposals
    // journal an `align` entry. Both write their change records.
    // Mirrors docs/compiler/reconciler.md#dirty-set.
    pub fn sync_docs(&mut self, parsed: &crate::reconcile::ParsedDocs) -> Vec<DirtyDoc> {
        self.sync_docs_inner(parsed, true)
    }

    // The same alignment without a write: the section trees, the dirty set, the
    // alignment blocks, and the records land in memory and the `edit` and `align`
    // entries are minted but never written, so a read-only consumer (status, preview,
    // the compile release, monitor, the GUI board) derives from the documents as they
    // stand while the entries stay a build's to write (the generate release writes
    // them, since it approves a generation number: control.rs).
    // Mirrors docs/compiler/reconciler.md#goal-derivation.
    pub fn sync_docs_in_memory(&mut self, parsed: &crate::reconcile::ParsedDocs) -> Vec<DirtyDoc> {
        self.sync_docs_inner(parsed, false)
    }

    fn sync_docs_inner(
        &mut self,
        parsed: &crate::reconcile::ParsedDocs,
        persist: bool,
    ) -> Vec<DirtyDoc> {
        use crate::align::Full;
        // Persisting alignment is a write like a commit: under the lock, against the
        // generation it loaded. Mirrors docs/compiler/graph.md#concurrency.
        let _flock = persist.then(|| FileLock::acquire(&self.out));
        let persist = persist && self.refuse("align").is_none();
        let mut entries: Vec<JournalEntry> = Vec::new();
        let mut removed_docs: Vec<String> = Vec::new();
        let changed: BTreeSet<String> = self
            .docs
            .iter()
            .filter(|(d, rec)| {
                parsed
                    .get(*d)
                    .map(|(h, _)| h != &rec.content_hash)
                    .unwrap_or(true)
            })
            .map(|(d, _)| d.clone())
            .chain(
                parsed
                    .keys()
                    .filter(|d| !self.docs.contains_key(*d))
                    .cloned(),
            )
            .collect();
        let mut out = Vec::new();
        if changed.is_empty() {
            return self.reevaluate_items(out);
        }
        let plan = crate::align::align(&self.docs, parsed, &self.graph, &self.align);

        // Exact moves apply now: references follow, coverage follows.
        let mut carried: BTreeMap<Full, Coverage> = BTreeMap::new();
        for (from, to) in &plan.exact_moves {
            if let Some(c) = self
                .docs
                .get(&from.doc)
                .and_then(|r| r.coverage.get(&from.section))
            {
                carried.insert(to.clone(), c.clone());
            }
            self.rewrite_section_refs(from, to);
        }
        // A rename carries its anchors the same way and its coverage not at all: the
        // section is dirty as an edit and the session that reads the new heading marks
        // it again. Mirrors docs/compiler/alignment.md#what-applies-and-what-is-proposed.
        for (from, to) in &plan.renames {
            self.rewrite_section_refs(from, to);
        }
        let moved_from: BTreeSet<&Full> = plan
            .exact_moves
            .iter()
            .chain(plan.renames.iter())
            .map(|(f, _)| f)
            .collect();
        let moved_to: BTreeSet<&Full> = plan.exact_moves.iter().map(|(_, t)| t).collect();
        // Anchors the model will place are not stale: keyed by id plus old location,
        // since an entity can hold several mentions.
        let proposed: BTreeSet<(String, String)> = plan
            .proposals
            .iter()
            .map(|p| (p.anchor.clone(), p.from.clone()))
            .collect();
        let is_proposed = |id: &str, at: &Full| proposed.contains(&(id.to_string(), at.render()));
        // An entity whose mentions in a section all coincide with requirement sources
        // there follows those requirements (placed with them, or pruned by the sweep
        // when they go); it is no anchor of its own.
        let derived_only = |id: &str, at: &Full| -> bool {
            let Some(e) = self.graph.entities.get(id) else {
                return false;
            };
            let sources: BTreeSet<&str> = self
                .graph
                .requirements
                .values()
                .filter_map(|r| r.source.as_ref())
                .filter(|s| s.doc == at.doc && s.section == at.section)
                .map(|s| s.quote.as_str())
                .collect();
            e.mentions
                .iter()
                .filter(|m| m.doc == at.doc && m.section == at.section)
                .all(|m| sources.contains(m.quote.as_str()))
        };
        let covered = |id: &str, at: &Full| {
            is_proposed(id, at) || (id.starts_with("ent:") && derived_only(id, at))
        };

        // (section reference, anchor id): what the edit left stale, for the records.
        let mut stale_at: Vec<(String, String)> = Vec::new();
        let mut removed_refs: Vec<String> = Vec::new();
        let mut dirty_refs: Vec<String> = Vec::new();
        // The line diff each dirtied section carries in its record, against its stored
        // body. Mirrors docs/compiler/goals/reconcile-section.md#created-when.
        let mut dirty_detail: BTreeMap<String, serde_json::Value> = BTreeMap::new();

        // Documents that disappeared from the project entirely.
        let gone: Vec<String> = self
            .docs
            .keys()
            .filter(|d| !parsed.contains_key(*d))
            .cloned()
            .collect();
        for doc in gone {
            let mut stale: Vec<String> = Vec::new();
            if let Some(rec) = self.docs.get(&doc) {
                for r in rec.sections.keys() {
                    let at = Full::new(&doc, r);
                    if moved_from.contains(&at) {
                        continue;
                    }
                    removed_refs.push(at.render());
                    for a in self
                        .anchors_in_doc(&doc, Some(r))
                        .into_iter()
                        .filter(|a| !covered(a, &at))
                    {
                        stale_at.push((at.render(), a.clone()));
                        stale.push(a);
                    }
                }
            }
            self.docs.remove(&doc);
            removed_docs.push(doc.clone());
            stale.sort();
            stale.dedup();
            if !stale.is_empty() {
                out.push(DirtyDoc {
                    doc,
                    dirty_sections: Vec::new(),
                    stale_anchors: stale,
                });
            }
        }
        for (doc, (content_hash, sections)) in parsed {
            if !changed.contains(doc) {
                continue;
            }
            let old = self.docs.get(doc).cloned().unwrap_or_default();
            // Dirty: new or changed sections (an unchanged or exactly moved one is neither).
            let mut dirty: Vec<String> = sections
                .keys()
                .filter(|r| {
                    let at = Full::new(doc, r);
                    !plan.unchanged.contains(&at) && !moved_to.contains(&at)
                })
                .cloned()
                .collect();
            // Removed: old sections gone from the new parse (excluding exact moves).
            let removed: Vec<String> = old
                .sections
                .keys()
                .filter(|r| !sections.contains_key(*r) && !moved_from.contains(&Full::new(doc, r)))
                .cloned()
                .collect();
            let mut stale = Vec::new();
            for r in &removed {
                let at = Full::new(doc, r);
                removed_refs.push(at.render());
                for a in self
                    .anchors_in_doc(doc, Some(r))
                    .into_iter()
                    .filter(|a| !covered(a, &at))
                {
                    stale_at.push((at.render(), a.clone()));
                    stale.push(a);
                }
            }
            // Also stale: anchors whose section changed and whose quote no longer
            // locates, requirement sources and entity mentions alike.
            for r in &dirty {
                let at = Full::new(doc, r);
                dirty_refs.push(at.render());
                let Some(sec) = sections.get(r) else { continue };
                if let Some(prev) = old.sections.get(r) {
                    dirty_detail.insert(at.render(), section_diff_detail(prev, &sec.raw));
                }
                for a in self.anchors_in_doc(doc, Some(r)) {
                    if covered(&a, &at) {
                        continue;
                    }
                    let ok = match self.graph.requirements.get(&a) {
                        Some(q) => q
                            .source
                            .as_ref()
                            .map(|s| text_contains(&sec.raw, &s.quote))
                            .unwrap_or(true),
                        None => self
                            .graph
                            .entities
                            .get(&a)
                            .map(|e| {
                                e.mentions
                                    .iter()
                                    .filter(|m| &m.doc == doc && m.section == *r)
                                    .all(|m| text_contains(&sec.raw, &m.quote))
                            })
                            .unwrap_or(true),
                    };
                    if !ok {
                        stale_at.push((at.render(), a.clone()));
                        stale.push(a);
                    }
                }
            }
            stale.sort();
            stale.dedup();

            // Carry coverage for unchanged sections and exact-move targets.
            let mut coverage = BTreeMap::new();
            for (r, c) in &old.coverage {
                if plan.unchanged.contains(&Full::new(doc, r)) {
                    coverage.insert(r.clone(), c.clone());
                }
            }
            for (to, c) in &carried {
                if &to.doc == doc {
                    coverage.insert(to.section.clone(), c.clone());
                }
            }
            self.docs.insert(
                doc.clone(),
                DocRecord {
                    content_hash: content_hash.clone(),
                    sections: sections.clone(),
                    coverage,
                },
            );
            if !dirty.is_empty() || !stale.is_empty() {
                dirty.sort();
                out.push(DirtyDoc {
                    doc: doc.clone(),
                    dirty_sections: dirty,
                    stale_anchors: stale,
                });
            }
        }

        // Proposals persist per target document, replacing any earlier block for a
        // document that changed again or that receives new proposals.
        let mut blocks: BTreeMap<String, Vec<AnchorProposal>> = BTreeMap::new();
        for p in &plan.proposals {
            blocks
                .entry(crate::align::target_doc(p))
                .or_default()
                .push(p.clone());
        }
        self.status
            .alignment
            .retain(|b| !changed.contains(&b.doc) && !blocks.contains_key(&b.doc));
        for (doc, proposals) in blocks {
            let touches = |r: &str| split_section_ref(r).map(|(d, _)| d == doc).unwrap_or(false);
            let changes: Vec<SectionOp> = plan
                .ops
                .iter()
                .filter(|o| o.from.iter().chain(o.to.iter()).any(|r| touches(r)))
                .cloned()
                .collect();
            self.status.alignment.push(DocAlignment {
                doc,
                changes,
                proposals,
            });
        }
        self.status.alignment.sort_by(|a, b| a.doc.cmp(&b.doc));

        // The human save is a generation of its own: one mutation per dirtied or removed
        // section, with the section records. Mirrors docs/compiler/graph.md#journal.
        dirty_refs.sort();
        dirty_refs.dedup();
        removed_refs.sort();
        removed_refs.dedup();
        if !dirty_refs.is_empty() || !removed_refs.is_empty() {
            self.status.generation += 1;
            let generation = self.status.generation;
            let build = format!("g{}", generation);
            let mut batch = RecordBatch::new(generation);
            let mut mutations = Vec::new();
            for (i, full) in dirty_refs.iter().chain(removed_refs.iter()).enumerate() {
                let removed = i >= dirty_refs.len();
                let (doc, section) = split_section_ref(full).unwrap_or_default();
                let kind = if removed {
                    CHANGE_SECTION_REMOVED
                } else {
                    CHANGE_SECTION_DIRTY
                };
                mutations.push(serde_json::json!({"op": kind, "doc": doc, "section": section}));
                let mut detail = serde_json::json!({"doc": doc, "section": section});
                if let Some(serde_json::Value::Object(diff)) = dirty_detail.get(full) {
                    for (k, v) in diff {
                        detail[k] = v.clone();
                    }
                }
                batch.push(i + 1, kind, full, "section", detail);
            }
            let mut entry = self.journal_entry(&build, &Commit::store("edit"), mutations);
            entry.dirtied = dirty_refs
                .iter()
                .chain(removed_refs.iter())
                .cloned()
                .collect();
            entries.push(entry);
            self.commit_records(batch);
        }

        // Mechanical moves and renames, and the proposals left pending: one align entry
        // per build.
        if !plan.exact_moves.is_empty() || !plan.renames.is_empty() || !plan.proposals.is_empty() {
            self.status.generation += 1;
            let build = format!("g{}", self.status.generation);
            let mut mutations: Vec<serde_json::Value> = plan
                .exact_moves
                .iter()
                .map(|(f, t)| serde_json::json!({"op": "move", "from": f.render(), "to": t.render()}))
                .collect();
            mutations.extend(plan.renames.iter().map(
                |(f, t)| serde_json::json!({"op": "rename", "from": f.render(), "to": t.render()}),
            ));
            mutations.extend(plan.proposals.iter().map(|p| {
                serde_json::json!({"op": "propose", "anchor": p.anchor, "from": p.from,
                    "candidates": p.candidates.iter().map(|c| c.section.clone()).collect::<Vec<_>>()})
            }));
            let entry = self.journal_entry(&build, &Commit::store("align"), mutations);
            entries.push(entry);
        }
        // Stale anchors and pending proposals, under the last generation written. That
        // generation may already hold records (the edit batch above, or an earlier
        // commit's), so the batch numbers after them.
        let generation = self.status.generation;
        let mut batch = RecordBatch::after(generation, &self.status.changes);
        for (section, anchor) in &stale_at {
            self.push_stale_anchor(&mut batch, 0, section, anchor);
        }
        for b in &self.status.alignment {
            let anchors: BTreeSet<String> = b.proposals.iter().map(|p| p.anchor.clone()).collect();
            batch.push(
                0,
                CHANGE_ALIGNMENT_PENDING,
                &b.doc,
                "alignment",
                serde_json::json!({ "proposals": anchors }),
            );
        }
        self.commit_records(batch);
        self.prune_records();
        if persist {
            for doc in &removed_docs {
                crate::vfs::remove_file(self.out.join("docs").join(format!("{}.yaml", doc))).ok();
            }
            for entry in &entries {
                self.write_journal(entry);
            }
            // Persist the synced records so context reads see the new sections.
            self.persist();
        }
        self.reevaluate_items(out)
    }

    // Anchors a place-anchors session flagged for re-evaluation ride on their document's
    // reconcile item as stale anchors, their section dirty, whether or not the document
    // changed this build. Mirrors docs/compiler/reconciler.md#dirty-set.
    fn reevaluate_items(&self, mut out: Vec<DirtyDoc>) -> Vec<DirtyDoc> {
        for doc in self.docs.keys() {
            let (stale, sections) = self.stale_extras(doc);
            if stale.is_empty() {
                continue;
            }
            let item = match out.iter_mut().find(|d| &d.doc == doc) {
                Some(d) => d,
                None => {
                    out.push(DirtyDoc {
                        doc: doc.clone(),
                        dirty_sections: Vec::new(),
                        stale_anchors: Vec::new(),
                    });
                    out.last_mut().unwrap()
                }
            };
            for a in stale {
                if !item.stale_anchors.contains(&a) {
                    item.stale_anchors.push(a);
                }
            }
            for sec in sections {
                if !item.dirty_sections.contains(&sec) {
                    item.dirty_sections.push(sec);
                }
            }
            item.dirty_sections.sort();
        }
        out
    }

    // Requirements of one document that owe the reconcile session a decision: the quote
    // no longer locates, or a place-anchors session flagged the anchor for re-evaluation.
    // Returns the ids and the existing sections they sit in (to be dirtied).
    pub fn stale_extras(&self, doc: &str) -> (Vec<String>, Vec<String>) {
        let mut stale: Vec<String> = Vec::new();
        let mut sections: Vec<String> = Vec::new();
        let rec = self.docs.get(doc);
        // An anchor with a pending proposal is the align session's to place, not stale.
        let proposed = self.proposed_anchors();
        for (rid, r) in &self.graph.requirements {
            let Some(src) = r.source.as_ref() else {
                continue;
            };
            if src.doc != doc || proposed.contains(rid) {
                continue;
            }
            let unlocated = !self.quote_locates(&src.doc, &src.section, &src.quote);
            if unlocated || self.status.reevaluate.iter().any(|x| x == rid) {
                stale.push(rid.clone());
                if rec
                    .map(|x| x.sections.contains_key(&src.section))
                    .unwrap_or(false)
                    && !sections.contains(&src.section)
                {
                    sections.push(src.section.clone());
                }
            }
        }
        (stale, sections)
    }

    // Anchors named by a pending alignment proposal: the model will place them, so the
    // store must not reap them in between.
    pub(super) fn proposed_anchors(&self) -> BTreeSet<String> {
        self.status
            .alignment
            .iter()
            .flat_map(|b| b.proposals.iter().map(|p| p.anchor.clone()))
            .collect()
    }

    // Node ids anchored to a document (optionally to one section of it).
    fn anchors_in_doc(&self, doc: &str, section: Option<&str>) -> Vec<String> {
        let mut out = Vec::new();
        for (id, r) in &self.graph.requirements {
            let Some(src) = r.source.as_ref() else {
                continue;
            };
            if src.doc == doc && section.map(|s| src.section == s).unwrap_or(true) {
                out.push(id.clone());
            }
        }
        for (id, e) in &self.graph.entities {
            if e.mentions
                .iter()
                .any(|m| m.doc == doc && section.map(|s| m.section == s).unwrap_or(true))
            {
                out.push(id.clone());
            }
        }
        out
    }

    // Mechanically rewrite anchored references when a section moved, within a document
    // or across documents.
    fn rewrite_section_refs(&mut self, from: &crate::align::Full, to: &crate::align::Full) {
        let moved = |s: &mut SourceRef| {
            if s.doc == from.doc && s.section == from.section {
                s.doc = to.doc.clone();
                s.section = to.section.clone();
            }
        };
        for src in self
            .graph
            .requirements
            .values_mut()
            .filter_map(|r| r.source.as_mut())
        {
            moved(src);
        }
        for e in self.graph.entities.values_mut() {
            for m in e.mentions.iter_mut() {
                moved(m);
            }
            for a in e.attributes.iter_mut() {
                if let Provenance::Quote(s) = &mut a.provenance {
                    moved(s);
                }
            }
        }
    }

    // ---- garbage collection ----
}
