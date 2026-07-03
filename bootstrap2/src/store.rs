// The graph store: persistent home of the semantic graph. Owns identifiers, enforces
// invariants at commit, records every change. Mirrors docs2/compiler/graph.md.
use crate::md;
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// One staged mutation. Serialized into the journal as written.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    CreateEntity { id: String, entity: Entity },
    UpdateEntity {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        add_aliases: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        add_mention: Option<SourceRef>,
    },
    DeleteEntity { id: String, reason: String },
    MergeEntities { keep: String, absorb: String, reason: String },
    CreateRequirement { id: String, requirement: Requirement },
    UpdateRequirement {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ears: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entities: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edges: Option<Vec<ReqEdge>>,
    },
    DeleteRequirement { id: String, reason: String },
    ReportDiagnostic { id: String, diagnostic: Diagnostic },
    ResolveDiagnostic { id: String, reason: String },
    SetCoverage {
        doc: String,
        section: String,
        state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}

pub struct CommitReport {
    pub applied: usize,
    pub skipped: Vec<String>,
    // Final entity ids touched by this commit (for scheduling review turns).
    pub touched_entities: BTreeSet<String>,
}

// A document that changed, with what a reconcile turn needs to know.
#[derive(Clone, Debug)]
pub struct DirtyDoc {
    pub doc: String,
    pub dirty_sections: Vec<String>,
    pub stale_anchors: Vec<String>,
}

#[derive(Clone, Default)]
pub struct Store {
    pub out: PathBuf,
    pub graph: Graph,
    pub docs: BTreeMap<String, DocRecord>,
    pub status: Status,
}

fn normalize(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

// Punctuation-insensitive statement normalization: the requirement natural key. A comma
// or spacing edit to a sentence keeps matching its existing requirement.
fn normalize_statement(s: &str) -> String {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// Whitespace-insensitive containment: a quote wrapped across source lines still locates.
pub fn text_contains(hay: &str, needle: &str) -> bool {
    let h = hay.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = needle.split_whitespace().collect::<Vec<_>>().join(" ");
    !n.is_empty() && h.contains(&n)
}

fn yaml_to<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_norway::from_str(&text).ok()
}

fn write_yaml<T: Serialize>(path: &Path, value: &T) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(text) = serde_norway::to_string(value) {
        std::fs::write(path, text).ok();
    }
}

impl Store {
    pub fn load(out: &Path) -> Store {
        let g = out.join("graph");
        let mut store = Store {
            out: out.to_path_buf(),
            graph: Graph {
                entities: yaml_to(&g.join("entities.yaml")).unwrap_or_default(),
                requirements: yaml_to(&g.join("requirements.yaml")).unwrap_or_default(),
                relationships: yaml_to(&g.join("relationships.yaml")).unwrap_or_default(),
                diagnostics: yaml_to(&g.join("diagnostics.yaml")).unwrap_or_default(),
                redirects: yaml_to(&g.join("redirects.yaml")).unwrap_or_default(),
            },
            docs: BTreeMap::new(),
            status: yaml_to(&out.join("status.yaml")).unwrap_or_default(),
        };
        let docs_dir = out.join("docs");
        let mut files = Vec::new();
        collect_yaml(&docs_dir, &mut files);
        for f in files {
            if let Ok(rel) = f.strip_prefix(&docs_dir) {
                let doc = rel.to_string_lossy().replace('\\', "/");
                let doc = doc.strip_suffix(".yaml").unwrap_or(&doc).to_string();
                if let Some(rec) = yaml_to::<DocRecord>(&f) {
                    store.docs.insert(doc, rec);
                }
            }
        }
        store
    }

    pub fn save(&self) {
        let g = self.out.join("graph");
        write_yaml(&g.join("entities.yaml"), &self.graph.entities);
        write_yaml(&g.join("requirements.yaml"), &self.graph.requirements);
        write_yaml(&g.join("relationships.yaml"), &self.graph.relationships);
        write_yaml(&g.join("diagnostics.yaml"), &self.graph.diagnostics);
        write_yaml(&g.join("redirects.yaml"), &self.graph.redirects);
        write_yaml(&self.out.join("status.yaml"), &self.status);
        for (doc, rec) in &self.docs {
            write_yaml(&self.out.join("docs").join(format!("{}.yaml", doc)), rec);
        }
    }

    pub fn save_status(&self) {
        write_yaml(&self.out.join("status.yaml"), &self.status);
    }

    // Follow merge redirects to the surviving id. A tombstone (empty target) stays dead.
    pub fn resolve_id<'a>(&'a self, id: &'a str) -> &'a str {
        let mut cur = id;
        let mut hops = 0;
        while let Some(next) = self.graph.redirects.get(cur) {
            if next.is_empty() || hops > 8 {
                return cur;
            }
            cur = next;
            hops += 1;
        }
        cur
    }

    // ---- id minting ----

    pub fn mint_entity_id(&self, name: &str, taken: &BTreeSet<String>) -> String {
        let base = format!("ent:{}", md::slug(name));
        let mut id = base.clone();
        let mut n = 1;
        while self.graph.entities.contains_key(&id) || self.graph.redirects.contains_key(&id) || taken.contains(&id) {
            n += 1;
            id = format!("{}-{}", base, n);
        }
        id
    }

