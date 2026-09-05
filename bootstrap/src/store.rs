// The graph store: persistent home of the semantic graph. Owns identifiers, enforces
// invariants at commit, recomputes derived data, records every change, and writes the
// typed dirtiness each commit causes. Mirrors docs/compiler/graph.md.
use crate::md;
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// One staged mutation. Serialized into the journal as written.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    CreateEntity {
        id: String,
        entity: Entity,
    },
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stereotype: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
        // Replace the whole attribute list, or refresh by name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        set_attributes: Option<Vec<Attribute>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        add_attributes: Vec<Attribute>,
        // A decree or derivation landing on the entity (docs/compiler/model/entity.md#fields).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provenance: Option<Provenance>,
    },
    DeleteEntity {
        id: String,
        reason: String,
    },
    MergeEntities {
        keep: String,
        absorb: String,
        reason: String,
    },
    // Dissolve a grouping: its children reparent to its parent and it tombstones with
    // a redirect to that parent. Refused on an entity a document states. The store
    // fills `parent` and `children` as applied, so the reparent flip replays from the
    // journal alone. Its inverse, `group_entities`, has no op of its own: it composes
    // one `CreateEntity` (derived provenance from the members) with one `UpdateEntity`
    // `parent` move per member, so the journal shows the create and each move with
    // its prior parent. Mirrors docs/compiler/concepts/levels.md#groupings.
    DissolveEntity {
        id: String,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        children: Vec<String>,
    },
    CreateRequirement {
        id: String,
        requirement: Requirement,
    },
    UpdateRequirement {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        statement: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entities: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edges: Option<Vec<ReqEdge>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transition: Option<Transition>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        facets: Option<Vec<Facet>>,
        // A revision may re-anchor its quote (docs/compiler/tools.md#write-tools).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<SourceRef>,
        // A decree or derivation replacing the quote (docs/compiler/compilation.md#edit-paths).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provenance: Option<Provenance>,
    },
    DeleteRequirement {
        id: String,
        reason: String,
    },
    CreateView {
        id: String,
        view: View,
    },
    UpdateView {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        members: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        add_members: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        remove_members: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<ViewQuery>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        collapse: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        exclude: Vec<Exclusion>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning: Option<String>,
    },
    DeleteView {
        id: String,
        reason: String,
    },
    // A ratified fact: its provenance flips to the quote of the sentence the document
    // gained. Mirrors docs/compiler/compilation.md#edit-paths.
    RatifyProvenance {
        id: String,
        source: SourceRef,
    },
    // Undo a decree: a node created by decree (or derivation) is deleted.
    RetractDecree {
        id: String,
        reason: String,
    },
    // A per-node limit bump with the decree behind it. Mirrors docs/compiler/graph.md#per-node-bumps.
    BumpLimit {
        id: String,
        limit: String,
        value: u64,
        provenance: Provenance,
    },
    // The place-anchors session's decisions: move one anchor (a requirement source or
    // the entity mention `from` names) to `to`, optionally flagging it for re-evaluation;
    // or leave it homeless. Mirrors docs/compiler/alignment.md.
    PlaceAnchor {
        id: String,
        from: SourceRef,
        to: SourceRef,
        reevaluate: bool,
    },
    OrphanAnchor {
        id: String,
        from: SourceRef,
    },
    ReportDiagnostic {
        id: String,
        diagnostic: Diagnostic,
    },
    ResolveDiagnostic {
        id: String,
        reason: String,
    },
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
    AnswerDiagnostic {
        id: String,
        answer: crate::model::DiagnosticAnswer,
    },
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
pub const CHECK_RULES: [&str; 28] = [
    "pinned-fact-drift",
    "empty-file",
    "broken-link",
    "uncovered-section",
    "suspicious-non-normative",
    "unused-entity",
    "unreachable-entity",
    "stale-provenance",
    "unstable-extraction",
    "unstable-derivation",
    "duplicate-requirement",
    "section-too-large",
    "doc-too-large",
    "unjustified-fact",
    "unplaced-behavior",
    "unrepresented-failure-mode",
    "containment-mismatch",
    "level-shape",
    "nonconformant-instance",
    "unreachable-state",
    "dead-end-state",
    "nondeterministic-transition",
    "unhandled-event",
    "provider-missing",
    "provider-ambiguous",
    "quality-unmeasured",
    "unratified",
    "incomplete-build",
];

// Rules a review session may file through report_diagnostic: judged findings, settled by
// sessions (or by deletion propagation), never by the checks.
pub const JUDGED_RULES: [&str; 6] = [
    "contradiction",
    "duplicate-entity",
    "duplicate-requirement",
    "missing-link",
    "ambiguity",
    "lint",
];

// The store version written to status.yaml. An out directory carrying another one is
// archived whole and the store starts empty. Mirrors docs/compiler/graph.md#store-version.
pub const STORE_VERSION: u32 = 2;

// Change record kinds. Mirrors docs/compiler/graph.md#change-records.
pub const CHANGE_SECTION_DIRTY: &str = "section-dirty";
pub const CHANGE_SECTION_REMOVED: &str = "section-removed";
pub const CHANGE_ANCHOR_STALE: &str = "anchor-stale";
pub const CHANGE_ALIGNMENT_PENDING: &str = "alignment-pending";
pub const CHANGE_REQ_CREATED: &str = "requirement-created";
pub const CHANGE_REQ_REVISED: &str = "requirement-revised";
pub const CHANGE_REQ_DELETED: &str = "requirement-deleted";
pub const CHANGE_ENTITY: &str = "entity-changed";
pub const CHANGE_ENTITY_DELETED: &str = "entity-deleted";
pub const CHANGE_NODE_DELETED: &str = "node-deleted";
pub const CHANGE_INSTANCE: &str = "instance-changed";
pub const CHANGE_PROMPT_UNANSWERED: &str = "prompt-unanswered";
pub const CHANGE_PROVENANCE_PENDING: &str = "provenance-pending";
pub const CHANGE_THRESHOLD_CROSSED: &str = "threshold-crossed";
pub const CHANGE_VIEW_MEMBER_GONE: &str = "view-member-gone";
pub const CHANGE_EDGES_MISSING: &str = "edges-missing";
pub const CHANGE_QUERY_MATCH: &str = "query-match";
// A child moved between the same two parents across generations; the subject is the
// child and `detail.between` names the two parents. Mirrors docs/compiler/reconciler.md#flip-detection.
pub const CHANGE_REPARENT_FLIP: &str = "reparent-flip";
// The scope root as a target: the parentless entities of a scope, addressed as
// `scope:<scope>` wherever a record, goal, or view needs a subject for the top level.
// Mirrors docs/compiler/concepts/levels.md#the-scope-root.
pub const SCOPE_ROOT_PREFIX: &str = "scope:";

pub fn scope_root_target(scope: &str) -> String {
    format!("{}{}", SCOPE_ROOT_PREFIX, scope)
}

// One entity's parent move as the journal records it. `None` is the scope root.
// Mirrors docs/compiler/graph.md#journal.
#[derive(Clone, Debug, PartialEq)]
pub struct ParentMove {
    pub generation: u64,
    pub child: String,
    pub from: Option<String>,
    pub to: Option<String>,
}
// Kinds whose subject is the dead node by design; never pruned for a missing subject.
pub const TRAIL_KINDS: [&str; 3] = [
    CHANGE_SECTION_REMOVED,
    CHANGE_REQ_DELETED,
    CHANGE_ENTITY_DELETED,
];
// The kinds that feed rejudge-pair on a requirement, and review-entity on an entity.
pub const REQ_REVIEW_KINDS: [&str; 3] =
    [CHANGE_REQ_CREATED, CHANGE_REQ_REVISED, CHANGE_NODE_DELETED];
pub const ENTITY_REVIEW_KINDS: [&str; 2] = [CHANGE_ENTITY, CHANGE_NODE_DELETED];

// What a changeset lands as: the journal kind, the goals it served, the goals it
// resolved, and its cost. Mirrors docs/compiler/graph.md#journal.
#[derive(Clone, Debug, Default)]
pub struct Commit {
    pub kind: String,
    pub batch: Vec<String>,
    pub resolved: Vec<Resolved>,
    pub rounds: u32,
    pub tokens: u64,
    // A changeset that lands whole or not at all: one skipped op rolls the graph
    // back and nothing is journaled (a retract, docs/compiler/goals/ratify.md#retract).
    pub all_or_nothing: bool,
}

impl Commit {
    // A store-level commit: edit, align, gc, settle-diagnostics, checks, decree,
    // dual-write, ratify, triage, answer.
    pub fn store(kind: &str) -> Commit {
        Commit {
            kind: kind.to_string(),
            ..Default::default()
        }
    }

    pub fn session(batch: Vec<String>, rounds: u32, tokens: u64) -> Commit {
        Commit {
            kind: "session".to_string(),
            batch,
            resolved: Vec::new(),
            rounds,
            tokens,
            all_or_nothing: false,
        }
    }
}

// The change records one generation writes, numbered as they are pushed. One record
// per kind and subject (and limit, for threshold crossings): the first push wins.
pub struct RecordBatch {
    generation: u64,
    records: Vec<ChangeRecord>,
}

impl RecordBatch {
    pub fn new(generation: u64) -> RecordBatch {
        RecordBatch {
            generation,
            records: Vec::new(),
        }
    }

    pub fn push(
        &mut self,
        mutation: usize,
        kind: &str,
        subject: &str,
        via: &str,
        detail: serde_json::Value,
    ) -> bool {
        if self
            .records
            .iter()
            .any(|c| c.kind == kind && c.subject == subject && c.detail["limit"] == detail["limit"])
        {
            return false;
        }
        let index = self.records.len() + 1;
        self.records.push(
            ChangeRecord::new(self.generation, index, mutation, kind, subject, via)
                .with_detail(detail),
        );
        true
    }

    pub fn has(&self, kind: &str, subject: &str) -> bool {
        self.records
            .iter()
            .any(|c| c.kind == kind && c.subject == subject)
    }

