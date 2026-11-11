// Deletion propagation, record pruning, the settle passes, the sweep, gc, and check
// diagnostics. Mirrors docs/compiler/graph.md#the-sweep.
use super::*;

impl Store {
    // Deleting nodes settles the open judged diagnostics naming them: all subjects gone
    // resolves the diagnostic in place (the returned ops go to the journal); a surviving
    // subject gets a node-deleted record, so a session re-judges the finding. Runs on
    // every deleting commit, session and sweep alike.
    // Mirrors docs/compiler/graph.md#the-sweep.
    pub(super) fn propagate_deletions(
        &mut self,
        deleted: &BTreeMap<String, usize>,
        build: &str,
        batch: &mut RecordBatch,
    ) -> Vec<Op> {
        let mut resolved = Vec::new();
        if deleted.is_empty() {
            return resolved;
        }
        let hit: Vec<String> = self
            .graph
            .diagnostics
            .iter()
            .filter(|(_, d)| {
                d.lifecycle == "open"
                    && JUDGED_RULES.contains(&d.rule.as_str())
                    && d.subjects.iter().any(|s| deleted.contains_key(s))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for did in hit {
            let subjects = self.graph.diagnostics[&did].subjects.clone();
            let gone: Vec<&String> = subjects
                .iter()
                .filter(|s| deleted.contains_key(*s))
                .collect();
            let mutation = gone.iter().map(|s| deleted[*s]).min().unwrap_or(0);
            let survivors: Vec<String> = subjects
                .iter()
                .map(|s| self.resolve_id(s).to_string())
                .filter(|s| self.node_exists(s))
                .collect();
            if survivors.is_empty() {
                let d = self.graph.diagnostics.get_mut(&did).unwrap();
                d.lifecycle = "resolved".to_string();
                d.updated = Some(build.to_string());
                resolved.push(Op::ResolveDiagnostic {
                    id: did,
                    reason: format!("every subject was deleted ({})", subjects.join(", ")),
                });
            } else {
                for s in survivors {
                    batch.push(
                        mutation,
                        CHANGE_NODE_DELETED,
                        &s,
                        "subjects",
                        serde_json::json!({
                            "deleted": gone.iter().map(|g| g.as_str()).collect::<Vec<_>>(),
                            "diagnostic": did,
                        }),
                    );
                }
            }
        }
        resolved
    }

    // Records for the live nodes that still reference dead ones: a curated view's
    // member (view-member-gone), a derived node's `from` (node-deleted).
    // Mirrors docs/compiler/compilation.md#edit-paths.
    pub(super) fn deletion_ripple(
        &self,
        deleted: &BTreeMap<String, usize>,
        batch: &mut RecordBatch,
    ) {
        if deleted.is_empty() {
            return;
        }
        let dead = |ids: &[String]| -> Vec<String> {
            ids.iter()
                .filter(|m| deleted.contains_key(*m))
                .cloned()
                .collect()
        };
        let mutation_of = |ids: &[String]| ids.iter().map(|m| deleted[m]).min().unwrap_or(0);
        for (vid, v) in &self.graph.views {
            if v.default {
                continue;
            }
            // `via` names the list the dead node sat in, the first of members,
            // collapse, excluded that lost one. Mirrors docs/compiler/goals/retrace.md.
            let excluded: Vec<String> = v.excluded.iter().map(|x| x.id.clone()).collect();
            let lists = [
                ("members", dead(&v.members)),
                ("collapse", dead(&v.collapse)),
                ("excluded", dead(&excluded)),
            ];
            let Some(via) = lists.iter().find(|(_, g)| !g.is_empty()).map(|(l, _)| *l) else {
                continue;
            };
            let mut gone: Vec<String> = lists.iter().flat_map(|(_, g)| g.clone()).collect();
            gone.sort();
            gone.dedup();
            batch.push(
                mutation_of(&gone),
                CHANGE_VIEW_MEMBER_GONE,
                vid,
                via,
                serde_json::json!({ "gone": gone }),
            );
        }
        let from_of = |p: &Option<Provenance>| -> Vec<String> {
            match p {
                Some(Provenance::Derived { from, .. }) => from.clone(),
                _ => Vec::new(),
            }
        };
        let reqs = self
            .graph
            .requirements
            .iter()
            .map(|(id, r)| (id, from_of(&r.provenance)));
        let ents = self
            .graph
            .entities
            .iter()
            .map(|(id, e)| (id, from_of(&e.provenance)));
        for (id, from) in reqs.chain(ents) {
            let gone = dead(&from);
            if !gone.is_empty() {
                batch.push(
                    mutation_of(&gone),
                    CHANGE_NODE_DELETED,
                    id,
                    "from",
                    serde_json::json!({ "deleted": gone }),
                );
            }
        }
    }

    // The lookalike memory and the name index, settled at every commit. A `separate`
    // verdict (a resolved `dedupe-candidates` goal, or one entry of a resolved
    // `review-entity` goal's `lookalikes` evidence) writes the pair's entry with the
    // formula's value over the graph as it stands and clears the pair's record. An
    // entry whose pair lost an id drops. Then the name index: every scored pair with no
    // standing verdict takes a `lookalike` record, a remembered pair whose score moved
    // drops its entry and takes one too, and a record whose pair is no longer pending
    // clears. Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
    pub(super) fn settle_lookalikes(
        &mut self,
        resolved: &[Resolved],
        generation: u64,
        batch: &mut RecordBatch,
    ) {
        use crate::goals::{
            lookalike_pair_score, pair_members, pair_target, pending_lookalikes, CHANGE_LOOKALIKE,
        };
        let is_separate = |v: &serde_json::Value| {
            v.as_str().or_else(|| v["verdict"].as_str()) == Some(LOOKALIKE_SEPARATE)
        };
        let mut kept: Vec<(String, String)> = Vec::new();
        for r in resolved {
            if let Some(target) = r.goal.strip_prefix("g:dedupe-candidates:") {
                if let Some((a, b)) = pair_members(target) {
                    if is_separate(&r.evidence) {
                        kept.push((a.to_string(), b.to_string()));
                    }
                }
            } else if let Some(target) = r.goal.strip_prefix("g:review-entity:") {
                for item in r.evidence["lookalikes"].as_array().into_iter().flatten() {
                    if let Some(other) = item["id"].as_str() {
                        if is_separate(item) {
                            kept.push((target.to_string(), other.to_string()));
                        }
                    }
                }
            }
        }
        for (a, b) in kept {
            let (a, b) = (
                self.resolve_id(&a).to_string(),
                self.resolve_id(&b).to_string(),
            );
            if a == b
                || !self.graph.entities.contains_key(&a)
                || !self.graph.entities.contains_key(&b)
            {
                continue;
            }
            let pair = pair_target(&a, &b);
            let score = lookalike_pair_score(self, &a, &b);
            self.status.lookalikes.retain(|l| l.pair != pair);
            self.status.lookalikes.push(LookalikeVerdict {
                pair: pair.clone(),
                verdict: LOOKALIKE_SEPARATE.to_string(),
                score,
                generation,
            });
            self.status.clear_changes(&[CHANGE_LOOKALIKE], &pair);
        }
        let entities = &self.graph.entities;
        self.status.lookalikes.retain(|l| {
            pair_members(&l.pair)
                .is_some_and(|(a, b)| entities.contains_key(a) && entities.contains_key(b))
        });
        self.status.lookalikes.sort_by(|x, y| x.pair.cmp(&y.pair));
        let pending = pending_lookalikes(self);
        let mut subjects: BTreeSet<String> = BTreeSet::new();
        for l in &pending {
            let pair = pair_target(&l.a, &l.b);
            subjects.insert(pair.clone());
            // A remembered verdict the pending set let through scored differently:
            // the pair is judged again over the new evidence.
            self.status.lookalikes.retain(|v| v.pair != pair);
            if !self.status.has_change(CHANGE_LOOKALIKE, &pair) {
                batch.push(
                    0,
                    CHANGE_LOOKALIKE,
                    &pair,
                    "lookalike",
                    serde_json::json!({
                        "score": l.score, "tokens": l.tokens, "documents": l.documents,
                        "shared": l.shared,
                        "mentions": {&l.a: l.docs_a, &l.b: l.docs_b},
                    }),
                );
            }
        }
        self.status
            .changes
            .retain(|c| c.kind != CHANGE_LOOKALIKE || subjects.contains(&c.subject));
    }

    // Append the partner to `detail.judged` on both sides' created and revised
    // records for every resolved rejudge-pair goal. A goal of another kind, or one
    // whose target is no pair, marks nothing.
    pub(super) fn mark_pairs_judged(&mut self, resolved: &[Resolved]) {
        for r in resolved {
            let Some(target) = r.goal.strip_prefix("g:rejudge-pair:") else {
                continue;
            };
            let Some((a, b)) = crate::goals::pair_members(target) else {
                continue;
            };
            self.mark_pair_judged(a, b);
        }
    }

    // An edit option's answer is the judgment of the pair its diagnostic concerned:
    // a diagnostic this commit answered `applied` and resolved, whose subjects hold
    // two requirements, marks them judged against each other. Runs after the commit
    // lands its records, so the revision the edit caused carries the pair too and
    // re-judges only its other neighbors.
    // Mirrors docs/compiler/model/diagnostic.md#answers.
    pub(super) fn mark_answered_pairs_judged(&mut self, applied: &[Op]) {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for op in applied {
            let Op::AnswerDiagnostic { id, answer } = op else {
                continue;
            };
            if answer.status != "applied" {
                continue;
            }
            let Some(d) = self.graph.diagnostics.get(id) else {
                continue;
            };
            if d.lifecycle != "resolved" {
                continue;
            }
            let mut reqs: Vec<String> = Vec::new();
            for sub in &d.subjects {
                let sid = self.resolve_id(sub).to_string();
                if self.graph.requirements.contains_key(&sid) && !reqs.contains(&sid) {
                    reqs.push(sid);
                }
            }
            for (i, a) in reqs.iter().enumerate() {
                for b in &reqs[i + 1..] {
                    pairs.push((a.clone(), b.clone()));
                }
            }
        }
        for (a, b) in pairs {
            self.mark_pair_judged(&a, &b);
        }
    }

    // Append each side to `detail.judged` on every created and revised record of the
    // other. Mirrors docs/compiler/graph.md#change-records.
    fn mark_pair_judged(&mut self, a: &str, b: &str) {
        for c in self.status.changes.iter_mut() {
            if c.kind != CHANGE_REQ_CREATED && c.kind != CHANGE_REQ_REVISED {
                continue;
            }
            if c.subject == a {
                c.add_judged(b);
            } else if c.subject == b {
                c.add_judged(a);
            }
        }
    }

    // Drop records whose evidence lapsed or whose subject no longer exists (the trail
    // kinds excepted). Mirrors docs/compiler/reconciler.md#change-records.
    pub(super) fn prune_records(&mut self) {
        // The scorer's inputs, computed once for the whole pass and only when a
        // created or revised record needs them (docs/compiler/reconciler.md#pairs).
        let scoring: std::cell::OnceCell<PairScoring> = std::cell::OnceCell::new();
        let alive: Vec<bool> = self
            .status
            .changes
            .iter()
            .map(|c| self.record_stands(c, &scoring))
            .collect();
        let mut i = 0;
        self.status.changes.retain(|_| {
            let keep = alive[i];
            i += 1;
            keep
        });
    }

    fn record_stands(&self, c: &ChangeRecord, scoring: &std::cell::OnceCell<PairScoring>) -> bool {
        if TRAIL_KINDS.contains(&c.kind.as_str()) {
            return true;
        }
        let section_exists = |full: &str| {
            split_section_ref(full)
                .map(|(d, s)| {
                    self.docs
                        .get(&d)
                        .is_some_and(|r| r.sections.contains_key(&s))
                })
                .unwrap_or(false)
        };
        match c.kind.as_str() {
            CHANGE_SECTION_DIRTY => section_exists(&c.subject),
            // A created or revised record stands while a candidate pair remains
            // (neighbors and sticky partners minus the judged partners), open or
            // waiting under the cap. Mirrors docs/compiler/reconciler.md#pairs.
            CHANGE_REQ_CREATED | CHANGE_REQ_REVISED => {
                self.graph.requirements.contains_key(&c.subject)
                    && !self
                        .pair_review_candidates_with(c, scoring.get_or_init(|| self.pair_scoring()))
                        .is_empty()
            }
            CHANGE_ANCHOR_STALE => c.detail["anchors"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .any(|id| self.node_exists(id))
                })
                .unwrap_or(false),
            CHANGE_ALIGNMENT_PENDING => self.status.alignment.iter().any(|b| b.doc == c.subject),
            CHANGE_EDGES_MISSING => self
                .graph
                .requirements
                .get(&c.subject)
                .is_some_and(|r| r.entities.len() >= 2 && r.edges.is_empty()),
            CHANGE_PROMPT_UNANSWERED => {
                self.graph.diagnostics.get(&c.subject).is_some_and(|d| {
                    d.lifecycle == "open" && d.prompt.is_some() && d.answer.is_none()
                })
            }
            CHANGE_PROVENANCE_PENDING => self
                .graph
                .requirements
                .get(&c.subject)
                .map(|r| {
                    r.provenance.as_ref().is_some_and(provenance_is_pending) && r.source.is_none()
                })
                .or_else(|| {
                    self.graph
                        .entities
                        .get(&c.subject)
                        .map(|e| e.provenance.as_ref().is_some_and(provenance_is_pending))
                })
                .unwrap_or(false),
            // Both ids live; the name index settles the rest at every commit.
            crate::goals::CHANGE_LOOKALIKE => {
                crate::goals::pair_members(&c.subject).is_some_and(|(a, b)| {
                    self.graph.entities.contains_key(a) && self.graph.entities.contains_key(b)
                })
            }
            CHANGE_VIEW_MEMBER_GONE => self.graph.views.get(&c.subject).is_some_and(|v| {
                v.members
                    .iter()
                    .chain(v.collapse.iter())
                    .any(|m| !self.node_exists(self.resolve_id(m)))
            }),
            // A scope root stands while the scope holds any entity.
            _ if c.subject.starts_with(SCOPE_ROOT_PREFIX) => {
                let scope = &c.subject[SCOPE_ROOT_PREFIX.len()..];
                self.graph.entities.values().any(|e| e.scope == scope)
            }
            _ => match id_kind(&c.subject) {
                "requirement" => self.graph.requirements.contains_key(&c.subject),
                "entity" => self.graph.entities.contains_key(&c.subject),
                "view" => self.graph.views.contains_key(&c.subject),
                "diagnostic" => self.graph.diagnostics.contains_key(&c.subject),
                _ => c.subject.contains('#') || !c.subject.contains(':'),
            },
        }
    }

    // Subjects of open judged diagnostics that no longer exist in the graph.
    fn missing_diag_subjects(&self) -> BTreeSet<String> {
        self.graph
            .diagnostics
            .values()
            .filter(|d| d.lifecycle == "open" && JUDGED_RULES.contains(&d.rule.as_str()))
            .flat_map(|d| d.subjects.iter())
            .filter(|s| s.starts_with("req:") || s.starts_with("ent:"))
            .filter(|s| !self.node_exists(self.resolve_id(s)))
            .cloned()
            .collect()
    }

    // The marker diagnostics whose condition cleared: an incomplete-build marker an
    // earlier store version filed, an uncovered-section marker whose section is marked covered
    // or non-normative, or is gone. Each with the reason its resolution records.
    // Mirrors docs/compiler/graph.md#the-sweep.
    fn cleared_markers(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (id, d) in &self.graph.diagnostics {
            if d.lifecycle != "open" || d.subjects.len() != 1 {
                continue;
            }
            let subject = &d.subjects[0];
            match d.rule.as_str() {
                // A marker an earlier store version filed for a parked goal: parked
                // goals are build progress, not graph state, so it settles.
                "incomplete-build" => {
                    out.push((
                        id.clone(),
                        format!(
                            "{} parked work is build progress, not a diagnostic",
                            subject
                        ),
                    ));
                }
                "uncovered-section" => {
                    let Some((doc, r)) = subject.split_once('#') else {
                        continue;
                    };
                    let rec = self.docs.get(doc);
                    let gone = rec.map(|rec| !rec.sections.contains_key(r)).unwrap_or(true);
                    let marked = rec
                        .and_then(|rec| rec.coverage.get(r))
                        .map(|c| c.state == "covered" || c.state == "non-normative")
                        .unwrap_or(false);
                    if gone {
                        out.push((id.clone(), format!("section {} is gone", subject)));
                    } else if marked {
                        out.push((id.clone(), format!("section {} is covered", subject)));
                    }
                }
                _ => {}
            }
        }
        out
    }

    // Whether any open judged diagnostic names a subject the graph no longer holds, or
    // a marker's condition cleared: the signal that the deterministic tail has settling
    // to do.
    pub fn has_dangling_diags(&self) -> bool {
        !self.missing_diag_subjects().is_empty() || !self.cleared_markers().is_empty()
    }

    // The level-triggered settle of the marker diagnostics, run at every sweep so a
    // session never adjudicates a marker whose condition already cleared. Journaled as
    // settle-diagnostics. Mirrors docs/compiler/graph.md#the-sweep.
    pub fn settle_cleared_markers(&mut self) -> Vec<String> {
        let _flock = FileLock::acquire(&self.out);
        if self.refuse("settle").is_some() {
            return Vec::new();
        }
        self.settle_cleared_markers_locked()
    }

    fn settle_cleared_markers_locked(&mut self) -> Vec<String> {
        let cleared = self.cleared_markers();
        if cleared.is_empty() {
            return Vec::new();
        }
        let generation = self.status.generation + 1;
        let build = format!("g{}", generation);
        let mut actions = Vec::new();
        let mut ops = Vec::new();
        for (id, reason) in cleared {
            if let Some(d) = self.graph.diagnostics.get_mut(&id) {
                d.lifecycle = "resolved".to_string();
                d.updated = Some(build.clone());
            }
            actions.push(format!("resolved {} ({})", id, reason));
            ops.push(
                serde_json::to_value(Op::ResolveDiagnostic { id, reason }).unwrap_or_default(),
            );
        }
        self.status.generation = generation;
        let entry = self.journal_entry(&build, &Commit::store("settle-diagnostics"), ops);
        self.write_journal(&entry);
        self.persist();
        actions
    }

    // The level-triggered half of deletion propagation: sweep every open judged
    // diagnostic for missing subjects and settle it, journaled like the sweep.
    // Mirrors docs/compiler/reconciler.md#pairs.
    pub fn settle_dangling_diags(&mut self) -> Vec<String> {
        let _flock = FileLock::acquire(&self.out);
        if self.refuse("settle").is_some() {
            return Vec::new();
        }
        self.settle_dangling_diags_locked()
    }

    fn settle_dangling_diags_locked(&mut self) -> Vec<String> {
        let missing = self.missing_diag_subjects();
        if missing.is_empty() {
            return Vec::new();
        }
        let generation = self.status.generation + 1;
        let build = format!("g{}", generation);
        let mut batch = RecordBatch::new(generation);
        let deleted: BTreeMap<String, usize> = missing.into_iter().map(|m| (m, 0)).collect();
        let ops = self.propagate_deletions(&deleted, &build, &mut batch);
        let actions: Vec<String> = ops
            .iter()
            .filter_map(|o| match o {
                Op::ResolveDiagnostic { id, reason } => {
                    Some(format!("resolved {} ({})", id, reason))
                }
                _ => None,
            })
            .collect();
        if !ops.is_empty() || !batch.is_empty() {
            self.status.generation = generation;
            self.commit_records(batch);
            let entry = self.journal_entry(
                &build,
                &Commit::store("settle-diagnostics"),
                ops.iter()
                    .map(|o| serde_json::to_value(o).unwrap_or_default())
                    .collect(),
            );
            self.write_journal(&entry);
            self.persist();
        }
        actions
    }

    // The deterministic sweep as one unit: the mechanical garbage collection, the
    // settle of judged diagnostics whose subjects are gone, and the settle of marker
    // diagnostics whose condition cleared. Every commit runs it behind its own entry
    // (`apply`); a build runs it after alignment too, before any session, since the
    // `edit` entry is no changeset. Returns one line per action.
    // Mirrors docs/compiler/graph.md#the-sweep.
    pub fn sweep(&mut self) -> Vec<String> {
        let _flock = FileLock::acquire(&self.out);
        if self.refuse("sweep").is_some() {
            return Vec::new();
        }
        self.sweep_locked()
    }

    pub(super) fn sweep_locked(&mut self) -> Vec<String> {
        let mut actions = self.gc();
        actions.extend(self.settle_dangling_diags_locked());
        actions.extend(self.settle_cleared_markers_locked());
        actions
    }

    // The sweep, the mechanical half of garbage collection: quoted requirements whose
    // source section vanished are deleted; mentions and quoted attributes pointing at
    // removed sections are pruned; an entity with zero mentions, zero requirements, zero
    // attributes, zero children, and no derived or decree provenance is deleted with a
    // tombstone. Derived and decreed nodes are never swept. A curated view's dangling
    // member is noted as view-member-gone. Journaled as one entry.
    // Mirrors docs/compiler/graph.md#the-sweep.
    pub fn gc(&mut self) -> Vec<String> {
        let mut actions = Vec::new();
        let generation = self.status.generation + 1;
        let mut batch = RecordBatch::new(generation);
        let proposed = self.proposed_anchors();
        let dead_reqs: Vec<String> = self
            .graph
            .requirements
            .iter()
            .filter(|(id, _)| !proposed.contains(*id))
            .filter(|(_, r)| {
                r.source
                    .as_ref()
                    .map(|s| {
                        !self
                            .docs
                            .get(&s.doc)
                            .map(|d| d.sections.contains_key(&s.section))
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut deleted: BTreeMap<String, usize> = BTreeMap::new();
        for id in dead_reqs {
            self.graph.requirements.remove(&id);
            actions.push(format!("deleted {} (source section gone)", id));
            deleted.insert(id, 0);
        }
        // A criterion goes with its requirement, or with its own section
        // (docs/compiler/graph.md#the-sweep).
        let dead_crits: Vec<(String, &'static str)> = self
            .graph
            .criteria
            .iter()
            .filter_map(|(cid, c)| {
                if !self.graph.requirements.contains_key(&c.requirement) {
                    return Some((cid.clone(), "requirement gone"));
                }
                let section_gone = c.source.as_ref().is_some_and(|s| {
                    !self
                        .docs
                        .get(&s.doc)
                        .map(|d| d.sections.contains_key(&s.section))
                        .unwrap_or(false)
                });
                section_gone.then(|| (cid.clone(), "source section gone"))
            })
            .collect();
        for (cid, why) in dead_crits {
            self.graph.criteria.remove(&cid);
            actions.push(format!("deleted {} ({})", cid, why));
            deleted.insert(cid, 0);
        }
        // Mentions derived from a proposed requirement's source follow it when placed.
        let protected: Vec<SourceRef> = self
            .graph
            .requirements
            .iter()
            .filter(|(id, _)| proposed.contains(*id))
            .filter_map(|(_, r)| r.source.clone())
            .collect();
        for (id, e) in self.graph.entities.iter_mut() {
            if proposed.contains(id) {
                continue;
            }
            let before = e.mentions.len();
            let docs = &self.docs;
            // A mention whose section is gone, or whose quote no longer locates in it,
            // is stale prose: left in place it leaks statements the documents no longer
            // make into later loaded sets.
            e.mentions.retain(|m| {
                protected.contains(m)
                    || docs
                        .get(&m.doc)
                        .and_then(|d| d.sections.get(&m.section))
                        .map(|s| text_contains(&s.raw, &m.quote))
                        .unwrap_or(false)
            });
            if e.mentions.len() < before {
                actions.push(format!(
                    "pruned {} mention(s) on {}",
                    before - e.mentions.len(),
                    id
                ));
            }
            let attrs_before = e.attributes.len();
            e.attributes.retain(|a| match &a.provenance {
                Provenance::Quote(s) => docs
                    .get(&s.doc)
                    .map(|d| d.sections.contains_key(&s.section))
                    .unwrap_or(false),
                _ => true,
            });
            if e.attributes.len() < attrs_before {
                actions.push(format!(
                    "pruned {} attribute(s) on {}",
                    attrs_before - e.attributes.len(),
                    id
                ));
            }
        }
        let parents: BTreeSet<&str> = self
            .graph
            .entities
            .values()
            .filter_map(|e| e.parent.as_deref())
            .collect();
        // The requirements naming each entity answer from one index, not a scan per
        // entity. Mirrors docs/compiler/graph.md#derived-data.
        let orphans: Vec<String> = {
            let _scope = ReadScope::enter(self);
            self.graph
                .entities
                .iter()
                .filter(|(id, e)| {
                    e.mentions.is_empty()
                        && e.attributes.is_empty()
                        && e.provenance.is_none()
                        && !parents.contains(id.as_str())
                        && self.requirements_referencing(id).is_empty()
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in orphans {
            self.graph.entities.remove(&id);
            self.graph.redirects.insert(id.clone(), String::new());
            actions.push(format!("deleted {} (no mentions, no requirements)", id));
            deleted.insert(id, 0);
        }
        let build = format!("g{}", generation);
        // The dissolve rule: a derived grouping with fewer than two children dissolves
        // as dissolve_entity would, journaled as a sweep mutation; below two there is
        // nothing to judge. Mirrors docs/compiler/graph.md#the-sweep.
        let mut child_counts: BTreeMap<String, usize> = BTreeMap::new();
        for e in self.graph.entities.values() {
            if let Some(p) = &e.parent {
                *child_counts.entry(p.clone()).or_insert(0) += 1;
            }
        }
        let under: Vec<String> = {
            let _scope = ReadScope::enter(self);
            self.graph
                .entities
                .keys()
                .filter(|id| {
                    child_counts.get(*id).copied().unwrap_or(0) < 2 && self.is_grouping(id)
                })
                .cloned()
                .collect()
        };
        let mut moves: Vec<(String, Option<String>, Option<String>, usize)> = Vec::new();
        let mut dissolved: Vec<serde_json::Value> = Vec::new();
        for id in under {
            let (parent, children) = self.dissolve(&id, &build);
            for c in &children {
                moves.push((c.clone(), Some(id.clone()), parent.clone(), 0));
            }
            // The move is the parents' business: the grandparent gained the children.
            // Mirrors docs/compiler/goals/review-entity.md#created-when.
            if let (Some(p), false) = (&parent, children.is_empty()) {
                batch.push(0, CHANGE_ENTITY, p, "parent", serde_json::Value::Null);
            }
            actions.push(format!(
                "dissolved {} ({} child(ren) reparented to {})",
                id,
                children.len(),
                parent.as_deref().unwrap_or("the scope root")
            ));
            dissolved.push(
                serde_json::to_value(Op::DissolveEntity {
                    id: id.clone(),
                    reason: "sweep: fewer than two children".into(),
                    parent,
                    children,
                })
                .unwrap_or_default(),
            );
            deleted.insert(id, 0);
        }
        self.record_reparent_flips(&moves, &mut batch);
        for op in self.propagate_deletions(&deleted, &build, &mut batch) {
            if let Op::ResolveDiagnostic { id, reason } = op {
                actions.push(format!("resolved {} ({})", id, reason));
            }
        }
        self.deletion_ripple(&deleted, &mut batch);
        // Level-triggered: a curated view whose member died by any path owes a retrace;
        // `via` names the first list (members, collapse, excluded) holding a dead node.
        let dangling: Vec<(String, &'static str, Vec<String>)> = self
            .graph
            .views
            .iter()
            .filter(|(_, v)| !v.default)
            .filter_map(|(id, v)| {
                let dead = |ids: Vec<&String>| -> Vec<String> {
                    ids.into_iter()
                        .filter(|m| !self.node_exists(self.resolve_id(m)))
                        .cloned()
                        .collect()
                };
                let lists = [
                    ("members", dead(v.members.iter().collect())),
                    ("collapse", dead(v.collapse.iter().collect())),
                    ("excluded", dead(v.excluded.iter().map(|x| &x.id).collect())),
                ];
                let via = lists.iter().find(|(_, g)| !g.is_empty()).map(|(l, _)| *l)?;
                let gone: Vec<String> = lists.iter().flat_map(|(_, g)| g.clone()).collect();
                Some((id.clone(), via, gone))
            })
            .filter(|(id, _, _)| {
                !self.status.has_change(CHANGE_VIEW_MEMBER_GONE, id)
                    && !batch.has(CHANGE_VIEW_MEMBER_GONE, id)
            })
            .collect();
        for (id, via, gone) in dangling {
            batch.push(
                0,
                CHANGE_VIEW_MEMBER_GONE,
                &id,
                via,
                serde_json::json!({ "gone": gone }),
            );
            actions.push(format!(
                "noted dangling member(s) {} on {}",
                gone.join(", "),
                id
            ));
        }
        // The sweep has run: the section-removed trail clears.
        let had_removed = self
            .status
            .changes
            .iter()
            .any(|c| c.kind == CHANGE_SECTION_REMOVED);
        self.status
            .changes
            .retain(|c| c.kind != CHANGE_SECTION_REMOVED);
        if !actions.is_empty() {
            crate::derive::recompute(self, &build, &mut batch);
            self.status.generation = generation;
            for (id, m) in &deleted {
                let kind = match id_kind(id) {
                    "requirement" => CHANGE_REQ_DELETED,
                    "criterion" => CHANGE_CRIT_DELETED,
                    _ => CHANGE_ENTITY_DELETED,
                };
                batch.push(*m, kind, id, "sweep", serde_json::Value::Null);
            }
            self.settle_lookalikes(&[], generation, &mut batch);
            self.prune_records();
            self.commit_records(batch);
            let entry = self.journal_entry(
                &build,
                &Commit::store("gc"),
                actions
                    .iter()
                    .map(|a| serde_json::json!({"op": "gc", "action": a}))
                    .chain(dissolved)
                    .collect(),
            );
            self.write_journal(&entry);
            self.persist();
            // The sweep is a commit like any other: deleted nodes leave the derived
            // views, so the renderer redraws. Mirrors docs/compiler/diagrams.md#rendering.
            crate::render::render_all(self, &self.out);
        } else if had_removed {
            self.save_status();
        }
        actions
    }

    // ---- deterministic check diagnostics ----

    // Reconcile the deterministic findings: new ones are reported, existing ones updated,
    // vanished ones resolved. Keyed by rule plus subjects, like the sticky rule in apply().
    // A run that changed anything is a `checks` generation of its own.
    pub fn reconcile_check_diags(&mut self, findings: Vec<crate::reconcile::Finding>) {
        let _flock = FileLock::acquire(&self.out);
        if self.refuse("checks").is_some() {
            return;
        }
        let generation = self.status.generation + 1;
        let build = format!("g{}", generation);
        let mut seen: BTreeSet<(String, Vec<String>)> = BTreeSet::new();
        let mut mutations: Vec<serde_json::Value> = Vec::new();
        let mut batch = RecordBatch::new(generation);
        for (rule, subjects, severity, message, prompt) in findings {
            seen.insert((rule.clone(), subjects.clone()));
            let existing = self
                .graph
                .diagnostics
                .iter()
                .find(|(_, d)| d.rule == rule && d.subjects == subjects && d.lifecycle == "open")
                .map(|(id, _)| id.clone());
            match existing {
                Some(id) => {
                    let d = self.graph.diagnostics.get_mut(&id).unwrap();
                    let mut changed = false;
                    if d.message != message || d.severity != severity {
                        d.message = message;
                        d.severity = severity;
                        changed = true;
                    }
                    // The question rides along on a finding that never had one and
                    // was never answered; an answered or standing prompt is kept.
                    if d.prompt.is_none() && d.answer.is_none() && prompt.is_some() {
                        d.prompt = prompt;
                        changed = true;
                        batch.push(
                            mutations.len() + 1,
                            CHANGE_PROMPT_UNANSWERED,
                            &id,
                            &rule,
                            serde_json::Value::Null,
                        );
                    }
                    if changed {
                        d.updated = Some(build.clone());
                        mutations.push(serde_json::json!({"op": "report_diagnostic", "id": id,
                            "rule": rule, "subjects": subjects}));
                    }
                }
                None => {
                    let id = self.mint_diag_id(&rule, &subjects, &BTreeSet::new());
                    if prompt.is_some() {
                        batch.push(
                            mutations.len() + 1,
                            CHANGE_PROMPT_UNANSWERED,
                            &id,
                            &rule,
                            serde_json::Value::Null,
                        );
                    }
                    mutations.push(serde_json::json!({"op": "report_diagnostic", "id": id,
                        "rule": rule, "subjects": subjects}));
                    self.graph.diagnostics.insert(
                        id,
                        Diagnostic {
                            goal: None,
                            rule,
                            severity,
                            subjects,
                            message,
                            reasoning: None,
                            lifecycle: "open".to_string(),
                            triage: None,
                            prompt,
                            answer: None,
                            created: Some(build.clone()),
                            updated: Some(build.clone()),
                        },
                    );
                }
            }
        }
        // Deterministic rules whose condition cleared: resolve. A check finding may
        // carry a pair (nondeterministic-transition), but a multi-subject diagnostic
        // under a rule sessions also file (a session's duplicate-requirement pair) is
        // judged work, not the checks' to resolve.
        for (id, d) in self.graph.diagnostics.iter_mut() {
            if d.lifecycle == "open"
                && (d.subjects.len() == 1 || !JUDGED_RULES.contains(&d.rule.as_str()))
                && CHECK_RULES.contains(&d.rule.as_str())
                && !seen.contains(&(d.rule.clone(), d.subjects.clone()))
            {
                d.lifecycle = "resolved".to_string();
                d.updated = Some(build.clone());
                mutations.push(serde_json::json!({"op": "resolve_diagnostic", "id": id,
                    "reason": "the condition cleared"}));
            }
        }
        if !mutations.is_empty() {
            self.status.generation = generation;
            self.prune_records();
            self.commit_records(batch);
            let entry = self.journal_entry(&build, &Commit::store("checks"), mutations);
            self.write_journal(&entry);
            self.persist();
        }
    }
}