    pub fn mint_req_id(&self, doc: &str, taken: &BTreeSet<String>) -> String {
        let stem = doc.rsplit('/').next().unwrap_or(doc);
        let stem = md::slug(stem.strip_suffix(".md").unwrap_or(stem));
        let prefix = format!("req:{}-", stem);
        let mut max = 0usize;
        for id in self.graph.requirements.keys().chain(taken.iter()) {
            if let Some(rest) = id.strip_prefix(&prefix) {
                if let Ok(n) = rest.parse::<usize>() {
                    max = max.max(n);
                }
            }
        }
        format!("{}{}", prefix, max + 1)
    }

    pub fn mint_diag_id(&self, rule: &str, taken: &BTreeSet<String>) -> String {
        let prefix = format!("diag:{}-", md::slug(rule));
        let mut max = 0usize;
        for id in self.graph.diagnostics.keys().chain(taken.iter()) {
            if let Some(rest) = id.strip_prefix(&prefix) {
                if let Ok(n) = rest.parse::<usize>() {
                    max = max.max(n);
                }
            }
        }
        format!("{}{}", prefix, max + 1)
    }

    // ---- lookups ----

    // Natural-key lookup: normalized name or alias, within the same scope.
    pub fn find_natural(&self, name: &str, scope: &str) -> Option<String> {
        let want = normalize(name);
        for (id, e) in &self.graph.entities {
            if e.scope != scope {
                continue;
            }
            if normalize(&e.name) == want || e.aliases.iter().any(|a| normalize(a) == want) {
                return Some(id.clone());
            }
        }
        None
    }

    // Deterministic search over names and aliases: exact, then substring, then token overlap.
    pub fn search(&self, query: &str) -> Vec<(String, String, String)> {
        let q = normalize(query);
        let q_tokens: BTreeSet<&str> = q.split(' ').collect();
        let mut scored: Vec<(u32, String, String, String)> = Vec::new();
        for (id, e) in &self.graph.entities {
            let mut names = vec![normalize(&e.name)];
            names.extend(e.aliases.iter().map(|a| normalize(a)));
            let mut best: Option<u32> = None;
            for n in &names {
                let tier = if *n == q {
                    Some(0)
                } else if n.contains(&q) || q.contains(n.as_str()) {
                    Some(1)
                } else {
                    let n_tokens: BTreeSet<&str> = n.split(' ').collect();
                    let overlap = q_tokens.intersection(&n_tokens).count();
                    if overlap > 0 {
                        Some(2)
                    } else {
                        None
                    }
                };
                if let Some(t) = tier {
                    best = Some(best.map_or(t, |b: u32| b.min(t)));
                }
            }
            if let Some(t) = best {
                scored.push((t, id.clone(), e.name.clone(), e.definition.clone().unwrap_or_default()));
            }
        }
        scored.sort();
        scored.into_iter().take(8).map(|(_, id, n, d)| (id, n, d)).collect()
    }

    // Whether a quote locates inside the named section (whitespace-insensitive).
    pub fn quote_locates(&self, doc: &str, section: &str, quote: &str) -> bool {
        self.docs
            .get(doc)
            .and_then(|d| d.sections.get(section))
            .map(|s| text_contains(&s.raw, quote))
            .unwrap_or(false)
    }

    pub fn requirements_referencing(&self, entity_id: &str) -> Vec<String> {
        self.graph
            .requirements
            .iter()
            .filter(|(_, r)| r.entities.iter().any(|e| self.resolve_id(e) == entity_id))
            .map(|(id, _)| id.clone())
            .collect()
    }

    // ---- commit ----