    pub fn records(&self) -> &[ChangeRecord] {
        &self.records
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[derive(Debug)]
pub struct CommitReport {
    pub applied: usize,
    pub skipped: Vec<String>,
    // The generation this commit landed as.
    pub generation: u64,
    // The change records this commit wrote.
    pub changes: Vec<ChangeRecord>,
    // Final entity ids touched by this commit.
    pub touched_entities: BTreeSet<String>,
    // Final requirement ids created or whose statement or quote changed in substance.
    pub changed_requirements: BTreeSet<String>,
    // What the sweep did right after this commit landed (the deletions, prunes,
    // dissolutions, and settled diagnostics, one line each), empty when nothing was
    // due. Mirrors docs/compiler/graph.md#the-sweep.
    pub swept: Vec<String>,
}

// A document that changed, with what a reconcile session needs to know.
#[derive(Clone, Debug)]
pub struct DirtyDoc {
    pub doc: String,
    pub dirty_sections: Vec<String>,
    pub stale_anchors: Vec<String>,
}

// A frontend that delegates file writes (the chat serving's edit sink) passes one;
// everyone else writes disk directly. Arguments: doc (relative), old_text, new_text,
// full new document text.
pub type WriteEdit<'a> = &'a dyn Fn(&str, &str, &str, &str) -> Result<(), String>;

// The prose half of a dual write: one text run in one section, with the document's
// text before and after the edit. Mirrors docs/compiler/graph.md#mutations.
#[derive(Clone, Debug)]
pub struct ProseEdit {
    pub doc: String,
    pub section: String,
    pub old_text: String,
    pub new_text: String,
    pub old_full: String,
    pub full: String,
}

impl ProseEdit {
    // Locate the replacement in the document on disk. A non-empty `old_text` must
    // locate inside the section's stored body, so the same phrase elsewhere in the
    // document never catches the edit (a section the tree no longer holds falls back to
    // the whole document). An empty `old_text` appends `new_text` after the section's
    // last line, which is how a ratification proposal lands.
    pub fn locate(
        doc: &str,
        section: &str,
        section_raw: Option<&str>,
        old_full: &str,
        old_text: &str,
        new_text: &str,
    ) -> Result<ProseEdit, String> {
        let window = section_raw
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .and_then(|raw| old_full.find(raw).map(|b| (b, b + raw.len())));
        let full = if old_text.trim().is_empty() {
            let Some((_, end)) = window else {
                return Err(format!(
                    "section {}#{} drifted from the document on disk; compile first",
                    doc, section
                ));
            };
            format!(
                "{}\n\n{}{}",
                old_full[..end].trim_end_matches('\n'),
                new_text.trim(),
                &old_full[end..]
            )
        } else {
            let stale = || {
                format!(
                    "the old text no longer locates in {}#{}; compile first, then edit",
                    doc, section
                )
            };
            let (b, e) = match window {
                Some((wb, we)) => md::locate_bytes(&old_full[wb..we], old_text)
                    .map(|(sb, se)| (wb + sb, wb + se))
                    .ok_or_else(stale)?,
                None => md::locate_bytes(old_full, old_text).ok_or_else(stale)?,
            };
            format!("{}{}{}", &old_full[..b], new_text, &old_full[e..])
        };
        Ok(ProseEdit {
            doc: doc.to_string(),
            section: section.to_string(),
            old_text: old_text.to_string(),
            new_text: new_text.to_string(),
            old_full: old_full.to_string(),
            full,
        })
    }
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
    // Alignment thresholds, registry constants (docs/compiler/graph.md#budgets-and-thresholds).
    pub align: crate::align::Thresholds,
}

pub(crate) fn normalize(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
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
// resumed build often re-extracts "X" as "X using Y" from the same sentence; that is
// one fact, not two. Distinct atomic facts sharing a sentence are not subsets.
pub(crate) fn statement_subsumes(a: &str, b: &str) -> bool {
    let toks = |s: &str| -> std::collections::BTreeSet<String> {
        normalize_statement(s)
            .split(' ')
            .map(String::from)
            .collect()
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

// The kind of a node id by prefix.
fn id_kind(id: &str) -> &'static str {
    if id.starts_with("req:") {
        "requirement"
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

fn provenance_is_pending(p: &Provenance) -> bool {
    !matches!(p, Provenance::Quote(_))
}

// The parent moves the journal entries record, oldest first. A mutation carrying both
// `parent` and `prior.parent` (null for parentless) moved its `id`; a `dissolve_entity`
// moved each of its `children` to its `parent`. Mirrors docs/compiler/graph.md#journal.
fn parent_moves_in(entries: &[JournalEntry]) -> Vec<ParentMove> {
    let id_or_none = |v: &serde_json::Value| v.as_str().map(String::from);
    let mut out = Vec::new();
    for entry in entries {
        for mv in &entry.mutations {
            let Some(id) = mv["id"].as_str() else {
                continue;
            };
            if mv["op"] == "dissolve_entity" {
                let to = id_or_none(&mv["parent"]);
                for child in mv["children"].as_array().into_iter().flatten() {
                    if let Some(child) = child.as_str() {
                        out.push(ParentMove {
                            generation: entry.generation,
                            child: child.to_string(),
                            from: Some(id.to_string()),
                            to: to.clone(),
                        });
                    }
                }
                continue;
            }
            let has =
                |v: &serde_json::Value, k: &str| v.as_object().is_some_and(|o| o.contains_key(k));
            if has(mv, "parent") && has(&mv["prior"], "parent") {
                out.push(ParentMove {
                    generation: entry.generation,
                    child: id.to_string(),
                    from: id_or_none(&mv["prior"]["parent"]),
                    to: id_or_none(&mv["parent"]),
                });
            }
        }
    }
    out
}

// The name and scope of every entity the journal created, by id: the natural key of
// a parent that no longer exists.
fn created_names_in(entries: &[JournalEntry]) -> BTreeMap<String, (String, String)> {
    let mut out = BTreeMap::new();
    for entry in entries {
        for mv in &entry.mutations {
            if mv["op"] != "create_entity" {
                continue;
            }
            let (Some(id), Some(name)) = (mv["id"].as_str(), mv["entity"]["name"].as_str()) else {
                continue;
            };
            let scope = mv["entity"]["scope"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| Entity::default().scope);
            out.insert(id.to_string(), (name.to_string(), scope));
        }
    }
    out
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
    pub fn load(out: &Path) -> Store {
        // Readers never take the lock: read the generation counter, load every shard,
        // and retry if the counter moved mid-read (a commit landed between shards).
        // A store of another version reads as empty; only a build archives it.
        // Mirrors docs/compiler/graph.md#concurrency and #store-version.
        let counter = || {
            yaml_to::<Status>(&out.join("status.yaml"))
                .map(|s| s.generation)
                .unwrap_or(0)
        };
        let mut store = Self::load_once(out);
        for _ in 0..4 {
            let before = counter();
            store = Self::load_once(out);
            if counter() == before {
                break;
            }
        }
        if store.status.version != STORE_VERSION {
            return Store {
                out: out.to_path_buf(),
                ..Default::default()
            };
        }
        store
    }

    // The build's opening: an out directory whose status.yaml lacks the version or
    // carries another one is archived whole to `<out>.bak` (an earlier archive is
    // replaced) and the store starts empty. Mirrors docs/compiler/graph.md#store-version.
    pub fn open_for_build(out: &Path) -> Store {
        let status_path = out.join("status.yaml");
        if status_path.exists() {
            let version = yaml_to::<Status>(&status_path)
                .map(|s| s.version)
                .unwrap_or(0);
            if version != STORE_VERSION {
                let name = out
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "jazyk-out".to_string());
                let bak = out.with_file_name(format!("{}.bak", name));
                std::fs::remove_dir_all(&bak).ok();
                match std::fs::rename(out, &bak) {
                    Ok(()) => eprintln!(
                        "[jazyk] store version {} is not {}; archived {} to {} and starting empty",
                        version,
                        STORE_VERSION,
                        out.display(),
                        bak.display()
                    ),
                    Err(e) => eprintln!(
                        "[jazyk] warning: could not archive {} to {}: {}",
                        out.display(),
                        bak.display(),
                        e
                    ),
                }
                return Store {
                    out: out.to_path_buf(),
                    ..Default::default()
                };
            }
        }
        Self::load(out)
    }

    fn load_once(out: &Path) -> Store {
        let g = out.join("graph");
        let mut store = Store {
            out: out.to_path_buf(),
            graph: Graph {
                entities: yaml_to(&g.join("entities.yaml")).unwrap_or_default(),
                requirements: yaml_to(&g.join("requirements.yaml")).unwrap_or_default(),
                views: yaml_to(&g.join("views.yaml")).unwrap_or_default(),
                relationships: yaml_to(&g.join("relationships.yaml")).unwrap_or_default(),
                state_machines: yaml_to(&g.join("state-machines.yaml")).unwrap_or_default(),
                diagnostics: yaml_to(&g.join("diagnostics.yaml")).unwrap_or_default(),
                redirects: yaml_to(&g.join("redirects.yaml")).unwrap_or_default(),
            },
            docs: BTreeMap::new(),
            status: yaml_to(&out.join("status.yaml")).unwrap_or_default(),
            align: Default::default(),
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
        write_yaml(&g.join("views.yaml"), &self.graph.views);
        write_yaml(&g.join("relationships.yaml"), &self.graph.relationships);
        write_yaml(&g.join("state-machines.yaml"), &self.graph.state_machines);
        write_yaml(&g.join("diagnostics.yaml"), &self.graph.diagnostics);
        write_yaml(&g.join("redirects.yaml"), &self.graph.redirects);
        self.save_status();
        for (doc, rec) in &self.docs {
            write_yaml(&self.out.join("docs").join(format!("{}.yaml", doc)), rec);
        }
    }

    // Every status write stamps the store version.
    pub fn save_status(&self) {
        let mut status = self.status.clone();
        status.version = STORE_VERSION;
        write_yaml(&self.out.join("status.yaml"), &status);
    }

    // The journal entry for one commit. Mirrors docs/compiler/graph.md#journal.
    fn journal_entry(
        &self,
        build: &str,
        commit: &Commit,
        mutations: Vec<serde_json::Value>,
    ) -> JournalEntry {
        JournalEntry {
            build: build.to_string(),
            generation: self.status.generation,
            kind: commit.kind.clone(),
            batch: commit.batch.clone(),
            mutations,
            resolved_goals: commit.resolved.clone(),
            opened_goals: Vec::new(),
            rounds: commit.rounds,
            tokens: commit.tokens,
            ..Default::default()
        }
    }

    fn journal_path(&self, generation: u64) -> PathBuf {
        self.out
            .join("journal")
            .join(format!("g{}.yaml", generation))
    }

    fn write_journal(&self, entry: &JournalEntry) {
        write_yaml(&self.journal_path(entry.generation), entry);
    }

    // The latest journal mutation that decreed over the given fact and recorded the
    // prior value it replaced. Mirrors docs/compiler/graph.md#journal: a decree
    // entry carries, on a mutation written over a quoted value, the prior value and
    // source.
    fn decree_prior(&self, id: &str, op: &str) -> Option<serde_json::Value> {
        for entry in self.journal_entries().iter().rev() {
            for mv in entry.mutations.iter().rev() {
                // Every parent move carries a prior; only a decree's prior restores.
                if mv["op"] == op
                    && mv["id"] == id
                    && mv["prior"].is_object()
                    && mv["provenance"]["decree"].is_object()
                {
                    return Some(mv["prior"].clone());
                }
            }
        }
        None
    }

    // Every journal entry on disk, oldest first.
    fn journal_entries(&self) -> Vec<JournalEntry> {
        let dir = self.out.join("journal");
        let Ok(read) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut gens: Vec<u64> = read
            .filter_map(|e| {
                let name = e.ok()?.file_name();
                let name = name.to_str()?;
                name.strip_prefix('g')?
                    .strip_suffix(".yaml")?
                    .parse::<u64>()
                    .ok()
            })
            .collect();
        gens.sort_unstable();
        gens.into_iter()
            .filter_map(|g| yaml_to::<JournalEntry>(&self.journal_path(g)))
            .collect()
    }

    // Every journaled parent move, oldest first: an `update_entity` or `retract_decree`
    // carrying `parent` and `prior.parent`, and each child of a `dissolve_entity` (a
    // tool's or the sweep's), which moved from the dissolved entity to its parent.
    // Mirrors docs/compiler/graph.md#journal.
    pub fn journaled_parent_moves(&self) -> Vec<ParentMove> {
        parent_moves_in(&self.journal_entries())
    }

    // The natural key a parent matches on across generations: the live entity's name
    // and scope, the same from the journal's create for a dead one (a grouping
    // dissolved and re-minted under a new id counts as the same parent), the id when
    // neither knows it; the scope root for a parentless child.
    // Mirrors docs/compiler/reconciler.md#flip-detection.
    fn parent_key(
        &self,
        parent: Option<&str>,
        child_scope: &str,
        names: &BTreeMap<String, (String, String)>,
    ) -> String {
        let Some(id) = parent else {
            return scope_root_target(child_scope);
        };
        let key = |name: &str, scope: &str| format!("{}|{}", normalize(name), scope);
        match self.graph.entities.get(id) {
            Some(e) => key(&e.name, &e.scope),
            None => names
                .get(id)
                .map(|(name, scope)| key(name, scope))
                .unwrap_or_else(|| id.to_string()),
        }
    }

    // The reparent flip: a commit moves a child to a parent it held before and away
    // from the parent it held last, the same two parents alternating across
    // generations. The journal's last move of the child went the other way between
    // the same keys. One `reparent-flip` record lands on the child, `between` naming
    // the parent it left and the one it returned to (a parentless side as the scope
    // root). Mirrors docs/compiler/reconciler.md#flip-detection.
    fn record_reparent_flips(
        &self,
        moves: &[(String, Option<String>, Option<String>, usize)],
        batch: &mut RecordBatch,
    ) {
        if moves.is_empty() {
            return;
        }
        let entries = self.journal_entries();
        let names = created_names_in(&entries);
        let history = parent_moves_in(&entries);
        for (child, from, to, m) in moves {
            let Some(e) = self.graph.entities.get(child) else {
                continue;
            };
            let scope = e.scope.clone();
            let key = |p: Option<&str>| self.parent_key(p, &scope, &names);
            let Some(last) = history.iter().rev().find(|h| h.child == *child) else {
                continue;
            };
            if key(last.from.as_deref()) != key(to.as_deref())
                || key(last.to.as_deref()) != key(from.as_deref())
            {
                continue;
            }
            let label = |p: &Option<String>| p.clone().unwrap_or_else(|| scope_root_target(&scope));
            batch.push(
                *m,
                CHANGE_REPARENT_FLIP,
                child,
                "parent",
                serde_json::json!({ "between": [label(from), label(to)] }),
            );
        }
    }

    // The reconciler re-derives the board after a commit and records the goals the
    // commit opened on that generation's journal entry, under the lock.
    pub fn record_opened_goals(&mut self, generation: u64, opened: Vec<OpenedGoal>) {
        let _flock = FileLock::acquire(&self.out);
        let path = self.journal_path(generation);
        let Some(mut entry) = yaml_to::<JournalEntry>(&path) else {
            return;
        };
        entry.opened_goals = opened;
        write_yaml(&path, &entry);
    }

    // Resolving a goal clears the records it stood on. Persists at once.
    pub fn clear_changes(&mut self, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        let _flock = FileLock::acquire(&self.out);
        self.status.changes.retain(|c| !ids.contains(&c.id));
        self.save_status();
    }

    // Land a batch of records in status.yaml (superseding same-kind same-subject ones).
    fn commit_records(&mut self, batch: RecordBatch) -> Vec<ChangeRecord> {
        let records = batch.records;
        for r in &records {
            self.status.record_change(r.clone());
        }
        records
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
        while self.graph.entities.contains_key(&id)
            || self.graph.redirects.contains_key(&id)
            || taken.contains(&id)
        {
            n += 1;
            id = format!("{}-{}", base, n);
        }
        id
    }

    // `req:<doc-stem>-<n>`; `doc` is the source document path, or `x` for a derived or
    // decreed requirement (docs/compiler/model.md#identifiers).
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

    // The id stem of a requirement: its source document, or `x` without a quote.
    pub fn req_stem(r: &Requirement) -> &str {
        r.source.as_ref().map(|s| s.doc.as_str()).unwrap_or("x")
    }

    pub fn mint_view_id(&self, kind: &str, title: &str, taken: &BTreeSet<String>) -> String {
        let base = format!("view:{}/{}", view_kind_slug(kind), md::slug(title));
        let mut id = base.clone();
        let mut n = 1;
        while self.graph.views.contains_key(&id) || taken.contains(&id) {
            n += 1;
            id = format!("{}-{}", base, n);
        }
        id
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

    // The entity natural key: normalized name or alias within the scope, and `parent`
    // when the caller supplies it. Without a parent, exactly one match lands; several
    // is an error naming the candidates, so the caller can say which parent it means.
    // Mirrors docs/compiler/concepts/identity.md#the-natural-key-under-containment.
    pub fn find_natural(
        &self,
        name: &str,
        scope: &str,
        parent: Option<&str>,
    ) -> Result<Option<String>, Vec<String>> {
        let want = normalize(name);
        let parent = parent.map(|p| self.resolve_id(p).to_string());
        let mut hits: Vec<String> = self
            .graph
            .entities
            .iter()
            .filter(|(_, e)| e.scope == scope)
            .filter(|(_, e)| {
                normalize(&e.name) == want || e.aliases.iter().any(|a| normalize(a) == want)
            })
            .filter(|(_, e)| match &parent {
                Some(p) => e.parent.as_deref() == Some(p.as_str()),
                None => true,
            })
            .map(|(id, _)| id.clone())
            .collect();
        hits.sort();
        match hits.len() {
            0 => Ok(None),
            1 => Ok(hits.pop()),
            _ => Err(hits),
        }
    }

    // The view natural key: kind plus normalized title.
    pub fn find_view(&self, kind: &str, title: &str) -> Option<String> {
        let want = normalize(title);
        self.graph
            .views
            .iter()
            .find(|(_, v)| v.kind == kind && normalize(&v.title) == want)
            .map(|(id, _)| id.clone())
    }

    // Whether `ancestor` is on `node`'s parent chain (bounded against a cycle).
    pub fn is_ancestor(&self, ancestor: &str, node: &str) -> bool {
        let mut cur = node;
        for _ in 0..64 {
            match self
                .graph
                .entities
                .get(cur)
                .and_then(|e| e.parent.as_deref())
            {
                Some(p) if p == ancestor => return true,
                Some(p) => cur = p,
                None => return false,
            }
        }
        false
    }

    // The parent chain of an entity, nearest first.
    fn ancestors(&self, node: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = node;
        for _ in 0..64 {
            match self
                .graph
                .entities
                .get(cur)
                .and_then(|e| e.parent.as_deref())
            {
                Some(p) => {
                    out.push(p.to_string());
                    cur = p;
                }
                None => break,
            }
        }
        out
    }

    fn children_of(&self, id: &str) -> Vec<String> {
        self.graph
            .entities
            .iter()
            .filter(|(_, e)| e.parent.as_deref() == Some(id))
            .map(|(cid, _)| cid.clone())
            .collect()
    }

    // A grouping: an entity no document states, holding a level. Derived provenance,
    // no mentions, and no requirements of its own: higher levels carry none, so a
    // derived entity holding requirements is a caps-variant sub-entity, never a
    // grouping. An entity a document states holds children in role, not in
    // provenance. Mirrors docs/compiler/concepts/levels.md#groupings.
    pub fn is_grouping(&self, id: &str) -> bool {
        self.graph.entities.get(id).is_some_and(|e| {
            matches!(e.provenance, Some(Provenance::Derived { .. }))
                && e.mentions.is_empty()
                && self.requirements_referencing(id).is_empty()
        })
    }

    // Dissolve one entity: its children reparent to its parent (parentless when it was
    // top-level, or when that parent dissolved in the same sweep), and it tombstones
    // with a redirect to that parent, so anything holding the old id resolves there.
    // Returns the parent and the children moved. Mirrors docs/compiler/graph.md#the-sweep.
    fn dissolve(&mut self, id: &str, build: &str) -> (Option<String>, Vec<String>) {
        let parent = self
            .graph
            .entities
            .get(id)
            .and_then(|e| e.parent.clone())
            .map(|p| self.resolve_id(&p).to_string())
            .filter(|p| self.graph.entities.contains_key(p));
        let children = self.children_of(id);
        for c in &children {
            let e = self.graph.entities.get_mut(c).unwrap();
            e.parent = parent.clone();
            e.updated = Some(build.to_string());
        }
        self.graph.entities.remove(id);
        self.graph
            .redirects
            .insert(id.to_string(), parent.clone().unwrap_or_default());
        (parent, children)
    }

    // Whether a node id names an existing entity or requirement.
    fn node_exists(&self, id: &str) -> bool {
        self.graph.entities.contains_key(id) || self.graph.requirements.contains_key(id)
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
                scored.push((
                    t,
                    id.clone(),
                    e.name.clone(),
                    e.definition.clone().unwrap_or_default(),
                ));
            }
        }
        scored.sort();
        scored
            .into_iter()
            .take(8)
            .map(|(_, id, n, d)| (id, n, d))
            .collect()
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
    fn content_tokens(&self, statement: &str, entities: &[String]) -> BTreeSet<String> {
        const STOP: [&str; 30] = [
            "the", "a", "an", "shall", "to", "of", "in", "on", "for", "is", "are", "be", "or",
            "and", "if", "with", "by", "it", "its", "when", "then", "that", "which", "this",
            "system", "not", "no", "only", "all", "each",
        ];
        // The suffix goes, then a trailing "e", so "files" and "filed" both land on
        // "fil" and "reverses" meets "reversed": the one stem per word the pairing
        // rules count on.
        let stem = |t: &str| -> String {
            let mut s = t.to_string();
            for suffix in ["ing", "ed", "s"] {
                if t.len() > suffix.len() + 2 && t.ends_with(suffix) {
                    s = t[..t.len() - suffix.len()].to_string();
                    break;
                }
            }
            if s.len() > 3 && s.ends_with('e') {
                s.pop();
            }
            s
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
        normalize_statement(statement)
            .split(' ')
            .filter(|t| !STOP.contains(t))
            .map(|t| stem(t))
            .filter(|t| !name_toks.contains(t))
            .collect()
    }

    // The graph's hub: the entity co-referenced with the most other entities across
    // requirements. A tie names none, and so does a graph whose entities never meet.
    // Mirrors docs/compiler/reconciler.md#pairs.
    fn hub_entity(&self) -> Option<String> {
        let mut peers: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for r in self.graph.requirements.values() {
            let ids: BTreeSet<&str> = r.entities.iter().map(|e| self.resolve_id(e)).collect();
            for a in &ids {
                for b in &ids {
                    if a != b {
                        peers.entry(a).or_default().insert(b);
                    }
                }
            }
        }
        let best = peers.values().map(|p| p.len()).max().unwrap_or(0);
        if best == 0 {
            return None;
        }
        let mut hubs = peers.iter().filter(|(_, p)| p.len() == best);
        let hub = hubs.next().map(|(id, _)| id.to_string());
        if hubs.next().is_some() {
            return None;
        }
        hub
    }

    // The section reference a document opens with: its first section in order.
    fn opening_section(&self, doc: &str) -> Option<&str> {
        self.docs
            .get(doc)?
            .sections
            .iter()
            .min_by_key(|(_, s)| s.order)
            .map(|(r, _)| r.as_str())
    }

    // Deterministic neighbor set for one requirement: other requirements sharing an
    // entity, scored by overlapping content tokens, at least two shared, best six.
    // The rejudge-pair locality. Mirrors docs/compiler/reconciler.md#pairs.
    pub fn requirement_neighbors(&self, rid: &str) -> Vec<String> {
        let Some(req) = self.graph.requirements.get(rid) else {
            return Vec::new();
        };
        let subject_entities: BTreeSet<&str> =
            req.entities.iter().map(|e| self.resolve_id(e)).collect();
        let toks = self.content_tokens(&req.statement, &req.entities);
        let hub = self.hub_entity();
        let norm = crate::derive::normalize_state;
        // The transition the subject carries, its subject resolved and its states
        // normalized, so a restated transition meets its original.
        let transition = req
            .transition
            .as_ref()
            .map(|t| (self.resolve_id(&t.subject), norm(&t.from), norm(&t.to)));
        // Whether the subject sits in its document's opening section, or in a later one.
        let placement = req.source.as_ref().and_then(|s| {
            let opening = self.opening_section(&s.doc)?;
            let order = self.docs.get(&s.doc)?.sections.get(&s.section)?.order;
            Some((s.doc.as_str(), s.section == opening, order))
        });
        let mut scored: Vec<(usize, &String)> = Vec::new();
        for (oid, other) in &self.graph.requirements {
            if oid == rid {
                continue;
            }
            let other_entities: BTreeSet<&str> =
                other.entities.iter().map(|e| self.resolve_id(e)).collect();
            let shared_entities = other_entities.intersection(&subject_entities).count();
            if shared_entities == 0 {
                continue;
            }
            let shared = self
                .content_tokens(&other.statement, &other.entities)
                .intersection(&toks)
                .count();
            // A pair whose only shared entity is the hub counts no entity and needs
            // three shared tokens: sharing the hub says little, and a hub-sharing
            // flood once derived forty-nine pair goals with one real pair among them.
            let hub_only = shared_entities == 1
                && hub
                    .as_deref()
                    .map(|h| subject_entities.contains(h) && other_entities.contains(h))
                    .unwrap_or(false);
            let (entity_score, token_bar) = if hub_only {
                (0, 3)
            } else {
                (shared_entities, 2)
            };
            // Both carry a transition with the same subject and the same from and to:
            // a restated transition is the likeliest duplicate. Qualifies on its own,
            // scores two more.
            let same_transition = match (&transition, &other.transition) {
                (Some((subj, from, to)), Some(t)) => {
                    self.resolve_id(&t.subject) == *subj
                        && norm(&t.from) == *from
                        && norm(&t.to) == *to
                }
                _ => false,
            };
            // An intro-versus-steps restatement: one sourced in the document's opening
            // section, the other in a later section of the same document. Qualifies
            // with one shared entity and one shared token (typically the verb stem),
            // scores one more.
            let intro_restatement = match (&placement, &other.source) {
                (Some((doc, intro, order)), Some(s)) if s.doc == *doc => {
                    let other_order = self
                        .docs
                        .get(&s.doc)
                        .and_then(|d| d.sections.get(&s.section))
                        .map(|sec| sec.order);
                    let other_intro = self.opening_section(&s.doc) == Some(s.section.as_str());
                    match other_order {
                        Some(o) => (*intro && o > *order) || (other_intro && *order > o),
                        None => false,
                    }
                }
                _ => false,
            };
            // Two shared content tokens qualify, and so do two shared entities: a
            // restatement built from the same entities can share every noun and
            // still share no other token, because the shared names leave the token
            // pool. Mirrors docs/compiler/reconciler.md#pairs.
            let qualifies = shared >= token_bar
                || shared_entities >= 2
                || same_transition
                || (intro_restatement && shared >= 1);
            if qualifies {
                let boost = usize::from(same_transition) * 2 + usize::from(intro_restatement);
                scored.push((shared + entity_score + boost, oid));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        scored
            .into_iter()
            .take(6)
            .map(|(_, id)| id.clone())
            .collect()
    }

    // The pair set: computed neighbors plus sticky partners. Open contradiction and
    // duplicate-requirement diagnostics tie pairs; editing one side of a known pair
    // always re-judges the other.
    pub fn pair_review_neighbors(&self, rid: &str) -> Vec<String> {
        let mut nbrs = self.requirement_neighbors(rid);
        for d in self.graph.diagnostics.values() {
            if d.lifecycle != "open"
                || !(d.rule == "contradiction" || d.rule == "duplicate-requirement")
            {
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

    // An open judged diagnostic naming this node: reason enough for a pair judgment
    // even when the neighbor computation finds nothing.
    pub fn has_open_judged_diag(&self, id: &str) -> bool {
        self.graph.diagnostics.values().any(|d| {
            d.lifecycle == "open"
                && JUDGED_RULES.contains(&d.rule.as_str())
                && d.subjects.iter().any(|s| s == id)
        })
    }

    // Whether a pair judgment on a requirement still has work: the requirement exists
    // and either computed neighbors or an open judged diagnostic tie it to a judgment.
    pub fn pair_review_due(&self, rid: &str) -> bool {
        self.graph.requirements.contains_key(rid)
            && (!self.pair_review_neighbors(rid).is_empty() || self.has_open_judged_diag(rid))
    }

    // Deleting nodes settles the open judged diagnostics naming them: all subjects gone
    // resolves the diagnostic in place (the returned ops go to the journal); a surviving
    // subject gets a node-deleted record, so a session re-judges the finding. Runs on
    // every deleting commit, session and sweep alike.
    // Mirrors docs/compiler/graph.md#the-sweep.
    fn propagate_deletions(
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
    fn deletion_ripple(&self, deleted: &BTreeMap<String, usize>, batch: &mut RecordBatch) {
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

    // Drop records whose evidence lapsed or whose subject no longer exists (the trail
    // kinds excepted). Mirrors docs/compiler/reconciler.md#change-records.
    fn prune_records(&mut self) {
        let alive: Vec<bool> = self
            .status
            .changes
            .iter()
            .map(|c| self.record_stands(c))
            .collect();
        let mut i = 0;
        self.status.changes.retain(|_| {
            let keep = alive[i];
            i += 1;
            keep
        });
    }

    fn record_stands(&self, c: &ChangeRecord) -> bool {
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

    // The marker diagnostics whose condition cleared: an incomplete-build marker whose
    // goal left `parked`, an uncovered-section marker whose section is marked covered
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
                "incomplete-build" => {
                    if !self.status.parked.iter().any(|p| &p.target == subject) {
                        out.push((
                            id.clone(),
                            format!("{} is no longer parked; a later build resumed it", subject),
                        ));
                    }
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
        self.save();
        actions
    }

    // The level-triggered half of deletion propagation: sweep every open judged
    // diagnostic for missing subjects and settle it, journaled like the sweep.
    // Mirrors docs/compiler/reconciler.md#pairs.
    pub fn settle_dangling_diags(&mut self) -> Vec<String> {
        let _flock = FileLock::acquire(&self.out);
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
            self.save();
        }
        actions
    }

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
                None => std::fs::write(&path, text)
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
        Ok(report)
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
        let splice = |text: &str| -> Option<String> {
            md::locate_bytes(text, &edit.old_text)
                .map(|(b, e)| format!("{}{}{}", &text[..b], edit.new_text, &text[e..]))
        };
        for (qid, r) in &self.graph.requirements {
            let Some(src) = r.source.as_ref() else {
                continue;
            };
            if src.doc != edit.doc || src.section != edit.section || handled(qid) {
                continue;
            }
            let Some(new_quote) = splice(&src.quote) else {
                continue;
            };
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
            rule: "ratification-pending".to_string(),
            severity: "warning".to_string(),
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

    // ---- commit ----

    // Apply a staged changeset atomically: reconcile creates by natural key against nodes
    // committed concurrently, apply ops in order under the commit-time gates, propagate
    // deletions, recompute derived data, write the change records, journal, bump the
    // generation, write shards. Mirrors docs/compiler/graph.md#changesets.
    pub fn apply(&mut self, ops: Vec<Op>, commit: &Commit) -> CommitReport {
        let _flock = FileLock::acquire(&self.out);
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
        // The state an all-or-nothing changeset rolls back to when an op is skipped.
        let snapshot = commit
            .all_or_nothing
            .then(|| (self.graph.clone(), self.status.clone(), self.docs.clone()));

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
                    add_mention,
                    stereotype,
                    parent,
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
                    let mut moved_between: Option<(Option<String>, String)> = None;
                    if let Some(p) = &parent {
                        if e.parent.as_deref() != Some(p.as_str()) {
                            prior.insert("parent".into(), serde_json::json!(e.parent));
                            moves.push((rid.clone(), e.parent.clone(), Some(p.clone()), m));
                            moved_between = Some((e.parent.clone(), p.clone()));
                        }
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
                        if let Some(f) = from {
                            dirt.entity(f, m, "parent");
                        }
                        dirt.entity(to, m, "parent");
                    }
                    let move_only = parent.is_some()
                        && name.is_none()
                        && definition.is_none()
                        && add_aliases.is_empty()
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
                        add_mention,
                        stereotype,
                        parent,
                        set_attributes,
                        add_attributes,
                        provenance,
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
                            from.dedup();
                        }
                    };
                    for r in self.graph.requirements.values_mut() {
                        r.entities.iter_mut().for_each(rewire);
                        r.entities.dedup();
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
                        rewire_provenance(&mut e.provenance);
                        for a in e.attributes.iter_mut() {
                            let mut p = Some(a.provenance.clone());
                            rewire_provenance(&mut p);
                            a.provenance = p.unwrap();
                        }
                    }
                    for v in self.graph.views.values_mut() {
                        v.members.iter_mut().for_each(rewire);
                        v.members.dedup();
                        v.collapse.iter_mut().for_each(rewire);
                        v.collapse.dedup();
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
                        d.subjects.dedup();
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
                    self.graph.redirects.insert(absorb.clone(), keep.clone());
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
                    requirement.entities.dedup();
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
                    let same_key = |r: &Requirement, incoming: &Requirement| -> bool {
                        let same_statement = normalize_statement(&r.statement)
                            == normalize_statement(&incoming.statement);
                        match (
                            &r.source,
                            &incoming.source,
                            &r.provenance,
                            &incoming.provenance,
                        ) {
                            (Some(a), Some(b), _, _) => {
                                a.doc == b.doc
                                    && a.section == b.section
                                    && (same_statement
                                        || (normalize_statement(&a.quote)
                                            == normalize_statement(&b.quote)
                                            && statement_subsumes(
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
                                same_statement
                                    && fa.iter().collect::<BTreeSet<_>>()
                                        == fb.iter().collect::<BTreeSet<_>>()
                            }
                            (
                                None,
                                None,
                                Some(Provenance::Decree { .. }),
                                Some(Provenance::Decree { .. }),
                            ) => same_statement,
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
                            r.statement = requirement.statement.clone();
                            refreshed_statement = Some(requirement.statement.clone());
                            dirt.revised(&existing, m, "fields");
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
                        final_id = self.mint_req_id(Self::req_stem(&requirement), &BTreeSet::new());
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
                        v.dedup();
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
                    let r = self.graph.requirements.get_mut(&rid).unwrap();
                    let before: Vec<String> = r.entities.clone();
                    let statement_before = r.statement.clone();
                    if let Some(e) = &statement {
                        if r.statement != *e {
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
                    dirt.entities(before.iter().chain(after.iter()), m);
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
                            applied.push(Op::DeleteRequirement { id: rid, reason });
                        }
                        None => skipped.push(format!("delete_requirement: unknown id {}", rid)),
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
                    view.members.dedup();
                    view.collapse = view
                        .collapse
                        .iter()
                        .map(|x| resolve(&remap, self, x))
                        .collect();
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
                    let members = members.map(&res);
                    let add_members = res(add_members);
                    let remove_members = res(remove_members);
                    let collapse = collapse.map(&res);
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
                                applied.extend(self.resolve_ratification_diags(
                                    &rid, &build, &reason,
                                ));
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
                        let old_source = std::mem::replace(&mut r.source, Some(to.clone()))
                            .unwrap_or_else(|| from.clone());
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
                                final_id = self.mint_diag_id(&diagnostic.rule, &BTreeSet::new());
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
        if let Some((graph, status, docs)) = snapshot {
            if !skipped.is_empty() {
                self.graph = graph;
                self.status = status;
                self.docs = docs;
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

        // The typed dirtiness this commit caused. Mirrors docs/compiler/graph.md#change-records.
        for (id, (m, via)) in &dirt.entities {
            if self.graph.entities.contains_key(id) {
                batch.push(*m, CHANGE_ENTITY, id, via, serde_json::Value::Null);
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
            }
        }
        for (id, (m, via)) in &dirt.revised {
            if self.graph.requirements.contains_key(id) && !dirt.created.contains_key(id) {
                batch.push(*m, CHANGE_REQ_REVISED, id, via, serde_json::Value::Null);
            }
        }
        for (id, m) in &dirt.deleted {
            let kind = match id_kind(id) {
                "requirement" => CHANGE_REQ_DELETED,
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
            let did = self.mint_diag_id("ratification-pending", &BTreeSet::new());
            dirt.prompt(&did, m, "ratification");
            applied.push(Op::ReportDiagnostic {
                id: did.clone(),
                diagnostic: diagnostic.clone(),
            });
            self.graph.diagnostics.insert(did, diagnostic);
        }
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
        self.status.spent.tokens += commit.tokens;
        self.prune_records();
        let changes = self.commit_records(batch);
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
        self.write_journal(&entry);
        self.save();
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

    // The deterministic sweep as one unit: the mechanical garbage collection, the
    // settle of judged diagnostics whose subjects are gone, and the settle of marker
    // diagnostics whose condition cleared. Every commit runs it behind its own entry
    // (`apply`); a build runs it after alignment too, before any session, since the
    // `edit` entry is no changeset. Returns one line per action.
    // Mirrors docs/compiler/graph.md#the-sweep.
    pub fn sweep(&mut self) -> Vec<String> {
        let _flock = FileLock::acquire(&self.out);
        self.sweep_locked()
    }

    fn sweep_locked(&mut self) -> Vec<String> {
        let mut actions = self.gc();
        actions.extend(self.settle_dangling_diags_locked());
        actions.extend(self.settle_cleared_markers_locked());
        actions
    }

    // An anchor-stale record on a section, its anchors merged with any earlier record
    // on the same section.
    fn push_stale_anchor(
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

    // Resolve the open ratification-pending diagnostics on a fact, returning the
    // journaled resolutions.
    fn resolve_ratification_diags(&mut self, subject: &str, build: &str, reason: &str) -> Vec<Op> {
        let mut out = Vec::new();
        for (did, d) in self.graph.diagnostics.iter_mut() {
            if d.lifecycle == "open"
                && d.rule == "ratification-pending"
                && d.subjects.iter().any(|s| s == subject)
            {
                d.lifecycle = "resolved".to_string();
                d.updated = Some(build.to_string());
                out.push(Op::ResolveDiagnostic {
                    id: did.clone(),
                    reason: reason.to_string(),
                });
            }
        }
        out
    }

    // ---- document sync (the dirty set) ----

    // Bring the stored document records in line with a fresh parse. Returns the dirty work.
    // Alignment matches the trees (docs/compiler/alignment.md): exact moves rewrite
    // anchored references mechanically and are not dirty; every other relocation is a
    // proposal persisted for the place-anchors session; an anchor with no candidate is
    // stale. Coverage carries over only for unchanged and exactly moved sections. A save
    // that dirtied sections journals an `edit` entry of its own; the moves and proposals
    // journal an `align` entry. Both write their change records.
    // Mirrors docs/compiler/reconciler.md#dirty-set.
    pub fn sync_docs(
        &mut self,
        parsed: &BTreeMap<String, (String, BTreeMap<String, Section>)>,
    ) -> Vec<DirtyDoc> {
        self.sync_docs_inner(parsed, true)
    }

    // The same alignment without a write: the section trees, the dirty set, the
    // alignment blocks, and the records land in memory and the `edit` and `align`
    // entries are minted but never written, so a read-only consumer (status, preview,
    // release, monitor, the GUI board) derives from the documents as they stand
    // while the entries stay a build's to write.
    // Mirrors docs/compiler/reconciler.md#goal-derivation.
    pub fn sync_docs_in_memory(
        &mut self,
        parsed: &BTreeMap<String, (String, BTreeMap<String, Section>)>,
    ) -> Vec<DirtyDoc> {
        self.sync_docs_inner(parsed, false)
    }

    fn sync_docs_inner(
        &mut self,
        parsed: &BTreeMap<String, (String, BTreeMap<String, Section>)>,
        persist: bool,
    ) -> Vec<DirtyDoc> {
        use crate::align::Full;
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
        let moved_from: BTreeSet<&Full> = plan.exact_moves.iter().map(|(f, _)| f).collect();
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
                batch.push(
                    i + 1,
                    kind,
                    full,
                    "section",
                    serde_json::json!({"doc": doc, "section": section}),
                );
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

        // Mechanical moves and the proposals left pending: one align entry per build.
        if !plan.exact_moves.is_empty() || !plan.proposals.is_empty() {
            self.status.generation += 1;
            let build = format!("g{}", self.status.generation);
            let mut mutations: Vec<serde_json::Value> = plan
                .exact_moves
                .iter()
                .map(|(f, t)| serde_json::json!({"op": "move", "from": f.render(), "to": t.render()}))
                .collect();
            mutations.extend(plan.proposals.iter().map(|p| {
                serde_json::json!({"op": "propose", "anchor": p.anchor, "from": p.from,
                    "candidates": p.candidates.iter().map(|c| c.section.clone()).collect::<Vec<_>>()})
            }));
            let entry = self.journal_entry(&build, &Commit::store("align"), mutations);
            entries.push(entry);
        }
        // Stale anchors and pending proposals, under the last generation written.
        let generation = self.status.generation;
        let mut batch = RecordBatch::new(generation);
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
                std::fs::remove_file(self.out.join("docs").join(format!("{}.yaml", doc))).ok();
            }
            for entry in &entries {
                self.write_journal(entry);
            }
            // Persist the synced records so context reads see the new sections.
            self.save();
        }
        self.reevaluate_items(out)
    }

    // Anchors a place-anchors session flagged for re-evaluation ride on their document's
    // reconcile item as stale anchors, their section dirty, whether or not the document
    // changed this build. Mirrors docs/compiler/reconciler.md#dirty-set.
    fn reevaluate_items(&self, mut out: Vec<DirtyDoc>) -> Vec<DirtyDoc> {
        for (doc, _) in &self.docs {
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
    fn proposed_anchors(&self) -> BTreeSet<String> {
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
        let orphans: Vec<String> = self
            .graph
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
            .collect();
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
        let under: Vec<String> = self
            .graph
            .entities
            .keys()
            .filter(|id| child_counts.get(*id).copied().unwrap_or(0) < 2 && self.is_grouping(id))
            .cloned()
            .collect();
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
                    _ => CHANGE_ENTITY_DELETED,
                };
                batch.push(*m, kind, id, "sweep", serde_json::Value::Null);
            }
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
            self.save();
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
    pub fn reconcile_check_diags(
        &mut self,
        findings: Vec<(
            String,
            Vec<String>,
            String,
            String,
            Option<crate::model::DiagnosticPrompt>,
        )>,
    ) {
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
                    let id = self.mint_diag_id(&rule, &BTreeSet::new());
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
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    use std::io::Write;
                    write!(f, "{}", std::process::id()).ok();
                    return FileLock { path };
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        eprintln!(
            "[jazyk] warning: stale lock at {}; proceeding",
            path.display()
        );
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

    // A directory of its own for a test that reloads from disk (the shared tmp dir is
    // stomped by parallel tests).
    fn own_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jazyk-{}-test-{}", label, std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    fn wi() -> WorkItem {
        WorkItem {
            task: "reconcile-doc".into(),
            target: "t.md".into(),
            dirty_sections: vec![],
            stale_anchors: vec![],
            proposals: Vec::new(),
        }
    }

    fn session() -> Commit {
        wi().commit(1, 0)
    }

    fn mention(doc: &str, sec: &str, quote: &str) -> SourceRef {
        SourceRef {
            doc: doc.into(),
            section: sec.into(),
            quote: quote.into(),
        }
    }

    fn seed_doc(store: &mut Store, doc: &str, text: &str) {
        let sections = crate::md::parse_sections(text);
        store.docs.insert(
            doc.to_string(),
            DocRecord {
                content_hash: hash_hex(text),
                sections,
                coverage: BTreeMap::new(),
            },
        );
    }

    // A fixture entity the sweep keeps. The sweep runs behind every commit and deletes
    // an entity with nothing on it (no mention, requirement, attribute, child, or
    // provenance); a decreed attribute stands in for the mention a real one carries.
    fn entity(name: &str) -> Entity {
        Entity {
            name: name.into(),
            attributes: vec![Attribute {
                name: "fixture".into(),
                r#type: None,
                value: None,
                provenance: Provenance::Decree {
                    author: "test".into(),
                    at: String::new(),
                    note: None,
                },
            }],
            ..Default::default()
        }
    }

    fn create(id: &str, name: &str) -> Op {
        Op::CreateEntity {
            id: id.into(),
            entity: entity(name),
        }
    }

    fn derived(from: &[&str]) -> Provenance {
        Provenance::Derived {
            from: from.iter().map(|f| f.to_string()).collect(),
            reasoning: "invented structure".into(),
        }
    }

    fn journal_entry(s: &Store, generation: u64) -> JournalEntry {
        let path = s.out.join("journal").join(format!("g{}.yaml", generation));
        serde_norway::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    // A dual write lands the prose and the graph as one changeset, absorbs its own
    // hashes so the document is not dirty against the graph, re-anchors the quoted
    // facts in the section, and puts the file back when the commit skips.
    // Mirrors docs/compiler/compilation.md#edit-paths.
    #[test]
    fn dual_write_absorbs_its_own_hashes_and_rolls_back_a_stale_edit() {
        let root = own_dir("dual-write");
        let text = "# Pay\n\nAn Order is paid within 30 days.\n";
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/pay.md"), text).unwrap();
        let mut s = Store {
            out: root.join("jazyk-out"),
            ..Default::default()
        };
        let parsed_of = |text: &str| {
            let mut m = BTreeMap::new();
            m.insert(
                "docs/pay.md".to_string(),
                (hash_hex(text), crate::md::parse_sections(text)),
            );
            m
        };
        s.sync_docs(&parsed_of(text));
        s.apply(
            vec![
                create("ent:order", "Order"),
                Op::CreateRequirement {
                    id: "req:pay-1".into(),
                    requirement: Requirement {
                        statement: "An Order is paid within 30 days.".into(),
                        entities: vec!["ent:order".into()],
                        source: Some(mention(
                            "docs/pay.md",
                            "/pay",
                            "An Order is paid within 30 days.",
                        )),
                        ..Default::default()
                    },
                },
            ],
            &session(),
        );
        let before = s.status.generation;
        let raw = s.docs["docs/pay.md"].sections["/pay"].raw.clone();
        let edit = ProseEdit::locate(
            "docs/pay.md",
            "/pay",
            Some(&raw),
            text,
            "within 30 days",
            "within 21 days",
        )
        .unwrap();
        let report = s
            .dual_write(
                &root,
                &edit,
                vec![Op::UpdateRequirement {
                    id: "req:pay-1".into(),
                    statement: Some("An Order is paid within 21 days.".into()),
                    entities: None,
                    edges: None,
                    transition: None,
                    facets: None,
                    source: Some(mention(
                        "docs/pay.md",
                        "/pay",
                        "An Order is paid within 21 days.",
                    )),
                    provenance: None,
                }],
                &Commit::store("dual-write"),
                None,
            )
            .unwrap();
        assert_eq!(report.generation, before + 1, "one changeset");
        let on_disk = std::fs::read_to_string(root.join("docs/pay.md")).unwrap();
        assert!(on_disk.contains("within 21 days"), "{}", on_disk);
        assert_eq!(s.docs["docs/pay.md"].content_hash, hash_hex(&on_disk));
        let r = &s.graph.requirements["req:pay-1"];
        assert_eq!(
            r.source.as_ref().unwrap().quote,
            "An Order is paid within 21 days."
        );
        assert!(r.statement.contains("21 days"));
        // The entity mention derived from the sentence followed it.
        assert!(s.graph.entities["ent:order"]
            .mentions
            .iter()
            .any(|m| m.quote == "An Order is paid within 21 days."));
        let entry = journal_entry(&s, report.generation);
        assert_eq!(entry.kind, "dual-write");
        assert!(entry.mutations.iter().any(|m| m["op"] == "edit_doc_prose"));
        assert!(entry
            .mutations
            .iter()
            .any(|m| m["op"] == "update_requirement"));
        // No re-dirtying: a following sync reports nothing and writes no record.
        let records = s.status.changes.clone();
        let dirty = s.sync_docs(&parsed_of(&on_disk));
        assert!(dirty.is_empty(), "{:?}", dirty);
        assert_eq!(s.status.changes, records);
        // A stale edit (the old text no longer stands) skips whole; the file comes back.
        let stale = ProseEdit {
            doc: "docs/pay.md".into(),
            section: "/pay".into(),
            old_text: "within 30 days".into(),
            new_text: "within 7 days".into(),
            old_full: on_disk.clone(),
            full: on_disk.replace("21", "7"),
        };
        let err = s
            .dual_write(
                &root,
                &stale,
                vec![Op::DeleteRequirement {
                    id: "req:pay-1".into(),
                    reason: "x".into(),
                }],
                &Commit::store("dual-write"),
                None,
            )
            .unwrap_err();
        assert!(err.contains("edit-stale"), "{}", err);
        assert_eq!(
            std::fs::read_to_string(root.join("docs/pay.md")).unwrap(),
            on_disk
        );
        assert!(s.graph.requirements.contains_key("req:pay-1"));
        // A prose edit never lands alone.
        assert!(s
            .dual_write(&root, &edit, Vec::new(), &Commit::store("dual-write"), None)
            .is_err());
        // An empty old_text appends the sentence to the section's body.
        let on_disk = std::fs::read_to_string(root.join("docs/pay.md")).unwrap();
        let raw = s.docs["docs/pay.md"].sections["/pay"].raw.clone();
        let insert = ProseEdit::locate(
            "docs/pay.md",
            "/pay",
            Some(&raw),
            &on_disk,
            "",
            "A late Order is cancelled.",
        )
        .unwrap();
        assert!(
            insert
                .full
                .ends_with("within 21 days.\n\nA late Order is cancelled.\n"),
            "{}",
            insert.full
        );
        std::fs::remove_dir_all(&root).ok();
    }

    // A commit writes the change records its mutations caused, each with its cause;
    // resolving the goals clears them by id. Mirrors docs/compiler/graph.md#change-records.
    #[test]
    fn commit_writes_change_records_and_clear_changes_removes_them() {
        let mut s = Store {
            out: own_dir("records"),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
        let ops = vec![
            create("ent:cart", "Cart"),
            Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    statement: "The Cart holds items.".into(),
                    entities: vec!["ent:cart".into()],
                    source: Some(mention("t.md", "/t", "The Cart holds items.")),
                    ..Default::default()
                },
            },
        ];
        let report = s.apply(ops, &session());
        assert_eq!(report.generation, 1);
        assert_eq!(report.changes.len(), 2, "{:?}", report.changes);
        let entity = report
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_ENTITY)
            .unwrap();
        assert_eq!(
            (
                entity.subject.as_str(),
                entity.mutation,
                entity.via.as_str()
            ),
            ("ent:cart", 1, "fields")
        );
        let created = report
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_REQ_CREATED)
            .unwrap();
        assert_eq!(
            (
                created.subject.as_str(),
                created.mutation,
                created.via.as_str()
            ),
            ("req:t-1", 2, "section")
        );
        assert!(report.changes.iter().all(|c| c.id.starts_with("c1-")));
        assert_ne!(report.changes[0].id, report.changes[1].id);
        // The journal entry names the session's goal and the store version is stamped.
        let entry: JournalEntry = yaml_to(&s.out.join("journal").join("g1.yaml")).unwrap();
        assert_eq!(entry.kind, "session");
        assert_eq!(entry.batch, vec!["g:reconcile-section:t.md".to_string()]);
        assert!(entry.opened_goals.is_empty());
        assert_eq!(Store::load(&s.out).status.version, STORE_VERSION);
        // Resolved goals ride on the entry; opened goals land after the board re-derives.
        let mut commit = wi().commit(2, 10);
        commit.resolved.push(Resolved {
            goal: "g:reconcile-section:t.md".into(),
            justification: "covered".into(),
            evidence: serde_json::Value::Null,
        });
        let report = s.apply(vec![], &commit);
        s.record_opened_goals(
            report.generation,
            vec![OpenedGoal {
                goal: "g:review-entity:ent:cart".into(),
                cause: Cause {
                    generation: 1,
                    mutation: 1,
                    via: "fields".into(),
                },
            }],
        );
        let entry: JournalEntry = yaml_to(&s.out.join("journal").join("g2.yaml")).unwrap();
        assert_eq!(entry.resolved_goals.len(), 1);
        assert_eq!(entry.opened_goals[0].goal, "g:review-entity:ent:cart");
        assert_eq!(entry.rounds, 2);
        // Clearing by id, persisted.
        let ids = s.status.change_ids(&[CHANGE_ENTITY], "ent:cart");
        s.clear_changes(&ids);
        assert!(!s.status.has_change(CHANGE_ENTITY, "ent:cart"));
        assert!(s.status.has_change(CHANGE_REQ_CREATED, "req:t-1"));
        assert_eq!(Store::load(&s.out).status.changes.len(), 1);
    }

    // The natural key: name plus scope, and parent when supplied. Two same-named
    // children under different parents stay apart; an upsert that names neither is
    // an ambiguity error naming the candidates.
    // Mirrors docs/compiler/concepts/identity.md#the-natural-key-under-containment.
    #[test]
    fn natural_key_with_and_without_parent() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nTwo modules each carry a Config.\n");
        s.apply(
            vec![create("ent:auth", "Auth"), create("ent:billing", "Billing")],
            &session(),
        );
        let under = |id: &str, parent: &str| Op::CreateEntity {
            id: id.into(),
            entity: Entity {
                parent: Some(parent.into()),
                ..entity("Config")
            },
        };
        let r = s.apply(
            vec![
                under("ent:config", "ent:auth"),
                under("ent:config-b", "ent:billing"),
            ],
            &session(),
        );
        assert_eq!(r.applied, 2, "{:?}", r.skipped);
        assert_eq!(s.graph.entities.len(), 4);
        assert_eq!(
            s.graph.entities["ent:config"].parent.as_deref(),
            Some("ent:auth")
        );
        assert_eq!(
            s.graph.entities["ent:config-b"].parent.as_deref(),
            Some("ent:billing")
        );
        // With the parent, the key lands on that child alone.
        assert_eq!(
            s.find_natural("config", "public", Some("ent:billing")),
            Ok(Some("ent:config-b".into()))
        );
        // A parent nobody has: no match, never a wrong merge.
        assert_eq!(
            s.find_natural("config", "public", Some("ent:auth-2")),
            Ok(None)
        );
        // Without the parent: two candidates is an error naming them.
        assert_eq!(
            s.find_natural("Config", "public", None),
            Err(vec!["ent:config".to_string(), "ent:config-b".to_string()])
        );
        let r = s.apply(vec![create("ent:config-c", "Config")], &session());
        assert_eq!(r.applied, 0);
        assert!(r.skipped[0].contains("ambiguous name"), "{:?}", r.skipped);
        assert!(r.skipped[0].contains("ent:config-b"));
        // A single match lands whatever its parent.
        assert_eq!(
            s.find_natural("auth", "public", None),
            Ok(Some("ent:auth".into()))
        );
        let r = s.apply(vec![create("ent:auth-x", "auth")], &session());
        assert_eq!(r.applied, 1);
        assert_eq!(s.graph.entities.len(), 4);
        assert_eq!(s.find_natural("auth", "other", None), Ok(None));
    }

    #[test]
    fn mint_and_create_and_natural_key_reconcile() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
        let e = Entity {
            name: "Cart".into(),
            mentions: vec![mention("t.md", "/t", "The Cart holds items.")],
            ..Default::default()
        };
        let r = s.apply(
            vec![Op::CreateEntity {
                id: "ent:cart".into(),
                entity: e.clone(),
            }],
            &wi().commit(1, 10),
        );
        assert_eq!(r.applied, 1);
        assert!(s.graph.entities.contains_key("ent:cart"));
        // A second create with the same natural key becomes an update, not a duplicate.
        let e2 = Entity {
            name: "cart".into(),
            mentions: vec![mention("t.md", "/t", "holds items")],
            ..Default::default()
        };
        s.apply(
            vec![Op::CreateEntity {
                id: "ent:cart-x".into(),
                entity: e2,
            }],
            &wi().commit(1, 10),
        );
        assert_eq!(s.graph.entities.len(), 1);
        assert_eq!(s.graph.entities["ent:cart"].mentions.len(), 2);
        assert_eq!(
            s.mint_view_id("use-case", "Checkout", &BTreeSet::new()),
            "view:usecase/checkout"
        );
    }

    #[test]
    fn same_sentence_subsumed_statement_refreshes_in_place() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(
            &mut s,
            "t.md",
            "# T\n- `createUser` - creates a new user account\n",
        );
        let base = Requirement {
            statement: "The user management system creates a new user account.".into(),
            entities: vec!["ent:um".into()],
            source: Some(mention(
                "t.md",
                "/t",
                "- `createUser` - creates a new user account",
            )),
            ..Default::default()
        };
        s.apply(
            vec![
                create("ent:um", "User Management"),
                Op::CreateRequirement {
                    id: "req:t-1".into(),
                    requirement: base.clone(),
                },
            ],
            &session(),
        );
        // A resumed build rewords the same sentence's statement; one fact, one node.
        let reworded = Requirement {
            statement: "The user management system creates a new user account using createUser."
                .into(),
            ..base
        };
        let r = s.apply(
            vec![Op::CreateRequirement {
                id: "req:t-2".into(),
                requirement: reworded,
            }],
            &session(),
        );
        assert_eq!(s.graph.requirements.len(), 1);
        assert!(s.graph.requirements["req:t-1"]
            .statement
            .contains("using createUser"));
        assert!(r.changed_requirements.contains("req:t-1"));
        assert!(s.status.has_change(CHANGE_REQ_REVISED, "req:t-1"));
        // Distinct atomic facts from one sentence stay separate.
        assert!(!statement_subsumes(
            "The gateway is a REST service.",
            "The gateway is built with Go."
        ));
    }

    #[test]
    fn requirement_remaps_provisional_ids_and_derives_edges() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(
            &mut s,
            "t.md",
            "# T\nWhen checkout completes, the system empties the Cart of Products.\n",
        );
        let ops = vec![
            create("prov:1", "Cart"),
            create("prov:2", "Product"),
            Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    statement: "When checkout completes, the system empties the Cart.".into(),
                    entities: vec!["prov:1".into(), "prov:2".into()],
                    edges: vec![ReqEdge {
                        a: "prov:1".into(),
                        b: "prov:2".into(),
                        rel_type: Some("composition".into()),
                        cardinality: Some("1..*".into()),
                    }],
                    source: Some(mention("t.md", "/t", "the system empties the Cart")),
                    ..Default::default()
                },
            },
        ];
        let r = s.apply(ops, &wi().commit(3, 100));
        assert_eq!(r.applied, 3);
        let req = &s.graph.requirements["req:t-1"];
        assert_eq!(
            req.entities,
            vec!["ent:cart".to_string(), "ent:product".to_string()]
        );
        assert_eq!(s.graph.relationships.len(), 1);
        let rel = s.graph.relationships.values().next().unwrap();
        assert_eq!(rel.strongest(), "composition");
        assert_eq!(rel.contributions[0].cardinality.as_deref(), Some("1..*"));
        // A two-entity requirement with edges owes no declare-edges; one without does.
        assert!(!s.status.has_change(CHANGE_EDGES_MISSING, "req:t-1"));
        s.apply(
            vec![Op::UpdateRequirement {
                id: "req:t-1".into(),
                statement: None,
                entities: None,
                edges: Some(vec![]),
                transition: None,
                facets: None,
                source: None,
                provenance: None,
            }],
            &session(),
        );
        assert!(s.status.has_change(CHANGE_EDGES_MISSING, "req:t-1"));
        assert!(s.graph.relationships.is_empty());
    }

    // A merge rewires every reference: parent, transition subject, view members,
    // provenance from, and refuses to make the survivor its own ancestor.
    #[test]
    fn merge_rewires_every_reference_and_redirects() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nbuyer and customer\n");
        s.apply(
            vec![
                create("ent:buyer", "buyer"),
                create("ent:customer", "Customer"),
                Op::CreateEntity {
                    id: "ent:wishlist".into(),
                    entity: Entity {
                        parent: Some("ent:buyer".into()),
                        ..entity("Wishlist")
                    },
                },
                Op::CreateRequirement {
                    id: "req:t-1".into(),
                    requirement: Requirement {
                        statement: "The buyer pays.".into(),
                        entities: vec!["ent:buyer".into()],
                        transition: Some(Transition {
                            subject: "ent:buyer".into(),
                            from: "browsing".into(),
                            to: "paid".into(),
                            trigger: None,
                            guard: None,
                        }),
                        source: Some(mention("t.md", "/t", "buyer and customer")),
                        ..Default::default()
                    },
                },
                Op::CreateRequirement {
                    id: "req:x-1".into(),
                    requirement: Requirement {
                        statement: "The buyer is split into tiers.".into(),
                        entities: vec!["ent:buyer".into()],
                        provenance: Some(derived(&["ent:buyer"])),
                        ..Default::default()
                    },
                },
                Op::CreateView {
                    id: "view:class/people".into(),
                    view: View {
                        kind: "class".into(),
                        title: "People".into(),
                        members: vec!["ent:buyer".into(), "ent:customer".into()],
                        collapse: vec!["ent:buyer".into()],
                        provenance: Some(derived(&["ent:buyer", "ent:customer"])),
                        ..Default::default()
                    },
                },
            ],
            &session(),
        );
        assert!(s.graph.state_machines.contains_key("sm:buyer"));
        let r = s.apply(
            vec![Op::MergeEntities {
                keep: "ent:customer".into(),
                absorb: "ent:buyer".into(),
                reason: "same concept".into(),
            }],
            &session(),
        );
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        assert!(!s.graph.entities.contains_key("ent:buyer"));
        assert_eq!(s.graph.redirects["ent:buyer"], "ent:customer");
        assert_eq!(
            s.graph.requirements["req:t-1"].entities,
            vec!["ent:customer".to_string()]
        );
        assert_eq!(
            s.graph.requirements["req:t-1"]
                .transition
                .as_ref()
                .unwrap()
                .subject,
            "ent:customer"
        );
        assert_eq!(
            s.graph.entities["ent:wishlist"].parent.as_deref(),
            Some("ent:customer")
        );
        assert!(matches!(
            s.graph.requirements["req:x-1"].provenance.as_ref(),
            Some(Provenance::Derived { from, .. }) if from == &vec!["ent:customer".to_string()]
        ));
        let v = &s.graph.views["view:class/people"];
        assert_eq!(v.members, vec!["ent:customer"]);
        assert_eq!(v.collapse, vec!["ent:customer"]);
        assert!(matches!(
            v.provenance.as_ref(),
            Some(Provenance::Derived { from, .. }) if from == &vec!["ent:customer".to_string()]
        ));
        assert!(s.graph.entities["ent:customer"]
            .aliases
            .contains(&"buyer".to_string()));
        assert_eq!(s.resolve_id("ent:buyer"), "ent:customer");
        assert!(s.graph.state_machines.contains_key("sm:customer"));
        assert!(!s.graph.state_machines.contains_key("sm:buyer"));
        assert_eq!(
            s.status.change_ids(&[CHANGE_ENTITY], "ent:customer").len(),
            1
        );
        // Merging a child into its ancestor is refused: it would make the survivor its
        // own ancestor.
        let r = s.apply(
            vec![Op::MergeEntities {
                keep: "ent:wishlist".into(),
                absorb: "ent:customer".into(),
                reason: "bad idea".into(),
            }],
            &session(),
        );
        assert_eq!(r.applied, 0);
        assert!(r.skipped[0].contains("own ancestor"), "{:?}", r.skipped);
    }

    // A move alone writes `parent` on the parent left and the one joined and no
    // `fields` record on the child; a move with a definition still reviews the child.
    // Mirrors docs/compiler/goals/review-entity.md#created-when.
    #[test]
    fn a_move_alone_records_parent_on_both_parents_and_nothing_on_the_child() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nshop\n");
        s.apply(
            vec![
                create("ent:orders", "Orders"),
                create("ent:shipping", "Shipping"),
                Op::CreateEntity {
                    id: "ent:parcel".into(),
                    entity: Entity {
                        parent: Some("ent:orders".into()),
                        ..entity("Parcel")
                    },
                },
            ],
            &session(),
        );
        let update = |id: &str, parent: &str, definition: Option<&str>| Op::UpdateEntity {
            id: id.into(),
            name: None,
            definition: definition.map(String::from),
            add_aliases: Vec::new(),
            add_mention: None,
            stereotype: None,
            parent: Some(parent.into()),
            set_attributes: None,
            add_attributes: Vec::new(),
            provenance: None,
        };
        let r = s.apply(vec![update("ent:parcel", "ent:shipping", None)], &session());
        let entity_records: Vec<(String, String)> = r
            .changes
            .iter()
            .filter(|c| c.kind == CHANGE_ENTITY)
            .map(|c| (c.subject.clone(), c.via.clone()))
            .collect();
        assert!(
            entity_records.contains(&("ent:orders".into(), "parent".into())),
            "{:?}",
            entity_records
        );
        assert!(
            entity_records.contains(&("ent:shipping".into(), "parent".into())),
            "{:?}",
            entity_records
        );
        assert!(
            !entity_records.iter().any(|(s, _)| s == "ent:parcel"),
            "a move alone never reviews the child: {:?}",
            entity_records
        );
        // Moved back with a new definition: the child's own facts changed, so it
        // gets its `fields` record beside the parents' `parent` records.
        let r = s.apply(
            vec![update("ent:parcel", "ent:orders", Some("The package."))],
            &session(),
        );
        assert!(r
            .changes
            .iter()
            .any(|c| c.kind == CHANGE_ENTITY && c.subject == "ent:parcel" && c.via == "fields"));
    }

    // The commit-time gates: a parent cycle, an unknown view member, and deleting a
    // parent are refused with the rule spelled out. Mirrors docs/compiler/graph.md#validation-gates.
    #[test]
    fn commit_gates_refuse_cycles_unknown_members_and_deleting_a_parent() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nshop\n");
        s.apply(
            vec![
                create("ent:shop", "Shop"),
                Op::CreateEntity {
                    id: "ent:orders".into(),
                    entity: Entity {
                        parent: Some("ent:shop".into()),
                        ..entity("Orders")
                    },
                },
            ],
            &session(),
        );
        let update_parent =|id: &str, parent: &str| Op::UpdateEntity {
            id: id.into(),
            name: None,
            definition: None,
            add_aliases: Vec::new(),
            add_mention: None,
            stereotype: None,
            parent: Some(parent.into()),
            set_attributes: None,
            add_attributes: Vec::new(),
            provenance: None,
        };
        let r = s.apply(
            vec![
                update_parent("ent:shop", "ent:orders"),
                update_parent("ent:shop", "ent:shop"),
                update_parent("ent:shop", "ent:nowhere"),
                Op::CreateView {
                    id: "view:class/x".into(),
                    view: View {
                        kind: "class".into(),
                        title: "X".into(),
                        members: vec!["ent:shop".into(), "ent:ghost".into()],
                        provenance: Some(derived(&["ent:shop"])),
                        ..Default::default()
                    },
                },
                Op::CreateView {
                    id: "view:mindmap/x".into(),
                    view: View {
                        kind: "mindmap".into(),
                        title: "X".into(),
                        provenance: Some(derived(&["ent:shop"])),
                        ..Default::default()
                    },
                },
                Op::DeleteEntity {
                    id: "ent:shop".into(),
                    reason: "trying".into(),
                },
            ],
            &session(),
        );
        assert_eq!(r.applied, 0, "{:?}", r.skipped);
        assert_eq!(r.skipped.len(), 6);
        assert!(
            r.skipped[0].contains("parent cycle through ent:orders > ent:shop"),
            "{:?}",
            r.skipped
        );
        assert!(r.skipped[1].contains("parent cycle"));
        assert!(r.skipped[2].contains("unknown parent ent:nowhere"));
        assert!(r.skipped[3].contains("unknown member ent:ghost"));
        assert!(r.skipped[4].contains("unknown kind"));
        assert!(r.skipped[5].contains("still a parent of ent:orders"));
        assert!(s.graph.entities["ent:shop"].parent.is_none());
        // Deleting the child first frees the parent, and the delete tombstones.
        let r = s.apply(
            vec![
                Op::DeleteEntity {
                    id: "ent:orders".into(),
                    reason: "gone".into(),
                },
                Op::DeleteEntity {
                    id: "ent:shop".into(),
                    reason: "gone".into(),
                },
            ],
            &session(),
        );
        assert_eq!(r.applied, 2, "{:?}", r.skipped);
        assert_eq!(s.graph.redirects["ent:shop"], "");
        assert!(s.status.has_change(CHANGE_ENTITY_DELETED, "ent:shop"));
        // A prose edit staged alone is refused.
        let r = s.apply(
            vec![Op::EditDocProse {
                doc: "t.md".into(),
                section: "/t".into(),
                old_text: "shop".into(),
                new_text: "store".into(),
                text: "# T\nstore\n".into(),
            }],
            &Commit::store("dual-write"),
        );
        assert_eq!(r.applied, 0);
        assert!(r.skipped[0].contains("edit-needs-mutation"));
    }

    // Views: upsert by kind and title, curation clears default, deletes of a member
    // are allowed and noted, a default view refuses deletion, a bump raises the limit.
    // `via` on a view-member-gone record names the list the dead node sat in, and a
    // change on a type reaches its instances as `attributes`.
    // Mirrors docs/compiler/reconciler.md#change-records.
    #[test]
    fn via_names_the_view_list_and_a_types_change_reaches_instances_as_attributes() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nAna is a customer. The extra thing.\n");
        let stated = |name: &str, quote: &str| Entity {
            name: name.into(),
            mentions: vec![mention("t.md", "/t", quote)],
            ..Default::default()
        };
        let r = s.apply(
            vec![
                Op::CreateEntity {
                    id: "ent:customer".into(),
                    entity: stated("Customer", "customer"),
                },
                Op::CreateEntity {
                    id: "ent:ana".into(),
                    entity: stated("Ana", "Ana"),
                },
                Op::CreateEntity {
                    id: "ent:extra".into(),
                    entity: stated("Extra", "extra"),
                },
                Op::CreateRequirement {
                    id: "req:t-1".into(),
                    requirement: Requirement {
                        statement: "Ana is a Customer.".into(),
                        entities: vec!["ent:ana".into(), "ent:customer".into()],
                        edges: vec![ReqEdge {
                            a: "ent:ana".into(),
                            b: "ent:customer".into(),
                            rel_type: Some("instantiation".into()),
                            cardinality: None,
                        }],
                        source: Some(mention("t.md", "/t", "Ana is a customer")),
                        ..Default::default()
                    },
                },
                Op::CreateView {
                    id: "view:class/zoo".into(),
                    view: View {
                        kind: "class".into(),
                        title: "Zoo".into(),
                        members: vec!["ent:customer".into()],
                        collapse: vec!["ent:extra".into()],
                        provenance: Some(derived(&["ent:customer"])),
                        ..Default::default()
                    },
                },
            ],
            &session(),
        );
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        let r = s.apply(
            vec![Op::DeleteEntity {
                id: "ent:extra".into(),
                reason: "noise".into(),
            }],
            &session(),
        );
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        let gone = r
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_VIEW_MEMBER_GONE && c.subject == "view:class/zoo")
            .expect("view-member-gone");
        assert_eq!(gone.via, "collapse");
        assert_eq!(gone.detail["gone"], serde_json::json!(["ent:extra"]));
        let r = s.apply(
            vec![Op::UpdateEntity {
                id: "ent:customer".into(),
                name: None,
                definition: Some("a person who buys".into()),
                add_aliases: Vec::new(),
                add_mention: None,
                stereotype: None,
                parent: None,
                set_attributes: None,
                add_attributes: Vec::new(),
                provenance: None,
            }],
            &session(),
        );
        let inst = r
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_INSTANCE && c.subject == "ent:ana")
            .expect("instance-changed on the instance");
        assert_eq!(inst.via, "attributes");
        assert_eq!(inst.detail["type"], "ent:customer");
    }

    #[test]
    fn views_curate_delete_and_bump() {
        let mut s = Store {
            out: own_dir("views"),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\ncart and order\n");
        let req = |id: &str, statement: &str| Op::CreateRequirement {
            id: id.into(),
            requirement: Requirement {
                statement: statement.into(),
                entities: vec!["ent:cart".into()],
                facets: vec![Facet {
                    facet: "behavior".into(),
                    reasoning: "a step".into(),
                    measure: None,
                }],
                source: Some(mention("t.md", "/t", "cart and order")),
                ..Default::default()
            },
        };
        s.apply(
            vec![
                create("ent:cart", "Cart"),
                create("ent:order", "Order"),
                req("req:t-1", "The cart opens."),
                req("req:t-2", "The cart closes."),
            ],
            &session(),
        );
        // Defaults derived: a class view per scope and the cart's flow.
        assert!(s.graph.views["view:class/public"].default);
        assert_eq!(
            s.graph.views["view:usecase/cart-t"].members,
            vec!["req:t-1", "req:t-2"]
        );
        let r = s.apply(
            vec![Op::DeleteView {
                id: "view:class/public".into(),
                reason: "noise".into(),
            }],
            &session(),
        );
        assert_eq!(r.applied, 0);
        assert!(r.skipped[0].contains("default view"), "{:?}", r.skipped);
        // The scope root's level view lists its members and carries no query.
        assert_eq!(
            s.graph.views["view:class/public"].members,
            vec!["ent:cart", "ent:order"]
        );
        assert!(s.graph.views["view:class/public"].query.is_none());
        // An upsert on the default's kind and title lands on it and curates it.
        let r = s.apply(
            vec![Op::CreateView {
                id: "view:class/whatever".into(),
                view: View {
                    kind: "class".into(),
                    title: "public".into(),
                    members: vec!["ent:cart".into()],
                    query: Some(ViewQuery {
                        scope: Some("public".into()),
                        ..Default::default()
                    }),
                    provenance: Some(derived(&["ent:cart"])),
                    ..Default::default()
                },
            }],
            &session(),
        );
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        let v = &s.graph.views["view:class/public"];
        assert!(!v.default);
        // The query the session gave the curated view keeps recomputing membership:
        // the match the session left out joins as a query-match change.
        assert_eq!(v.members, vec!["ent:cart", "ent:order"]);
        assert!(s.status.has_change(CHANGE_QUERY_MATCH, "view:class/public"));
        assert!(!s.graph.views.contains_key("view:class/whatever"));
        // A curated view is never removed by the recompute, and its members survive.
        s.apply(
            vec![Op::UpdateView {
                id: "view:usecase/cart-t".into(),
                title: Some("Cart flow".into()),
                members: None,
                add_members: Vec::new(),
                remove_members: Vec::new(),
                query: None,
                collapse: None,
                exclude: vec![Exclusion {
                    id: "req:t-2".into(),
                    note: "example, not flow".into(),
                }],
                reasoning: Some("trimmed".into()),
            }],
            &session(),
        );
        let v = &s.graph.views["view:usecase/cart-t"];
        assert!(!v.default);
        assert_eq!(v.title, "Cart flow");
        assert_eq!(v.members, vec!["req:t-1"]);
        assert_eq!(v.excluded.len(), 1);
        // Deleting a member of a curated view is allowed; the view owes a retrace.
        let r = s.apply(
            vec![Op::DeleteRequirement {
                id: "req:t-1".into(),
                reason: "gone".into(),
            }],
            &session(),
        );
        assert_eq!(r.applied, 1);
        assert_eq!(
            s.graph.views["view:usecase/cart-t"].members,
            vec!["req:t-1"]
        );
        let gone = s
            .status
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_VIEW_MEMBER_GONE)
            .expect("view-member-gone");
        assert_eq!(gone.subject, "view:usecase/cart-t");
        assert_eq!(gone.via, "members");
        assert_eq!(gone.detail["gone"], serde_json::json!(["req:t-1"]));
        // A bump on a view records the limit and curates it; a limit of the wrong kind
        // is refused.
        let r = s.apply(
            vec![
                Op::BumpLimit {
                    id: "view:class/public".into(),
                    limit: "members-per-structural-view".into(),
                    value: 25,
                    provenance: Provenance::Decree {
                        author: "owner".into(),
                        at: "now".into(),
                        note: None,
                    },
                },
                Op::BumpLimit {
                    id: "ent:cart".into(),
                    limit: "edges-per-view".into(),
                    value: 25,
                    provenance: Provenance::Decree {
                        author: "owner".into(),
                        at: "now".into(),
                        note: None,
                    },
                },
            ],
            &Commit::store("decree"),
        );
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        assert_eq!(
            s.graph.views["view:class/public"].limits["members-per-structural-view"].value,
            25
        );
        assert!(r.skipped[0].contains("does not apply to an entity"));
        let entry: JournalEntry = yaml_to(
            &s.out
                .join("journal")
                .join(format!("g{}.yaml", r.generation)),
        )
        .unwrap();
        assert_eq!(entry.kind, "decree");
        assert_eq!(entry.mutations[0]["op"], "bump_limit");
        // Now deletable.
        let r = s.apply(
            vec![Op::DeleteView {
                id: "view:class/public".into(),
                reason: "noise".into(),
            }],
            &session(),
        );
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        // And the next commit derives it again as a default.
        s.apply(vec![], &session());
        assert!(s.graph.views["view:class/public"].default);
    }

    // The 51st requirement crosses requirements-per-entity; a bump on the entity
    // raises the threshold and the record clears. Mirrors docs/compiler/graph.md#limits.
    #[test]
    fn threshold_crossed_on_the_fifty_first_requirement_and_cleared_by_a_bump() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nthe order\n");
        let mut ops = vec![create("ent:order", "Order")];
        for i in 0..50 {
            ops.push(Op::CreateRequirement {
                id: format!("req:t-{}", i + 1),
                requirement: Requirement {
                    statement: format!("The order does thing number {}.", i),
                    entities: vec!["ent:order".into()],
                    source: Some(mention("t.md", "/t", "the order")),
                    ..Default::default()
                },
            });
        }
        s.apply(ops, &session());
        assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:order"));
        let r = s.apply(
            vec![Op::CreateRequirement {
                id: "req:t-51".into(),
                requirement: Requirement {
                    statement: "The order does one thing more.".into(),
                    entities: vec!["ent:order".into()],
                    source: Some(mention("t.md", "/t", "the order")),
                    ..Default::default()
                },
            }],
            &session(),
        );
        let crossed = r
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_THRESHOLD_CROSSED)
            .expect("threshold-crossed");
        assert_eq!(crossed.subject, "ent:order");
        assert_eq!(crossed.via, "limits");
        assert_eq!(crossed.detail["limit"], "requirements-per-entity");
        assert_eq!(crossed.detail["count"], 51);
        assert_eq!(crossed.detail["level"], "soft");
        assert_eq!(crossed.detail["goal"], "abstract-entity");
        let id = crossed.id.clone();
        // Another commit keeps the record (no duplicate, same id), refreshing the count.
        let r = s.apply(
            vec![Op::CreateRequirement {
                id: "req:t-52".into(),
                requirement: Requirement {
                    statement: "The order ships by courier.".into(),
                    entities: vec!["ent:order".into()],
                    source: Some(mention("t.md", "/t", "the order")),
                    ..Default::default()
                },
            }],
            &session(),
        );
        assert!(r.changes.iter().all(|c| c.kind != CHANGE_THRESHOLD_CROSSED));
        let standing: Vec<&ChangeRecord> = s
            .status
            .changes
            .iter()
            .filter(|c| c.kind == CHANGE_THRESHOLD_CROSSED)
            .collect();
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].id, id);
        assert_eq!(standing[0].detail["count"], 52);
        // The bump raises the threshold: the crossing lapses.
        s.apply(
            vec![Op::BumpLimit {
                id: "ent:order".into(),
                limit: "requirements-per-entity".into(),
                value: 70,
                provenance: Provenance::Decree {
                    author: "owner".into(),
                    at: "now".into(),
                    note: Some("the order is the hub".into()),
                },
            }],
            &Commit::store("decree"),
        );
        assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:order"));
        assert_eq!(
            s.graph.entities["ent:order"].limits["requirements-per-entity"].value,
            70
        );
    }

    // A stated entity with a mention in the seeded section, under an optional parent.
    fn stated(name: &str, quote: &str, parent: Option<&str>) -> Entity {
        Entity {
            name: name.into(),
            mentions: vec![mention("t.md", "/t", quote)],
            parent: parent.map(String::from),
            ..Default::default()
        }
    }

    // A grouping: derived from its members, a definition, no mentions.
    fn grouping(name: &str, members: &[&str], parent: Option<&str>) -> Entity {
        Entity {
            name: name.into(),
            definition: Some(format!("{} holds its members.", name)),
            provenance: Some(derived(members)),
            parent: parent.map(String::from),
            ..Default::default()
        }
    }

    fn move_to(id: &str, parent: &str) -> Op {
        Op::UpdateEntity {
            id: id.into(),
            name: None,
            definition: None,
            add_aliases: Vec::new(),
            add_mention: None,
            stereotype: None,
            parent: Some(parent.into()),
            set_attributes: None,
            add_attributes: Vec::new(),
            provenance: None,
        }
    }

    // children-per-entity: the record lands on the node when its direct children cross
    // soft, and on `scope:<scope>` when the parentless entities do; a count back under
    // soft clears it without a session. Mirrors docs/compiler/reconciler.md#fan-out.
    #[test]
    fn threshold_crossed_on_children_per_entity_and_cleared_on_dropping_back() {
        let mut s = Store {
            out: own_dir("fanout"),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nthe hub\n");
        let (soft, hard) = crate::limits::threshold("children-per-entity", None).unwrap();
        let part = |i: u64| Op::CreateEntity {
            id: format!("ent:part-{}", i),
            entity: stated(&format!("Part {}", i), "the hub", Some("ent:hub")),
        };
        let mut ops = vec![Op::CreateEntity {
            id: "ent:hub".into(),
            entity: stated("Hub", "the hub", None),
        }];
        ops.extend((0..soft).map(part));
        s.apply(ops, &session());
        assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:hub"));
        let r = s.apply(vec![part(soft)], &session());
        let crossed = r
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_THRESHOLD_CROSSED && c.subject == "ent:hub")
            .expect("threshold-crossed on the node");
        assert_eq!(crossed.via, "limits");
        assert_eq!(crossed.detail["limit"], "children-per-entity");
        assert_eq!(crossed.detail["count"], soft + 1);
        assert_eq!(crossed.detail["soft"], soft);
        assert_eq!(crossed.detail["hard"], hard);
        assert_eq!(crossed.detail["level"], "soft");
        assert_eq!(crossed.detail["goal"], "abstract-entity");
        // Dropping back under soft clears the record without a session.
        s.apply(
            vec![Op::DeleteEntity {
                id: format!("ent:part-{}", soft),
                reason: "test".into(),
            }],
            &session(),
        );
        assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:hub"));
        // The scope root: the hub plus soft more parentless entities cross on scope:public.
        let top = |i: u64| Op::CreateEntity {
            id: format!("ent:top-{}", i),
            entity: stated(&format!("Top {}", i), "the hub", None),
        };
        let r = s.apply((0..soft).map(top).collect(), &session());
        let crossed = r
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_THRESHOLD_CROSSED && c.subject == "scope:public")
            .expect("threshold-crossed on the scope root");
        assert_eq!(crossed.detail["limit"], "children-per-entity");
        assert_eq!(crossed.detail["count"], soft + 1);
        assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:hub"));
        // An unrelated commit keeps the root's record standing.
        s.apply(
            vec![Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    statement: "The hub holds.".into(),
                    entities: vec!["ent:hub".into()],
                    source: Some(mention("t.md", "/t", "the hub")),
                    ..Default::default()
                },
            }],
            &session(),
        );
        assert!(s
            .status
            .has_change(CHANGE_THRESHOLD_CROSSED, "scope:public"));
        // One parentless entity moved under a sibling drops the root back under soft.
        s.apply(vec![move_to("ent:top-0", "ent:top-1")], &session());
        assert!(!s
            .status
            .has_change(CHANGE_THRESHOLD_CROSSED, "scope:public"));
        assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:hub"));
    }

    // The sweep's dissolve rule: a derived grouping with fewer than two children
    // dissolves, its children reparent to its parent, a tombstone redirect to the
    // parent stays, and the gc entry carries the dissolve mutation. A grouping with
    // two children and a stated entity with one child are untouched.
    // Mirrors docs/compiler/graph.md#the-sweep.
    #[test]
    fn sweep_dissolves_an_under_membered_grouping_with_a_redirect() {
        let mut s = Store {
            out: own_dir("dissolve"),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nthe backend\n");
        let ents = [
            ("ent:infra", grouping("Infra", &["ent:backend"], None)),
            (
                "ent:backend",
                stated("Backend", "the backend", Some("ent:infra")),
            ),
            (
                "ent:storage",
                grouping("Storage", &["ent:cache"], Some("ent:backend")),
            ),
            (
                "ent:cache",
                stated("Cache", "the backend", Some("ent:storage")),
            ),
            (
                "ent:messaging",
                grouping("Messaging", &["ent:queue", "ent:db"], Some("ent:backend")),
            ),
            (
                "ent:queue",
                stated("Queue", "the backend", Some("ent:messaging")),
            ),
            ("ent:db", stated("Db", "the backend", Some("ent:messaging"))),
            (
                "ent:shell",
                stated("Shell", "the backend", Some("ent:backend")),
            ),
            ("ent:core", stated("Core", "the backend", Some("ent:shell"))),
        ];
        for (id, e) in ents {
            s.graph.entities.insert(id.into(), e);
        }
        let actions = s.gc();
        assert!(
            actions
                .iter()
                .any(|a| a.starts_with("dissolved ent:storage")),
            "{:?}",
            actions
        );
        assert!(
            actions.iter().any(|a| a.starts_with("dissolved ent:infra")),
            "{:?}",
            actions
        );
        assert!(!s.graph.entities.contains_key("ent:storage"));
        assert!(!s.graph.entities.contains_key("ent:infra"));
        assert_eq!(
            s.graph.entities["ent:cache"].parent.as_deref(),
            Some("ent:backend")
        );
        assert_eq!(s.graph.entities["ent:backend"].parent, None);
        // The tombstone redirects to the parent, so the old id resolves there; a
        // top-level grouping leaves a dead tombstone.
        assert_eq!(s.graph.redirects["ent:storage"], "ent:backend");
        assert_eq!(s.resolve_id("ent:storage"), "ent:backend");
        assert_eq!(s.graph.redirects["ent:infra"], "");
        assert!(s.graph.entities.contains_key("ent:messaging"));
        assert_eq!(
            s.graph.entities["ent:queue"].parent.as_deref(),
            Some("ent:messaging")
        );
        assert!(s.graph.entities.contains_key("ent:shell"));
        assert_eq!(
            s.graph.entities["ent:core"].parent.as_deref(),
            Some("ent:shell")
        );
        assert!(s.status.has_change(CHANGE_ENTITY_DELETED, "ent:storage"));
        // The move is the parents' business: the backend gained the cache; the
        // cache itself is not reviewed for a move alone.
        assert!(s.status.has_change(CHANGE_ENTITY, "ent:backend"));
        assert!(!s.status.has_change(CHANGE_ENTITY, "ent:cache"));
        assert!(!s.status.has_change(CHANGE_REPARENT_FLIP, "ent:cache"));
        let entry = journal_entry(&s, s.status.generation);
        assert_eq!(entry.kind, "gc");
        let dissolve = entry
            .mutations
            .iter()
            .find(|m| m["op"] == "dissolve_entity" && m["id"] == "ent:storage")
            .expect("dissolve mutation");
        assert_eq!(dissolve["parent"], "ent:backend");
        assert_eq!(dissolve["children"], serde_json::json!(["ent:cache"]));
        assert_eq!(
            s.journaled_parent_moves(),
            vec![
                ParentMove {
                    generation: entry.generation,
                    child: "ent:backend".into(),
                    from: Some("ent:infra".into()),
                    to: None,
                },
                ParentMove {
                    generation: entry.generation,
                    child: "ent:cache".into(),
                    from: Some("ent:storage".into()),
                    to: Some("ent:backend".into()),
                },
            ]
        );
        // A second sweep is a no-op.
        assert!(s.gc().is_empty());
        // The tool path: a stated entity is refused; a grouping dissolves the same way.
        let r = s.apply(
            vec![Op::DissolveEntity {
                id: "ent:shell".into(),
                reason: "test".into(),
                parent: None,
                children: Vec::new(),
            }],
            &session(),
        );
        assert!(
            r.skipped.iter().any(|x| x.contains("stated-entity")),
            "{:?}",
            r.skipped
        );
        assert!(s.graph.entities.contains_key("ent:shell"));
        let r = s.apply(
            vec![Op::DissolveEntity {
                id: "ent:messaging".into(),
                reason: "the two belong beside the cache".into(),
                parent: None,
                children: Vec::new(),
            }],
            &session(),
        );
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        assert_eq!(s.resolve_id("ent:messaging"), "ent:backend");
        assert_eq!(
            s.graph.entities["ent:queue"].parent.as_deref(),
            Some("ent:backend")
        );
        assert!(s.status.has_change(CHANGE_ENTITY_DELETED, "ent:messaging"));
        let entry = journal_entry(&s, r.generation);
        assert_eq!(entry.mutations[0]["op"], "dissolve_entity");
        assert_eq!(
            entry.mutations[0]["children"],
            serde_json::json!(["ent:db", "ent:queue"])
        );
    }

    // The reparent flip: the first move of a child records nothing; the move back
    // between the same two parents writes `reparent-flip` on the child with the pair
    // in `between`; a move to a third parent is a first move again; and a grouping
    // dissolved and re-minted under a new id counts as the same parent.
    // Mirrors docs/compiler/reconciler.md#flip-detection.
    #[test]
    fn reparent_flip_recorded_on_the_second_alternation_not_the_first() {
        let mut s = Store {
            out: own_dir("flip"),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nthe backend\n");
        s.apply(
            vec![
                Op::CreateEntity {
                    id: "ent:backend".into(),
                    entity: stated("Backend", "the backend", None),
                },
                Op::CreateEntity {
                    id: "ent:storage".into(),
                    entity: stated("Storage", "the backend", None),
                },
                Op::CreateEntity {
                    id: "ent:cache".into(),
                    entity: stated("Cache", "the backend", Some("ent:backend")),
                },
            ],
            &session(),
        );
        let r = s.apply(vec![move_to("ent:cache", "ent:storage")], &session());
        assert!(r.changes.iter().all(|c| c.kind != CHANGE_REPARENT_FLIP));
        let entry = journal_entry(&s, r.generation);
        assert_eq!(entry.mutations[0]["parent"], "ent:storage");
        assert_eq!(entry.mutations[0]["prior"]["parent"], "ent:backend");
        let r = s.apply(vec![move_to("ent:cache", "ent:backend")], &session());
        let flip = r
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_REPARENT_FLIP)
            .expect("reparent-flip");
        assert_eq!(flip.subject, "ent:cache");
        assert_eq!(flip.via, "parent");
        assert_eq!(flip.mutation, 1);
        assert_eq!(
            flip.detail["between"],
            serde_json::json!(["ent:storage", "ent:backend"])
        );
        assert!(s.status.has_change(CHANGE_REPARENT_FLIP, "ent:cache"));
        // A third parent is a first move again.
        let r = s.apply(
            vec![
                Op::CreateEntity {
                    id: "ent:edge".into(),
                    entity: stated("Edge", "the backend", None),
                },
                move_to("ent:cache", "ent:edge"),
            ],
            &session(),
        );
        assert!(r.changes.iter().all(|c| c.kind != CHANGE_REPARENT_FLIP));
        // A grouping over the child, dissolved, then re-minted under a collision
        // suffix: the move back under it matches the dead grouping by natural key.
        let tier = |id: &str| Op::CreateEntity {
            id: id.into(),
            entity: grouping("Tier", &["ent:cache", "ent:other"], Some("ent:edge")),
        };
        s.apply(
            vec![
                Op::CreateEntity {
                    id: "ent:other".into(),
                    entity: stated("Other", "the backend", Some("ent:edge")),
                },
                tier("ent:tier"),
                move_to("ent:cache", "ent:tier"),
                move_to("ent:other", "ent:tier"),
            ],
            &session(),
        );
        assert_eq!(
            s.graph.entities["ent:cache"].parent.as_deref(),
            Some("ent:tier")
        );
        let r = s.apply(
            vec![Op::DissolveEntity {
                id: "ent:tier".into(),
                reason: "test".into(),
                parent: None,
                children: Vec::new(),
            }],
            &session(),
        );
        // Grouping and dissolving in consecutive generations is itself the alternation.
        assert!(r
            .changes
            .iter()
            .any(|c| c.kind == CHANGE_REPARENT_FLIP && c.subject == "ent:cache"));
        // Both members move back under the re-minted grouping: the sweep behind the
        // commit dissolves a grouping left with one child.
        let r = s.apply(
            vec![
                tier("ent:tier"),
                move_to("ent:cache", "ent:tier"),
                move_to("ent:other", "ent:tier"),
            ],
            &session(),
        );
        let (new_id, _) = s
            .graph
            .entities
            .iter()
            .find(|(_, e)| e.name == "Tier")
            .expect("re-minted grouping");
        assert_ne!(new_id, "ent:tier");
        assert_eq!(
            s.graph.entities["ent:cache"].parent.as_deref(),
            Some(new_id.as_str())
        );
        let flip = r
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_REPARENT_FLIP && c.subject == "ent:cache")
            .expect("reparent-flip across the re-mint");
        assert_eq!(
            flip.detail["between"],
            serde_json::json!(["ent:edge", new_id])
        );
    }

    // The store version: a build archives an out directory of another version to
    // `<out>.bak` and starts empty; a reader treats it as empty without touching it.
    // Mirrors docs/compiler/graph.md#store-version.
    #[test]
    fn store_version_archives_on_build_and_reads_as_empty() {
        let dir = own_dir("version");
        let mut s = Store {
            out: dir.clone(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\ncart\n");
        s.apply(vec![create("ent:cart", "Cart")], &session());
        assert_eq!(Store::open_for_build(&dir).graph.entities.len(), 1);
        // Rewrite status.yaml without a version, as an older store would have it.
        std::fs::write(dir.join("status.yaml"), "generation: 7\n").unwrap();
        let read = Store::load(&dir);
        assert!(read.graph.entities.is_empty());
        assert_eq!(read.status.generation, 0);
        assert!(dir.join("graph").join("entities.yaml").exists());
        let bak = dir.with_file_name(format!(
            "{}.bak",
            dir.file_name().unwrap().to_string_lossy()
        ));
        std::fs::remove_dir_all(&bak).ok();
        let fresh = Store::open_for_build(&dir);
        assert!(fresh.graph.entities.is_empty());
        assert!(!dir.join("status.yaml").exists());
        assert!(bak.join("graph").join("entities.yaml").exists());
        // A second archive replaces the first.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("status.yaml"), "version: 1\ngeneration: 2\n").unwrap();
        std::fs::write(dir.join("marker"), "second").unwrap();
        let fresh = Store::open_for_build(&dir);
        assert!(fresh.graph.entities.is_empty());
        assert!(bak.join("marker").exists());
        assert!(!bak.join("graph").exists());
        // A fresh directory is simply empty: nothing to archive.
        std::fs::remove_dir_all(&dir).ok();
        assert!(Store::open_for_build(&dir).graph.entities.is_empty());
        assert!(!dir.join("status.yaml").exists());
    }

    // A derived requirement mints under stem x, lands a provenance-pending record, and
    // ratifies to a quote in place; a decree retracts. Mirrors docs/compiler/compilation.md#edit-paths.
    #[test]
    fn derived_and_decreed_requirements_pend_ratify_and_retract() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nThe cart is split by category.\n");
        let r = s.apply(
            vec![
                create("ent:cart", "Cart"),
                Op::CreateRequirement {
                    id: "prov:new".into(),
                    requirement: Requirement {
                        statement: "The cart is split by category.".into(),
                        entities: vec!["ent:cart".into()],
                        provenance: Some(derived(&["ent:cart"])),
                        ..Default::default()
                    },
                },
                Op::CreateRequirement {
                    id: "prov:decree".into(),
                    requirement: Requirement {
                        statement: "The cart never exceeds ten categories.".into(),
                        entities: vec!["ent:cart".into()],
                        provenance: Some(Provenance::Decree {
                            author: "owner".into(),
                            at: "now".into(),
                            note: None,
                        }),
                        ..Default::default()
                    },
                },
                Op::CreateRequirement {
                    id: "prov:none".into(),
                    requirement: Requirement {
                        statement: "Nothing backs this.".into(),
                        entities: vec!["ent:cart".into()],
                        ..Default::default()
                    },
                },
                Op::ReportDiagnostic {
                    id: String::new(),
                    diagnostic: Diagnostic {
                        rule: "ratification-pending".into(),
                        severity: "info".into(),
                        subjects: vec!["req:x-1".into()],
                        message: "the docs should say so".into(),
                        reasoning: None,
                        lifecycle: "open".into(),
                        triage: None,
                        prompt: None,
                        answer: None,
                        created: None,
                        updated: None,
                    },
                },
            ],
            &session(),
        );
        assert_eq!(r.applied, 5, "{:?}", r.skipped);
        assert!(r.skipped[0].contains("no provenance"));
        // The commit filed the decreed fact's proposal, statement verbatim; the
        // staged diagnostic on req:x-1 already stood, so no second one landed there.
        let proposal_id = s
            .graph
            .diagnostics
            .iter()
            .find(|(_, d)| {
                d.rule == "ratification-pending" && d.subjects == vec!["req:x-2".to_string()]
            })
            .map(|(id, _)| id.clone())
            .expect("the commit files the proposal for the decreed fact");
        let prompt = s.graph.diagnostics[&proposal_id].prompt.as_ref().unwrap();
        assert!(prompt
            .question
            .contains("The cart never exceeds ten categories."));
        assert_eq!(
            s.graph
                .diagnostics
                .values()
                .filter(|d| d.rule == "ratification-pending"
                    && d.subjects == vec!["req:x-1".to_string()])
                .count(),
            1
        );
        assert!(s.graph.requirements.contains_key("req:x-1"));
        assert!(s.graph.requirements.contains_key("req:x-2"));
        assert!(s.status.has_change(CHANGE_PROVENANCE_PENDING, "req:x-1"));
        assert!(s.status.has_change(CHANGE_PROVENANCE_PENDING, "req:x-2"));
        // The same derived statement from the same sources folds, never duplicates.
        s.apply(
            vec![Op::CreateRequirement {
                id: "prov:again".into(),
                requirement: Requirement {
                    statement: "The cart is split by category".into(),
                    entities: vec!["ent:cart".into()],
                    provenance: Some(derived(&["ent:cart"])),
                    ..Default::default()
                },
            }],
            &session(),
        );
        assert_eq!(s.graph.requirements.len(), 2);
        // The sweep never deletes derived or decreed nodes.
        s.gc();
        assert_eq!(s.graph.requirements.len(), 2);
        // Ratification flips the provenance to the quote and resolves the diagnostic.
        let r = s.apply(
            vec![Op::RatifyProvenance {
                id: "req:x-1".into(),
                source: mention("t.md", "/t", "The cart is split by category."),
            }],
            &Commit::store("ratify"),
        );
        assert_eq!(r.applied, 2, "{:?}", r.skipped);
        let q = &s.graph.requirements["req:x-1"];
        assert!(q.source.is_some() && q.provenance.is_none());
        assert!(!s.status.has_change(CHANGE_PROVENANCE_PENDING, "req:x-1"));
        assert!(s
            .graph
            .diagnostics
            .values()
            .filter(|d| d.subjects.contains(&"req:x-1".to_string()))
            .all(|d| d.lifecycle == "resolved"));
        assert_eq!(s.graph.diagnostics[&proposal_id].lifecycle, "open");
        assert!(s.graph.entities["ent:cart"]
            .mentions
            .iter()
            .any(|m| m.section == "/t"));
        // Retracting the decree deletes it; a quoted fact is not a decree.
        let r = s.apply(
            vec![
                Op::RetractDecree {
                    id: "req:x-2".into(),
                    reason: "withdrawn".into(),
                },
                Op::RetractDecree {
                    id: "req:x-1".into(),
                    reason: "withdrawn".into(),
                },
            ],
            &Commit::store("decree"),
        );
        assert_eq!(r.applied, 2, "{:?}", r.skipped);
        assert!(!s.graph.requirements.contains_key("req:x-2"));
        assert!(s.status.has_change(CHANGE_REQ_DELETED, "req:x-2"));
        assert!(r.skipped[0].contains("not a decree"));
        assert!(s
            .graph
            .diagnostics
            .get(&proposal_id)
            .is_none_or(|d| d.lifecycle == "resolved"));
    }

    // Retracting a derived grouping dissolves it: the children return to its parent,
    // a quoted requirement that named it re-points there, the pending record clears,
    // and the ratify entry journals the dissolve_entity mutation so the moves replay.
    // Mirrors docs/compiler/goals/ratify.md#retract.
    #[test]
    fn retracting_a_derived_grouping_dissolves_it_onto_its_parent() {
        let mut s = Store {
            out: own_dir("retract-grouping"),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nThe cart holds items.\n");
        let under = |name: &str, parent: &str, provenance: Option<Provenance>| Entity {
            parent: Some(parent.into()),
            provenance,
            ..entity(name)
        };
        let r = s.apply(
            vec![
                create("ent:checkout", "Checkout"),
                Op::CreateEntity {
                    id: "ent:selection".into(),
                    entity: under(
                        "Selection",
                        "ent:checkout",
                        Some(derived(&["ent:cart", "ent:wishlist"])),
                    ),
                },
                Op::CreateEntity {
                    id: "ent:cart".into(),
                    entity: under("Cart", "ent:selection", None),
                },
                Op::CreateEntity {
                    id: "ent:wishlist".into(),
                    entity: under("Wishlist", "ent:selection", None),
                },
                Op::CreateRequirement {
                    id: "prov:q".into(),
                    requirement: Requirement {
                        statement: "The cart holds items.".into(),
                        entities: vec!["ent:cart".into(), "ent:selection".into()],
                        source: Some(mention("t.md", "/t", "The cart holds items.")),
                        ..Default::default()
                    },
                },
            ],
            &session(),
        );
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        assert!(s.status.has_change(CHANGE_PROVENANCE_PENDING, "ent:selection"));
        let rid = s
            .graph
            .requirements
            .iter()
            .find(|(_, q)| q.statement == "The cart holds items.")
            .map(|(id, _)| id.clone())
            .unwrap();
        let r = s.apply(
            vec![Op::RetractDecree {
                id: "ent:selection".into(),
                reason: "retracted".into(),
            }],
            &Commit::store("ratify"),
        );
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        assert!(!s.graph.entities.contains_key("ent:selection"));
        assert_eq!(s.graph.redirects["ent:selection"], "ent:checkout");
        for c in ["ent:cart", "ent:wishlist"] {
            assert_eq!(s.graph.entities[c].parent.as_deref(), Some("ent:checkout"));
        }
        let q = &s.graph.requirements[&rid];
        assert!(!q.entities.iter().any(|e| e == "ent:selection"));
        assert!(q.entities.iter().any(|e| e == "ent:checkout"));
        assert!(!s.status.has_change(CHANGE_PROVENANCE_PENDING, "ent:selection"));
        let entry: JournalEntry = yaml_to(
            &s.out
                .join("journal")
                .join(format!("g{}.yaml", r.generation)),
        )
        .unwrap();
        assert_eq!(entry.kind, "ratify");
        assert!(entry.mutations.iter().any(|m| m["op"] == "dissolve_entity"));
        let moves = s.journaled_parent_moves();
        assert!(moves.iter().any(|mv| mv.child == "ent:wishlist"
            && mv.from.as_deref() == Some("ent:selection")
            && mv.to.as_deref() == Some("ent:checkout")));
        // A parentless entity keeps its quoted requirements: the refusal names them.
        let r = s.apply(
            vec![
                Op::CreateEntity {
                    id: "ent:top".into(),
                    entity: Entity {
                        provenance: Some(derived(&["ent:checkout"])),
                        ..entity("Top")
                    },
                },
                Op::CreateRequirement {
                    id: "prov:t".into(),
                    requirement: Requirement {
                        statement: "The top holds the checkout.".into(),
                        entities: vec!["ent:top".into()],
                        provenance: Some(derived(&["ent:top"])),
                        ..Default::default()
                    },
                },
            ],
            &session(),
        );
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        let r = s.apply(
            vec![Op::RetractDecree {
                id: "ent:top".into(),
                reason: "retracted".into(),
            }],
            &Commit::store("ratify"),
        );
        assert!(r.skipped[0].contains("no parent to take its requirements"));
        assert!(s.graph.entities.contains_key("ent:top"));
        // All or nothing: the refused retract takes the answer beside it down with
        // it, nothing lands, no generation is minted, and the diagnostic can still
        // be answered.
        let before = s.status.generation;
        let proposal = s
            .graph
            .diagnostics
            .iter()
            .find(|(_, d)| d.rule == "ratification-pending" && d.subjects == vec!["ent:top"])
            .map(|(id, _)| id.clone())
            .expect("the decree filed its proposal");
        let r = s.apply(
            vec![
                Op::RetractDecree {
                    id: "ent:top".into(),
                    reason: "retracted".into(),
                },
                Op::AnswerDiagnostic {
                    id: proposal.clone(),
                    answer: crate::model::DiagnosticAnswer {
                        choice: Some(1),
                        text: "retract".into(),
                        status: "applied".into(),
                    },
                },
            ],
            &Commit {
                all_or_nothing: true,
                ..Commit::store("ratify")
            },
        );
        assert_eq!(r.applied, 0);
        assert!(r.skipped[0].contains("no parent to take its requirements"));
        assert_eq!(s.status.generation, before);
        assert!(s.graph.diagnostics[&proposal].answer.is_none());
        assert!(!s
            .out
            .join("journal")
            .join(format!("g{}.yaml", before + 1))
            .exists());
    }

    // A save that dirtied sections is a generation of its own with its records.
    // Mirrors docs/compiler/graph.md#journal.
    #[test]
    fn sync_docs_journals_an_edit_entry_with_section_records() {
        let mut s = Store {
            out: own_dir("edit"),
            ..Default::default()
        };
        let v1 = "# T\nintro\n\n## Alpha\nalpha body\n\n## Beta\nbeta body\n";
        let mut parsed = BTreeMap::new();
        parsed.insert(
            "t.md".to_string(),
            (hash_hex(v1), crate::md::parse_sections(v1)),
        );
        s.sync_docs(&parsed);
        assert_eq!(s.status.generation, 1);
        let entry: JournalEntry = yaml_to(&s.out.join("journal").join("g1.yaml")).unwrap();
        assert_eq!(entry.kind, "edit");
        assert_eq!(entry.dirtied.len(), 3);
        assert!(entry
            .mutations
            .iter()
            .all(|m| m["op"] == CHANGE_SECTION_DIRTY));
        assert!(s.status.has_change(CHANGE_SECTION_DIRTY, "t.md#/t/alpha"));
        let rec = s
            .status
            .changes
            .iter()
            .find(|c| c.subject == "t.md#/t/alpha")
            .unwrap();
        assert_eq!(rec.generation, 1);
        assert_eq!(rec.via, "section");
        assert_eq!(rec.detail["section"], "/t/alpha");
        // Nothing changed: no entry, no generation.
        s.sync_docs(&parsed);
        assert_eq!(s.status.generation, 1);
        // Beta removed, Alpha edited: the entry lists both, the removed record is a trail
        // the sweep clears, and Alpha's stale anchor is recorded on its section.
        s.graph.requirements.insert(
            "req:t-1".into(),
            Requirement {
                statement: "alpha body".into(),
                entities: vec![],
                source: Some(mention("t.md", "/t/alpha", "alpha body")),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:t-2".into(),
            Requirement {
                statement: "beta body".into(),
                entities: vec![],
                source: Some(mention("t.md", "/t/beta", "beta body")),
                ..Default::default()
            },
        );
        let v2 = "# T\nintro\n\n## Alpha\nalpha CHANGED\n";
        let mut parsed2 = BTreeMap::new();
        parsed2.insert(
            "t.md".to_string(),
            (hash_hex(v2), crate::md::parse_sections(v2)),
        );
        let d2 = s.sync_docs(&parsed2);
        assert!(
            d2[0].stale_anchors.contains(&"req:t-1".to_string()) || !s.status.alignment.is_empty()
        );
        let edit_gen = s
            .status
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_SECTION_REMOVED)
            .map(|c| c.generation)
            .expect("section-removed");
        let entry: JournalEntry =
            yaml_to(&s.out.join("journal").join(format!("g{}.yaml", edit_gen))).unwrap();
        assert_eq!(entry.kind, "edit");
        assert!(entry.dirtied.contains(&"t.md#/t/beta".to_string()));
        assert!(entry
            .mutations
            .iter()
            .any(|m| m["op"] == CHANGE_SECTION_REMOVED && m["section"] == "/t/beta"));
        assert!(entry
            .mutations
            .iter()
            .any(|m| m["op"] == CHANGE_SECTION_DIRTY && m["section"] == "/t/alpha"));
        assert!(
            s.status.has_change(CHANGE_ANCHOR_STALE, "t.md#/t/beta")
                || s.status.has_change(CHANGE_ALIGNMENT_PENDING, "t.md"),
            "{:?}",
            s.status.changes
        );
        // The sweep deletes the homeless requirement and clears the trail.
        s.gc();
        assert!(!s.graph.requirements.contains_key("req:t-2") || !s.status.alignment.is_empty());
        assert!(!s.status.has_change(CHANGE_SECTION_REMOVED, "t.md#/t/beta"));
    }

    #[test]
    fn sync_docs_dirty_moved_removed() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        let v1 =
            "# T\nintro\n\n## Group\ngroup body\n\n### Alpha\nalpha body\n\n## Beta\nbeta body\n";
        let mut parsed = BTreeMap::new();
        parsed.insert(
            "t.md".to_string(),
            (hash_hex(v1), crate::md::parse_sections(v1)),
        );
        let d1 = s.sync_docs(&parsed);
        assert_eq!(d1.len(), 1);
        assert_eq!(d1[0].dirty_sections.len(), 4);

        // Anchor nodes in Alpha (which will move) and Beta (which will change).
        s.graph.entities.insert(
            "ent:a".into(),
            Entity {
                name: "A".into(),
                mentions: vec![mention("t.md", "/t/group/alpha", "alpha body")],
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:t-1".into(),
            Requirement {
                statement: "The A alphas.".into(),
                entities: vec!["ent:a".into()],
                source: Some(mention("t.md", "/t/group/alpha", "alpha body")),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:t-2".into(),
            Requirement {
                statement: "The B betas.".into(),
                entities: vec!["ent:a".into()],
                source: Some(mention("t.md", "/t/beta", "beta body")),
                ..Default::default()
            },
        );
        s.docs.get_mut("t.md").unwrap().coverage.insert(
            "/t/group/alpha".into(),
            Coverage {
                state: "covered".into(),
                note: None,
                claimed_by: None,
            },
        );

        // Rename the Group heading (Alpha moves under the new reference, its raw unchanged)
        // and edit Beta so the anchored quote no longer locates.
        let v2 = "# T\nintro\n\n## Bunch\ngroup body\n\n### Alpha\nalpha body\n\n## Beta\nbeta CHANGED body\n";
        let mut parsed2 = BTreeMap::new();
        parsed2.insert(
            "t.md".to_string(),
            (hash_hex(v2), crate::md::parse_sections(v2)),
        );
        let d2 = s.sync_docs(&parsed2);
        assert_eq!(d2.len(), 1);
        // Bunch is a changed section, Beta is a changed section; the moved Alpha is not dirty.
        assert_eq!(
            d2[0].dirty_sections,
            vec!["/t/beta".to_string(), "/t/bunch".to_string()]
        );
        // Beta's quote no longer locates -> a proposal for the align session (the section
        // was edited in place, so it has a candidate); Alpha's references were rewritten.
        assert!(!d2[0].stale_anchors.contains(&"req:t-2".to_string()));
        assert!(s
            .status
            .alignment
            .iter()
            .any(|b| b.doc == "t.md" && b.proposals.iter().any(|p| p.anchor == "req:t-2")));
        assert!(s.status.has_change(CHANGE_ALIGNMENT_PENDING, "t.md"));
        assert!(!d2[0].stale_anchors.contains(&"req:t-1".to_string()));
        assert_eq!(
            s.graph.requirements["req:t-1"]
                .source
                .as_ref()
                .unwrap()
                .section,
            "/t/bunch/alpha"
        );
        assert_eq!(
            s.graph.entities["ent:a"].mentions[0].section,
            "/t/bunch/alpha"
        );
        let rec = &s.docs["t.md"];
        assert!(rec.coverage.contains_key("/t/bunch/alpha"));
    }

    #[test]
    fn gc_removes_unanchored_and_spares_derived() {
        let mut s = Store {
            out: own_dir("gc"),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nbody\n");
        s.graph.requirements.insert(
            "req:gone-1".into(),
            Requirement {
                statement: "The X ys.".into(),
                entities: vec!["ent:x".into()],
                source: Some(mention("gone.md", "/gone", "x")),
                ..Default::default()
            },
        );
        s.graph.entities.insert(
            "ent:x".into(),
            Entity {
                name: "X".into(),
                mentions: vec![mention("gone.md", "/gone", "x")],
                ..Default::default()
            },
        );
        s.graph.entities.insert(
            "ent:invented".into(),
            Entity {
                name: "Invented".into(),
                provenance: Some(derived(&["ent:x"])),
                ..Default::default()
            },
        );
        // A derived node holding requirements is a caps-variant sub-entity, never a
        // grouping: the dissolve rule leaves it to judgment.
        s.graph.requirements.insert(
            "req:t-1".into(),
            Requirement {
                statement: "The invented thing holds.".into(),
                entities: vec!["ent:invented".into()],
                source: Some(mention("t.md", "/t", "body")),
                ..Default::default()
            },
        );
        s.graph.views.insert(
            "view:class/kept".into(),
            View {
                kind: "class".into(),
                title: "Kept".into(),
                members: vec!["ent:x".into()],
                provenance: Some(derived(&["ent:x"])),
                ..Default::default()
            },
        );
        let actions = s.gc();
        assert!(actions.len() >= 2, "{:?}", actions);
        assert!(!s.graph.requirements.contains_key("req:gone-1"));
        assert!(s.graph.requirements.contains_key("req:t-1"));
        assert!(!s.graph.entities.contains_key("ent:x"));
        assert_eq!(s.graph.redirects["ent:x"], "");
        // Derived nodes are judgment's, not the sweep's; the ones citing the dead node
        // and the curated view listing it owe a retrace.
        assert!(s.graph.entities.contains_key("ent:invented"));
        let node = s
            .status
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_NODE_DELETED && c.subject == "ent:invented")
            .expect("node-deleted");
        assert_eq!(node.via, "from");
        assert!(s
            .status
            .has_change(CHANGE_VIEW_MEMBER_GONE, "view:class/kept"));
        assert!(s.status.has_change(CHANGE_ENTITY_DELETED, "ent:x"));
        let entry: JournalEntry = yaml_to(
            &s.out
                .join("journal")
                .join(format!("g{}.yaml", s.status.generation)),
        )
        .unwrap();
        assert_eq!(entry.kind, "gc");
        // A second sweep is a no-op: the standing record is not rewritten.
        let gen = s.status.generation;
        assert!(s.gc().is_empty());
        assert_eq!(s.status.generation, gen);
    }

    // The sweep runs at every commit, not only inside a build: a decree committed
    // through `apply` on a store holding an unanchored requirement lands its own
    // entry, then the sweep lands a `gc` entry behind it and the report says so.
    // Mirrors docs/compiler/graph.md#changesets and #the-sweep.
    #[test]
    fn every_commit_runs_the_sweep_behind_its_own_entry() {
        let mut s = Store {
            out: own_dir("sweep-at-commit"),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nbody\n");
        s.graph.requirements.insert(
            "req:gone-1".into(),
            Requirement {
                statement: "The X ys.".into(),
                entities: vec!["ent:x".into()],
                source: Some(mention("gone.md", "/gone", "x")),
                ..Default::default()
            },
        );
        s.graph.entities.insert(
            "ent:x".into(),
            Entity {
                name: "X".into(),
                mentions: vec![mention("gone.md", "/gone", "x")],
                ..Default::default()
            },
        );
        let report = s.apply(
            vec![create("ent:fresh", "Fresh")],
            &Commit::store("decree"),
        );
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        assert!(
            report.swept.iter().any(|a| a.contains("req:gone-1")),
            "the sweep ran behind the decree: {:?}",
            report.swept
        );
        assert!(!s.graph.requirements.contains_key("req:gone-1"));
        assert!(!s.graph.entities.contains_key("ent:x"));
        assert!(s.graph.entities.contains_key("ent:fresh"));
        // The decree's own entry, then the sweep's, each a generation.
        let decree = journal_entry(&s, report.generation);
        assert_eq!(decree.kind, "decree");
        let gc = journal_entry(&s, report.generation + 1);
        assert_eq!(gc.kind, "gc");
        assert_eq!(s.status.generation, report.generation + 1);
        // A clean store commits without a sweep entry.
        let report = s.apply(
            vec![create("ent:other", "Other")],
            &Commit::store("decree"),
        );
        assert!(report.swept.is_empty(), "{:?}", report.swept);
        assert_eq!(s.status.generation, report.generation);
    }

    #[test]
    fn check_diags_reconcile_not_regenerate() {
        let mut s = Store {
            out: own_dir("checks"),
            ..Default::default()
        };
        s.reconcile_check_diags(vec![(
            "uncovered-section".into(),
            vec!["t.md#/t".into()],
            "warning".into(),
            "section /t is unprocessed".into(),
            None,
        )]);
        assert_eq!(s.graph.diagnostics.len(), 1);
        assert_eq!(s.status.generation, 1);
        let entry: JournalEntry = yaml_to(&s.out.join("journal").join("g1.yaml")).unwrap();
        assert_eq!(entry.kind, "checks");
        assert_eq!(entry.mutations[0]["op"], "report_diagnostic");
        let id = s.graph.diagnostics.keys().next().unwrap().clone();
        // Same finding again: same id, no duplicate, no generation.
        s.reconcile_check_diags(vec![(
            "uncovered-section".into(),
            vec!["t.md#/t".into()],
            "warning".into(),
            "section /t is unprocessed".into(),
            None,
        )]);
        assert_eq!(s.graph.diagnostics.len(), 1);
        assert!(s.graph.diagnostics.contains_key(&id));
        assert_eq!(s.status.generation, 1);
        // A prompt landing on a finding opens an answer.
        s.reconcile_check_diags(vec![(
            "uncovered-section".into(),
            vec!["t.md#/t".into()],
            "warning".into(),
            "section /t is unprocessed".into(),
            Some(crate::model::DiagnosticPrompt {
                question: "skip it?".into(),
                options: Vec::new(),
                freeform: true,
            }),
        )]);
        assert!(s.status.has_change(CHANGE_PROMPT_UNANSWERED, &id));
        // Finding cleared: resolved, not deleted, and the answer debt goes with it.
        s.reconcile_check_diags(vec![]);
        assert_eq!(s.graph.diagnostics[&id].lifecycle, "resolved");
        assert!(!s.status.has_change(CHANGE_PROMPT_UNANSWERED, &id));
        assert_eq!(s.status.generation, 3);
    }

    #[test]
    fn search_tiers() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        s.graph.entities.insert(
            "ent:shopping-cart".into(),
            Entity {
                name: "Shopping Cart".into(),
                aliases: vec!["cart".into()],
                ..Default::default()
            },
        );
        s.graph.entities.insert(
            "ent:card".into(),
            Entity {
                name: "Credit Card".into(),
                ..Default::default()
            },
        );
        let hits = s.search("cart");
        assert_eq!(hits[0].0, "ent:shopping-cart");
        let hits2 = s.search("credit card");
        assert_eq!(hits2[0].0, "ent:card");
    }

    #[test]
    fn create_under_existing_id_folds_in_place_and_reports_change() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(
            &mut s,
            "t.md",
            "# T\nThe Cart holds items a Customer intends to buy. It keeps items.\n",
        );
        s.graph.entities.insert("ent:cart".into(), entity("Cart"));
        s.graph.requirements.insert(
            "req:t-1".into(),
            Requirement {
                statement: "The Cart holds items a Customer intends to buy.".into(),
                entities: vec!["ent:cart".into()],
                source: Some(mention(
                    "t.md",
                    "/t",
                    "holds items a Customer intends to buy",
                )),
                ..Default::default()
            },
        );
        // Stage-time resolution staged a create under the anchor's id with a reworded,
        // subsuming statement and a fresh quote; commit folds it in place.
        let report = s.apply(
            vec![Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    statement: "The Cart holds items.".into(),
                    entities: vec!["ent:cart".into()],
                    source: Some(mention("t.md", "/t", "keeps items")),
                    ..Default::default()
                },
            }],
            &session(),
        );
        assert_eq!(s.graph.requirements.len(), 1);
        let r = &s.graph.requirements["req:t-1"];
        assert_eq!(r.statement, "The Cart holds items.");
        assert_eq!(r.source.as_ref().unwrap().quote, "keeps items");
        assert!(
            report.changed_requirements.contains("req:t-1"),
            "{:?}",
            report.changed_requirements
        );
    }

    #[test]
    fn requirement_neighbors_find_the_topic_cluster() {
        let mut s = Store::default();
        s.graph
            .entities
            .insert("ent:util".into(), entity("Sorting Algorithm CLI Utility"));
        let mk = |statement: &str| Requirement {
            statement: statement.into(),
            entities: vec!["ent:util".into()],
            source: Some(mention("m.md", "/m", "q")),
            ..Default::default()
        };
        // The example-sort failure: three statements about reverse-order sorting were
        // never put side by side. Stemmed content-token overlap pairs them.
        s.graph.requirements.insert(
            "req:m-2".into(),
            mk("The system allows the -r argument, which reverses sorting order to descending."),
        );
        s.graph.requirements.insert(
            "req:m-3".into(),
            mk("The Sorting Algorithm CLI Utility keeps track of reverse order with `-r`."),
        );
        s.graph.requirements.insert("req:m-5".into(), mk("The Sorting Algorithm CLI Utility strips out whitespace before and after the current line."));
        s.graph.requirements.insert("req:m-8".into(), mk("The Sorting Algorithm CLI Utility sorts lines descending, or ascending if reverse order is set."));
        let n = s.requirement_neighbors("req:m-8");
        assert!(n.contains(&"req:m-2".to_string()), "{:?}", n);
        assert!(n.contains(&"req:m-3".to_string()), "{:?}", n);
        assert!(!n.contains(&"req:m-5".to_string()), "{:?}", n);
    }

    #[test]
    fn requirement_neighbors_pair_on_shared_entities_alone() {
        let mut s = Store::default();
        s.graph
            .entities
            .insert("ent:reorder-point".into(), entity("Reorder point"));
        s.graph
            .entities
            .insert("ent:restock-task".into(), entity("Restock task"));
        let mk = |statement: &str, entities: &[&str]| Requirement {
            statement: statement.into(),
            entities: entities.iter().map(|e| e.to_string()).collect(),
            source: Some(mention("m.md", "/m", "q")),
            ..Default::default()
        };
        // The f2 failure: a glossary definition restating an inventory rule shares
        // both entities and, with their names out of the token pool, no other
        // token. Two shared entities qualify the pair on their own.
        s.graph.requirements.insert(
            "req:inv-3".into(),
            mk(
                "When Stock falls below the Reorder point, the system creates a Restock task.",
                &["ent:reorder-point", "ent:restock-task"],
            ),
        );
        s.graph.requirements.insert(
            "req:glos-3".into(),
            mk(
                "The Reorder point is whatever level triggers a Restock task.",
                &["ent:reorder-point", "ent:restock-task"],
            ),
        );
        s.graph.requirements.insert(
            "req:other".into(),
            mk(
                "The Restock task queue drains nightly.",
                &["ent:restock-task"],
            ),
        );
        let n = s.requirement_neighbors("req:glos-3");
        assert!(n.contains(&"req:inv-3".to_string()), "{:?}", n);
        assert!(!n.contains(&"req:other".to_string()), "{:?}", n);
    }

    // The example-org expenses shapes: an opening-paragraph sentence against a step
    // below it sharing one entity and the verb stem pairs (expenses-5 against
    // expenses-6), and so does a restated transition (expenses-1 against expenses-15).
    // Mirrors docs/compiler/reconciler.md#pairs.
    #[test]
    fn requirement_neighbors_pair_intro_restatements_and_same_transitions() {
        let mut s = Store::default();
        seed_doc(
            &mut s,
            "expenses.md",
            "# Expenses\n\nAn employee files an expense claim.\n\n## The claim\n\nClaims are filed on form EX-12.\n\n## Steps\n\nFinance reimburses.\n",
        );
        let opening = s.opening_section("expenses.md").unwrap().to_string();
        let mut later: Vec<String> = s.docs["expenses.md"]
            .sections
            .iter()
            .filter(|(r, _)| **r != opening)
            .map(|(r, _)| r.clone())
            .collect();
        later.sort();
        assert_eq!(later.len(), 2, "{:?}", later);
        s.graph
            .entities
            .insert("ent:expense-claim".into(), entity("Expense claim"));
        s.graph
            .entities
            .insert("ent:employees".into(), entity("Employees"));
        s.graph
            .entities
            .insert("ent:expense-tool".into(), entity("Expense tool"));
        s.graph
            .entities
            .insert("ent:finance".into(), entity("Finance"));
        s.graph
            .entities
            .insert("ent:payroll".into(), entity("Payroll"));
        let mk = |statement: &str, entities: &[&str], section: &str| Requirement {
            statement: statement.into(),
            entities: entities.iter().map(|e| e.to_string()).collect(),
            source: Some(mention("expenses.md", section, "q")),
            ..Default::default()
        };
        // expenses-5 ~ expenses-6: one shared entity, one shared token ("file").
        s.graph.requirements.insert(
            "req:expenses-5".into(),
            mk(
                "An employee who spends their own money on company business files an expense claim to be reimbursed.",
                &["ent:employees", "ent:expense-claim"],
                &opening,
            ),
        );
        s.graph.requirements.insert(
            "req:expenses-6".into(),
            mk(
                "Claims are filed on form EX-12 in the expense tool.",
                &["ent:expense-claim", "ent:expense-tool"],
                &later[0],
            ),
        );
        // The same shape between two later sections: one token is not enough.
        s.graph.requirements.insert(
            "req:expenses-9".into(),
            mk(
                "A claim is filed with a receipt for every line.",
                &["ent:expense-claim"],
                &later[1],
            ),
        );
        let n = s.requirement_neighbors("req:expenses-5");
        assert!(n.contains(&"req:expenses-6".to_string()), "{:?}", n);
        let n = s.requirement_neighbors("req:expenses-6");
        assert!(n.contains(&"req:expenses-5".to_string()), "{:?}", n);
        assert!(!n.contains(&"req:expenses-9".to_string()), "{:?}", n);
        // expenses-1 ~ expenses-15 as a transition pair alone: same subject, same from
        // and to, no shared token, one shared entity, different documents.
        let tr = |from: &str, to: &str| {
            Some(crate::model::Transition {
                subject: "ent:expense-claim".into(),
                from: from.into(),
                to: to.into(),
                trigger: None,
                guard: None,
            })
        };
        s.graph.requirements.insert(
            "req:expenses-1".into(),
            Requirement {
                transition: tr("approved", "reimbursed"),
                ..mk(
                    "Finance pays it back.",
                    &["ent:finance", "ent:expense-claim"],
                    &later[1],
                )
            },
        );
        s.graph.requirements.insert(
            "req:policies-4".into(),
            Requirement {
                transition: tr("Approved", "Reimbursed"),
                source: Some(mention("policies.md", "/policies", "q")),
                ..mk(
                    "The next run settles the money owed.",
                    &["ent:expense-claim", "ent:payroll"],
                    &later[1],
                )
            },
        );
        s.graph.requirements.insert(
            "req:policies-5".into(),
            Requirement {
                transition: tr("approved", "rejected"),
                source: Some(mention("policies.md", "/policies", "q")),
                ..mk(
                    "A wrong total sends it back.",
                    &["ent:expense-claim"],
                    &later[1],
                )
            },
        );
        let n = s.requirement_neighbors("req:expenses-1");
        assert!(n.contains(&"req:policies-4".to_string()), "{:?}", n);
        assert!(!n.contains(&"req:policies-5".to_string()), "{:?}", n);
    }

    // The qwen pair flood: requirements sharing only the hub entity need three shared
    // tokens, a pair sharing any other entity still needs two, and a graph whose
    // entities never meet has no hub. Mirrors docs/compiler/reconciler.md#pairs.
    #[test]
    fn requirement_neighbors_discount_the_hub() {
        let mut s = Store::default();
        for (id, name) in [
            ("ent:order", "Order"),
            ("ent:customer", "Customer"),
            ("ent:shipment", "Shipment"),
            ("ent:payment", "Payment"),
        ] {
            s.graph.entities.insert(id.into(), entity(name));
        }
        let mk = |statement: &str, entities: &[&str]| Requirement {
            statement: statement.into(),
            entities: entities.iter().map(|e| e.to_string()).collect(),
            source: Some(mention("m.md", "/m", "q")),
            ..Default::default()
        };
        // ent:order meets three peers; every other entity meets one.
        s.graph.requirements.insert(
            "req:o-1".into(),
            mk(
                "The Customer places an Order; the tool records the total and the date.",
                &["ent:order", "ent:customer"],
            ),
        );
        s.graph.requirements.insert(
            "req:o-2".into(),
            mk(
                "An Order ships in one Shipment; the tool records the carrier.",
                &["ent:order", "ent:shipment"],
            ),
        );
        s.graph.requirements.insert(
            "req:o-3".into(),
            mk(
                "An Order is paid by one Payment; the tool records the date.",
                &["ent:order", "ent:payment"],
            ),
        );
        assert_eq!(s.hub_entity().as_deref(), Some("ent:order"));
        // Two generic tokens over the hub alone ("tool", "record"): not a pair.
        s.graph.requirements.insert(
            "req:o-4".into(),
            mk("The tool records every Order change.", &["ent:order"]),
        );
        // Three shared tokens over the hub alone: still a pair.
        s.graph.requirements.insert(
            "req:o-5".into(),
            mk(
                "The tool records the date of an Order total.",
                &["ent:order"],
            ),
        );
        // Two shared tokens over a non-hub entity: a pair as before.
        s.graph.requirements.insert(
            "req:c-1".into(),
            mk(
                "A Customer who places nothing records no total.",
                &["ent:customer"],
            ),
        );
        let n = s.requirement_neighbors("req:o-1");
        assert!(!n.contains(&"req:o-4".to_string()), "{:?}", n);
        assert!(n.contains(&"req:o-5".to_string()), "{:?}", n);
        assert!(n.contains(&"req:c-1".to_string()), "{:?}", n);
        // A graph of one entity, or of tied peers, names no hub.
        let mut lone = Store::default();
        lone.graph
            .entities
            .insert("ent:util".into(), entity("Util"));
        lone.graph
            .requirements
            .insert("req:u-1".into(), mk("Util sorts.", &["ent:util"]));
        assert_eq!(lone.hub_entity(), None);
    }

    #[test]
    fn pair_diagnostic_sticky_regardless_of_subject_order() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        let d = |subjects: Vec<String>| Diagnostic {
            rule: "contradiction".into(),
            severity: "warning".into(),
            subjects,
            message: "conflict".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        };
        // The subjects exist: the sweep behind every commit settles a finding whose
        // subjects are gone.
        for id in ["req:a", "req:b"] {
            s.graph.requirements.insert(
                id.into(),
                Requirement {
                    statement: format!("{} holds.", id),
                    ..Default::default()
                },
            );
        }
        s.apply(
            vec![Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: d(vec!["req:a".into(), "req:b".into()]),
            }],
            &session(),
        );
        // The same pair reported from the other endpoint updates the finding in place.
        s.apply(
            vec![Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: d(vec!["req:b".into(), "req:a".into()]),
            }],
            &session(),
        );
        assert_eq!(s.graph.diagnostics.len(), 1);
    }

