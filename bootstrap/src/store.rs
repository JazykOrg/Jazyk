// The graph store: persistent home of the semantic graph. Owns identifiers, enforces
// invariants at commit, records every change. Mirrors docs/compiler/graph.md.
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
        // A revision may re-anchor its provenance (docs/compiler/tools.md#write-tools).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<SourceRef>,
    },
    DeleteRequirement { id: String, reason: String },
    ReportDiagnostic { id: String, diagnostic: Diagnostic },
    ResolveDiagnostic { id: String, reason: String },
    // The human triage decision (acknowledged | suppressed | wontfix, None to clear).
    // Only humans set it; the compiler never overwrites it.
    TriageDiagnostic {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        triage: Option<String>,
    },
    // Maintain the question on a finding; None removes it. Never touches a
    // human-set answer. Mirrors docs/compiler/model/diagnostic.md#prompts.
    UpdateDiagnosticPrompt {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<crate::model::DiagnosticPrompt>,
    },
    // The human response to a prompt. Frontends stage it; the compiler never does.
    // Mirrors docs/compiler/model/diagnostic.md#answers.
    AnswerDiagnostic { id: String, answer: crate::model::DiagnosticAnswer },
    SetCoverage {
        doc: String,
        section: String,
        state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    // A dual-write prose edit: one text run replaced in one section, never staged
    // alone (docs/compiler/graph.md#mutations). The op carries the whole new document
    // text so the commit can absorb the hashes without reading the file; the caller
    // performed (or delegated) the file write before applying.
    EditDocProse {
        doc: String,
        section: String,
        #[serde(rename = "oldText")]
        old_text: String,
        #[serde(rename = "newText")]
        new_text: String,
        // The full document text after the edit, for hash absorption and audit.
        text: String,
    },
}