    // Apply a staged changeset atomically: reconcile creates by natural key against nodes
    // committed concurrently, apply ops in order, recompute derived relationships, journal,
    // bump the generation, write shards.
    pub fn apply(&mut self, ops: Vec<Op>, work_item: &WorkItem, rounds: u32, tokens: u64) -> CommitReport {
        let _flock = FileLock::acquire(&self.out);
        let build = format!("g{}", self.status.generation + 1);
        let mut remap: BTreeMap<String, String> = BTreeMap::new();
        let mut skipped: Vec<String> = Vec::new();
        let mut touched: BTreeSet<String> = BTreeSet::new();
        let mut applied: Vec<Op> = Vec::new();

        let resolve = |remap: &BTreeMap<String, String>, store: &Store, id: &str| -> String {
            let id = remap.get(id).cloned().unwrap_or_else(|| id.to_string());
            store.resolve_id(&id).to_string()
        };

        for op in ops {
            match op {
                Op::CreateEntity { id, mut entity } => {
                    entity.created = Some(build.clone());
                    entity.updated = Some(build.clone());
                    // Commit-time natural-key reconciliation: a create whose key now matches
                    // an existing node becomes an update, with mentions unioned.
                    if let Some(existing) = self.find_natural(&entity.name, &entity.scope) {
                        remap.insert(id.clone(), existing.clone());
                        let e = self.graph.entities.get_mut(&existing).unwrap();
                        for m in entity.mentions {
                            if !e.mentions.contains(&m) {
                                e.mentions.push(m);
                            }
                        }
                        if e.definition.as_deref().unwrap_or("").is_empty() {
                            e.definition = entity.definition.clone();
                        }
                        for a in entity.aliases {
                            if !e.aliases.contains(&a) {
                                e.aliases.push(a);
                            }
                        }
                        e.updated = Some(build.clone());
                        touched.insert(existing.clone());
                        applied.push(Op::UpdateEntity {
                            id: existing,
                            name: None,
                            definition: entity.definition,
                            add_aliases: Vec::new(),
                            add_mention: None,
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
                        touched.insert(final_id.clone());
                        applied.push(Op::CreateEntity { id: final_id.clone(), entity: entity.clone() });
                        self.graph.entities.insert(final_id, entity);
                    }
                }
                Op::UpdateEntity { id, name, definition, add_aliases, add_mention } => {
                    let rid = resolve(&remap, self, &id);
                    match self.graph.entities.get_mut(&rid) {
                        Some(e) => {
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
                            if let Some(m) = &add_mention {
                                if !e.mentions.contains(m) {
                                    e.mentions.push(m.clone());
                                }
                            }
                            e.updated = Some(build.clone());
                            touched.insert(rid.clone());
                            applied.push(Op::UpdateEntity { id: rid, name, definition, add_aliases, add_mention });
                        }
                        None => skipped.push(format!("update_entity: unknown id {}", rid)),
                    }
                }
                Op::DeleteEntity { id, reason } => {
                    let rid = resolve(&remap, self, &id);
                    if !self.requirements_referencing(&rid).is_empty() {
                        skipped.push(format!("delete_entity {}: still referenced", rid));
                    } else if self.graph.entities.remove(&rid).is_some() {
                        self.graph.redirects.insert(rid.clone(), String::new());
                        touched.insert(rid.clone());
                        applied.push(Op::DeleteEntity { id: rid, reason });
                    } else {
                        skipped.push(format!("delete_entity: unknown id {}", rid));
                    }
                }
                Op::MergeEntities { keep, absorb, reason } => {
                    let keep = resolve(&remap, self, &keep);
                    let absorb = resolve(&remap, self, &absorb);
                    if keep == absorb || !self.graph.entities.contains_key(&keep) {
                        skipped.push(format!("merge_entities: bad pair {} {}", keep, absorb));
                        continue;
                    }
                    let Some(ab) = self.graph.entities.remove(&absorb) else {
                        skipped.push(format!("merge_entities: unknown id {}", absorb));
                        continue;
                    };
                    for r in self.graph.requirements.values_mut() {
                        for e in r.entities.iter_mut() {
                            if *e == absorb {
                                *e = keep.clone();
                            }
                        }
                        r.entities.dedup();
                        for edge in r.edges.iter_mut() {
                            if edge.a == absorb {
                                edge.a = keep.clone();
                            }
                            if edge.b == absorb {
                                edge.b = keep.clone();
                            }
                        }
                        r.edges.retain(|e| e.a != e.b);
                    }
                    for d in self.graph.diagnostics.values_mut() {
                        for s in d.subjects.iter_mut() {
                            if *s == absorb {
                                *s = keep.clone();
                            }
                        }
                        d.subjects.dedup();
                    }
                    {
                        let k = self.graph.entities.get_mut(&keep).unwrap();
                        if !k.aliases.contains(&ab.name) && normalize(&ab.name) != normalize(&k.name) {
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
                        if k.definition.as_deref().unwrap_or("").is_empty() {
                            k.definition = ab.definition;
                        }
                        k.updated = Some(build.clone());
                    }
                    self.graph.redirects.insert(absorb.clone(), keep.clone());
                    touched.insert(keep.clone());
                    applied.push(Op::MergeEntities { keep, absorb, reason });
                }
                Op::CreateRequirement { id, mut requirement } => {
                    requirement.entities = requirement.entities.iter().map(|e| resolve(&remap, self, e)).collect();
                    requirement.entities.dedup();
                    for edge in requirement.edges.iter_mut() {
                        edge.a = resolve(&remap, self, &edge.a);
                        edge.b = resolve(&remap, self, &edge.b);
                    }
                    if let Some(missing) = requirement.entities.iter().find(|e| !self.graph.entities.contains_key(*e)) {
                        skipped.push(format!("create_requirement {}: unknown entity {}", id, missing));
                        continue;
                    }
                    // Natural key for requirements: source section plus the punctuation-
                    // insensitive statement. A same-statement create becomes an update,
                    // never a duplicate; a lightly reworded statement refreshes in place.
                    if let Some(existing) = self
                        .graph
                        .requirements
                        .iter()
                        .find(|(_, r)| {
                            r.source.doc == requirement.source.doc
                                && r.source.section == requirement.source.section
                                && normalize_statement(&r.ears) == normalize_statement(&requirement.ears)
                        })
                        .map(|(rid, _)| rid.clone())
                    {
                        remap.insert(id, existing.clone());
                        let r = self.graph.requirements.get_mut(&existing).unwrap();
                        for e in &requirement.entities {
                            if !r.entities.contains(e) {
                                r.entities.push(e.clone());
                            }
                        }
                        for edge in requirement.edges {
                            if !r.edges.iter().any(|x| (x.a == edge.a && x.b == edge.b) || (x.a == edge.b && x.b == edge.a)) {
                                r.edges.push(edge);
                            }
                        }
                        // The matched statement's ears and quote refresh in place (same
                        // statement modulo punctuation); the id never churns.
                        if r.ears != requirement.ears {
                            r.ears = requirement.ears.clone();
                        }
                        if r.source.quote != requirement.source.quote {
                            r.source = requirement.source.clone();
                        }
                        r.updated = Some(build.clone());
                        touched.extend(r.entities.iter().cloned());
                        continue;
                    }
                    requirement.created = Some(build.clone());
                    requirement.updated = Some(build.clone());
                    let mut final_id = id.clone();
                    if !final_id.starts_with("req:") || self.graph.requirements.contains_key(&final_id) {
                        final_id = self.mint_req_id(&requirement.source.doc, &BTreeSet::new());
                    }
                    if final_id != id {
                        remap.insert(id, final_id.clone());
                    }
                    touched.extend(requirement.entities.iter().cloned());
                    // A committed requirement adds its source as a mention on every entity
                    // it references, so reuse accumulates cross-document presence.
                    for e in &requirement.entities {
                        if let Some(ent) = self.graph.entities.get_mut(e) {
                            if !ent.mentions.contains(&requirement.source) {
                                ent.mentions.push(requirement.source.clone());
                                ent.updated = Some(build.clone());
                            }
                        }
                    }
                    applied.push(Op::CreateRequirement { id: final_id.clone(), requirement: requirement.clone() });
                    self.graph.requirements.insert(final_id, requirement);
                }
                Op::UpdateRequirement { id, ears, entities, edges } => {
                    let rid = resolve(&remap, self, &id);
                    let resolved_entities = entities
                        .map(|es| es.iter().map(|e| resolve(&remap, self, e)).collect::<Vec<_>>());
                    match self.graph.requirements.get_mut(&rid) {
                        Some(r) => {
                            if let Some(e) = &ears {
                                r.ears = e.clone();
                            }
                            if let Some(es) = &resolved_entities {
                                r.entities = es.clone();
                            }
                            if let Some(ed) = &edges {
                                r.edges = ed.clone();
                            }
                            r.updated = Some(build.clone());
                            touched.extend(r.entities.iter().cloned());
                            applied.push(Op::UpdateRequirement { id: rid, ears, entities: resolved_entities, edges });
                        }
                        None => skipped.push(format!("update_requirement: unknown id {}", rid)),
                    }
                }
                Op::DeleteRequirement { id, reason } => {
                    let rid = resolve(&remap, self, &id);
                    match self.graph.requirements.remove(&rid) {
                        Some(r) => {
                            touched.extend(r.entities.iter().cloned());
                            applied.push(Op::DeleteRequirement { id: rid, reason });
                        }
                        None => skipped.push(format!("delete_requirement: unknown id {}", rid)),
                    }
                }
                Op::ReportDiagnostic { id, mut diagnostic } => {
                    diagnostic.subjects = diagnostic.subjects.iter().map(|s| resolve(&remap, self, s)).collect();
                    // Sticky: an open diagnostic with the same rule and subjects is updated,
                    // not duplicated. Human triage is never touched.
                    let existing = self
                        .graph
                        .diagnostics
                        .iter()
                        .find(|(_, d)| {
                            d.rule == diagnostic.rule && d.lifecycle == "open" && d.subjects == diagnostic.subjects
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
                            d.updated = Some(build.clone());
                            applied.push(Op::UpdateEntity {
                                id: did,
                                name: None,
                                definition: None,
                                add_aliases: Vec::new(),
                                add_mention: None,
                            });
                        }
                        None => {
                            diagnostic.created = Some(build.clone());
                            diagnostic.updated = Some(build.clone());
                            let mut final_id = id.clone();
                            if final_id.is_empty() || self.graph.diagnostics.contains_key(&final_id) {
                                final_id = self.mint_diag_id(&diagnostic.rule, &BTreeSet::new());
                            }
                            applied.push(Op::ReportDiagnostic { id: final_id.clone(), diagnostic: diagnostic.clone() });
                            self.graph.diagnostics.insert(final_id, diagnostic);
                        }
                    }
                }
                Op::ResolveDiagnostic { id, reason } => {
                    let rid = resolve(&remap, self, &id);
                    match self.graph.diagnostics.get_mut(&rid) {
                        Some(d) => {
                            d.lifecycle = "resolved".to_string();
                            d.updated = Some(build.clone());
                            applied.push(Op::ResolveDiagnostic { id: rid, reason });
                        }
                        None => skipped.push(format!("resolve_diagnostic: unknown id {}", rid)),
                    }
                }
                Op::SetCoverage { doc, section, state, note } => {
                    match self.docs.get_mut(&doc) {
                        Some(rec) if rec.sections.contains_key(&section) => {
                            rec.coverage.insert(
                                section.clone(),
                                Coverage { state: state.clone(), note: note.clone(), claimed_by: Some(build.clone()) },
                            );
                            applied.push(Op::SetCoverage { doc, section, state, note });
                        }
                        _ => skipped.push(format!("set_coverage: unknown section {}#{}", doc, section)),
                    }
                }
            }
        }

        self.recompute_relationships();
        self.status.generation += 1;
        self.status.spent.turns += 1;
        self.status.spent.rounds += rounds as u64;
        self.status.spent.tokens += tokens;
        let entry = JournalEntry {
            build: build.clone(),
            work_item: work_item.clone(),
            mutations: applied.iter().map(|o| serde_json::to_value(o).unwrap_or_default()).collect(),
            rounds,
            tokens,
        };
        write_yaml(
            &self.out.join("journal").join(format!("{}.yaml", build)),
            &entry,
        );
        self.save();
        CommitReport { applied: applied.len(), skipped, touched_entities: touched }
    }

    // Relationships are a materialized view over requirements: group requirement edges by
    // entity pair, union the contributing requirements, keep the strongest implied type.
    pub fn recompute_relationships(&mut self) {
        let mut edges: BTreeMap<String, Relationship> = BTreeMap::new();
        for (rid, r) in &self.graph.requirements {
            for e in &r.edges {
                let a = self.resolve_id(&e.a).to_string();
                let b = self.resolve_id(&e.b).to_string();
                if a == b || !self.graph.entities.contains_key(&a) || !self.graph.entities.contains_key(&b) {
                    continue;
                }
                let (x, y) = if a <= b { (&a, &b) } else { (&b, &a) };
                let key = format!(
                    "rel:{}~{}",
                    x.strip_prefix("ent:").unwrap_or(x),
                    y.strip_prefix("ent:").unwrap_or(y)
                );
                let t = e.rel_type.clone().unwrap_or_else(|| "reference".to_string());
                let entry = edges.entry(key).or_insert_with(|| Relationship {
                    rel_type: "reference".to_string(),
                    members: vec![x.clone(), y.clone()],
                    requirements: Vec::new(),
                });
                if rel_rank(&t) < rel_rank(&entry.rel_type) {
                    entry.rel_type = t;
                }
                if !entry.requirements.contains(rid) {
                    entry.requirements.push(rid.clone());
                }
            }
        }
        self.graph.relationships = edges;
    }

    // ---- document sync (the dirty set) ----

    // Bring the stored document records in line with a fresh parse. Returns the dirty work.
    // Moves (same hash, new reference) rewrite anchored references mechanically and are not
    // dirty. Coverage carries over only for unchanged sections.
    pub fn sync_docs(&mut self, parsed: &BTreeMap<String, (String, BTreeMap<String, Section>)>) -> Vec<DirtyDoc> {
        let mut out = Vec::new();
        // Documents that disappeared from the project entirely.
        let gone: Vec<String> = self.docs.keys().filter(|d| !parsed.contains_key(*d)).cloned().collect();
        for doc in gone {
            let stale = self.anchors_in_doc(&doc, None);
            self.docs.remove(&doc);
            std::fs::remove_file(self.out.join("docs").join(format!("{}.yaml", doc))).ok();
            if !stale.is_empty() {
                out.push(DirtyDoc { doc, dirty_sections: Vec::new(), stale_anchors: stale });
            }
        }
        for (doc, (content_hash, sections)) in parsed {
            let old = self.docs.get(doc).cloned().unwrap_or_default();
            if old.content_hash == *content_hash {
                continue;
            }
            // Detect moves: an old reference whose hash reappears under a new reference.
            let mut moves: Vec<(String, String)> = Vec::new();
            for (old_ref, old_sec) in &old.sections {
                if sections.contains_key(old_ref) {
                    continue;
                }
                if let Some((new_ref, _)) = sections
                    .iter()
                    .find(|(r, s)| s.hash == old_sec.hash && !old.sections.contains_key(*r))
                {
                    moves.push((old_ref.clone(), new_ref.clone()));
                }
            }
            for (from, to) in &moves {
                self.rewrite_section_refs(doc, from, to);
            }
            let moved_from: BTreeSet<&String> = moves.iter().map(|(f, _)| f).collect();
            let moved_to: BTreeMap<&String, &String> =
                moves.iter().map(|(f, t)| (t, f)).collect();

            // Dirty: new or changed sections (a moved section is neither).
            let mut dirty: Vec<String> = Vec::new();
            for (r, s) in sections {
                match old.sections.get(r) {
                    Some(o) if o.hash == s.hash => {}
                    _ if moved_to.contains_key(r) => {}
                    _ => dirty.push(r.clone()),
                }
            }
            // Removed: old sections gone from the new parse (excluding moves).
            let removed: Vec<String> = old
                .sections
                .keys()
                .filter(|r| !sections.contains_key(*r) && !moved_from.contains(*r))
                .cloned()
                .collect();
            let mut stale = Vec::new();
            for r in &removed {
                stale.extend(self.anchors_in_doc(doc, Some(r)));
            }
            // Also stale: anchors whose section changed and whose quote no longer locates.
            for r in &dirty {
                for a in self.anchors_in_doc(doc, Some(r)) {
                    let ok = match a.split(':').next() {
                        Some("req") => {
                            let q = &self.graph.requirements[&a].source.quote;
                            sections.get(r).map(|s| text_contains(&s.raw, q)).unwrap_or(false)
                        }
                        _ => true,
                    };
                    if !ok {
                        stale.push(a);
                    }
                }
            }
            stale.sort();
            stale.dedup();

            // Carry coverage only for sections whose content is unchanged.
            let mut coverage = BTreeMap::new();
            for (r, c) in &old.coverage {
                if let (Some(o), Some(n)) = (old.sections.get(r), sections.get(r)) {
                    if o.hash == n.hash {
                        coverage.insert(r.clone(), c.clone());
                    }
                }
            }
            // A moved section keeps its coverage under the new reference.
            for (from, to) in &moves {
                if let Some(c) = old.coverage.get(from) {
                    coverage.insert(to.clone(), c.clone());
                }
            }
            self.docs.insert(
                doc.clone(),
                DocRecord { content_hash: content_hash.clone(), sections: sections.clone(), coverage },
            );
            if !dirty.is_empty() || !stale.is_empty() {
                dirty.sort();
                out.push(DirtyDoc { doc: doc.clone(), dirty_sections: dirty, stale_anchors: stale });
            }
        }
        // Persist the synced records so context reads see the new sections.
        self.save();
        out
    }

    // Node ids anchored to a document (optionally to one section of it).
    fn anchors_in_doc(&self, doc: &str, section: Option<&str>) -> Vec<String> {
        let mut out = Vec::new();
        for (id, r) in &self.graph.requirements {
            if r.source.doc == doc && section.map(|s| r.source.section == s).unwrap_or(true) {
                out.push(id.clone());
            }
        }
        for (id, e) in &self.graph.entities {
            if e.mentions.iter().any(|m| m.doc == doc && section.map(|s| m.section == s).unwrap_or(true)) {
                out.push(id.clone());
            }
        }
        out
    }

    // Mechanically rewrite anchored references when a section moved.
    fn rewrite_section_refs(&mut self, doc: &str, from: &str, to: &str) {
        for r in self.graph.requirements.values_mut() {
            if r.source.doc == doc && r.source.section == from {
                r.source.section = to.to_string();
            }
        }
        for e in self.graph.entities.values_mut() {
            for m in e.mentions.iter_mut() {
                if m.doc == doc && m.section == from {
                    m.section = to.to_string();
                }
            }
        }
    }

    // ---- garbage collection ----

    // Deterministic cleanup after reconcile: requirements whose source section vanished are
    // deleted; mentions pointing at removed sections are pruned; an entity with zero
    // mentions and zero requirements is deleted with a tombstone. Journaled as one entry.
    pub fn gc(&mut self) -> Vec<String> {
        let mut actions = Vec::new();
        let dead_reqs: Vec<String> = self
            .graph
            .requirements
            .iter()
            .filter(|(_, r)| {
                !self
                    .docs
                    .get(&r.source.doc)
                    .map(|d| d.sections.contains_key(&r.source.section))
                    .unwrap_or(false)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in dead_reqs {
            self.graph.requirements.remove(&id);
            actions.push(format!("deleted {} (source section gone)", id));
        }
        for (id, e) in self.graph.entities.iter_mut() {
            let before = e.mentions.len();
            let docs = &self.docs;
            e.mentions.retain(|m| {
                docs.get(&m.doc).map(|d| d.sections.contains_key(&m.section)).unwrap_or(false)
            });
            if e.mentions.len() < before {
                actions.push(format!("pruned {} mention(s) on {}", before - e.mentions.len(), id));
            }
        }
        let orphans: Vec<String> = self
            .graph
            .entities
            .iter()
            .filter(|(id, e)| e.mentions.is_empty() && self.requirements_referencing(id).is_empty())
            .map(|(id, _)| id.clone())
            .collect();
        for id in orphans {
            self.graph.entities.remove(&id);
            self.graph.redirects.insert(id.clone(), String::new());
            actions.push(format!("deleted {} (no mentions, no requirements)", id));
        }
        if !actions.is_empty() {
            self.recompute_relationships();
            let wi = WorkItem {
                task: "gc".to_string(),
                target: "graph".to_string(),
                dirty_sections: Vec::new(),
                stale_anchors: Vec::new(),
            };
            self.status.generation += 1;
            let build = format!("g{}", self.status.generation);
            let entry = JournalEntry {
                build: build.clone(),
                work_item: wi,
                mutations: actions.iter().map(|a| serde_json::json!({"op": "gc", "action": a})).collect(),
                rounds: 0,
                tokens: 0,
            };
            write_yaml(&self.out.join("journal").join(format!("{}.yaml", build)), &entry);
            self.save();
        }
        actions
    }

    // ---- deterministic check diagnostics ----

    // Reconcile the deterministic findings: new ones are reported, existing ones updated,
    // vanished ones resolved. Keyed by rule plus subjects, like the sticky rule in apply().
    pub fn reconcile_check_diags(&mut self, findings: Vec<(String, String, String, String)>) {
        let build = format!("g{}", self.status.generation + 1);
        let mut seen: BTreeSet<(String, Vec<String>)> = BTreeSet::new();
        let mut changed = false;
        for (rule, subject, severity, message) in findings {
            let subjects = vec![subject];
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
                    if d.message != message || d.severity != severity {
                        d.message = message;
                        d.severity = severity;
                        d.updated = Some(build.clone());
                        changed = true;
                    }
                }
                None => {
                    let id = self.mint_diag_id(&rule, &BTreeSet::new());
                    self.graph.diagnostics.insert(
                        id,
                        Diagnostic {
                            rule,
                            severity,
                            subjects,
                            message,
                            reasoning: None,
                            lifecycle: "open".to_string(),
                            triage: None,
                            created: Some(build.clone()),
                            updated: Some(build.clone()),
                        },
                    );
                    changed = true;
                }
            }
        }
        // Deterministic rules whose condition cleared: resolve.
        const CHECK_RULES: [&str; 11] = [
            "uncovered-section",
            "suspicious-non-normative",
            "unused-entity",
            "unreachable-entity",
            "stale-provenance",
            "unstable-extraction",
            "duplicate-requirement",
            "section-too-large",
            "doc-too-large",
            "entity-too-dense",
            "incomplete-build",
        ];
        for d in self.graph.diagnostics.values_mut() {
            if d.lifecycle == "open"
                && CHECK_RULES.contains(&d.rule.as_str())
                && !seen.contains(&(d.rule.clone(), d.subjects.clone()))
            {
                d.lifecycle = "resolved".to_string();
                d.updated = Some(build.clone());
                changed = true;
            }
        }
        if changed {
            self.status.generation += 1;
            self.save();
        }
    }
}

fn collect_yaml(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_yaml(&p, out);
            } else if p.extension().map(|e| e == "yaml").unwrap_or(false) {
                out.push(p);
            }
        }
    }
}