    #[test]
    fn answered_prompt_is_never_reasked_and_resolve_marks_handled() {
        use crate::model::{DiagnosticAnswer, DiagnosticPrompt};
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        let prompt = |q: &str| DiagnosticPrompt {
            question: q.into(),
            options: Vec::new(),
            freeform: true,
        };
        let with_prompt = |q: Option<&str>| Diagnostic {
            rule: "contradiction".into(),
            severity: "warning".into(),
            subjects: vec!["req:a".into(), "req:b".into()],
            message: "conflict".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: q.map(prompt),
            answer: None,
            created: None,
            updated: None,
        };
        for id in ["req:a", "req:b"] {
            s.graph.requirements.insert(
                id.into(),
                Requirement {
                    statement: format!("{} holds.", id),
                    ..Default::default()
                },
            );
        }
        let r = s.apply(
            vec![Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: with_prompt(Some("which?")),
            }],
            &session(),
        );
        let id = s.graph.diagnostics.keys().next().unwrap().clone();
        // A prompt landing opens an answer; its cause is the report.
        let unanswered = r
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_PROMPT_UNANSWERED)
            .unwrap();
        assert_eq!(
            (unanswered.subject.as_str(), unanswered.via.as_str()),
            (id.as_str(), "report_diagnostic")
        );
        // A promptless re-report keeps the question; a fresh one replaces it.
        s.apply(
            vec![Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: with_prompt(None),
            }],
            &session(),
        );
        assert_eq!(
            s.graph.diagnostics[&id].prompt.as_ref().unwrap().question,
            "which?"
        );
        s.apply(
            vec![Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: with_prompt(Some("sharper?")),
            }],
            &session(),
        );
        assert_eq!(
            s.graph.diagnostics[&id].prompt.as_ref().unwrap().question,
            "sharper?"
        );
        // Once answered, a re-report never re-asks, and the answer debt is paid.
        s.apply(
            vec![Op::AnswerDiagnostic {
                id: id.clone(),
                answer: DiagnosticAnswer {
                    choice: None,
                    text: "both".into(),
                    status: "handling".into(),
                },
            }],
            &Commit::store("answer"),
        );
        assert!(!s.status.has_change(CHANGE_PROMPT_UNANSWERED, &id));
        s.apply(
            vec![Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: with_prompt(Some("again?")),
            }],
            &session(),
        );
        assert_eq!(
            s.graph.diagnostics[&id].prompt.as_ref().unwrap().question,
            "sharper?"
        );
        // Resolving while handling is the handling session finishing.
        s.apply(
            vec![Op::ResolveDiagnostic {
                id: id.clone(),
                reason: "settled".into(),
            }],
            &session(),
        );
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

    fn seeded_req(doc: &str, statement: &str) -> Requirement {
        Requirement {
            statement: statement.into(),
            entities: vec!["ent:cart".into()],
            source: Some(mention(doc, "/t", "The Cart holds items.")),
            ..Default::default()
        }
    }

    // Deleting one side of a filed contradiction settles or re-judges: a surviving
    // subject gets a node-deleted record, a diagnostic with no subject left resolves.
    // Mirrors docs/compiler/reconciler.md#pairs.
    #[test]
    fn deleting_a_subject_settles_or_rejudges_its_diagnostics() {
        let mut s = Store {
            out: own_dir("propagate"),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
        s.graph.entities.insert(
            "ent:cart".into(),
            Entity {
                name: "Cart".into(),
                mentions: vec![mention("t.md", "/t", "The Cart holds items.")],
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:t-1".into(),
            seeded_req("t.md", "The Cart holds items."),
        );
        s.graph.requirements.insert(
            "req:t-2".into(),
            seeded_req("t.md", "The Cart stays empty."),
        );
        s.apply(
            vec![Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: diag("contradiction", vec!["req:t-1", "req:t-2"]),
            }],
            &session(),
        );
        let did = s.graph.diagnostics.keys().next().unwrap().clone();
        s.status.changes.clear();

        // One subject deleted: the diagnostic stands, the survivor owes a re-judgment.
        let r = s.apply(
            vec![Op::DeleteRequirement {
                id: "req:t-1".into(),
                reason: "fact gone".into(),
            }],
            &session(),
        );
        assert_eq!(s.graph.diagnostics[&did].lifecycle, "open");
        let node = r
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_NODE_DELETED && c.subject == "req:t-2")
            .expect("node-deleted on the survivor");
        assert_eq!(node.via, "subjects");
        assert_eq!(node.mutation, 1);
        assert_eq!(node.detail["diagnostic"], did);
        assert!(s
            .status
            .changed_subjects(&REQ_REVIEW_KINDS)
            .contains(&"req:t-2".to_string()));
        assert!(s.status.has_change(CHANGE_REQ_DELETED, "req:t-1"));

        // The open diagnostic alone keeps the survivor's pair judgment due, with no
        // computed neighbor left.
        assert!(s.pair_review_neighbors("req:t-2").is_empty());
        assert!(s.pair_review_due("req:t-2"));

        // Every subject deleted: the store resolves the diagnostic itself.
        s.apply(
            vec![Op::DeleteRequirement {
                id: "req:t-2".into(),
                reason: "fact gone".into(),
            }],
            &session(),
        );
        assert_eq!(s.graph.diagnostics[&did].lifecycle, "resolved");
        assert!(!s.status.has_change(CHANGE_NODE_DELETED, "req:t-2"));
    }

    // A graph deleted into a stranded state heals at the deterministic tail.
    #[test]
    fn settle_dangling_diags_heals_a_stranded_graph() {
        let mut s = Store {
            out: own_dir("settle"),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
        s.graph.entities.insert("ent:cart".into(), entity("Cart"));
        s.graph.requirements.insert(
            "req:t-2".into(),
            seeded_req("t.md", "The Cart stays empty."),
        );
        s.graph.diagnostics.insert(
            "diag:contradiction-1".into(),
            diag("contradiction", vec!["req:gone", "req:t-2"]),
        );
        s.graph.diagnostics.insert(
            "diag:contradiction-2".into(),
            diag("contradiction", vec!["req:gone-a", "req:gone-b"]),
        );
        assert!(s.has_dangling_diags());

        let actions = s.settle_dangling_diags();
        // All subjects gone: resolved by the store, with a journaled action.
        assert_eq!(
            s.graph.diagnostics["diag:contradiction-2"].lifecycle,
            "resolved"
        );
        assert!(
            actions.iter().any(|a| a.contains("diag:contradiction-2")),
            "{:?}",
            actions
        );
        let entry: JournalEntry = yaml_to(
            &s.out
                .join("journal")
                .join(format!("g{}.yaml", s.status.generation)),
        )
        .unwrap();
        assert_eq!(entry.kind, "settle-diagnostics");
        // A survivor remains: the diagnostic stands and the survivor owes a judgment.
        assert_eq!(
            s.graph.diagnostics["diag:contradiction-1"].lifecycle,
            "open"
        );
        assert_eq!(
            s.status.changed_subjects(&REQ_REVIEW_KINDS),
            vec!["req:t-2".to_string()]
        );
        // Idempotent: a second sweep resolves nothing new.
        assert!(s.settle_dangling_diags().is_empty());
    }

    // The marker diagnostics settle at the sweep once their condition clears: an
    // incomplete-build marker whose goal left parked, an uncovered-section marker whose
    // section is marked or gone. Mirrors docs/compiler/graph.md#the-sweep.
    #[test]
    fn sweep_settles_cleared_markers() {
        let mut s = Store {
            out: own_dir("markers"),
            ..Default::default()
        };
        seed_doc(
            &mut s,
            "t.md",
            "# T\n\nThe Cart holds items.\n\n## A\n\nA body.\n\n## B\n\nB body.\n\n## C\n\nC body.\n",
        );
        let refs: Vec<String> = s.docs["t.md"].sections.keys().cloned().collect();
        assert_eq!(refs.len(), 4, "{:?}", refs);
        let mark = |s: &mut Store, r: &str, state: &str| {
            s.docs.get_mut("t.md").unwrap().coverage.insert(
                r.to_string(),
                Coverage {
                    state: state.into(),
                    note: None,
                    claimed_by: None,
                },
            );
        };
        mark(&mut s, &refs[1], "covered");
        mark(&mut s, &refs[2], "non-normative");
        let marker = |rule: &str, subject: &str| Diagnostic {
            rule: rule.into(),
            severity: "warning".into(),
            subjects: vec![subject.into()],
            message: "marker".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        };
        for (i, r) in refs.iter().enumerate() {
            s.graph.diagnostics.insert(
                format!("diag:uncovered-section-{}", i),
                marker("uncovered-section", &format!("t.md#{}", r)),
            );
        }
        s.graph.diagnostics.insert(
            "diag:uncovered-section-9".into(),
            marker("uncovered-section", "t.md#/t/gone"),
        );
        s.status.parked.push(Goal {
            id: "g:reconcile-section:t.md#/t/c".into(),
            kind: "reconcile-section".into(),
            target: format!("t.md#{}", refs[3]),
            ..Default::default()
        });
        s.graph.diagnostics.insert(
            "diag:incomplete-build-1".into(),
            marker("incomplete-build", &format!("t.md#{}", refs[3])),
        );
        s.graph.diagnostics.insert(
            "diag:incomplete-build-2".into(),
            marker("incomplete-build", &format!("t.md#{}", refs[1])),
        );
        assert!(s.has_dangling_diags());

        let actions = s.settle_cleared_markers();
        let lc = |s: &Store, id: &str| s.graph.diagnostics[id].lifecycle.clone();
        // Unmarked sections keep their marker; covered, non-normative, and gone resolve.
        assert_eq!(lc(&s, "diag:uncovered-section-0"), "open");
        assert_eq!(lc(&s, "diag:uncovered-section-1"), "resolved");
        assert_eq!(lc(&s, "diag:uncovered-section-2"), "resolved");
        assert_eq!(lc(&s, "diag:uncovered-section-3"), "open");
        assert_eq!(lc(&s, "diag:uncovered-section-9"), "resolved");
        // The goal still parked keeps its marker; the resumed one's resolves.
        assert_eq!(lc(&s, "diag:incomplete-build-1"), "open");
        assert_eq!(lc(&s, "diag:incomplete-build-2"), "resolved");
        assert_eq!(actions.len(), 4, "{:?}", actions);
        let entry: JournalEntry = yaml_to(
            &s.out
                .join("journal")
                .join(format!("g{}.yaml", s.status.generation)),
        )
        .unwrap();
        assert_eq!(entry.kind, "settle-diagnostics");
        assert_eq!(entry.mutations.len(), 4);
        // Idempotent, and the goal leaving parked clears the last marker next sweep.
        assert!(s.settle_cleared_markers().is_empty());
        s.status.parked.clear();
        let actions = s.settle_cleared_markers();
        assert_eq!(actions.len(), 1, "{:?}", actions);
        assert_eq!(lc(&s, "diag:incomplete-build-1"), "resolved");
        assert!(!s.has_dangling_diags());
    }

    // Check reconciliation resolves only its own single-subject findings; a judged
    // pair filed under a shared rule name is a session's to resolve.
    #[test]
    fn check_reconcile_leaves_judged_pairs_alone() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        s.graph.diagnostics.insert(
            "diag:duplicate-requirement-1".into(),
            diag("duplicate-requirement", vec!["req:a", "req:b"]),
        );
        s.reconcile_check_diags(Vec::new());
        assert_eq!(
            s.graph.diagnostics["diag:duplicate-requirement-1"].lifecycle,
            "open"
        );
    }

    fn req_at(doc: &str, sec: &str, quote: &str) -> Requirement {
        Requirement {
            statement: format!("The A {}", quote.trim_end_matches('.')),
            entities: vec!["ent:a".into()],
            source: Some(mention(doc, sec, quote)),
            ..Default::default()
        }
    }

    const CART: &str = "The cart holds items a customer intends to buy. Items stay until checkout. A cart may hold up to fifty items at once.";
    const ORDERS: &str = "Orders are placed from a cart. An order records the address and the total. Payment is taken at placement.";

    // A moved-and-edited section yields proposals, not stale anchors; the block persists
    // in status.yaml and the anchor survives the sweep until the session decides.
    #[test]
    fn sync_docs_proposes_for_a_moved_and_edited_section_and_gc_waits() {
        let mut s = Store {
            out: own_dir("proposal"),
            ..Default::default()
        };
        let v1 = format!("# T\nintro\n\n## Cart\n{}\n\n## Orders\n{}\n", CART, ORDERS);
        let mut parsed = BTreeMap::new();
        parsed.insert(
            "t.md".to_string(),
            (hash_hex(&v1), crate::md::parse_sections(&v1)),
        );
        s.sync_docs(&parsed);
        s.graph.entities.insert(
            "ent:a".into(),
            Entity {
                name: "A".into(),
                mentions: vec![mention("t.md", "/t/cart", "Items stay until checkout.")],
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:t-1".into(),
            req_at("t.md", "/t/cart", "Items stay until checkout."),
        );
        s.graph.requirements.insert(
            "req:t-2".into(),
            req_at(
                "t.md",
                "/t/cart",
                "A cart may hold up to fifty items at once.",
            ),
        );

        let v2 = format!(
            "# T\nintro\n\n## Orders\n{}\n\n## Basket\n{}\n",
            ORDERS,
            CART.replace("fifty", "sixty")
        );
        let mut parsed2 = BTreeMap::new();
        parsed2.insert(
            "t.md".to_string(),
            (hash_hex(&v2), crate::md::parse_sections(&v2)),
        );
        let d2 = s.sync_docs(&parsed2);
        assert_eq!(d2.len(), 1);
        assert_eq!(d2[0].dirty_sections, vec!["/t/basket".to_string()]);
        assert!(d2[0].stale_anchors.is_empty(), "{:?}", d2[0].stale_anchors);
        let block = s
            .status
            .alignment
            .iter()
            .find(|b| b.doc == "t.md")
            .expect("alignment block");
        // ent:a's mention coincides with req:t-1's source: derived, it follows the
        // requirement and is no proposal of its own.
        let anchors: BTreeSet<&str> = block.proposals.iter().map(|p| p.anchor.as_str()).collect();
        assert_eq!(anchors, ["req:t-1", "req:t-2"].into_iter().collect());
        assert!(block
            .changes
            .iter()
            .any(|c| c.op == "moved" && c.to == vec!["t.md#/t/basket"]));
        // The align entry carries the proposals.
        let entry: JournalEntry = yaml_to(
            &s.out
                .join("journal")
                .join(format!("g{}.yaml", s.status.generation)),
        )
        .unwrap();
        assert_eq!(entry.kind, "align");
        assert!(entry
            .mutations
            .iter()
            .any(|m| m["op"] == "propose" && m["anchor"] == "req:t-2"));
        // The persisted form round-trips.
        let reloaded = Store::load(&s.out);
        assert_eq!(reloaded.status.alignment, s.status.alignment);
        // Anchors still point at the vanished section, and the sweep leaves them alone.
        assert_eq!(
            s.graph.requirements["req:t-1"]
                .source
                .as_ref()
                .unwrap()
                .section,
            "/t/cart"
        );
        s.gc();
        assert!(s.graph.requirements.contains_key("req:t-1"));
        assert_eq!(s.graph.entities["ent:a"].mentions.len(), 1);
    }

    // place_anchor moves the source; reevaluate lists it as a stale anchor on the
    // target document; a commit naming it pays the debt. orphan_anchor leaves the
    // anchor where it was, and the decided proposals clear the block.
    #[test]
    fn place_and_orphan_anchor_apply_and_clear_the_block() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        let v1 = format!("# T\nintro\n\n## Cart\n{}\n", CART);
        let mut parsed = BTreeMap::new();
        parsed.insert(
            "t.md".to_string(),
            (hash_hex(&v1), crate::md::parse_sections(&v1)),
        );
        s.sync_docs(&parsed);
        s.graph.entities.insert(
            "ent:a".into(),
            Entity {
                name: "A".into(),
                mentions: vec![mention("t.md", "/t/cart", "Items stay until checkout.")],
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:t-1".into(),
            req_at("t.md", "/t/cart", "Items stay until checkout."),
        );
        s.graph.requirements.insert(
            "req:t-2".into(),
            req_at(
                "t.md",
                "/t/cart",
                "A cart may hold up to fifty items at once.",
            ),
        );
        s.graph.requirements.insert(
            "req:t-3".into(),
            req_at(
                "t.md",
                "/t/cart",
                "The cart holds items a customer intends to buy.",
            ),
        );
        let v2 = format!(
            "# T\nintro\n\n## Basket\n{}\n",
            CART.replace("fifty", "sixty")
        );
        let mut parsed2 = BTreeMap::new();
        parsed2.insert(
            "t.md".to_string(),
            (hash_hex(&v2), crate::md::parse_sections(&v2)),
        );
        s.sync_docs(&parsed2);
        assert_eq!(s.status.alignment.len(), 1);
        assert!(s.status.has_change(CHANGE_ALIGNMENT_PENDING, "t.md"));

        let align_item = WorkItem {
            task: "align-doc".into(),
            target: "t.md".into(),
            dirty_sections: vec![],
            stale_anchors: vec![],
            proposals: vec!["req:t-1".into(), "req:t-2".into(), "req:t-3".into()],
        };
        let from = |q: &str| mention("t.md", "/t/cart", q);
        let report = s.apply(
            vec![
                Op::PlaceAnchor {
                    id: "req:t-1".into(),
                    from: from("Items stay until checkout."),
                    to: mention("t.md", "/t/basket", "Items stay until checkout."),
                    reevaluate: false,
                },
                Op::PlaceAnchor {
                    id: "req:t-2".into(),
                    from: from("A cart may hold up to fifty items at once."),
                    to: mention(
                        "t.md",
                        "/t/basket",
                        "A cart may hold up to sixty items at once.",
                    ),
                    reevaluate: true,
                },
                Op::OrphanAnchor {
                    id: "req:t-3".into(),
                    from: from("The cart holds items a customer intends to buy."),
                },
            ],
            &align_item.commit(1, 0),
        );
        assert_eq!(report.applied, 3, "{:?}", report.skipped);
        assert!(s.status.alignment.is_empty());
        assert!(!s.status.has_change(CHANGE_ALIGNMENT_PENDING, "t.md"));
        assert_eq!(
            s.graph.requirements["req:t-1"]
                .source
                .as_ref()
                .unwrap()
                .section,
            "/t/basket"
        );
        assert_eq!(
            s.graph.requirements["req:t-2"]
                .source
                .as_ref()
                .unwrap()
                .quote,
            "A cart may hold up to sixty items at once."
        );
        // The mention derived from req:t-1's source followed the requirement.
        assert_eq!(s.graph.entities["ent:a"].mentions[0].section, "/t/basket");
        // The orphaned anchor's section is gone and no proposal names it any more:
        // the sweep behind the commit deletes the requirement, journaled, and the
        // report says so. Mirrors docs/compiler/graph.md#the-sweep.
        assert!(!s.graph.requirements.contains_key("req:t-3"));
        assert!(
            report.swept.iter().any(|a| a.contains("req:t-3")),
            "{:?}",
            report.swept
        );
        assert!(s.status.has_change(CHANGE_REQ_DELETED, "req:t-3"));
        assert_eq!(s.status.reevaluate, vec!["req:t-2".to_string()]);
        // A substantive quote change is a revision owed a pair judgment; the flagged
        // anchor is recorded stale on its new section.
        assert!(report.changed_requirements.contains("req:t-2"));
        assert!(s.status.has_change(CHANGE_REQ_REVISED, "req:t-2"));
        let stale = s
            .status
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_ANCHOR_STALE && c.subject == "t.md#/t/basket")
            .expect("anchor-stale");
        assert_eq!(stale.detail["anchors"], serde_json::json!(["req:t-2"]));

        // The next sync lists the flagged anchor as stale on its document even though
        // its quote locates; the orphan is gone, so it is nobody's stale anchor.
        let d3 = s.sync_docs(&parsed2);
        let t = d3.iter().find(|d| d.doc == "t.md").expect("item");
        assert!(t.stale_anchors.contains(&"req:t-2".to_string()));
        assert!(!t.stale_anchors.contains(&"req:t-3".to_string()));
        assert!(!t.stale_anchors.contains(&"req:t-1".to_string()));
        assert!(t.dirty_sections.contains(&"/t/basket".to_string()));

        // A commit that leaves the flagged anchor alone leaves the flag; resolving
        // the section's reconcile goal clears it.
        s.apply(vec![], &session());
        assert_eq!(s.status.reevaluate, vec!["req:t-2".to_string()]);
        let mut commit = session();
        commit.resolved.push(Resolved {
            goal: "g:reconcile-section:t.md#/t/basket".into(),
            justification: "re-evaluated".into(),
            evidence: serde_json::Value::Null,
        });
        s.apply(vec![], &commit);
        assert!(s.status.reevaluate.is_empty());
    }

    // Exact moves are journaled, and a heading level change is an exact move: nothing
    // dirty beyond the new heading, coverage carried.
    #[test]
    fn exact_moves_are_journaled_and_survive_a_level_change() {
        let mut s = Store {
            out: own_dir("align-journal"),
            ..Default::default()
        };
        let v1 = format!("# T\nintro\n\n## Cart\n{}\n", CART);
        let mut parsed = BTreeMap::new();
        parsed.insert(
            "t.md".to_string(),
            (hash_hex(&v1), crate::md::parse_sections(&v1)),
        );
        s.sync_docs(&parsed);
        s.graph.entities.insert("ent:a".into(), entity("A"));
        s.graph.requirements.insert(
            "req:t-1".into(),
            req_at("t.md", "/t/cart", "Items stay until checkout."),
        );
        s.docs.get_mut("t.md").unwrap().coverage.insert(
            "/t/cart".into(),
            Coverage {
                state: "covered".into(),
                note: None,
                claimed_by: None,
            },
        );
        let gen_before = s.status.generation;
        let v2 = format!("# T\nintro\n\n## Group\n\n### Cart\n{}\n", CART);
        let mut parsed2 = BTreeMap::new();
        parsed2.insert(
            "t.md".to_string(),
            (hash_hex(&v2), crate::md::parse_sections(&v2)),
        );
        let d2 = s.sync_docs(&parsed2);
        assert_eq!(d2[0].dirty_sections, vec!["/t/group".to_string()]);
        assert!(d2[0].stale_anchors.is_empty());
        assert_eq!(
            s.graph.requirements["req:t-1"]
                .source
                .as_ref()
                .unwrap()
                .section,
            "/t/group/cart"
        );
        assert!(s.docs["t.md"].coverage.contains_key("/t/group/cart"));
        // The edit entry, then the align entry.
        assert_eq!(s.status.generation, gen_before + 2);
        let edit: JournalEntry = yaml_to(
            &s.out
                .join("journal")
                .join(format!("g{}.yaml", gen_before + 1)),
        )
        .unwrap();
        assert_eq!(edit.kind, "edit");
        assert_eq!(edit.dirtied, vec!["t.md#/t/group".to_string()]);
        let entry: JournalEntry = yaml_to(
            &s.out
                .join("journal")
                .join(format!("g{}.yaml", gen_before + 2)),
        )
        .unwrap();
        assert_eq!(entry.kind, "align");
        assert!(entry.batch.is_empty());
        assert_eq!(entry.mutations[0]["from"], "t.md#/t/cart");
    }

    // An entity mention whose quote stopped locating in a changed section is a stale
    // anchor too, not only a requirement source.
    #[test]
    fn dirty_section_checks_entity_mention_quotes() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        let v1 = "# T\nintro\n\n## Cart\nThe cart holds items.\nA customer owns the cart.\n";
        let mut parsed = BTreeMap::new();
        parsed.insert(
            "t.md".to_string(),
            (hash_hex(v1), crate::md::parse_sections(v1)),
        );
        s.sync_docs(&parsed);
        s.graph.entities.insert(
            "ent:a".into(),
            Entity {
                name: "A".into(),
                mentions: vec![mention("t.md", "/t/cart", "A customer owns the cart.")],
                ..Default::default()
            },
        );
        let v2 =
            "# T\nintro\n\n## Cart\nThe cart holds items.\nNobody owns anything here at all.\n";
        let mut parsed2 = BTreeMap::new();
        parsed2.insert(
            "t.md".to_string(),
            (hash_hex(v2), crate::md::parse_sections(v2)),
        );
        let d2 = s.sync_docs(&parsed2);
        let all: Vec<String> = d2[0].stale_anchors.clone();
        let proposed: Vec<String> = s
            .status
            .alignment
            .iter()
            .flat_map(|b| b.proposals.iter().map(|p| p.anchor.clone()))
            .collect();
        assert!(
            all.contains(&"ent:a".to_string()) || proposed.contains(&"ent:a".to_string()),
            "stale {:?} proposed {:?}",
            all,
            proposed
        );
    }
}