// Rules the deterministic checks own: reconciled (reported, updated, resolved) against
// each build's findings. Mirrors docs/compiler/model/diagnostic.md#rules-catalog.
pub const CHECK_RULES: [&str; 14] = [
    "pinned-fact-drift",
    "empty-file",
    "broken-link",
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

// Rules a review turn may file through report_diagnostic: judged findings, settled by
// turns (or by deletion propagation), never by the checks.
pub const JUDGED_RULES: [&str; 6] =
    ["contradiction", "duplicate-entity", "duplicate-requirement", "missing-link", "ambiguity", "lint"];

pub struct CommitReport {
    pub applied: usize,
    pub skipped: Vec<String>,
    // Final entity ids touched by this commit (for scheduling review turns).
    pub touched_entities: BTreeSet<String>,
    // Final requirement ids created or whose statement changed (for scheduling
    // review-requirement pair turns). A quote-only refresh does not qualify.
    pub changed_requirements: BTreeSet<String>,
}

// A document that changed, with what a reconcile turn needs to know.
#[derive(Clone, Debug)]
pub struct DirtyDoc {
    pub doc: String,
    pub dirty_sections: Vec<String>,
    pub stale_anchors: Vec<String>,
}

// Cheap generation read: one line of status.yaml, no shard parsing.
pub fn read_generation(out: &Path) -> u64 {
    std::fs::read_to_string(out.join("status.yaml"))
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("generation:"))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|v| v.trim().parse::<u64>().ok())
        })
        .unwrap_or(0)
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
pub(crate) fn normalize_statement(s: &str) -> String {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// Same fact reworded: one statement's content tokens contained in the other's. A
// resumed build often re-extracts "shall X" as "shall X using Y" from the same
// sentence; that is one fact, not two. Distinct atomic facts sharing a sentence
// ("shall be a REST service" / "shall be built with Go") are not subsets.
pub(crate) fn statement_subsumes(a: &str, b: &str) -> bool {
    let toks = |s: &str| -> std::collections::BTreeSet<String> {
        normalize_statement(s).split(' ').map(String::from).collect()
    };
    let (ta, tb) = (toks(a), toks(b));
    !ta.is_empty() && !tb.is_empty() && (ta.is_subset(&tb) || tb.is_subset(&ta))
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
        // Readers never take the lock: read the generation counter, load every shard,
        // and retry if the counter moved mid-read (a commit landed between shards).
        // Mirrors docs/compiler/graph.md#concurrency.
        let counter = || yaml_to::<Status>(&out.join("status.yaml")).map(|s| s.generation).unwrap_or(0);
        for _ in 0..4 {
            let before = counter();
            let store = Self::load_once(out);
            if counter() == before {
                return store;
            }
        }
        Self::load_once(out)
    }

    fn load_once(out: &Path) -> Store {
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

    // Content tokens of a statement: normalized tokens minus stop words and the given
    // entities' own name tokens, reduced to crude stems so "reverses" meets "reverse"
    // and "sorting" meets "sort". Feeds requirement_neighbors.
    fn content_tokens(&self, ears: &str, entities: &[String]) -> BTreeSet<String> {
        const STOP: [&str; 30] = [
            "the", "a", "an", "shall", "to", "of", "in", "on", "for", "is", "are", "be", "or", "and", "if", "with",
            "by", "it", "its", "when", "then", "that", "which", "this", "system", "not", "no", "only", "all", "each",
        ];
        let stem = |t: &str| -> String {
            for suffix in ["ing", "ed", "s"] {
                if t.len() > suffix.len() + 2 && t.ends_with(suffix) {
                    return t[..t.len() - suffix.len()].to_string();
                }
            }
            t.to_string()
        };
        let mut name_toks: BTreeSet<String> = BTreeSet::new();
        for e in entities {
            if let Some(ent) = self.graph.entities.get(self.resolve_id(e)) {
                for n in std::iter::once(&ent.name).chain(ent.aliases.iter()) {
                    for t in normalize_statement(n).split(' ') {
                        name_toks.insert(stem(t));
                    }
                }
            }
        }
        normalize_statement(ears)
            .split(' ')
            .filter(|t| !STOP.contains(t))
            .map(|t| stem(t))
            .filter(|t| !name_toks.contains(t))
            .collect()
    }

    // Deterministic neighbor set for one requirement: other requirements sharing an
    // entity, scored by overlapping content tokens, at least two shared, best six.
    // Feeds the review-requirement pair wave (docs/compiler/compilation.md#waves).
    pub fn requirement_neighbors(&self, rid: &str) -> Vec<String> {
        let Some(req) = self.graph.requirements.get(rid) else { return Vec::new() };
        let subject_entities: BTreeSet<&str> = req.entities.iter().map(|e| self.resolve_id(e)).collect();
        let toks = self.content_tokens(&req.ears, &req.entities);
        let mut scored: Vec<(usize, &String)> = Vec::new();
        for (oid, other) in &self.graph.requirements {
            if oid == rid || !other.entities.iter().any(|e| subject_entities.contains(self.resolve_id(e))) {
                continue;
            }
            let shared = self.content_tokens(&other.ears, &other.entities).intersection(&toks).count();
            if shared >= 2 {
                scored.push((shared, oid));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        scored.into_iter().take(6).map(|(_, id)| id.clone()).collect()
    }

    // The pair-review set: computed neighbors plus sticky partners. Open contradiction
    // and duplicate-requirement diagnostics tie pairs; editing one side of a known pair
    // always re-judges the other.
    pub fn pair_review_neighbors(&self, rid: &str) -> Vec<String> {
        let mut nbrs = self.requirement_neighbors(rid);
        for d in self.graph.diagnostics.values() {
            if d.lifecycle != "open" || !(d.rule == "contradiction" || d.rule == "duplicate-requirement") {
                continue;
            }
            if !d.subjects.iter().any(|s| s == rid) {
                continue;
            }
            for sub in &d.subjects {
                if sub != rid && self.graph.requirements.contains_key(sub) && !nbrs.contains(sub) {
                    nbrs.push(sub.clone());
                }
            }
        }
        nbrs
    }

    // Open diagnostic counts by severity, suppressed excluded: the health line that
    // rides beside every verdict. Mirrors docs/compiler/compilation.md#convergence.
    pub fn open_diag_counts(&self) -> BTreeMap<String, u64> {
        let mut m: BTreeMap<String, u64> = BTreeMap::new();
        for d in self.graph.diagnostics.values() {
            if d.lifecycle == "open" && d.triage.as_deref() != Some("suppressed") {
                *m.entry(d.severity.clone()).or_insert(0) += 1;
            }
        }
        m
    }

    // An open judged diagnostic naming this node: reason enough to schedule a review
    // even when the neighbor computation finds nothing (a partner may be deleted; the
    // diagnostic itself is the remaining work). Mirrors docs/compiler/compilation.md#waves.
    pub fn has_open_judged_diag(&self, id: &str) -> bool {
        self.graph.diagnostics.values().any(|d| {
            d.lifecycle == "open" && JUDGED_RULES.contains(&d.rule.as_str()) && d.subjects.iter().any(|s| s == id)
        })
    }

    // Whether a pending pair review still has work: the requirement exists and either
    // computed neighbors or an open judged diagnostic tie it to a judgment.
    pub fn pair_review_due(&self, rid: &str) -> bool {
        self.graph.requirements.contains_key(rid)
            && (!self.pair_review_neighbors(rid).is_empty() || self.has_open_judged_diag(rid))
    }

    // Deleting nodes settles the open judged diagnostics naming them: all subjects gone
    // resolves the diagnostic in place (the returned ops go to the journal); surviving
    // subjects re-enqueue for review, so a turn re-judges the finding. Runs on every
    // deleting commit, turn and GC alike. Mirrors docs/compiler/graph.md#garbage-collection
    // and docs/compiler/compilation.md#waves.
    fn propagate_deletions(&mut self, deleted: &BTreeSet<String>, build: &str) -> Vec<Op> {
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
                    && d.subjects.iter().any(|s| deleted.contains(s))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for did in hit {
            let subjects = self.graph.diagnostics[&did].subjects.clone();
            let survivors: Vec<String> = subjects
                .iter()
                .map(|s| self.resolve_id(s).to_string())
                .filter(|s| self.graph.requirements.contains_key(s) || self.graph.entities.contains_key(s))
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
                    if self.graph.requirements.contains_key(&s) {
                        if !self.status.pending.requirements.contains(&s) {
                            self.status.pending.requirements.push(s);
                        }
                    } else if !self.status.pending.entities.contains(&s) {
                        self.status.pending.entities.push(s);
                    }
                }
            }
        }
        resolved
    }

    // Subjects of open judged diagnostics that no longer exist in the graph.
    fn missing_diag_subjects(&self) -> BTreeSet<String> {
        self.graph
            .diagnostics
            .values()
            .filter(|d| d.lifecycle == "open" && JUDGED_RULES.contains(&d.rule.as_str()))
            .flat_map(|d| d.subjects.iter())
            .filter(|s| s.starts_with("req:") || s.starts_with("ent:"))
            .filter(|s| {
                let r = self.resolve_id(s);
                !self.graph.requirements.contains_key(r) && !self.graph.entities.contains_key(r)
            })
            .cloned()
            .collect()
    }

    // Whether any open judged diagnostic names a subject the graph no longer holds:
    // the queue's signal that the deterministic tail has settling to do.
    pub fn has_dangling_diags(&self) -> bool {
        !self.missing_diag_subjects().is_empty()
    }

    // The level-triggered half of deletion propagation: sweep every open judged
    // diagnostic for missing subjects and settle it, journaled like GC. A graph
    // deleted into a stranded state heals here instead of staying wedged.
    // Mirrors docs/compiler/compilation.md#waves.
    pub fn settle_dangling_diags(&mut self) -> Vec<String> {
        let _flock = FileLock::acquire(&self.out);
        let missing = self.missing_diag_subjects();
        if missing.is_empty() {
            return Vec::new();
        }
        let pending_before =
            (self.status.pending.requirements.len(), self.status.pending.entities.len());
        let build = format!("g{}", self.status.generation + 1);
        let ops = self.propagate_deletions(&missing, &build);
        let actions: Vec<String> = ops
            .iter()
            .filter_map(|o| match o {
                Op::ResolveDiagnostic { id, reason } => Some(format!("resolved {} ({})", id, reason)),
                _ => None,
            })
            .collect();
        if !ops.is_empty() {
            self.status.generation += 1;
            let entry = JournalEntry {
                build: build.clone(),
                work_item: WorkItem {
                    task: "settle-diagnostics".to_string(),
                    target: "graph".to_string(),
                    dirty_sections: Vec::new(),
                    stale_anchors: Vec::new(),
                },
                mutations: ops.iter().map(|o| serde_json::to_value(o).unwrap_or_default()).collect(),
                rounds: 0,
                tokens: 0,
            };
            write_yaml(&self.out.join("journal").join(format!("{}.yaml", build)), &entry);
            self.save();
        } else if (self.status.pending.requirements.len(), self.status.pending.entities.len())
            != pending_before
        {
            self.save_status();
        }
        actions
    }

    // Completing a pair task also completes its mirror: when two changed requirements
    // are each other's only neighbor, one judgment covers the pair, and scheduling the
    // reverse would judge the identical pair again.
    // Mirrors docs/compiler/compilation.md#waves.
    pub fn complete_pair_mirrors(&mut self, rid: &str) {
        let judged: std::collections::BTreeSet<String> = self.pair_review_neighbors(rid).into_iter().collect();
        for r in self.status.pending.requirements.clone() {
            if r == rid || !judged.contains(&r) {
                continue;
            }
            let nbrs = self.pair_review_neighbors(&r);
            if !nbrs.is_empty() && nbrs.iter().all(|n| n == rid) {
                self.complete_review("review-requirement", &r);
            }
        }
    }

    // ---- commit ----

    // Apply a staged changeset atomically: reconcile creates by natural key against nodes
    // committed concurrently, apply ops in order, recompute derived relationships, journal,
    // bump the generation, write shards.
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

    pub fn apply(&mut self, ops: Vec<Op>, work_item: &WorkItem, rounds: u32, tokens: u64) -> CommitReport {
        let _flock = FileLock::acquire(&self.out);
        let build = format!("g{}", self.status.generation + 1);
        let mut remap: BTreeMap<String, String> = BTreeMap::new();
        let mut skipped: Vec<String> = Vec::new();
        let mut touched: BTreeSet<String> = BTreeSet::new();
        let mut changed_reqs: BTreeSet<String> = BTreeSet::new();
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
                    // A re-extraction from the same source sentence whose statement
                    // subsumes (or is subsumed by) the existing one is the same fact
                    // reworded and also refreshes in place. A create staged under an
                    // existing id whose statement subsumes the incoming one in the same
                    // section is the stage-time resolution of a stale anchor: it folds
                    // into that id (an accidental id race with a different statement
                    // still re-mints below).
                    let fold_target = self
                        .graph
                        .requirements
                        .iter()
                        .find(|(_, r)| {
                            r.source.doc == requirement.source.doc
                                && r.source.section == requirement.source.section
                                && (normalize_statement(&r.ears) == normalize_statement(&requirement.ears)
                                    || (normalize_statement(&r.source.quote)
                                        == normalize_statement(&requirement.source.quote)
                                        && statement_subsumes(&r.ears, &requirement.ears)))
                        })
                        .map(|(rid, _)| rid.clone())
                        .or_else(|| {
                            self.graph.requirements.get(&id).and_then(|r| {
                                (r.source.doc == requirement.source.doc
                                    && r.source.section == requirement.source.section
                                    && statement_subsumes(&r.ears, &requirement.ears))
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
                        for edge in requirement.edges {
                            if !r.edges.iter().any(|x| (x.a == edge.a && x.b == edge.b) || (x.a == edge.b && x.b == edge.a)) {
                                r.edges.push(edge);
                            }
                        }
                        // The matched statement's ears and quote refresh in place (same
                        // statement modulo punctuation); the id never churns. A refresh
                        // that changes something is journaled as the update it is.
                        let mut refreshed_ears: Option<String> = None;
                        let mut refreshed_source: Option<SourceRef> = None;
                        if r.ears != requirement.ears {
                            r.ears = requirement.ears.clone();
                            refreshed_ears = Some(requirement.ears.clone());
                            changed_reqs.insert(existing.clone());
                        }
                        if r.source.quote != requirement.source.quote {
                            // A quote that changed in substance (not punctuation) means
                            // the document text under the statement changed: revised,
                            // even when the turn kept the old wording.
                            if normalize_statement(&r.source.quote) != normalize_statement(&requirement.source.quote) {
                                changed_reqs.insert(existing.clone());
                            }
                            r.source = requirement.source.clone();
                            refreshed_source = Some(requirement.source.clone());
                        }
                        r.updated = Some(build.clone());
                        touched.extend(r.entities.iter().cloned());
                        if refreshed_ears.is_some() || refreshed_source.is_some() {
                            applied.push(Op::UpdateRequirement {
                                id: existing,
                                ears: refreshed_ears,
                                entities: None,
                                edges: None,
                                source: refreshed_source,
                            });
                        }
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
                    changed_reqs.insert(final_id.clone());
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
                Op::UpdateRequirement { id, ears, entities, edges, source } => {
                    let rid = resolve(&remap, self, &id);
                    let resolved_entities = entities
                        .map(|es| es.iter().map(|e| resolve(&remap, self, e)).collect::<Vec<_>>());
                    match self.graph.requirements.get_mut(&rid) {
                        Some(r) => {
                            if let Some(e) = &ears {
                                if r.ears != *e {
                                    changed_reqs.insert(rid.clone());
                                }
                                r.ears = e.clone();
                            }
                            if let Some(es) = &resolved_entities {
                                r.entities = es.clone();
                            }
                            if let Some(ed) = &edges {
                                r.edges = ed.clone();
                            }
                            if let Some(s) = &source {
                                // Same rule as the create fold: a re-anchored quote that
                                // changed in substance marks the statement revised.
                                if normalize_statement(&r.source.quote) != normalize_statement(&s.quote) {
                                    changed_reqs.insert(rid.clone());
                                }
                                r.source = s.clone();
                            }
                            r.updated = Some(build.clone());
                            touched.extend(r.entities.iter().cloned());
                            applied.push(Op::UpdateRequirement { id: rid, ears, entities: resolved_entities, edges, source });
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
                    // not duplicated. Subject order does not matter: a pair reported from
                    // either endpoint is the same finding. Human triage is never touched.
                    let subject_set = |v: &[String]| -> BTreeSet<String> { v.iter().cloned().collect() };
                    let incoming_subjects = subject_set(&diagnostic.subjects);
                    let existing = self
                        .graph
                        .diagnostics
                        .iter()
                        .find(|(_, d)| {
                            d.rule == diagnostic.rule
                                && d.lifecycle == "open"
                                && subject_set(&d.subjects) == incoming_subjects
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
                            }
                            d.updated = Some(build.clone());
                            // Journal the update as what it is: the merged diagnostic,
                            // not an entity op carrying a diagnostic id.
                            let merged = d.clone();
                            applied.push(Op::ReportDiagnostic { id: did, diagnostic: merged });
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
                            // Resolving while an answer is being handled is the
                            // handling turn finishing its work.
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
                Op::EditDocProse { doc, section, old_text, new_text, text } => {
                    // The graph mutation paired with this edit lands in the same
                    // changeset; absorbing the new hashes here is its reconciliation,
                    // so the edit does not dirty the document it just reconciled.
                    // Mirrors docs/compiler/graph.md#mutations.
                    self.absorb_doc_edit(&doc, &text);
                    applied.push(Op::EditDocProse { doc, section, old_text, new_text, text });
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

        // Deletion propagation: the ops above may have removed diagnostic subjects.
        let deleted: BTreeSet<String> = applied
            .iter()
            .filter_map(|o| match o {
                Op::DeleteRequirement { id, .. } | Op::DeleteEntity { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        let auto_resolved = self.propagate_deletions(&deleted, &build);
        applied.extend(auto_resolved);

        self.recompute_relationships();
        self.status.generation += 1;
        self.status.spent.turns += 1;
        self.status.spent.rounds += rounds as u64;
        self.status.spent.tokens += tokens;
        // A reconcile commit owes reviews for what it touched; recording them here is
        // what makes the task queue derivable by any process, not just the loop that
        // ran the ingest. Mirrors docs/compiler/reconciler.md#the-task-queue.
        if work_item.task == "reconcile-doc" {
            for id in &touched {
                if self.graph.entities.contains_key(id) && !self.status.pending.entities.contains(id) {
                    self.status.pending.entities.push(id.clone());
                }
            }
            for id in &changed_reqs {
                if self.graph.requirements.contains_key(id) && !self.status.pending.requirements.contains(id) {
                    self.status.pending.requirements.push(id.clone());
                }
            }
        }
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
        CommitReport { applied: applied.len(), skipped, touched_entities: touched, changed_requirements: changed_reqs }
    }

    // A completed review task pays its debt: the target leaves the pending block, and
    // the queue stops offering it. Completion, not commit: a review that stages nothing
    // still completes. Persists immediately so any process sees it.
    pub fn complete_review(&mut self, task: &str, target: &str) {
        let _flock = FileLock::acquire(&self.out);
        match task {
            "review-entity" => self.status.pending.entities.retain(|e| e != target),
            "review-requirement" => self.status.pending.requirements.retain(|r| r != target),
            _ => return,
        }
        self.save_status();
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
        let mut deleted: BTreeSet<String> = BTreeSet::new();
        for id in dead_reqs {
            self.graph.requirements.remove(&id);
            actions.push(format!("deleted {} (source section gone)", id));
            deleted.insert(id);
        }
        for (id, e) in self.graph.entities.iter_mut() {
            let before = e.mentions.len();
            let docs = &self.docs;
            // A mention whose section is gone, or whose quote no longer locates in it,
            // is stale prose: left in place it leaks statements the documents no longer
            // make into later context packs.
            e.mentions.retain(|m| {
                docs.get(&m.doc)
                    .and_then(|d| d.sections.get(&m.section))
                    .map(|s| text_contains(&s.raw, &m.quote))
                    .unwrap_or(false)
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
            deleted.insert(id);
        }
        for op in self.propagate_deletions(&deleted, &format!("g{}", self.status.generation + 1)) {
            if let Op::ResolveDiagnostic { id, reason } = op {
                actions.push(format!("resolved {} ({})", id, reason));
            }
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
    pub fn reconcile_check_diags(
        &mut self,
        findings: Vec<(String, String, String, String, Option<crate::model::DiagnosticPrompt>)>,
    ) {
        let build = format!("g{}", self.status.generation + 1);
        let mut seen: BTreeSet<(String, Vec<String>)> = BTreeSet::new();
        let mut changed = false;
        for (rule, subject, severity, message, prompt) in findings {
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
                    // The question rides along on a finding that never had one and
                    // was never answered; an answered or standing prompt is kept.
                    if d.prompt.is_none() && d.answer.is_none() && prompt.is_some() {
                        d.prompt = prompt;
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
                            prompt,
                            answer: None,
                            created: Some(build.clone()),
                            updated: Some(build.clone()),
                        },
                    );
                    changed = true;
                }
            }
        }
        // Deterministic rules whose condition cleared: resolve. Check findings carry
        // exactly one subject; a multi-subject diagnostic under a shared rule name
        // (a review turn's duplicate-requirement pair) is judged work, not the checks'
        // to resolve.
        for d in self.graph.diagnostics.values_mut() {
            if d.lifecycle == "open"
                && d.subjects.len() == 1
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

    // A reconcile commit records the reviews it owes; completing a review pays the
    // debt. What makes the task queue derivable by any process.
    // Mirrors docs/compiler/reconciler.md#the-task-queue.
    #[test]
    fn reconcile_commit_records_pending_reviews_and_completion_clears_them() {
        // Own directory: this test reloads from disk, and the shared tmp dir is
        // stomped by parallel tests.
        let dir = std::env::temp_dir().join(format!("jazyk-pending-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).ok();
        let mut s = Store { out: dir, ..Default::default() };
        seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
        let ops = vec![
            Op::CreateEntity { id: "ent:cart".into(), entity: Entity { name: "Cart".into(), ..Default::default() } },
            Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    ears: "The Cart shall hold items.".into(),
                    entities: vec!["ent:cart".into()],
                    edges: vec![],
                    source: mention("t.md", "/t", "The Cart holds items."),
                    confidence: None,
                    reasoning: None,
                    created: None,
                    updated: None,
                },
            },
        ];
        s.apply(ops, &wi(), 1, 0);
        assert_eq!(s.status.pending.entities, vec!["ent:cart".to_string()]);
        assert_eq!(s.status.pending.requirements, vec!["req:t-1".to_string()]);
        // A review changeset owes nothing new.
        let review = WorkItem { task: "review-entity".into(), target: "ent:cart".into(), dirty_sections: vec![], stale_anchors: vec![] };
        s.apply(vec![], &review, 1, 0);
        assert_eq!(s.status.pending.entities.len(), 1);
        // Completion pays the debt, task by task.
        s.complete_review("review-requirement", "req:t-1");
        s.complete_review("review-entity", "ent:cart");
        assert!(s.status.pending.is_empty());
        // And it persisted: a fresh load agrees.
        assert!(Store::load(&s.out).status.pending.is_empty());
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
    fn same_sentence_subsumed_statement_refreshes_in_place() {
        let mut s = Store { out: tmp(), ..Default::default() };
        seed_doc(&mut s, "t.md", "# T\n- `createUser` - creates a new user account\n");
        let base = Requirement {
            ears: "The user management system shall create a new user account.".into(),
            entities: vec!["ent:um".into()],
            edges: Vec::new(),
            source: mention("t.md", "/t", "- `createUser` - creates a new user account"),
            confidence: None,
            reasoning: None,
            created: None,
            updated: None,
        };
        s.apply(
            vec![
                Op::CreateEntity { id: "ent:um".into(), entity: Entity { name: "User Management".into(), ..Default::default() } },
                Op::CreateRequirement { id: "req:t-1".into(), requirement: base.clone() },
            ],
            &wi(),
            1,
            10,
        );
        // A resumed build rewords the same sentence's statement; one fact, one node.
        let reworded = Requirement {
            ears: "The user management system shall create a new user account using createUser.".into(),
            ..base
        };
        s.apply(vec![Op::CreateRequirement { id: "req:t-2".into(), requirement: reworded }], &wi(), 1, 10);
        assert_eq!(s.graph.requirements.len(), 1);
        assert!(s.graph.requirements["req:t-1"].ears.contains("using createUser"));
        // Distinct atomic facts from one sentence stay separate.
        assert!(!statement_subsumes(
            "The gateway shall be a REST service.",
            "The gateway shall be built with Go."
        ));
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
        s.reconcile_check_diags(vec![("uncovered-section".into(), "t.md#/t".into(), "warning".into(), "section /t is unprocessed".into(), None)]);
        assert_eq!(s.graph.diagnostics.len(), 1);
        let id = s.graph.diagnostics.keys().next().unwrap().clone();
        // Same finding again: same id, no duplicate.
        s.reconcile_check_diags(vec![("uncovered-section".into(), "t.md#/t".into(), "warning".into(), "section /t is unprocessed".into(), None)]);
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

    #[test]
    fn create_under_existing_id_folds_in_place_and_reports_change() {
        let mut s = Store { out: tmp(), ..Default::default() };
        s.graph.entities.insert("ent:cart".into(), Entity { name: "Cart".into(), ..Default::default() });
        s.graph.requirements.insert("req:t-1".into(), Requirement {
            ears: "The Cart shall hold items a Customer intends to buy.".into(), entities: vec!["ent:cart".into()], edges: vec![],
            source: mention("t.md", "/t", "holds items a Customer intends to buy"), confidence: None, reasoning: None, created: None, updated: None,
        });
        // Stage-time resolution staged a create under the anchor's id with a reworded,
        // subsuming statement and a fresh quote; commit folds it in place.
        let report = s.apply(vec![Op::CreateRequirement { id: "req:t-1".into(), requirement: Requirement {
            ears: "The Cart shall hold items.".into(), entities: vec!["ent:cart".into()], edges: vec![],
            source: mention("t.md", "/t", "keeps items"), confidence: None, reasoning: None, created: None, updated: None,
        }}], &wi(), 1, 10);
        assert_eq!(s.graph.requirements.len(), 1);
        let r = &s.graph.requirements["req:t-1"];
        assert_eq!(r.ears, "The Cart shall hold items.");
        assert_eq!(r.source.quote, "keeps items");
        assert!(report.changed_requirements.contains("req:t-1"), "{:?}", report.changed_requirements);
    }

    #[test]
    fn requirement_neighbors_find_the_topic_cluster() {
        let mut s = Store::default();
        s.graph.entities.insert("ent:util".into(), Entity { name: "Sorting Algorithm CLI Utility".into(), ..Default::default() });
        let mk = |ears: &str| Requirement {
            ears: ears.into(), entities: vec!["ent:util".into()], edges: vec![],
            source: mention("m.md", "/m", "q"), confidence: None, reasoning: None, created: None, updated: None,
        };
        // The example-sort failure: three statements about reverse-order sorting were
        // never put side by side. Stemmed content-token overlap pairs them.
        s.graph.requirements.insert("req:m-2".into(), mk("The system shall allow the -r argument, which reverses sorting order to descending."));
        s.graph.requirements.insert("req:m-3".into(), mk("The Sorting Algorithm CLI Utility shall keep track of reverse order with `-r`."));
        s.graph.requirements.insert("req:m-5".into(), mk("The Sorting Algorithm CLI Utility shall strip out whitespace before and after the current line."));
        s.graph.requirements.insert("req:m-8".into(), mk("The Sorting Algorithm CLI Utility shall sort lines descending, or ascending if reverse order is set."));
        let n = s.requirement_neighbors("req:m-8");
        assert!(n.contains(&"req:m-2".to_string()), "{:?}", n);
        assert!(n.contains(&"req:m-3".to_string()), "{:?}", n);
        assert!(!n.contains(&"req:m-5".to_string()), "{:?}", n);
    }

    #[test]
    fn pair_diagnostic_sticky_regardless_of_subject_order() {
        let mut s = Store { out: tmp(), ..Default::default() };
        let d = |subjects: Vec<String>| Diagnostic {
            rule: "contradiction".into(), severity: "warning".into(), subjects, message: "conflict".into(),
            reasoning: None, lifecycle: "open".into(), triage: None, prompt: None, answer: None,
            created: None, updated: None,
        };
        s.apply(vec![Op::ReportDiagnostic { id: String::new(), diagnostic: d(vec!["req:a".into(), "req:b".into()]) }], &wi(), 1, 1);
        // The same pair reported from the other endpoint updates the finding in place.
        s.apply(vec![Op::ReportDiagnostic { id: String::new(), diagnostic: d(vec!["req:b".into(), "req:a".into()]) }], &wi(), 1, 1);
        assert_eq!(s.graph.diagnostics.len(), 1);
    }

    #[test]
    fn answered_prompt_is_never_reasked_and_resolve_marks_handled() {
        use crate::model::{DiagnosticAnswer, DiagnosticPrompt};
        let mut s = Store { out: tmp(), ..Default::default() };
        let prompt = |q: &str| DiagnosticPrompt { question: q.into(), options: Vec::new(), freeform: true };
        let with_prompt = |q: Option<&str>| Diagnostic {
            rule: "contradiction".into(), severity: "warning".into(),
            subjects: vec!["req:a".into(), "req:b".into()], message: "conflict".into(),
            reasoning: None, lifecycle: "open".into(), triage: None,
            prompt: q.map(prompt), answer: None, created: None, updated: None,
        };
        s.apply(vec![Op::ReportDiagnostic { id: String::new(), diagnostic: with_prompt(Some("which?")) }], &wi(), 1, 0);
        let id = s.graph.diagnostics.keys().next().unwrap().clone();
        // A promptless re-report keeps the question; a fresh one replaces it.
        s.apply(vec![Op::ReportDiagnostic { id: String::new(), diagnostic: with_prompt(None) }], &wi(), 1, 0);
        assert_eq!(s.graph.diagnostics[&id].prompt.as_ref().unwrap().question, "which?");
        s.apply(vec![Op::ReportDiagnostic { id: String::new(), diagnostic: with_prompt(Some("sharper?")) }], &wi(), 1, 0);
        assert_eq!(s.graph.diagnostics[&id].prompt.as_ref().unwrap().question, "sharper?");
        // Once answered, a re-report never re-asks.
        s.apply(
            vec![Op::AnswerDiagnostic {
                id: id.clone(),
                answer: DiagnosticAnswer { choice: None, text: "both".into(), status: "handling".into() },
            }],
            &wi(), 0, 0,
        );
        s.apply(vec![Op::ReportDiagnostic { id: String::new(), diagnostic: with_prompt(Some("again?")) }], &wi(), 1, 0);
        assert_eq!(s.graph.diagnostics[&id].prompt.as_ref().unwrap().question, "sharper?");
        // Resolving while handling is the handling turn finishing.
        s.apply(vec![Op::ResolveDiagnostic { id: id.clone(), reason: "settled".into() }], &wi(), 0, 0);
        let d = &s.graph.diagnostics[&id];
        assert_eq!(d.lifecycle, "resolved");
        assert_eq!(d.answer.as_ref().unwrap().status, "handled");
    }

    fn diag(rule: &str, subjects: Vec<&str>) -> Diagnostic {
        Diagnostic {
            rule: rule.into(),
            severity: "error".into(),
            subjects: subjects.into_iter().map(String::from).collect(),
            message: "conflict".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        }
    }

    fn seeded_req(doc: &str, ears: &str) -> Requirement {
        Requirement {
            ears: ears.into(),
            entities: vec!["ent:cart".into()],
            edges: vec![],
            source: mention(doc, "/t", "The Cart holds items."),
            confidence: None,
            reasoning: None,
            created: None,
            updated: None,
        }
    }

    // The example-sort failure: deleting one side of a filed contradiction left the
    // diagnostic open forever. Deletion now settles or re-enqueues.
    // Mirrors docs/compiler/compilation.md#waves.
    #[test]
    fn deleting_a_subject_settles_or_reenqueues_its_diagnostics() {
        let dir = std::env::temp_dir().join(format!("jazyk-propagate-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).ok();
        let mut s = Store { out: dir, ..Default::default() };
        seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
        s.graph.entities.insert("ent:cart".into(), Entity { name: "Cart".into(), mentions: vec![mention("t.md", "/t", "The Cart holds items.")], ..Default::default() });
        s.graph.requirements.insert("req:t-1".into(), seeded_req("t.md", "The Cart shall hold items."));
        s.graph.requirements.insert("req:t-2".into(), seeded_req("t.md", "The Cart shall stay empty."));
        s.apply(vec![Op::ReportDiagnostic { id: String::new(), diagnostic: diag("contradiction", vec!["req:t-1", "req:t-2"]) }], &wi(), 1, 0);
        let did = s.graph.diagnostics.keys().next().unwrap().clone();
        s.status.pending = PendingReviews::default();

        // One subject deleted: the diagnostic stands, the survivor is re-enqueued.
        s.apply(vec![Op::DeleteRequirement { id: "req:t-1".into(), reason: "fact gone".into() }], &wi(), 1, 0);
        assert_eq!(s.graph.diagnostics[&did].lifecycle, "open");
        assert!(s.status.pending.requirements.contains(&"req:t-2".to_string()), "{:?}", s.status.pending.requirements);

        // The open diagnostic alone keeps the survivor's pair review due, with no
        // computed neighbor left.
        assert!(s.pair_review_neighbors("req:t-2").is_empty());
        assert!(s.pair_review_due("req:t-2"));

        // Every subject deleted: the store resolves the diagnostic itself.
        s.apply(vec![Op::DeleteRequirement { id: "req:t-2".into(), reason: "fact gone".into() }], &wi(), 1, 0);
        assert_eq!(s.graph.diagnostics[&did].lifecycle, "resolved");
    }

    // A graph deleted into a stranded state before propagation existed heals at the
    // deterministic tail. Mirrors docs/compiler/compilation.md#waves.
    #[test]
    fn settle_dangling_diags_heals_a_stranded_graph() {
        let dir = std::env::temp_dir().join(format!("jazyk-settle-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).ok();
        let mut s = Store { out: dir, ..Default::default() };
        seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
        s.graph.entities.insert("ent:cart".into(), Entity { name: "Cart".into(), ..Default::default() });
        s.graph.requirements.insert("req:t-2".into(), seeded_req("t.md", "The Cart shall stay empty."));
        s.graph.diagnostics.insert("diag:contradiction-1".into(), diag("contradiction", vec!["req:gone", "req:t-2"]));
        s.graph.diagnostics.insert("diag:contradiction-2".into(), diag("contradiction", vec!["req:gone-a", "req:gone-b"]));
        assert!(s.has_dangling_diags());

        let actions = s.settle_dangling_diags();
        // All subjects gone: resolved by the store, with a journaled action.
        assert_eq!(s.graph.diagnostics["diag:contradiction-2"].lifecycle, "resolved");
        assert!(actions.iter().any(|a| a.contains("diag:contradiction-2")), "{:?}", actions);
        // A survivor remains: the diagnostic stands and the survivor is re-enqueued.
        assert_eq!(s.graph.diagnostics["diag:contradiction-1"].lifecycle, "open");
        assert_eq!(s.status.pending.requirements, vec!["req:t-2".to_string()]);
        // Idempotent: a second sweep resolves nothing new.
        assert!(s.settle_dangling_diags().is_empty());
    }

    // Check reconciliation resolves only its own single-subject findings; a judged
    // pair filed under a shared rule name is a turn's to resolve.
    #[test]
    fn check_reconcile_leaves_judged_pairs_alone() {
        let mut s = Store { out: tmp(), ..Default::default() };
        s.graph.diagnostics.insert("diag:duplicate-requirement-1".into(), diag("duplicate-requirement", vec!["req:a", "req:b"]));
        s.reconcile_check_diags(Vec::new());
        assert_eq!(s.graph.diagnostics["diag:duplicate-requirement-1"].lifecycle, "open");
    }
}
