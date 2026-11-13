// The changeset commit: `Store::apply` and the dirtiness it collects.
// Mirrors docs/compiler/graph.md#changesets.
use super::*;

// Drop repeated ids, keeping each first occurrence in place. A rewire or resolve can
// make two ids equal anywhere in the list, so adjacent-only `Vec::dedup` is not enough.
fn dedup_keep_order(v: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    v.retain(|x| seen.insert(x.clone()));
}

// The kind of a node id by prefix.
pub(super) fn id_kind(id: &str) -> &'static str {
    if id.starts_with("req:") {
        "requirement"
    } else if id.starts_with("crit:") {
        "criterion"
    } else if id.starts_with("ent:") {
        "entity"
    } else if id.starts_with("view:") {
        "view"
    } else if id.starts_with("diag:") {
        "diagnostic"
    } else {
        "node"
    }
}

pub(super) fn provenance_is_pending(p: &Provenance) -> bool {
    !matches!(p, Provenance::Quote(_))
}

// The sticky identity of an invented-choice finding includes the choice sentence:
// its message opens with the sentence and closes with the unattached measure, which
// varies per record. Mirrors docs/consumers/gen.md#invented-choices.
fn invented_choice_key(message: &str) -> &str {
    message
        .split(" Unattached remainder on the entity:")
        .next()
        .unwrap_or(message)
        .trim_end()
}