// Cross-process single-writer lock. In-process serialization is the caller's mutex.
struct FileLock {
    path: PathBuf,
}

impl FileLock {
    fn acquire(out: &Path) -> FileLock {
        let path = out.join(".lock");
        std::fs::create_dir_all(out).ok();
        for _ in 0..100 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    use std::io::Write;
                    write!(f, "{}", std::process::id()).ok();
                    return FileLock { path };
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        eprintln!("[jazyk] warning: stale lock at {}; proceeding", path.display());
        FileLock { path }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jazyk-store-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    fn wi() -> WorkItem {
        WorkItem { task: "reconcile-doc".into(), target: "t.md".into(), dirty_sections: vec![], stale_anchors: vec![] }
    }

    fn mention(doc: &str, sec: &str, quote: &str) -> SourceRef {
        SourceRef { doc: doc.into(), section: sec.into(), quote: quote.into() }
    }

    fn seed_doc(store: &mut Store, doc: &str, text: &str) {
        let sections = crate::md::parse_sections(text);
        store.docs.insert(doc.to_string(), DocRecord {
            content_hash: hash_hex(text),
            sections,
            coverage: BTreeMap::new(),
        });
    }

    #[test]
    fn mint_and_create_and_natural_key_reconcile() {
        let mut s = Store { out: tmp(), ..Default::default() };
        seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
        let e = Entity { name: "Cart".into(), mentions: vec![mention("t.md", "/t", "The Cart holds items.")], ..Default::default() };
        let r = s.apply(vec![Op::CreateEntity { id: "ent:cart".into(), entity: e.clone() }], &wi(), 1, 10);
        assert_eq!(r.applied, 1);
        assert!(s.graph.entities.contains_key("ent:cart"));
        // A second create with the same natural key becomes an update, not a duplicate.
        let e2 = Entity { name: "cart".into(), mentions: vec![mention("t.md", "/t", "holds items")], ..Default::default() };
        s.apply(vec![Op::CreateEntity { id: "ent:cart-x".into(), entity: e2 }], &wi(), 1, 10);
        assert_eq!(s.graph.entities.len(), 1);
        assert_eq!(s.graph.entities["ent:cart"].mentions.len(), 2);
    }

    #[test]
    fn requirement_remaps_provisional_ids_and_derives_edges() {
        let mut s = Store { out: tmp(), ..Default::default() };
        seed_doc(&mut s, "t.md", "# T\nWhen checkout completes, the system shall empty the Cart of Products.\n");
        let ops = vec![
            Op::CreateEntity { id: "prov:1".into(), entity: Entity { name: "Cart".into(), ..Default::default() } },
            Op::CreateEntity { id: "prov:2".into(), entity: Entity { name: "Product".into(), ..Default::default() } },
            Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    ears: "When checkout completes, the system shall empty the Cart.".into(),
                    entities: vec!["prov:1".into(), "prov:2".into()],
                    edges: vec![ReqEdge { a: "prov:1".into(), b: "prov:2".into(), rel_type: Some("composition".into()) }],
                    source: mention("t.md", "/t", "the system shall empty the Cart"),
                    confidence: None, reasoning: None, created: None, updated: None,
                },
            },
        ];
        let r = s.apply(ops, &wi(), 3, 100);
        assert_eq!(r.applied, 3);
        let req = &s.graph.requirements["req:t-1"];
        assert_eq!(req.entities, vec!["ent:cart".to_string(), "ent:product".to_string()]);
        assert_eq!(s.graph.relationships.len(), 1);
        let rel = s.graph.relationships.values().next().unwrap();
        assert_eq!(rel.rel_type, "composition");
        assert_eq!(rel.requirements, vec!["req:t-1".to_string()]);
    }

    #[test]
    fn merge_rewires_and_redirects() {
        let mut s = Store { out: tmp(), ..Default::default() };
        seed_doc(&mut s, "t.md", "# T\nbuyer and customer\n");
        s.apply(vec![
            Op::CreateEntity { id: "ent:buyer".into(), entity: Entity { name: "buyer".into(), ..Default::default() } },
            Op::CreateEntity { id: "ent:customer".into(), entity: Entity { name: "Customer".into(), ..Default::default() } },
            Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    ears: "The buyer shall pay.".into(),
                    entities: vec!["ent:buyer".into()],
                    edges: vec![],
                    source: mention("t.md", "/t", "buyer and customer"),
                    confidence: None, reasoning: None, created: None, updated: None,
                },
            },
        ], &wi(), 1, 10);
        s.apply(vec![Op::MergeEntities { keep: "ent:customer".into(), absorb: "ent:buyer".into(), reason: "same concept".into() }], &wi(), 1, 10);
        assert!(!s.graph.entities.contains_key("ent:buyer"));
        assert_eq!(s.graph.redirects["ent:buyer"], "ent:customer");
        assert_eq!(s.graph.requirements["req:t-1"].entities, vec!["ent:customer".to_string()]);
        assert!(s.graph.entities["ent:customer"].aliases.contains(&"buyer".to_string()));
        assert_eq!(s.resolve_id("ent:buyer"), "ent:customer");
    }

    #[test]
    fn sync_docs_dirty_moved_removed() {
        let mut s = Store { out: tmp(), ..Default::default() };
        let v1 = "# T\nintro\n\n## Group\ngroup body\n\n### Alpha\nalpha body\n\n## Beta\nbeta body\n";
        let mut parsed = BTreeMap::new();
        parsed.insert("t.md".to_string(), (hash_hex(v1), crate::md::parse_sections(v1)));
        let d1 = s.sync_docs(&parsed);
        assert_eq!(d1.len(), 1);
        assert_eq!(d1[0].dirty_sections.len(), 4);

        // Anchor nodes in Alpha (which will move) and Beta (which will change).
        s.graph.entities.insert("ent:a".into(), Entity { name: "A".into(), mentions: vec![mention("t.md", "/t/group/alpha", "alpha body")], ..Default::default() });
        s.graph.requirements.insert("req:t-1".into(), Requirement {
            ears: "The A shall alpha.".into(), entities: vec!["ent:a".into()], edges: vec![],
            source: mention("t.md", "/t/group/alpha", "alpha body"), confidence: None, reasoning: None, created: None, updated: None,
        });
        s.graph.requirements.insert("req:t-2".into(), Requirement {
            ears: "The B shall beta.".into(), entities: vec!["ent:a".into()], edges: vec![],
            source: mention("t.md", "/t/beta", "beta body"), confidence: None, reasoning: None, created: None, updated: None,
        });
        s.docs.get_mut("t.md").unwrap().coverage.insert("/t/group/alpha".into(), Coverage { state: "covered".into(), note: None, claimed_by: None });

        // Rename the Group heading (Alpha moves under the new reference, its raw unchanged)
        // and edit Beta so the anchored quote no longer locates.
        let v2 = "# T\nintro\n\n## Bunch\ngroup body\n\n### Alpha\nalpha body\n\n## Beta\nbeta CHANGED body\n";
        let mut parsed2 = BTreeMap::new();
        parsed2.insert("t.md".to_string(), (hash_hex(v2), crate::md::parse_sections(v2)));
        let d2 = s.sync_docs(&parsed2);
        assert_eq!(d2.len(), 1);
        // Bunch is a changed section, Beta is a changed section; the moved Alpha is not dirty.
        assert_eq!(d2[0].dirty_sections, vec!["/t/beta".to_string(), "/t/bunch".to_string()]);
        // Beta's quote no longer locates -> stale anchor; Alpha's references were rewritten.
        assert!(d2[0].stale_anchors.contains(&"req:t-2".to_string()));
        assert!(!d2[0].stale_anchors.contains(&"req:t-1".to_string()));
        assert_eq!(s.graph.requirements["req:t-1"].source.section, "/t/bunch/alpha");
        assert_eq!(s.graph.entities["ent:a"].mentions[0].section, "/t/bunch/alpha");
        let rec = &s.docs["t.md"];
        assert!(rec.coverage.contains_key("/t/bunch/alpha"));
    }

    #[test]
    fn gc_removes_unanchored() {
        let mut s = Store { out: tmp(), ..Default::default() };
        seed_doc(&mut s, "t.md", "# T\nbody\n");
        s.graph.requirements.insert("req:gone-1".into(), Requirement {
            ears: "The X shall y.".into(), entities: vec!["ent:x".into()], edges: vec![],
            source: mention("gone.md", "/gone", "x"), confidence: None, reasoning: None, created: None, updated: None,
        });
        s.graph.entities.insert("ent:x".into(), Entity { name: "X".into(), mentions: vec![mention("gone.md", "/gone", "x")], ..Default::default() });
        let actions = s.gc();
        assert!(actions.len() >= 2);
        assert!(s.graph.requirements.is_empty());
        assert!(s.graph.entities.is_empty());
        assert_eq!(s.graph.redirects["ent:x"], "");
    }

    #[test]
    fn check_diags_reconcile_not_regenerate() {
        let mut s = Store { out: tmp(), ..Default::default() };
        s.reconcile_check_diags(vec![("uncovered-section".into(), "t.md#/t".into(), "warning".into(), "section /t is unprocessed".into())]);
        assert_eq!(s.graph.diagnostics.len(), 1);
        let id = s.graph.diagnostics.keys().next().unwrap().clone();
        // Same finding again: same id, no duplicate.
        s.reconcile_check_diags(vec![("uncovered-section".into(), "t.md#/t".into(), "warning".into(), "section /t is unprocessed".into())]);
        assert_eq!(s.graph.diagnostics.len(), 1);
        assert!(s.graph.diagnostics.contains_key(&id));
        // Finding cleared: resolved, not deleted.
        s.reconcile_check_diags(vec![]);
        assert_eq!(s.graph.diagnostics[&id].lifecycle, "resolved");
    }

    #[test]
    fn search_tiers() {
        let mut s = Store { out: tmp(), ..Default::default() };
        s.graph.entities.insert("ent:shopping-cart".into(), Entity { name: "Shopping Cart".into(), aliases: vec!["cart".into()], ..Default::default() });
        s.graph.entities.insert("ent:card".into(), Entity { name: "Credit Card".into(), ..Default::default() });
        let hits = s.search("cart");
        assert_eq!(hits[0].0, "ent:shopping-cart");
        let hits2 = s.search("credit card");
        assert_eq!(hits2[0].0, "ent:card");
    }
}