// What one commit dirtied, accumulated while its ops apply: the first mutation that
// touched a subject is its cause.
#[derive(Default)]
struct Dirt {
    entities: BTreeMap<String, (usize, &'static str)>,
    created: BTreeMap<String, usize>,
    revised: BTreeMap<String, (usize, &'static str)>,
    // Requirements a create or update named: edges-missing is re-evaluated on them.
    named_reqs: BTreeMap<String, usize>,
    deleted: BTreeMap<String, usize>,
    provenance_pending: BTreeMap<String, (usize, &'static str)>,
    prompts: BTreeMap<String, (usize, &'static str)>,
    // (section reference, anchor id, mutation) for anchors placed under re-evaluation.
    stale_anchors: Vec<(String, String, usize)>,
}

impl Dirt {
    fn entity(&mut self, id: &str, m: usize, via: &'static str) {
        self.entities.entry(id.to_string()).or_insert((m, via));
    }
    fn entities<'a>(&mut self, ids: impl IntoIterator<Item = &'a String>, m: usize) {
        for id in ids {
            self.entity(id, m, "entities");
        }
    }
    fn created(&mut self, id: &str, m: usize) {
        self.created.entry(id.to_string()).or_insert(m);
        self.named_reqs.entry(id.to_string()).or_insert(m);
    }
    fn revised(&mut self, id: &str, m: usize, via: &'static str) {
        self.revised.entry(id.to_string()).or_insert((m, via));
    }
    fn named(&mut self, id: &str, m: usize) {
        self.named_reqs.entry(id.to_string()).or_insert(m);
    }
    fn deleted(&mut self, id: &str, m: usize) {
        self.deleted.entry(id.to_string()).or_insert(m);
    }
    fn pending(&mut self, id: &str, m: usize, p: &Provenance) {
        if provenance_is_pending(p) {
            self.provenance_pending
                .entry(id.to_string())
                .or_insert((m, p.kind()));
        }
    }
    fn prompt(&mut self, id: &str, m: usize, via: &'static str) {
        self.prompts.entry(id.to_string()).or_insert((m, via));
    }
}

impl Store {
    // Apply a staged changeset atomically: reconcile creates by natural key against nodes
    // committed concurrently, apply ops in order under the commit-time gates, propagate
    // deletions, recompute derived data, write the change records, journal, bump the
    // generation, write shards. Mirrors docs/compiler/graph.md#changesets.
    pub fn apply(&mut self, ops: Vec<Op>, commit: &Commit) -> CommitReport {
        self.apply_inner(ops, commit, None)
    }

    // Chat can validate quotes against current documents without persisting a sync
    // before the graph edit passes. The same lock, rollback, and commit cover both.
    pub(crate) fn apply_with_docs(
        &mut self,
        ops: Vec<Op>,
        commit: &Commit,
        parsed: &crate::reconcile::ParsedDocs,
    ) -> CommitReport {
        let mut commit = commit.clone();
        commit.all_or_nothing = true;
        self.apply_inner(ops, &commit, Some(parsed))
    }

    fn apply_inner(
        &mut self,
        ops: Vec<Op>,
        commit: &Commit,
        parsed: Option<&crate::reconcile::ParsedDocs>,
    ) -> CommitReport {
        let _flock = FileLock::acquire(&self.out);
        if let Some(why) = self.refuse("commit") {
            return CommitReport {
                applied: 0,
                skipped: vec![why],
                generation: self.status.generation,
                changes: Vec::new(),
                touched_entities: BTreeSet::new(),
                changed_requirements: BTreeSet::new(),
                swept: Vec::new(),
            };
        }
        let snapshot = commit.all_or_nothing.then(|| self.clone());
        let sync_entries = parsed
            .map(|parsed| self.sync_docs_for_commit(parsed))
            .unwrap_or_default();
        let generation = self.status.generation + 1;
        let build = format!("g{}", generation);
        let mut remap: BTreeMap<String, String> = BTreeMap::new();
        let mut skipped: Vec<String> = Vec::new();
        let mut applied: Vec<Op> = Vec::new();
        // Fields added to the journal mutations by applied index: the prior values a
        // decree overwrote, the prior parent of every move, the parent a retract
        // restored. Mirrors docs/compiler/graph.md#journal.
        let mut annotations: Vec<(usize, serde_json::Value)> = Vec::new();
        // The parent moves this commit made (child, from, to, mutation), the input of
        // the reparent flip. Mirrors docs/compiler/reconciler.md#flip-detection.
        let mut moves: Vec<(String, Option<String>, Option<String>, usize)> = Vec::new();
        let mut dirt = Dirt::default();
        let mut batch = RecordBatch::new(generation);
        // A human edit is the review: a dual write and an accepted ratification
        // proposal land a sentence the person just read, and the mention it adds to
        // each entity it names is the quote they confirmed, so neither writes
        // `entity-changed`. An answer whose option carried a suggested edit is the
        // same edit under another entry. A retract (journaled as `ratify` too) removes
        // facts the person did not read and writes its records as usual.
        // Mirrors docs/compiler/compilation.md#edit-paths.
        let human_confirmed = commit.kind == "dual-write"
            || (commit.kind == "ratify"
                && !ops.iter().any(|o| matches!(o, Op::RetractDecree { .. })))
            || (commit.kind == "answer"
                && ops.iter().any(|o| matches!(o, Op::EditDocProse { .. })));
        let resolve = |remap: &BTreeMap<String, String>, store: &Store, id: &str| -> String {
            let id = remap.get(id).cloned().unwrap_or_else(|| id.to_string());
            store.resolve_id(&id).to_string()
        };

        // A prose edit is never staged alone: it rides with the graph mutation it carries.
        let ops = if !ops.is_empty() && ops.iter().all(|o| matches!(o, Op::EditDocProse { .. })) {
            skipped.push(
                "edit_doc_prose: edit-needs-mutation; stage the graph mutation in the same changeset"
                    .to_string(),
            );
            Vec::new()
        } else {
            ops
        };
        // Processing-mode transitions see the final graph mutations, including
        // quote relocations and retractions staged in the same changeset.
        let (mut ops, classifications): (Vec<_>, Vec<_>) = ops
            .into_iter()
            .partition(|op| !matches!(op, Op::ClassifyInput { .. }));
        let cached_mentions = (!classifications.is_empty()).then(|| reference_sources(&self.graph));
        ops.extend(classifications);
        // The old text must still stand in the stored section, or the document moved
        // underneath the edit and the whole pair skips: a document edited underneath
        // loses the edit, never its consistency.
        let stale: Vec<String> = ops
            .iter()
            .filter_map(|o| match o {
                Op::EditDocProse {
                    doc,
                    section,
                    old_text,
                    ..
                } if !old_text.trim().is_empty() => {
                    let present = self
                        .docs
                        .get(doc)
                        .and_then(|d| d.sections.get(section))
                        .is_some_and(|s| text_contains(&s.raw, old_text));
                    (!present).then(|| {
                        format!(
                            "edit_doc_prose: edit-stale; the old text no longer stands in {}#{}",
                            doc, section
                        )
                    })
                }
                _ => None,
            })
            .collect();
        let ops = if stale.is_empty() {
            ops
        } else {
            skipped.extend(stale);
            Vec::new()
        };

        for op in ops {
            // The 1-based index this op takes in the journal if it lands.
            let m = applied.len() + 1;
            match op {
                Op::CreateEntity { id, mut entity } => {
                    entity.created = Some(build.clone());
                    entity.updated = Some(build.clone());
                    if let Some(p) = entity.parent.as_ref() {
                        let p = resolve(&remap, self, p);
                        if !self.graph.entities.contains_key(&p) {
                            skipped.push(format!("create_entity {}: unknown parent {}", id, p));
                            continue;
                        }
                        entity.parent = Some(p);
                    }
                    if let Some(Provenance::Derived { from, .. }) = entity.provenance.as_mut() {
                        *from = from.iter().map(|f| resolve(&remap, self, f)).collect();
                    }
                    if !entity.mentions.is_empty() {
                        entity.provenance = None;
                    }
                    // Commit-time natural-key reconciliation: a create whose key now matches
                    // an existing node becomes an update, with mentions unioned.
                    let found = match self.find_natural(
                        &entity.name,
                        &entity.scope,
                        entity.parent.as_deref(),
                    ) {
                        Ok(found) => found,
                        Err(candidates) => {
                            skipped.push(format!(
                                "create_entity {}: ambiguous name `{}` ({}); pass parent to say which",
                                id,
                                entity.name,
                                candidates.join(", ")
                            ));
                            continue;
                        }
                    };
                    if let Some(existing) = found {
                        remap.insert(id.clone(), existing.clone());
                        let e = self.graph.entities.get_mut(&existing).unwrap();
                        let had_mentions = !entity.mentions.is_empty();
                        for m in entity.mentions {
                            if !e.mentions.contains(&m) {
                                e.mentions.push(m);
                            }
                        }
                        if had_mentions {
                            e.provenance = None;
                        }
                        if e.definition.as_deref().unwrap_or("").is_empty() {
                            e.definition = entity.definition.clone();
                        }
                        for a in entity.aliases {
                            if !e.aliases.contains(&a) {
                                e.aliases.push(a);
                            }
                        }
                        if e.stereotype.is_none() {
                            e.stereotype = entity.stereotype.clone();
                        }
                        for attr in &entity.attributes {
                            match e.attributes.iter_mut().find(|x| x.name == attr.name) {
                                Some(mine) => *mine = attr.clone(),
                                None => e.attributes.push(attr.clone()),
                            }
                        }
                        e.updated = Some(build.clone());
                        dirt.entity(&existing, m, "fields");
                        applied.push(Op::UpdateEntity {
                            id: existing,
                            name: None,
                            definition: entity.definition,
                            add_aliases: Vec::new(),
                            remove_aliases: Vec::new(),
                            clear_parent: false,
                            not_about_add: Vec::new(),
                            add_mention: None,
                            stereotype: entity.stereotype,
                            parent: None,
                            set_attributes: None,
                            add_attributes: entity.attributes,
                            provenance: None,
                        });
                    } else {
                        // The store mints ids: a non-canonical or colliding staged id is re-minted.
                        let mut final_id = id.clone();
                        if !final_id.starts_with("ent:")
                            || self.graph.entities.contains_key(&final_id)
                            || self.graph.redirects.contains_key(&final_id)
                        {
                            final_id = self.mint_entity_id(&entity.name, &BTreeSet::new());
                        }
                        if final_id != id {
                            remap.insert(id, final_id.clone());
                        }
                        dirt.entity(&final_id, m, "fields");
                        if let Some(p) = entity.provenance.as_ref() {
                            dirt.pending(&final_id, m, p);
                        }
                        applied.push(Op::CreateEntity {
                            id: final_id.clone(),
                            entity: entity.clone(),
                        });
                        self.graph.entities.insert(final_id, entity);
                    }
                }
                Op::UpdateEntity {
                    id,
                    name,
                    definition,
                    add_aliases,
                    remove_aliases,
                    add_mention,
                    stereotype,
                    parent,
                    clear_parent,
                    not_about_add,
                    set_attributes,
                    add_attributes,
                    provenance,
                } => {
                    let rid = resolve(&remap, self, &id);
                    if !self.graph.entities.contains_key(&rid) {
                        skipped.push(format!("update_entity: unknown id {}", rid));
                        continue;
                    }
                    // The parent must exist and the tree stays acyclic.
                    let parent = match parent {
                        Some(p) => {
                            let p = resolve(&remap, self, &p);
                            if !self.graph.entities.contains_key(&p) {
                                skipped
                                    .push(format!("update_entity {}: unknown parent {}", rid, p));
                                continue;
                            }
                            if p == rid || self.is_ancestor(&rid, &p) {
                                let mut chain = vec![p.clone()];
                                chain.extend(self.ancestors(&p));
                                skipped.push(format!(
                                    "update_entity {}: parent cycle through {}",
                                    rid,
                                    chain.join(" > ")
                                ));
                                continue;
                            }
                            Some(p)
                        }
                        None => None,
                    };
                    let mut provenance = provenance;
                    if let Some(Provenance::Derived { from, .. }) = provenance.as_mut() {
                        *from = from.iter().map(|f| resolve(&remap, self, f)).collect();
                    }
                    let e = self.graph.entities.get_mut(&rid).unwrap();
                    // A decree over a quote-anchored entity records the prior values
                    // of the fields it overwrites, so a later retract restores them.
                    // Every parent move records the prior parent, so the reparent
                    // flip replays from the journal alone.
                    let mut prior = serde_json::Map::new();
                    if matches!(&provenance, Some(Provenance::Decree { .. }))
                        && e.provenance.is_none()
                    {
                        if definition.is_some() {
                            prior.insert("definition".into(), serde_json::json!(e.definition));
                        }
                        if stereotype.is_some() {
                            prior.insert("stereotype".into(), serde_json::json!(e.stereotype));
                        }
                    }
                    // `clear_parent` is a move out of the tree: it wins over `parent`,
                    // as the apply below does.
                    let target = if clear_parent { None } else { parent.clone() };
                    let mut moved_between: Option<(Option<String>, Option<String>)> = None;
                    if (clear_parent || parent.is_some()) && e.parent != target {
                        prior.insert("parent".into(), serde_json::json!(e.parent));
                        moves.push((rid.clone(), e.parent.clone(), target.clone(), m));
                        moved_between = Some((e.parent.clone(), target.clone()));
                    }
                    if !prior.is_empty() {
                        annotations.push((
                            applied.len(),
                            serde_json::json!({ "prior": serde_json::Value::Object(prior) }),
                        ));
                    }
                    if let Some(n) = &name {
                        e.name = n.clone();
                    }
                    if let Some(d) = &definition {
                        e.definition = Some(d.clone());
                    }
                    for a in &add_aliases {
                        if !e.aliases.contains(a) {
                            e.aliases.push(a.clone());
                        }
                    }
                    e.aliases.retain(|a| !remove_aliases.contains(a));
                    if let Some(m) = &add_mention {
                        if !e.mentions.contains(m) {
                            e.mentions.push(m.clone());
                        }
                        // A mention that names the entity makes it quoted.
                        e.provenance = None;
                    }
                    if let Some(s) = &stereotype {
                        e.stereotype = Some(s.clone());
                    }
                    if let Some(p) = &parent {
                        e.parent = Some(p.clone());
                    }
                    if clear_parent {
                        e.parent = None;
                    }
                    for rid in &not_about_add {
                        if !e.not_about.contains(rid) {
                            e.not_about.push(rid.clone());
                        }
                    }
                    e.not_about.sort();
                    if let Some(attrs) = &set_attributes {
                        e.attributes = attrs.clone();
                    }
                    for attr in &add_attributes {
                        match e.attributes.iter_mut().find(|x| x.name == attr.name) {
                            Some(mine) => *mine = attr.clone(),
                            None => e.attributes.push(attr.clone()),
                        }
                    }
                    if let Some(p) = &provenance {
                        e.provenance = Some(p.clone());
                        dirt.pending(&rid, m, p);
                    }
                    e.updated = Some(build.clone());
                    // A move is the parents' business: `parent` lands on the parent
                    // left and the one joined, and a move alone writes no `fields`
                    // record on the child. Mirrors
                    // docs/compiler/goals/review-entity.md#created-when.
                    if let Some((from, to)) = &moved_between {
                        for p in from.iter().chain(to.iter()) {
                            dirt.entity(p, m, "parent");
                        }
                    }
                    let move_only = (parent.is_some() || clear_parent)
                        && name.is_none()
                        && definition.is_none()
                        && add_aliases.is_empty()
                        && remove_aliases.is_empty()
                        && add_mention.is_none()
                        && stereotype.is_none()
                        && set_attributes.is_none()
                        && add_attributes.is_empty()
                        && provenance.is_none();
                    if !move_only {
                        dirt.entity(&rid, m, "fields");
                    }
                    applied.push(Op::UpdateEntity {
                        id: rid,
                        name,
                        definition,
                        add_aliases,
                        remove_aliases,
                        clear_parent,
                        not_about_add: Vec::new(),
                        add_mention,
                        stereotype,
                        parent,
                        set_attributes,
                        add_attributes,
                        provenance,
                    });
                }
                Op::RetractEntityMention { id, source, reason } => {
                    let rid = resolve(&remap, self, &id);
                    if reason.trim().is_empty() {
                        skipped.push("retract_entity_mention: reason is empty".into());
                        continue;
                    }
                    if reference_sources(&self.graph)
                        .get(&rid)
                        .is_some_and(|refs| refs.contains(&source))
                    {
                        skipped.push(format!("retract_entity_mention {rid}: mention is supplied by a live requirement"));
                        continue;
                    }
                    let Some(entity) = self.graph.entities.get_mut(&rid) else {
                        skipped.push(format!("retract_entity_mention: unknown entity {rid}"));
                        continue;
                    };
                    if !entity.mentions.contains(&source) {
                        skipped.push(format!(
                            "retract_entity_mention {rid}: source no longer matches a mention"
                        ));
                        continue;
                    }
                    entity.mentions.retain(|mention| mention != &source);
                    entity.updated = Some(build.clone());
                    dirt.entity(&rid, m, "fields");
                    applied.push(Op::RetractEntityMention {
                        id: rid,
                        source,
                        reason,
                    });
                }
                Op::DeleteEntity { id, reason } => {
                    let rid = resolve(&remap, self, &id);
                    if !self.graph.entities.contains_key(&rid) {
                        skipped.push(format!("delete_entity: unknown id {}", rid));
                        continue;
                    }
                    let refs = self.requirements_referencing(&rid);
                    if !refs.is_empty() {
                        skipped.push(format!(
                            "delete_entity {}: still referenced by {}",
                            rid,
                            refs.join(", ")
                        ));
                        continue;
                    }
                    let children = self.children_of(&rid);
                    if !children.is_empty() {
                        skipped.push(format!(
                            "delete_entity {}: still a parent of {}",
                            rid,
                            children.join(", ")
                        ));
                        continue;
                    }
                    self.graph.entities.remove(&rid);
                    self.graph.redirects.insert(rid.clone(), String::new());
                    dirt.deleted(&rid, m);
                    applied.push(Op::DeleteEntity { id: rid, reason });
                }
                Op::DissolveEntity { id, reason, .. } => {
                    let rid = resolve(&remap, self, &id);
                    if !self.graph.entities.contains_key(&rid) {
                        skipped.push(format!("dissolve_entity: unknown id {}", rid));
                        continue;
                    }
                    let refs = self.requirements_referencing(&rid);
                    if !refs.is_empty() {
                        skipped.push(format!(
                            "dissolve_entity {}: still referenced by {}",
                            rid,
                            refs.join(", ")
                        ));
                        continue;
                    }
                    if !self.is_grouping(&rid) {
                        skipped.push(format!(
                            "dissolve_entity {}: stated-entity; a document states it, revise the documents instead",
                            rid
                        ));
                        continue;
                    }
                    let (parent, children) = self.dissolve(&rid, &build);
                    for c in &children {
                        moves.push((c.clone(), Some(rid.clone()), parent.clone(), m));
                    }
                    // The children moved up: the grandparent gained them.
                    if let Some(p) = &parent {
                        if !children.is_empty() {
                            dirt.entity(p, m, "parent");
                        }
                    }
                    dirt.deleted(&rid, m);
                    applied.push(Op::DissolveEntity {
                        id: rid,
                        reason,
                        parent,
                        children,
                    });
                }
                Op::MergeEntities {
                    keep,
                    absorb,
                    reason,
                } => {
                    let keep = resolve(&remap, self, &keep);
                    let absorb = resolve(&remap, self, &absorb);
                    if keep == absorb || !self.graph.entities.contains_key(&keep) {
                        skipped.push(format!("merge_entities: bad pair {} {}", keep, absorb));
                        continue;
                    }
                    if !self.graph.entities.contains_key(&absorb) {
                        skipped.push(format!("merge_entities: unknown id {}", absorb));
                        continue;
                    }
                    // A merge never makes the survivor its own ancestor.
                    if self.is_ancestor(&absorb, &keep) {
                        skipped.push(format!(
                            "merge_entities: {} is contained in {}; merging would make it its own ancestor",
                            keep, absorb
                        ));
                        continue;
                    }
                    let ab = self.graph.entities.remove(&absorb).unwrap();
                    let rewire = |id: &mut String| {
                        if *id == absorb {
                            *id = keep.clone();
                        }
                    };
                    let rewire_provenance = |p: &mut Option<Provenance>| {
                        if let Some(Provenance::Derived { from, .. }) = p.as_mut() {
                            from.iter_mut().for_each(rewire);
                            dedup_keep_order(from);
                        }
                    };
                    for r in self.graph.requirements.values_mut() {
                        r.entities.iter_mut().for_each(rewire);
                        dedup_keep_order(&mut r.entities);
                        for edge in r.edges.iter_mut() {
                            rewire(&mut edge.a);
                            rewire(&mut edge.b);
                        }
                        r.edges.retain(|e| e.a != e.b);
                        if let Some(t) = r.transition.as_mut() {
                            rewire(&mut t.subject);
                        }
                        rewire_provenance(&mut r.provenance);
                    }
                    for e in self.graph.entities.values_mut() {
                        if let Some(p) = e.parent.as_mut() {
                            rewire(p);
                        }
                        if let Some(note) = e.distinct_from.remove(&absorb) {
                            e.distinct_from.entry(keep.clone()).or_insert(note);
                        }
                        rewire_provenance(&mut e.provenance);
                        for a in e.attributes.iter_mut() {
                            let mut p = Some(a.provenance.clone());
                            rewire_provenance(&mut p);
                            a.provenance = p.unwrap();
                        }
                    }
                    for v in self.graph.views.values_mut() {
                        v.members.iter_mut().for_each(rewire);
                        dedup_keep_order(&mut v.members);
                        v.collapse.iter_mut().for_each(rewire);
                        dedup_keep_order(&mut v.collapse);
                        for x in v.excluded.iter_mut() {
                            rewire(&mut x.id);
                        }
                        if let Some(q) = v.query.as_mut() {
                            if let Some(p) = q.parent.as_mut() {
                                rewire(p);
                            }
                        }
                        rewire_provenance(&mut v.provenance);
                    }
                    for d in self.graph.diagnostics.values_mut() {
                        d.subjects.iter_mut().for_each(rewire);
                        dedup_keep_order(&mut d.subjects);
                    }
                    let adopt_parent = match (
                        self.graph.entities[&keep].parent.is_none(),
                        ab.parent.as_deref(),
                    ) {
                        (true, Some(p)) if p != keep && !self.is_ancestor(&keep, p) => {
                            Some(p.to_string())
                        }
                        _ => None,
                    };
                    {
                        let k = self.graph.entities.get_mut(&keep).unwrap();
                        if !k.aliases.contains(&ab.name)
                            && normalize(&ab.name) != normalize(&k.name)
                        {
                            k.aliases.push(ab.name.clone());
                        }
                        for a in ab.aliases {
                            if !k.aliases.contains(&a) {
                                k.aliases.push(a);
                            }
                        }
                        for m in ab.mentions {
                            if !k.mentions.contains(&m) {
                                k.mentions.push(m);
                            }
                        }
                        // The survivor's attribute stands on a name clash.
                        for attr in ab.attributes {
                            if !k.attributes.iter().any(|x| x.name == attr.name) {
                                k.attributes.push(attr);
                            }
                        }
                        for r in ab.not_about {
                            if !k.not_about.contains(&r) {
                                k.not_about.push(r);
                            }
                        }
                        k.not_about.sort();
                        for (id, note) in ab.distinct_from {
                            k.distinct_from.entry(id).or_insert(note);
                        }
                        // The pair itself is one entity now.
                        k.distinct_from.remove(&keep);
                        k.distinct_from.remove(&absorb);
                        // The higher bump stands on a clash.
                        for (limit, bump) in ab.limits {
                            let mine = k.limits.entry(limit).or_insert(bump);
                            if bump.value > mine.value {
                                *mine = bump;
                            }
                        }
                        if k.definition.as_deref().unwrap_or("").is_empty() {
                            k.definition = ab.definition;
                        }
                        if k.stereotype.is_none() {
                            k.stereotype = ab.stereotype;
                        }
                        if let Some(p) = adopt_parent {
                            k.parent = Some(p);
                        }
                        if !k.mentions.is_empty() {
                            k.provenance = None;
                        }
                        k.updated = Some(build.clone());
                    }
                    self.redirect(&absorb, &keep);
                    dirt.entity(&keep, m, "merge");
                    applied.push(Op::MergeEntities {
                        keep,
                        absorb,
                        reason,
                    });
                }
                Op::CreateRequirement {
                    id,
                    mut requirement,
                } => {
                    requirement.entities = requirement
                        .entities
                        .iter()
                        .map(|e| resolve(&remap, self, e))
                        .collect();
                    dedup_keep_order(&mut requirement.entities);
                    for edge in requirement.edges.iter_mut() {
                        edge.a = resolve(&remap, self, &edge.a);
                        edge.b = resolve(&remap, self, &edge.b);
                    }
                    if let Some(t) = requirement.transition.as_mut() {
                        t.subject = resolve(&remap, self, &t.subject);
                    }
                    if let Some(Provenance::Derived { from, .. }) = requirement.provenance.as_mut()
                    {
                        *from = from.iter().map(|f| resolve(&remap, self, f)).collect();
                    }
                    if requirement.source.is_some() {
                        requirement.provenance = None;
                    }
                    if requirement.source.is_none() && requirement.provenance.is_none() {
                        skipped.push(format!(
                            "create_requirement {}: no provenance (a quote source, a derivation, or a decree)",
                            id
                        ));
                        continue;
                    }
                    if let Some(missing) = requirement
                        .entities
                        .iter()
                        .find(|e| !self.graph.entities.contains_key(*e))
                    {
                        skipped.push(format!(
                            "create_requirement {}: unknown entity {}",
                            id, missing
                        ));
                        continue;
                    }
                    if let Some(t) = requirement.transition.as_ref() {
                        if !requirement.entities.contains(&t.subject) {
                            skipped.push(format!(
                                "create_requirement {}: transition subject {} not among entities",
                                id, t.subject
                            ));
                            continue;
                        }
                    }
                    // Natural key: source section plus the punctuation-insensitive
                    // statement for a quoted requirement (a same-sentence reword that
                    // subsumes the existing statement refreshes in place too); the
                    // statement within its `from` set for a derived one; the statement
                    // alone for a decree. A create staged under an existing id whose
                    // statement subsumes the incoming one in the same section is the
                    // stage-time resolution of a stale anchor and folds into that id.
                    // The incoming side normalizes once; a stored requirement only
                    // when its section or provenance kind already matches.
                    let incoming_statement = normalize_statement(&requirement.statement);
                    let incoming_quote = requirement
                        .source
                        .as_ref()
                        .map(|s| normalize_statement(&s.quote));
                    let same_key = |r: &Requirement, incoming: &Requirement| -> bool {
                        let same_statement =
                            || normalize_statement(&r.statement) == incoming_statement;
                        match (
                            &r.source,
                            &incoming.source,
                            &r.provenance,
                            &incoming.provenance,
                        ) {
                            (Some(a), Some(b), _, _) => {
                                a.doc == b.doc
                                    && a.section == b.section
                                    && (same_statement()
                                        || (incoming_quote.as_deref()
                                            == Some(normalize_statement(&a.quote).as_str())
                                            && statement_rewords(
                                                &r.statement,
                                                &incoming.statement,
                                            )))
                            }
                            (
                                None,
                                None,
                                Some(Provenance::Derived { from: fa, .. }),
                                Some(Provenance::Derived { from: fb, .. }),
                            ) => {
                                same_statement()
                                    && fa.iter().collect::<BTreeSet<_>>()
                                        == fb.iter().collect::<BTreeSet<_>>()
                            }
                            (
                                None,
                                None,
                                Some(Provenance::Decree { .. }),
                                Some(Provenance::Decree { .. }),
                            ) => same_statement(),
                            _ => false,
                        }
                    };
                    let same_anchor = |r: &Requirement, incoming: &Requirement| -> bool {
                        match (&r.source, &incoming.source) {
                            (Some(a), Some(b)) => a.doc == b.doc && a.section == b.section,
                            _ => false,
                        }
                    };
                    let fold_target = self
                        .graph
                        .requirements
                        .iter()
                        .find(|(_, r)| same_key(r, &requirement))
                        .map(|(rid, _)| rid.clone())
                        .or_else(|| {
                            self.graph.requirements.get(&id).and_then(|r| {
                                (same_anchor(r, &requirement)
                                    && statement_subsumes(&r.statement, &requirement.statement))
                                .then(|| id.clone())
                            })
                        });
                    if let Some(existing) = fold_target {
                        remap.insert(id, existing.clone());
                        let r = self.graph.requirements.get_mut(&existing).unwrap();
                        for e in &requirement.entities {
                            if !r.entities.contains(e) {
                                r.entities.push(e.clone());
                            }
                        }
                        // Edges are directional: the same pair in the other direction, or
                        // under another type, is another edge.
                        for edge in requirement.edges {
                            if !r.edges.iter().any(|x| {
                                x.a == edge.a && x.b == edge.b && x.rel_type == edge.rel_type
                            }) {
                                r.edges.push(edge);
                            }
                        }
                        if requirement.transition.is_some() {
                            r.transition = requirement.transition.clone();
                        }
                        if !requirement.facets.is_empty() {
                            r.facets = requirement.facets.clone();
                        }
                        // The matched statement and quote refresh in place (same
                        // statement modulo punctuation); the id never churns. A refresh
                        // that changes something is journaled as the update it is.
                        let mut refreshed_statement: Option<String> = None;
                        let mut refreshed_source: Option<SourceRef> = None;
                        if r.statement != requirement.statement {
                            // Case and punctuation refresh in place; only a change in
                            // substance reopens the requirement's pairs
                            // (docs/compiler/graph.md#change-records).
                            if normalize_statement(&r.statement)
                                != normalize_statement(&requirement.statement)
                            {
                                dirt.revised(&existing, m, "fields");
                            }
                            r.statement = requirement.statement.clone();
                            refreshed_statement = Some(requirement.statement.clone());
                        }
                        if let Some(incoming) = requirement.source.as_ref() {
                            let old_quote = r.source.as_ref().map(|s| s.quote.clone());
                            if old_quote.as_deref() != Some(incoming.quote.as_str()) {
                                // A quote that changed in substance (not punctuation) means
                                // the document text under the statement changed: revised,
                                // even when the session kept the old wording.
                                if old_quote.map(|q| normalize_statement(&q))
                                    != Some(normalize_statement(&incoming.quote))
                                {
                                    dirt.revised(&existing, m, "quote");
                                }
                                r.source = Some(incoming.clone());
                                r.provenance = None;
                                refreshed_source = Some(incoming.clone());
                            }
                        }
                        r.updated = Some(build.clone());
                        let ents = r.entities.clone();
                        dirt.entities(ents.iter(), m);
                        dirt.named(&existing, m);
                        if refreshed_statement.is_some() || refreshed_source.is_some() {
                            applied.push(Op::UpdateRequirement {
                                id: existing,
                                statement: refreshed_statement,
                                entities: None,
                                edges: None,
                                transition: None,
                                facets: None,
                                source: refreshed_source,
                                provenance: None,
                            });
                        }
                        continue;
                    }
                    requirement.created = Some(build.clone());
                    requirement.updated = Some(build.clone());
                    let mut final_id = id.clone();
                    if !final_id.starts_with("req:")
                        || self.graph.requirements.contains_key(&final_id)
                    {
                        final_id = self.mint_req_id(&requirement, &BTreeSet::new());
                    }
                    if final_id != id {
                        remap.insert(id, final_id.clone());
                    }
                    dirt.entities(requirement.entities.iter(), m);
                    dirt.created(&final_id, m);
                    if let Some(p) = requirement.provenance.as_ref() {
                        dirt.pending(&final_id, m, p);
                    }
                    // A committed requirement adds its source as a mention on every entity
                    // it references, so reuse accumulates cross-document presence.
                    if let Some(src) = requirement.source.as_ref() {
                        for e in &requirement.entities {
                            if let Some(ent) = self.graph.entities.get_mut(e) {
                                if !ent.mentions.contains(src) {
                                    ent.mentions.push(src.clone());
                                    ent.updated = Some(build.clone());
                                }
                            }
                        }
                    }
                    applied.push(Op::CreateRequirement {
                        id: final_id.clone(),
                        requirement: requirement.clone(),
                    });
                    self.graph.requirements.insert(final_id, requirement);
                }
                Op::UpdateRequirement {
                    id,
                    statement,
                    entities,
                    edges,
                    transition,
                    facets,
                    source,
                    provenance,
                } => {
                    let rid = resolve(&remap, self, &id);
                    if !self.graph.requirements.contains_key(&rid) {
                        skipped.push(format!("update_requirement: unknown id {}", rid));
                        continue;
                    }
                    let resolved_entities = entities.map(|es| {
                        let mut v: Vec<String> =
                            es.iter().map(|e| resolve(&remap, self, e)).collect();
                        dedup_keep_order(&mut v);
                        v
                    });
                    if let Some(es) = &resolved_entities {
                        if let Some(missing) =
                            es.iter().find(|e| !self.graph.entities.contains_key(*e))
                        {
                            skipped.push(format!(
                                "update_requirement {}: unknown entity {}",
                                rid, missing
                            ));
                            continue;
                        }
                    }
                    let edges = edges.map(|ed| {
                        ed.into_iter()
                            .map(|mut e| {
                                e.a = resolve(&remap, self, &e.a);
                                e.b = resolve(&remap, self, &e.b);
                                e
                            })
                            .collect::<Vec<_>>()
                    });
                    let transition = transition.map(|mut t| {
                        t.subject = resolve(&remap, self, &t.subject);
                        t
                    });
                    let mut provenance = provenance;
                    if let Some(Provenance::Derived { from, .. }) = provenance.as_mut() {
                        *from = from.iter().map(|f| resolve(&remap, self, f)).collect();
                    }
                    if let Some(t) = transition.as_ref() {
                        let listed = resolved_entities
                            .as_ref()
                            .unwrap_or(&self.graph.requirements[&rid].entities)
                            .contains(&t.subject);
                        if !listed || !self.graph.entities.contains_key(&t.subject) {
                            skipped.push(format!(
                                "update_requirement {}: transition subject {} not among entities",
                                rid, t.subject
                            ));
                            continue;
                        }
                    }
                    // A statement change voids the word-match judgments on it
                    // (docs/compiler/model/entity.md#fields).
                    let statement_changes = statement
                        .as_ref()
                        .is_some_and(|e| self.graph.requirements[&rid].statement != *e);
                    if statement_changes {
                        for ent in self.graph.entities.values_mut() {
                            ent.not_about.retain(|x| x != &rid);
                        }
                        // The evidence was written against the old text
                        // (docs/compiler/model/criterion.md#lifecycle).
                        let derived: Vec<String> = self
                            .graph
                            .criteria
                            .iter()
                            .filter(|(_, c)| c.requirement == rid && c.source.is_none())
                            .map(|(cid, _)| cid.clone())
                            .collect();
                        for cid in derived {
                            if let Some(c) = self.graph.criteria.get_mut(&cid) {
                                c.stale = true;
                            }
                            dirt.revised(&cid, m, "requirement");
                        }
                    }
                    let r = self.graph.requirements.get_mut(&rid).unwrap();
                    let before: Vec<String> = r.entities.clone();
                    let edges_before = r.edges.clone();
                    let statement_before = r.statement.clone();
                    if let Some(e) = &statement {
                        if normalize_statement(&r.statement) != normalize_statement(e) {
                            dirt.revised(&rid, m, "fields");
                        }
                        r.statement = e.clone();
                    }
                    if let Some(es) = &resolved_entities {
                        r.entities = es.clone();
                    }
                    if let Some(ed) = &edges {
                        r.edges = ed.clone();
                    }
                    if let Some(t) = &transition {
                        r.transition = Some(t.clone());
                    }
                    if let Some(f) = &facets {
                        r.facets = f.clone();
                    }
                    if let Some(s) = &source {
                        // Same rule as the create fold: a re-anchored quote that
                        // changed in substance marks the statement revised. A quote
                        // landing on a derived or decreed fact makes it quoted.
                        if r.source.as_ref().map(|q| normalize_statement(&q.quote))
                            != Some(normalize_statement(&s.quote))
                        {
                            dirt.revised(&rid, m, "quote");
                        }
                        r.source = Some(s.clone());
                        r.provenance = None;
                    } else if let Some(p) = &provenance {
                        // A decree or derivation over the fact: the quote gives way.
                        // A decree over a quoted fact records the prior value and
                        // source in the journal, so a later retract restores them.
                        if matches!(p, Provenance::Decree { .. }) {
                            if let Some(prior_src) = &r.source {
                                annotations.push((
                                    applied.len(),
                                    serde_json::json!({
                                        "prior": {
                                            "statement": statement_before,
                                            "source": prior_src,
                                        }
                                    }),
                                ));
                            }
                        }
                        r.source = None;
                        r.provenance = Some(p.clone());
                        dirt.pending(&rid, m, p);
                    }
                    r.updated = Some(build.clone());
                    let after = r.entities.clone();
                    // What the update dirtied: the fact itself (statement, transition,
                    // facets, provenance) reopens every entity's review; a change to
                    // the entity list alone reaches only the entities added or removed;
                    // an edge change alone reaches the two ends of a new composition,
                    // whose part may need its parent set, and nothing else.
                    // Mirrors docs/compiler/goals/review-entity.md#created-when.
                    let fact_changed = r.statement != statement_before
                        || transition.is_some()
                        || facets.is_some()
                        || source.is_some()
                        || provenance.is_some();
                    if fact_changed {
                        dirt.entities(before.iter().chain(after.iter()), m);
                    } else {
                        for e in before.iter().filter(|e| !after.contains(e)) {
                            dirt.entity(e, m, "entities");
                        }
                        for e in after.iter().filter(|e| !before.contains(e)) {
                            dirt.entity(e, m, "entities");
                        }
                        for e in r.edges.iter().filter(|e| {
                            e.rel_type.as_deref() == Some("composition")
                                && !edges_before
                                    .iter()
                                    .any(|x| x.a == e.a && x.b == e.b && x.rel_type == e.rel_type)
                        }) {
                            dirt.entity(&e.a, m, "edges");
                            dirt.entity(&e.b, m, "edges");
                        }
                    }
                    dirt.named(&rid, m);
                    applied.push(Op::UpdateRequirement {
                        id: rid,
                        statement,
                        entities: resolved_entities,
                        edges,
                        transition,
                        facets,
                        source,
                        provenance,
                    });
                }
                Op::DeleteRequirement { id, reason } => {
                    let rid = resolve(&remap, self, &id);
                    match self.graph.requirements.remove(&rid) {
                        Some(r) => {
                            dirt.entities(r.entities.iter(), m);
                            dirt.deleted(&rid, m);
                            // The criteria go with their requirement
                            // (docs/compiler/graph.md#the-sweep). The requirement's
                            // delete journals first, at `m`, the index its records
                            // name; each cascaded criterion delete follows at its own.
                            let gone: Vec<String> = self
                                .graph
                                .criteria
                                .iter()
                                .filter(|(_, c)| c.requirement == rid)
                                .map(|(cid, _)| cid.clone())
                                .collect();
                            applied.push(Op::DeleteRequirement {
                                id: rid.clone(),
                                reason,
                            });
                            for cid in gone {
                                self.graph.criteria.remove(&cid);
                                dirt.deleted(&cid, applied.len() + 1);
                                applied.push(Op::DeleteCriterion {
                                    id: cid,
                                    reason: format!("its requirement {} was deleted", rid),
                                });
                            }
                        }
                        None => skipped.push(format!("delete_requirement: unknown id {}", rid)),
                    }
                }
                Op::CreateCriterion { id, mut criterion } => {
                    // Mirrors docs/compiler/model/criterion.md and graph.md#mutations.
                    criterion.requirement = resolve(&remap, self, &criterion.requirement);
                    let Some(req) = self.graph.requirements.get(&criterion.requirement) else {
                        skipped.push(format!(
                            "create_criterion {}: unknown requirement {}",
                            id, criterion.requirement
                        ));
                        continue;
                    };
                    if !CRITERION_KINDS.contains(&criterion.kind.as_str()) {
                        skipped.push(format!(
                            "create_criterion {}: kind `{}` is not one of {}",
                            id,
                            criterion.kind,
                            CRITERION_KINDS.join(", ")
                        ));
                        continue;
                    }
                    if criterion.source.is_some() {
                        criterion.provenance = None;
                    }
                    if criterion.source.is_none() && criterion.provenance.is_none() {
                        skipped.push(format!(
                            "create_criterion {}: no provenance (a quote source, a derivation, or a decree)",
                            id
                        ));
                        continue;
                    }
                    if normalize_statement(&criterion.statement)
                        == normalize_statement(&req.statement)
                    {
                        skipped.push(format!(
                            "create_criterion {}: criterion-restates {} ({})",
                            id, criterion.requirement, req.statement
                        ));
                        continue;
                    }
                    criterion.entities = criterion
                        .entities
                        .iter()
                        .map(|e| resolve(&remap, self, e))
                        .collect();
                    if let Some(Provenance::Derived { from, .. }) = criterion.provenance.as_mut() {
                        *from = from.iter().map(|f| resolve(&remap, self, f)).collect();
                    }
                    let key = normalize_statement(&criterion.statement);
                    let existing = self
                        .graph
                        .criteria
                        .iter()
                        .find(|(_, c)| {
                            c.requirement == criterion.requirement
                                && normalize_statement(&c.statement) == key
                        })
                        .map(|(cid, _)| cid.clone());
                    if let Some(cid) = existing {
                        remap.insert(id, cid.clone());
                        let c = self.graph.criteria.get_mut(&cid).unwrap();
                        let mut changed = false;
                        if c.statement != criterion.statement {
                            c.statement = criterion.statement.clone();
                            changed = true;
                        }
                        if c.kind != criterion.kind {
                            c.kind = criterion.kind.clone();
                            changed = true;
                        }
                        if criterion.measure.is_some() && c.measure != criterion.measure {
                            c.measure = criterion.measure.clone();
                            changed = true;
                        }
                        if let Some(src) = criterion.source.as_ref() {
                            if c.source.as_ref() != Some(src) {
                                c.source = Some(src.clone());
                                c.provenance = None;
                                changed = true;
                            }
                        }
                        if c.stale {
                            c.stale = false;
                            changed = true;
                        }
                        if changed {
                            c.updated = Some(build.clone());
                            dirt.revised(&cid, m, "fields");
                            applied.push(Op::UpdateCriterion {
                                id: cid,
                                statement: Some(criterion.statement),
                                kind: Some(criterion.kind),
                                measure: criterion.measure,
                                given: None,
                                when: None,
                                then: None,
                                entities: None,
                            });
                        }
                        continue;
                    }
                    criterion.created = Some(build.clone());
                    criterion.updated = Some(build.clone());
                    // The store mints the id from the quote's document stem; a staged
                    // id under another stem (a provisional one) is re-minted.
                    let source = self
                        .graph
                        .requirements
                        .get(&criterion.requirement)
                        .and_then(|r| r.source.as_ref())
                        .or(criterion.source.as_ref());
                    let base = format!("crit:{}-", Self::req_id_stem(source));
                    let mut final_id = id.clone();
                    if !final_id.starts_with(&base) || self.graph.criteria.contains_key(&final_id) {
                        final_id = self.mint_crit_id(&criterion, None, &BTreeSet::new());
                    }
                    if final_id != id {
                        remap.insert(id, final_id.clone());
                    }
                    dirt.created(&final_id, m);
                    // A derived criterion carries a ratification proposal like any
                    // derived fact (docs/compiler/model/criterion.md#lifecycle).
                    if let Some(p) = criterion.provenance.as_ref() {
                        dirt.pending(&final_id, m, p);
                    }
                    applied.push(Op::CreateCriterion {
                        id: final_id.clone(),
                        criterion: criterion.clone(),
                    });
                    self.graph.criteria.insert(final_id, criterion);
                }
                Op::UpdateCriterion {
                    id,
                    statement,
                    kind,
                    measure,
                    given,
                    when,
                    then,
                    entities,
                } => {
                    let cid = resolve(&remap, self, &id);
                    let Some(c) = self.graph.criteria.get(&cid) else {
                        skipped.push(format!("update_criterion: unknown id {}", cid));
                        continue;
                    };
                    if let Some(k) = &kind {
                        if !CRITERION_KINDS.contains(&k.as_str()) {
                            skipped.push(format!(
                                "update_criterion {}: kind `{}` is not one of {}",
                                cid,
                                k,
                                CRITERION_KINDS.join(", ")
                            ));
                            continue;
                        }
                    }
                    if let Some(st) = &statement {
                        let restates =
                            self.graph
                                .requirements
                                .get(&c.requirement)
                                .is_some_and(|r| {
                                    normalize_statement(&r.statement) == normalize_statement(st)
                                });
                        if restates {
                            skipped.push(format!(
                                "update_criterion {}: criterion-restates {}",
                                cid, c.requirement
                            ));
                            continue;
                        }
                    }
                    let resolved_entities: Option<Vec<String>> = entities
                        .as_ref()
                        .map(|es| es.iter().map(|e| resolve(&remap, self, e)).collect());
                    let c = self.graph.criteria.get_mut(&cid).unwrap();
                    let mut changed = false;
                    if let Some(st) = &statement {
                        if c.statement != *st {
                            c.statement = st.clone();
                            changed = true;
                        }
                    }
                    if let Some(k) = &kind {
                        if c.kind != *k {
                            c.kind = k.clone();
                            changed = true;
                        }
                    }
                    if measure.is_some() && c.measure != measure {
                        c.measure = measure.clone();
                        changed = true;
                    }
                    if given.is_some() && c.given != given {
                        c.given = given.clone();
                        changed = true;
                    }
                    if when.is_some() && c.when != when {
                        c.when = when.clone();
                        changed = true;
                    }
                    if then.is_some() && c.then != then {
                        c.then = then.clone();
                        changed = true;
                    }
                    if let Some(es) = resolved_entities {
                        if c.entities != es {
                            c.entities = es;
                            changed = true;
                        }
                    }
                    if c.stale {
                        c.stale = false;
                        changed = true;
                    }
                    if changed {
                        c.updated = Some(build.clone());
                        dirt.revised(&cid, m, "fields");
                        applied.push(Op::UpdateCriterion {
                            id: cid,
                            statement,
                            kind,
                            measure,
                            given,
                            when,
                            then,
                            entities,
                        });
                    }
                }
                Op::DeleteCriterion { id, reason } => {
                    let cid = resolve(&remap, self, &id);
                    match self.graph.criteria.remove(&cid) {
                        Some(_) => {
                            dirt.deleted(&cid, m);
                            applied.push(Op::DeleteCriterion { id: cid, reason });
                        }
                        None => skipped.push(format!("delete_criterion: unknown id {}", cid)),
                    }
                }
                Op::CreateView { id, mut view } => {
                    if !VIEW_KINDS.contains(&view.kind.as_str()) {
                        skipped.push(format!(
                            "create_view {}: unknown kind `{}`; one of: {}",
                            id,
                            view.kind,
                            VIEW_KINDS.join(", ")
                        ));
                        continue;
                    }
                    if view.title.trim().is_empty() {
                        skipped.push(format!("create_view {}: empty title", id));
                        continue;
                    }
                    let Some(provenance) = view.provenance.take() else {
                        skipped.push(format!("create_view {}: no provenance", id));
                        continue;
                    };
                    let mut provenance = provenance;
                    if let Provenance::Derived { from, .. } = &mut provenance {
                        *from = from.iter().map(|f| resolve(&remap, self, f)).collect();
                    }
                    view.provenance = Some(provenance);
                    view.members = view
                        .members
                        .iter()
                        .map(|x| resolve(&remap, self, x))
                        .collect();
                    dedup_keep_order(&mut view.members);
                    view.collapse = view
                        .collapse
                        .iter()
                        .map(|x| resolve(&remap, self, x))
                        .collect();
                    dedup_keep_order(&mut view.collapse);
                    for x in view.excluded.iter_mut() {
                        x.id = resolve(&remap, self, &x.id);
                    }
                    if let Some(q) = view.query.as_mut() {
                        if let Some(p) = q.parent.as_mut() {
                            *p = resolve(&remap, self, p);
                        }
                    }
                    if let Some(missing) = view
                        .members
                        .iter()
                        .chain(view.collapse.iter())
                        .chain(view.excluded.iter().map(|x| &x.id))
                        .chain(view.query.iter().filter_map(|q| q.parent.as_ref()))
                        .find(|x| !self.node_exists(x))
                    {
                        skipped.push(format!("create_view {}: unknown member {}", id, missing));
                        continue;
                    }
                    // Natural key: kind plus title. A default under that key is curated
                    // from here on.
                    if let Some(existing) = self.find_view(&view.kind, &view.title) {
                        remap.insert(id, existing.clone());
                        let v = self.graph.views.get_mut(&existing).unwrap();
                        if !view.members.is_empty() {
                            v.members = view.members.clone();
                        }
                        for x in &view.excluded {
                            if !v.excluded.iter().any(|y| y.id == x.id) {
                                v.excluded.push(x.clone());
                            }
                        }
                        if view.query.is_some() {
                            v.query = view.query.clone();
                        }
                        if !view.collapse.is_empty() {
                            v.collapse = view.collapse.clone();
                        }
                        v.provenance = view.provenance.clone();
                        v.default = false;
                        v.updated = Some(build.clone());
                        applied.push(Op::UpdateView {
                            id: existing,
                            title: None,
                            members: (!view.members.is_empty()).then(|| view.members.clone()),
                            add_members: Vec::new(),
                            remove_members: Vec::new(),
                            query: view.query.clone(),
                            collapse: (!view.collapse.is_empty()).then(|| view.collapse.clone()),
                            exclude: view.excluded.clone(),
                            reasoning: None,
                        });
                        continue;
                    }
                    let mut final_id = id.clone();
                    if !final_id.starts_with("view:") || self.graph.views.contains_key(&final_id) {
                        final_id = self.mint_view_id(&view.kind, &view.title, &BTreeSet::new());
                    }
                    if final_id != id {
                        remap.insert(id, final_id.clone());
                    }
                    view.default = false;
                    view.created = Some(build.clone());
                    view.updated = Some(build.clone());
                    applied.push(Op::CreateView {
                        id: final_id.clone(),
                        view: view.clone(),
                    });
                    self.graph.views.insert(final_id, view);
                }
                Op::UpdateView {
                    id,
                    title,
                    members,
                    add_members,
                    remove_members,
                    query,
                    collapse,
                    exclude,
                    reasoning,
                } => {
                    let rid = resolve(&remap, self, &id);
                    if !self.graph.views.contains_key(&rid) {
                        skipped.push(format!("update_view: unknown id {}", rid));
                        continue;
                    }
                    let res = |v: Vec<String>| -> Vec<String> {
                        v.iter().map(|x| resolve(&remap, self, x)).collect()
                    };
                    let members = members.map(res);
                    let add_members = res(add_members);
                    let remove_members = res(remove_members);
                    let collapse = collapse.map(res);
                    let exclude: Vec<Exclusion> = exclude
                        .into_iter()
                        .map(|x| Exclusion {
                            id: resolve(&remap, self, &x.id),
                            note: x.note,
                        })
                        .collect();
                    let query = query.map(|mut q| {
                        if let Some(p) = q.parent.as_mut() {
                            *p = resolve(&remap, self, p);
                        }
                        q
                    });
                    if let Some(missing) = members
                        .iter()
                        .flatten()
                        .chain(add_members.iter())
                        .chain(collapse.iter().flatten())
                        .chain(exclude.iter().map(|x| &x.id))
                        .chain(query.iter().filter_map(|q| q.parent.as_ref()))
                        .find(|x| !self.node_exists(x))
                    {
                        skipped.push(format!("update_view {}: unknown member {}", rid, missing));
                        continue;
                    }
                    let v = self.graph.views.get_mut(&rid).unwrap();
                    if let Some(t) = &title {
                        v.title = t.clone();
                    }
                    if let Some(ms) = &members {
                        v.members = ms.clone();
                    }
                    for x in &add_members {
                        if !v.members.contains(x) {
                            v.members.push(x.clone());
                        }
                    }
                    v.members.retain(|x| !remove_members.contains(x));
                    if let Some(q) = &query {
                        v.query = Some(q.clone());
                    }
                    if let Some(c) = &collapse {
                        v.collapse = c.clone();
                    }
                    for x in &exclude {
                        v.members.retain(|y| y != &x.id);
                        match v.excluded.iter_mut().find(|y| y.id == x.id) {
                            Some(mine) => mine.note = x.note.clone(),
                            None => v.excluded.push(x.clone()),
                        }
                    }
                    if let Some(why) = &reasoning {
                        if let Some(Provenance::Derived { reasoning: r, .. }) =
                            v.provenance.as_mut()
                        {
                            *r = why.clone();
                        }
                    }
                    // Any mutation naming a default view makes it curated.
                    v.default = false;
                    v.updated = Some(build.clone());
                    applied.push(Op::UpdateView {
                        id: rid,
                        title,
                        members,
                        add_members,
                        remove_members,
                        query,
                        collapse,
                        exclude,
                        reasoning,
                    });
                }
                Op::DeleteView { id, reason } => {
                    let rid = resolve(&remap, self, &id);
                    match self.graph.views.get(&rid) {
                        Some(v) if v.default => skipped.push(format!(
                            "delete_view {}: a default view derives again at the next commit; curate it first (update_view clears default)",
                            rid
                        )),
                        Some(_) => {
                            self.graph.views.remove(&rid);
                            dirt.deleted(&rid, m);
                            applied.push(Op::DeleteView { id: rid, reason });
                        }
                        None => skipped.push(format!("delete_view: unknown id {}", rid)),
                    }
                }
                Op::RatifyProvenance { id, source } => {
                    let rid = resolve(&remap, self, &id);
                    if !self.quote_locates(&source.doc, &source.section, &source.quote) {
                        skipped.push(format!(
                            "ratify_provenance {}: quote not found in {}#{}",
                            rid, source.doc, source.section
                        ));
                        continue;
                    }
                    if let Some(r) = self.graph.requirements.get_mut(&rid) {
                        if r.source.is_some() {
                            skipped.push(format!("ratify_provenance {}: already quoted", rid));
                            continue;
                        }
                        r.source = Some(source.clone());
                        r.provenance = None;
                        r.updated = Some(build.clone());
                        let ents = r.entities.clone();
                        for e in &ents {
                            if let Some(ent) = self.graph.entities.get_mut(e) {
                                if !ent.mentions.contains(&source) {
                                    ent.mentions.push(source.clone());
                                    ent.updated = Some(build.clone());
                                }
                            }
                        }
                        dirt.entities(ents.iter(), m);
                    } else if let Some(e) = self.graph.entities.get_mut(&rid) {
                        if e.provenance.is_none() {
                            skipped.push(format!("ratify_provenance {}: already quoted", rid));
                            continue;
                        }
                        if !e.mentions.contains(&source) {
                            e.mentions.push(source.clone());
                        }
                        e.provenance = None;
                        e.updated = Some(build.clone());
                        dirt.entity(&rid, m, "mentions");
                    } else if let Some(c) = self.graph.criteria.get_mut(&rid) {
                        if c.source.is_some() {
                            skipped.push(format!("ratify_provenance {}: already quoted", rid));
                            continue;
                        }
                        c.source = Some(source.clone());
                        c.provenance = None;
                        c.stale = false;
                        c.updated = Some(build.clone());
                    } else if self.graph.views.contains_key(&rid) {
                        skipped.push(format!("ratify_provenance {}: views are not ratified", rid));
                        continue;
                    } else {
                        skipped.push(format!("ratify_provenance: unknown id {}", rid));
                        continue;
                    }
                    self.status
                        .clear_changes(&[CHANGE_PROVENANCE_PENDING], &rid);
                    applied.push(Op::RatifyProvenance {
                        id: rid.clone(),
                        source,
                    });
                    applied.extend(self.resolve_ratification_diags(&rid, &build, "ratified"));
                }
                Op::RetractDecree { id, reason } => {
                    let rid = resolve(&remap, self, &id);
                    let pending =
                        |p: &Option<Provenance>| p.as_ref().is_some_and(provenance_is_pending);
                    if self.graph.requirements.contains_key(&rid) {
                        {
                            let r = &self.graph.requirements[&rid];
                            if r.source.is_some() || !pending(&r.provenance) {
                                skipped.push(format!("retract_decree {}: not a decree", rid));
                                continue;
                            }
                        }
                        // A fact that was quoted before the decree returns to the
                        // prior value and source the decree's journal entry recorded;
                        // a requirement created by decree or derivation is deleted.
                        // Mirrors docs/compiler/graph.md#mutations.
                        match self.decree_prior(&rid, "update_requirement") {
                            Some(prior) if prior["source"].is_object() => {
                                let Ok(source) =
                                    serde_json::from_value::<SourceRef>(prior["source"].clone())
                                else {
                                    skipped.push(format!(
                                        "retract_decree {}: unreadable prior source",
                                        rid
                                    ));
                                    continue;
                                };
                                let r = self.graph.requirements.get_mut(&rid).unwrap();
                                if let Some(st) = prior["statement"].as_str() {
                                    r.statement = st.to_string();
                                }
                                r.source = Some(source);
                                r.provenance = None;
                                r.updated = Some(build.clone());
                                let ents = r.entities.clone();
                                dirt.revised(&rid, m, "quote");
                                dirt.entities(ents.iter(), m);
                            }
                            _ => {
                                let r = self.graph.requirements.remove(&rid).unwrap();
                                dirt.entities(r.entities.iter(), m);
                                dirt.deleted(&rid, m);
                            }
                        }
                    } else if self.graph.entities.contains_key(&rid) {
                        if !pending(&self.graph.entities[&rid].provenance) {
                            skipped.push(format!("retract_decree {}: not a decree", rid));
                            continue;
                        }
                        if let Some(prior) = self.decree_prior(&rid, "update_entity") {
                            // The decreed fields return to their prior values; the
                            // mentions still quote the entity. A restored parent is
                            // a move, journaled with the parent it leaves.
                            let e = self.graph.entities.get_mut(&rid).unwrap();
                            if let Some(v) = prior.get("definition") {
                                e.definition = v.as_str().map(String::from);
                            }
                            if let Some(v) = prior.get("stereotype") {
                                e.stereotype = v.as_str().map(String::from);
                            }
                            if let Some(v) = prior.get("parent") {
                                let restored = v.as_str().map(String::from);
                                if restored != e.parent {
                                    annotations.push((
                                        applied.len(),
                                        serde_json::json!({
                                            "parent": restored,
                                            "prior": { "parent": e.parent },
                                        }),
                                    ));
                                    moves.push((
                                        rid.clone(),
                                        e.parent.clone(),
                                        restored.clone(),
                                        m,
                                    ));
                                    e.parent = restored;
                                }
                            }
                            e.provenance = None;
                            e.updated = Some(build.clone());
                            dirt.entity(&rid, m, "fields");
                        } else {
                            // A created entity goes. Quoted requirements that named
                            // it re-point to its parent (refused when it has none),
                            // and a grouping dissolves: its children reparent to
                            // its parent, journaled as the dissolve_entity mutation
                            // so the moves replay. Mirrors
                            // docs/compiler/goals/ratify.md#retract.
                            let parent = self.graph.entities[&rid]
                                .parent
                                .clone()
                                .map(|p| self.resolve_id(&p).to_string())
                                .filter(|p| self.graph.entities.contains_key(p));
                            let refs = self.requirements_referencing(&rid);
                            if !refs.is_empty() {
                                let Some(p) = &parent else {
                                    skipped.push(format!(
                                        "retract_decree {}: no parent to take its requirements; still referenced by {}",
                                        rid,
                                        refs.join(", ")
                                    ));
                                    continue;
                                };
                                for r in &refs {
                                    let mut ents: Vec<String> = self.graph.requirements[r]
                                        .entities
                                        .iter()
                                        .filter(|e| self.resolve_id(e) != rid)
                                        .cloned()
                                        .collect();
                                    if !ents.iter().any(|e| self.resolve_id(e) == p.as_str()) {
                                        ents.push(p.clone());
                                    }
                                    let req = self.graph.requirements.get_mut(r).unwrap();
                                    req.entities = ents;
                                    req.updated = Some(build.clone());
                                    dirt.revised(r, m, "entities");
                                }
                                dirt.entity(p, m, "requirements");
                            }
                            let children = self.children_of(&rid);
                            if children.is_empty() {
                                self.graph.entities.remove(&rid);
                                self.graph.redirects.insert(rid.clone(), String::new());
                                dirt.deleted(&rid, m);
                            } else {
                                let (parent, children) = self.dissolve(&rid, &build);
                                for c in &children {
                                    moves.push((c.clone(), Some(rid.clone()), parent.clone(), m));
                                }
                                if let Some(p) = &parent {
                                    dirt.entity(p, m, "parent");
                                }
                                dirt.deleted(&rid, m);
                                self.status
                                    .clear_changes(&[CHANGE_PROVENANCE_PENDING], &rid);
                                applied.push(Op::DissolveEntity {
                                    id: rid.clone(),
                                    reason: reason.clone(),
                                    parent,
                                    children,
                                });
                                applied
                                    .extend(self.resolve_ratification_diags(&rid, &build, &reason));
                                continue;
                            }
                        }
                    } else if let Some(v) = self.graph.views.get(&rid) {
                        if v.default || !pending(&v.provenance) {
                            skipped.push(format!("retract_decree {}: not a decree", rid));
                            continue;
                        }
                        self.graph.views.remove(&rid);
                        dirt.deleted(&rid, m);
                    } else if self.graph.criteria.contains_key(&rid) {
                        // A derived criterion retracted goes; its requirement stands
                        // (docs/compiler/goals/ratify.md#created-when).
                        if !pending(&self.graph.criteria[&rid].provenance) {
                            skipped.push(format!("retract_decree {}: not a decree", rid));
                            continue;
                        }
                        self.graph.criteria.remove(&rid);
                        dirt.deleted(&rid, m);
                    } else {
                        skipped.push(format!("retract_decree: unknown id {}", rid));
                        continue;
                    }
                    self.status
                        .clear_changes(&[CHANGE_PROVENANCE_PENDING], &rid);
                    applied.push(Op::RetractDecree {
                        id: rid.clone(),
                        reason: reason.clone(),
                    });
                    applied.extend(self.resolve_ratification_diags(&rid, &build, &reason));
                }
                Op::BumpLimit {
                    id,
                    limit,
                    value,
                    provenance,
                } => {
                    let rid = resolve(&remap, self, &id);
                    if crate::limits::limit(&limit).is_none() {
                        skipped.push(format!(
                            "bump_limit {}: unknown limit `{}`; one of: {}",
                            rid,
                            limit,
                            crate::limits::LIMITS
                                .iter()
                                .map(|l| l.name)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                        continue;
                    }
                    if value == 0 {
                        skipped.push(format!("bump_limit {}: value must be positive", rid));
                        continue;
                    }
                    let bump = LimitBump { value };
                    if let Some(e) = self.graph.entities.get_mut(&rid) {
                        if !crate::limits::ENTITY_LIMITS.contains(&limit.as_str()) {
                            skipped.push(format!(
                                "bump_limit {}: `{}` does not apply to an entity",
                                rid, limit
                            ));
                            continue;
                        }
                        e.limits.insert(limit.clone(), bump);
                        e.updated = Some(build.clone());
                    } else if let Some(v) = self.graph.views.get_mut(&rid) {
                        if !crate::limits::VIEW_LIMITS.contains(&limit.as_str()) {
                            skipped.push(format!(
                                "bump_limit {}: `{}` does not apply to a view",
                                rid, limit
                            ));
                            continue;
                        }
                        v.limits.insert(limit.clone(), bump);
                        v.default = false;
                        v.updated = Some(build.clone());
                    } else {
                        skipped.push(format!("bump_limit: unknown id {}", rid));
                        continue;
                    }
                    applied.push(Op::BumpLimit {
                        id: rid,
                        limit,
                        value,
                        provenance,
                    });
                }
                Op::PlaceAnchor {
                    id,
                    from,
                    to,
                    reevaluate,
                } => {
                    let rid = resolve(&remap, self, &id);
                    let locates = self.quote_locates(&to.doc, &to.section, &to.quote);
                    if let Some(r) = self.graph.requirements.get_mut(&rid) {
                        if r.source.as_ref().map(|s| normalize_statement(&s.quote))
                            != Some(normalize_statement(&to.quote))
                        {
                            dirt.revised(&rid, m, "quote");
                        }
                        let old_source =
                            r.source.replace(to.clone()).unwrap_or_else(|| from.clone());
                        r.provenance = None;
                        r.updated = Some(build.clone());
                        let ents = r.entities.clone();
                        dirt.entities(ents.iter(), m);
                        // Mentions derived from this source at commit follow it.
                        for e in self.graph.entities.values_mut() {
                            if let Some(i) = e.mentions.iter().position(|m| *m == old_source) {
                                if e.mentions.contains(&to) {
                                    e.mentions.remove(i);
                                } else {
                                    e.mentions[i] = to.clone();
                                }
                                e.updated = Some(build.clone());
                            }
                        }
                        // Flagged, or placed under a quote that does not locate: the
                        // reconcile session owes this anchor a decision.
                        if reevaluate || !locates {
                            if !self.status.reevaluate.contains(&rid) {
                                self.status.reevaluate.push(rid.clone());
                            }
                            dirt.stale_anchors.push((
                                format!("{}#{}", to.doc, to.section),
                                rid.clone(),
                                m,
                            ));
                        }
                        applied.push(Op::PlaceAnchor {
                            id: rid,
                            from,
                            to,
                            reevaluate,
                        });
                    } else if let Some(e) = self.graph.entities.get_mut(&rid) {
                        match e.mentions.iter().position(|m| *m == from) {
                            Some(i) => {
                                if e.mentions.contains(&to) {
                                    e.mentions.remove(i);
                                } else {
                                    e.mentions[i] = to.clone();
                                }
                                e.updated = Some(build.clone());
                                dirt.entity(&rid, m, "mentions");
                                applied.push(Op::PlaceAnchor {
                                    id: rid,
                                    from,
                                    to,
                                    reevaluate,
                                });
                            }
                            None => skipped.push(format!(
                                "place_anchor {}: no mention at {}#{}",
                                rid, from.doc, from.section
                            )),
                        }
                    } else {
                        skipped.push(format!("place_anchor: unknown id {}", rid));
                    }
                }
                Op::OrphanAnchor { id, from } => {
                    let rid = resolve(&remap, self, &id);
                    applied.push(Op::OrphanAnchor { id: rid, from });
                }
                Op::ReportDiagnostic { id, mut diagnostic } => {
                    diagnostic.subjects = diagnostic
                        .subjects
                        .iter()
                        .map(|s| resolve(&remap, self, s))
                        .collect();
                    // Sticky: an open diagnostic with the same rule and subjects is updated,
                    // not duplicated. Subject order does not matter: a pair reported from
                    // either endpoint is the same finding. An invented-choice finding also
                    // keys on its choice sentence, so two choices over the same subjects
                    // stay distinct. Human triage is never touched.
                    let subject_set =
                        |v: &[String]| -> BTreeSet<String> { v.iter().cloned().collect() };
                    let incoming_subjects = subject_set(&diagnostic.subjects);
                    let existing = self
                        .graph
                        .diagnostics
                        .iter()
                        .find(|(_, d)| {
                            d.rule == diagnostic.rule
                                && d.lifecycle == "open"
                                && subject_set(&d.subjects) == incoming_subjects
                                && (d.rule != "invented-choice"
                                    || invented_choice_key(&d.message)
                                        == invented_choice_key(&diagnostic.message))
                        })
                        .map(|(id, _)| id.clone());
                    match existing {
                        Some(did) => {
                            let d = self.graph.diagnostics.get_mut(&did).unwrap();
                            d.message = diagnostic.message;
                            d.severity = diagnostic.severity;
                            if diagnostic.reasoning.is_some() {
                                d.reasoning = diagnostic.reasoning;
                            }
                            // A human answer is final: the finding is never re-asked,
                            // so a re-report keeps the answered prompt as the record
                            // of what was asked. Unanswered, a fresh prompt replaces
                            // the question; a promptless re-report keeps the old one.
                            if d.answer.is_none() && diagnostic.prompt.is_some() {
                                d.prompt = diagnostic.prompt;
                                dirt.prompt(&did, m, "report_diagnostic");
                            }
                            d.updated = Some(build.clone());
                            // Journal the update as what it is: the merged diagnostic.
                            let merged = d.clone();
                            applied.push(Op::ReportDiagnostic {
                                id: did,
                                diagnostic: merged,
                            });
                        }
                        None => {
                            diagnostic.created = Some(build.clone());
                            diagnostic.updated = Some(build.clone());
                            let mut final_id = id.clone();
                            if final_id.is_empty() || self.graph.diagnostics.contains_key(&final_id)
                            {
                                final_id = self.mint_diag_id(
                                    &diagnostic.rule,
                                    &diagnostic.subjects,
                                    &BTreeSet::new(),
                                );
                            }
                            if diagnostic.prompt.is_some() && diagnostic.answer.is_none() {
                                dirt.prompt(&final_id, m, "report_diagnostic");
                            }
                            applied.push(Op::ReportDiagnostic {
                                id: final_id.clone(),
                                diagnostic: diagnostic.clone(),
                            });
                            self.graph.diagnostics.insert(final_id, diagnostic);
                        }
                    }
                }
                Op::ResolveDiagnostic { id, reason } => {
                    let rid = resolve(&remap, self, &id);
                    match self.graph.diagnostics.get_mut(&rid) {
                        Some(d) => {
                            d.lifecycle = "resolved".to_string();
                            // Resolving while an answer is being handled is the
                            // handling session finishing its work.
                            if let Some(a) = d.answer.as_mut() {
                                if a.status == "handling" {
                                    a.status = "handled".to_string();
                                }
                            }
                            d.updated = Some(build.clone());
                            applied.push(Op::ResolveDiagnostic { id: rid, reason });
                        }
                        None => skipped.push(format!("resolve_diagnostic: unknown id {}", rid)),
                    }
                }
                Op::TriageDiagnostic { id, triage } => {
                    let rid = resolve(&remap, self, &id);
                    match self.graph.diagnostics.get_mut(&rid) {
                        Some(d) => {
                            d.triage = triage.clone();
                            d.updated = Some(build.clone());
                            applied.push(Op::TriageDiagnostic { id: rid, triage });
                        }
                        None => skipped.push(format!("triage_diagnostic: unknown id {}", rid)),
                    }
                }
                Op::UpdateDiagnosticPrompt { id, prompt } => {
                    let rid = resolve(&remap, self, &id);
                    match self.graph.diagnostics.get_mut(&rid) {
                        Some(d) => {
                            d.prompt = prompt.clone();
                            d.updated = Some(build.clone());
                            if d.prompt.is_some() && d.answer.is_none() {
                                dirt.prompt(&rid, m, "update_diagnostic");
                            }
                            applied.push(Op::UpdateDiagnosticPrompt { id: rid, prompt });
                        }
                        None => skipped.push(format!("update_diagnostic: unknown id {}", rid)),
                    }
                }
                Op::AnswerDiagnostic { id, answer } => {
                    let rid = resolve(&remap, self, &id);
                    match self.graph.diagnostics.get_mut(&rid) {
                        Some(d) => {
                            d.answer = Some(answer.clone());
                            d.updated = Some(build.clone());
                            applied.push(Op::AnswerDiagnostic { id: rid, answer });
                        }
                        None => skipped.push(format!("answer_diagnostic: unknown id {}", rid)),
                    }
                }
                Op::EditDocProse {
                    doc,
                    section,
                    old_text,
                    new_text,
                    text,
                } => {
                    // The graph mutation paired with this edit lands in the same
                    // changeset; absorbing the new hashes here is its reconciliation,
                    // so the edit does not dirty the document it just reconciled.
                    // Mirrors docs/compiler/graph.md#mutations.
                    self.absorb_doc_edit(&doc, &text);
                    applied.push(Op::EditDocProse {
                        doc,
                        section,
                        old_text,
                        new_text,
                        text,
                    });
                }
                Op::ClassifyInput { doc, decision } => {
                    // Entity files omit requirement-derived mentions. The loaded
                    // cache must follow quote retractions and relocations in this
                    // changeset before the provenance gate examines it.
                    if let Some(cached) = &cached_mentions {
                        let current = reference_sources(&self.graph);
                        for (id, prior) in cached {
                            if let Some(entity) = self.graph.entities.get_mut(id) {
                                entity.mentions.retain(|source| {
                                    !prior.contains(source)
                                        || current.get(id).is_some_and(|live| live.contains(source))
                                });
                            }
                        }
                    }
                    match self.classified_input(&doc, &decision, true) {
                        Ok((hash, sections)) => {
                            if let Some(previous) = self.docs.get(&doc) {
                                for section in previous
                                    .sections
                                    .keys()
                                    .filter(|s| !sections.contains_key(*s))
                                {
                                    batch.push(
                                        applied.len() + 1,
                                        CHANGE_SECTION_REMOVED,
                                        &format!("{doc}#{section}"),
                                        "section",
                                        serde_json::json!({"doc":doc,"section":section}),
                                    );
                                }
                            }
                            for section in sections.keys() {
                                batch.push(
                                    applied.len() + 1,
                                    CHANGE_SECTION_DIRTY,
                                    &format!("{doc}#{section}"),
                                    "section",
                                    serde_json::json!({"doc":doc,"section":section}),
                                );
                            }
                            self.docs.insert(
                                doc.clone(),
                                DocRecord {
                                    content_hash: hash,
                                    sections,
                                    coverage: BTreeMap::new(),
                                },
                            );
                            self.decisions.inputs.insert(doc.clone(), decision.clone());
                            applied.push(Op::ClassifyInput { doc, decision });
                        }
                        Err(e) => skipped.push(format!("classify_input: {e}")),
                    }
                }
                Op::SetCoverage {
                    doc,
                    section,
                    state,
                    note,
                } => match self.docs.get_mut(&doc) {
                    Some(rec) if rec.sections.contains_key(&section) => {
                        rec.coverage.insert(
                            section.clone(),
                            Coverage {
                                state: state.clone(),
                                note: note.clone(),
                                claimed_by: Some(build.clone()),
                            },
                        );
                        applied.push(Op::SetCoverage {
                            doc,
                            section,
                            state,
                            note,
                        });
                    }
                    _ => skipped.push(format!("set_coverage: unknown section {}#{}", doc, section)),
                },
            }
        }

        // An all-or-nothing changeset with a skipped op lands nothing: the graph
        // returns to the snapshot and the refusals are the whole report.
        if let Some(snapshot) = snapshot {
            if !skipped.is_empty() {
                *self = snapshot;
                return CommitReport {
                    applied: 0,
                    skipped,
                    generation: self.status.generation,
                    changes: Vec::new(),
                    touched_entities: BTreeSet::new(),
                    changed_requirements: BTreeSet::new(),
                    swept: Vec::new(),
                };
            }
        }

        // Deletion propagation: the ops above may have removed diagnostic subjects, view
        // members, and derivation sources.
        let auto_resolved = self.propagate_deletions(&dirt.deleted, &build, &mut batch);
        applied.extend(auto_resolved);
        self.deletion_ripple(&dirt.deleted, &mut batch);

        // Derived data: relationships, state machines, default views, limit counts.
        crate::derive::recompute(self, &build, &mut batch);
        self.record_reparent_flips(&moves, &mut batch);

        // The typed dirtiness this commit caused. A review's own mutations on its
        // target write no record for it: the review saw the result it staged, and a
        // definition refresh must not reopen the review that made it.
        // Mirrors docs/compiler/graph.md#change-records and
        // docs/compiler/goals/review-entity.md#created-when.
        let reviewed: BTreeSet<&str> = commit
            .resolved
            .iter()
            .filter_map(|r| r.goal.strip_prefix("g:review-entity:"))
            .collect();
        // `human_confirmed` (a dual write or an accepted proposal) skips the whole
        // list: a revised statement still writes `requirement-revised` below.
        if !human_confirmed {
            for (id, (m, via)) in &dirt.entities {
                if self.graph.entities.contains_key(id) && !reviewed.contains(id.as_str()) {
                    batch.push(*m, CHANGE_ENTITY, id, via, serde_json::Value::Null);
                }
            }
        }
        for (id, m) in &dirt.created {
            if self.graph.requirements.contains_key(id) {
                batch.push(
                    *m,
                    CHANGE_REQ_CREATED,
                    id,
                    "section",
                    serde_json::Value::Null,
                );
            } else if self.graph.criteria.contains_key(id) {
                batch.push(
                    *m,
                    CHANGE_CRIT_CREATED,
                    id,
                    "requirement",
                    serde_json::Value::Null,
                );
            }
        }
        for (id, (m, via)) in &dirt.revised {
            if dirt.created.contains_key(id) {
                continue;
            }
            if self.graph.requirements.contains_key(id) {
                batch.push(*m, CHANGE_REQ_REVISED, id, via, serde_json::Value::Null);
            } else if self.graph.criteria.contains_key(id) {
                batch.push(*m, CHANGE_CRIT_REVISED, id, via, serde_json::Value::Null);
            }
        }
        for (id, m) in &dirt.deleted {
            let kind = match id_kind(id) {
                "requirement" => CHANGE_REQ_DELETED,
                "criterion" => CHANGE_CRIT_DELETED,
                "entity" => CHANGE_ENTITY_DELETED,
                _ => continue,
            };
            batch.push(*m, kind, id, "fields", serde_json::Value::Null);
        }
        // Instances: an instance touched (via the instance's own change), or the type
        // of one changed, which reaches the instance through the type's attributes.
        // Mirrors docs/compiler/goals/conform-instance.md#created-when.
        let types = crate::derive::instance_types(self);
        for (id, (m, via)) in &dirt.entities {
            if types.contains_key(id) {
                batch.push(*m, CHANGE_INSTANCE, id, via, serde_json::Value::Null);
            }
            for (inst, ty) in &types {
                if ty == id {
                    batch.push(
                        *m,
                        CHANGE_INSTANCE,
                        inst,
                        "attributes",
                        serde_json::json!({ "type": ty }),
                    );
                }
            }
        }
        for (id, m) in &dirt.named_reqs {
            if let Some(r) = self.graph.requirements.get(id) {
                if r.entities.len() >= 2 && r.edges.is_empty() {
                    batch.push(
                        *m,
                        CHANGE_EDGES_MISSING,
                        id,
                        "edges",
                        serde_json::Value::Null,
                    );
                }
            }
        }
        for (id, (m, kind)) in &dirt.provenance_pending {
            if self.node_exists(id) {
                batch.push(
                    *m,
                    CHANGE_PROVENANCE_PENDING,
                    id,
                    "provenance",
                    serde_json::json!({ "provenance": kind }),
                );
            }
        }
        // The commit that lands a derived or decreed fact files its ratification
        // proposal, mechanically: one `ratification-pending` diagnostic whose prompt
        // the blocked `ratify` goal surfaces. A fact whose proposal is already open
        // (a decree path staged one in this changeset, a prior commit filed one)
        // keeps it. The sentence follows the composition rules: a requirement's
        // `statement` verbatim, an entity's name and `definition` as one sentence.
        // Mirrors docs/consumers/docsgen.md#ratification-proposals.
        let pending: Vec<(String, usize)> = dirt
            .provenance_pending
            .iter()
            .map(|(id, (m, _))| (id.clone(), *m))
            .collect();
        for (id, m) in pending {
            if !self.node_exists(&id) {
                continue;
            }
            let open = self.graph.diagnostics.values().any(|d| {
                d.lifecycle == "open"
                    && d.rule == "ratification-pending"
                    && d.subjects.iter().any(|s| s == &id)
            });
            if open {
                continue;
            }
            let (sentence, prov, entities) = if let Some(r) = self.graph.requirements.get(&id) {
                (
                    r.statement.clone(),
                    r.provenance.clone(),
                    r.entities.clone(),
                )
            } else if let Some(e) = self.graph.entities.get(&id) {
                let sentence = match e.definition.as_deref() {
                    Some(d) => format!("{}: {}", e.name, d),
                    None => e.name.clone(),
                };
                (
                    sentence,
                    e.provenance.clone(),
                    e.parent.clone().into_iter().collect(),
                )
            } else if let Some(c) = self.graph.criteria.get(&id) {
                // The sentence lands under the owning requirement's section
                // (docs/compiler/model/criterion.md#lifecycle).
                (
                    c.statement.clone(),
                    c.provenance.clone(),
                    vec![c.requirement.clone()],
                )
            } else {
                continue;
            };
            let Some(prov) = prov else {
                continue;
            };
            let mut diagnostic =
                self.ratification_proposal(&id, &sentence, &prov, None, &entities, None);
            diagnostic.created = Some(build.clone());
            diagnostic.updated = Some(build.clone());
            let did = self.mint_diag_id(
                "ratification-pending",
                &diagnostic.subjects,
                &BTreeSet::new(),
            );
            dirt.prompt(&did, m, "ratification");
            applied.push(Op::ReportDiagnostic {
                id: did.clone(),
                diagnostic: diagnostic.clone(),
            });
            self.graph.diagnostics.insert(did, diagnostic);
        }
        applied.extend(self.combine_criterion_proposals(&build));
        for (id, (m, via)) in &dirt.prompts {
            batch.push(
                *m,
                CHANGE_PROMPT_UNANSWERED,
                id,
                via,
                serde_json::Value::Null,
            );
        }
        for (section, anchor, m) in &dirt.stale_anchors {
            self.push_stale_anchor(&mut batch, *m, section, anchor);
        }

        // A decided proposal leaves its document's alignment block; an emptied block
        // goes. A resolved place-anchors goal clears its document's block outright.
        let decided: Vec<(String, String)> = applied
            .iter()
            .filter_map(|o| match o {
                Op::PlaceAnchor { id, from, .. } | Op::OrphanAnchor { id, from } => {
                    Some((id.clone(), format!("{}#{}", from.doc, from.section)))
                }
                _ => None,
            })
            .collect();
        for b in self.status.alignment.iter_mut() {
            b.proposals.retain(|p| {
                !decided
                    .iter()
                    .any(|(id, from)| *id == p.anchor && *from == p.from)
            });
        }
        self.status.alignment.retain(|b| !b.proposals.is_empty());
        // An anchor under re-evaluation is addressed by an update, delete, or re-record
        // on its id, or by the reconcile-section goal of its section.
        let addressed: BTreeSet<String> = dirt
            .named_reqs
            .keys()
            .chain(dirt.deleted.keys())
            .cloned()
            .collect();
        let resolved_targets: Vec<(&str, &str)> = commit
            .resolved
            .iter()
            .filter_map(|r| {
                r.goal
                    .strip_prefix("g:")
                    .and_then(|rest| rest.split_once(':'))
            })
            .collect();
        for (kind, target) in &resolved_targets {
            if *kind == "place-anchors" {
                self.status.alignment.retain(|b| b.doc != *target);
            }
        }
        let reqs = &self.graph.requirements;
        self.status.reevaluate.retain(|rid| {
            if addressed.contains(rid) {
                return false;
            }
            let Some(src) = reqs.get(rid).and_then(|r| r.source.as_ref()) else {
                return false;
            };
            !resolved_targets.iter().any(|(kind, target)| {
                *kind == "reconcile-section"
                    && (*target == src.doc || *target == format!("{}#{}", src.doc, src.section))
            })
        });

        self.status.generation = generation;
        if commit.kind == "session" {
            self.status.spent.sessions += 1;
        }
        self.status.spent.rounds += commit.rounds as u64;
        self.status.spent.tokens += commit.tokens.unwrap_or(0);
        // An accepted pair verdict marks the partner judged on every created or
        // revised record of either side that stood before this commit; the records
        // this commit lands start with an empty list, so a side repaired in the
        // same session derives the pair again with the new cause. The prune then
        // clears a record with no candidate left.
        // Mirrors docs/compiler/graph.md#change-records.
        self.mark_pairs_judged(&commit.resolved);
        self.settle_lookalikes(&commit.resolved, generation, &mut batch);
        self.prune_records();
        let changes = self.commit_records(batch);
        self.mark_answered_pairs_judged(&applied);
        let mut mutations: Vec<serde_json::Value> = applied
            .iter()
            .map(|o| serde_json::to_value(o).unwrap_or_default())
            .collect();
        // A mutation that decreed over a quoted value carries the prior value and
        // source it replaced; one that moved a parent carries the prior parent.
        // Mirrors docs/compiler/graph.md#journal.
        for (i, extra) in annotations {
            let (Some(mv), Some(fields)) = (mutations.get_mut(i), extra.as_object()) else {
                continue;
            };
            for (k, v) in fields {
                mv[k] = v.clone();
            }
        }
        let entry = self.journal_entry(&build, commit, mutations);
        for synced in &sync_entries {
            self.write_journal(synced);
        }
        self.write_journal(&entry);
        self.persist();
        // The renderer redraws the views the commit touched; a view whose emitted
        // `.puml` matches the file on disk is skipped.
        // Mirrors docs/compiler/diagrams.md#rendering.
        crate::render::render_all(self, &self.out);
        // The sweep runs at every commit, whichever path committed: a session's
        // done, a decree, a dual write, a ratification, an answer, a triage. It
        // lands behind this commit's entry under the same lock, as entries of its
        // own when it did anything. Mirrors docs/compiler/graph.md#changesets.
        let swept = self.sweep_locked();
        CommitReport {
            applied: applied.len(),
            skipped,
            generation,
            changes,
            touched_entities: dirt.entities.keys().cloned().collect(),
            changed_requirements: dirt
                .created
                .keys()
                .chain(dirt.revised.keys())
                .cloned()
                .collect(),
            swept,
        }
    }
}
