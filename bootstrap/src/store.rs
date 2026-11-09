use crate::vfs::VPathExt;
// The graph store: persistent home of the semantic graph. Owns identifiers, enforces
// invariants at commit, recomputes derived data, records every change, and writes the
// typed dirtiness each commit causes. Mirrors docs/compiler/graph.md.
use crate::md;
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
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
        // A wording the session takes back (docs/compiler/tools.md#write-tools).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        remove_aliases: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        add_mention: Option<SourceRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stereotype: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
        // Take the entity out of the containment tree (docs/compiler/tools.md#write-tools).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_parent: bool,
        // Requirement ids judged word matches by a review (entity.md#fields).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        not_about_add: Vec<String>,
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
    // Criteria (docs/compiler/model/criterion.md).
    CreateCriterion {
        id: String,
        criterion: Criterion,
    },
    UpdateCriterion {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        statement: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        measure: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        given: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        then: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entities: Option<Vec<String>>,
    },
    DeleteCriterion {
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
pub const CHECK_RULES: [&str; 34] = [
    "criterion-missing",
    "criterion-restates",
    "criteria-conflict",
    "failing-binding",
    "pinned-fact-drift",
    "unsupported-format",
    "parse-error",
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
pub const STORE_VERSION: u32 = 4;

// The one earlier version a load reads as it is: its next save writes the version 4
// layout, ids kept. Mirrors docs/compiler/graph.md#store-version.
pub const UPGRADABLE_VERSION: u32 = 3;

pub fn readable_version(v: u32) -> bool {
    v == STORE_VERSION || v == UPGRADABLE_VERSION
}

// Change record kinds. Mirrors docs/compiler/graph.md#change-records.
pub const CHANGE_SECTION_DIRTY: &str = "section-dirty";

// The most lines of each side a `section-dirty` record keeps.
pub const SECTION_DIFF_LINES: usize = 20;

// A line that counts in a section diff: not blank. Lines compare trimmed.
fn diff_lines(raw: &str) -> Vec<&str> {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect()
}

// The diff a `section-dirty` record carries: the previous hash, the counts, and the
// added and removed lines (a multiset difference of trimmed, non-blank lines), each list
// capped. Mirrors docs/compiler/goals/reconcile-section.md#created-when.
pub fn section_diff_detail(prev: &crate::model::Section, cur: &str) -> serde_json::Value {
    let one_side = |from: &str, minus: &str| -> Vec<String> {
        let mut pool: BTreeMap<&str, usize> = BTreeMap::new();
        for l in diff_lines(minus) {
            *pool.entry(l).or_default() += 1;
        }
        let mut out = Vec::new();
        for l in diff_lines(from) {
            match pool.get_mut(l) {
                Some(n) if *n > 0 => *n -= 1,
                _ => out.push(l.to_string()),
            }
        }
        out
    };
    let added = one_side(cur, &prev.raw);
    let removed = one_side(&prev.raw, cur);
    let cap = |v: &[String]| {
        v.iter()
            .take(SECTION_DIFF_LINES)
            .cloned()
            .collect::<Vec<_>>()
    };
    serde_json::json!({
        "previous": prev.hash,
        "added": added.len(),
        "removed": removed.len(),
        "addedLines": cap(&added),
        "removedLines": cap(&removed),
    })
}
pub const CHANGE_SECTION_REMOVED: &str = "section-removed";
pub const CHANGE_ANCHOR_STALE: &str = "anchor-stale";
pub const CHANGE_ALIGNMENT_PENDING: &str = "alignment-pending";
pub const CHANGE_REQ_CREATED: &str = "requirement-created";
pub const CHANGE_REQ_REVISED: &str = "requirement-revised";
pub const CHANGE_REQ_DELETED: &str = "requirement-deleted";
// Criteria (docs/compiler/graph.md#change-records).
pub const CHANGE_CRIT_CREATED: &str = "criterion-created";
pub const CHANGE_CRIT_REVISED: &str = "criterion-revised";
pub const CHANGE_CRIT_DELETED: &str = "criterion-deleted";
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

// The pair scorer's inputs over one graph state, computed once per pass: the hub,
// each entity's statement and token counts, and the requirements naming each entity
// (resolved), so a requirement's candidates are the union over its entities rather
// than a walk of every requirement. Mirrors docs/compiler/reconciler.md#pairs.
pub struct PairScoring {
    hub: Option<String>,
    counts: BTreeMap<String, (usize, BTreeMap<String, usize>)>,
    by_entity: BTreeMap<String, Vec<String>>,
    // Each requirement's content tokens, stemmed once.
    tokens: BTreeMap<String, BTreeSet<String>>,
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
    // None when an external agent drove the session: its spend is unknowable here
    // (docs/compiler/graph.md#journal).
    pub tokens: Option<u64>,
    // The cached input among `tokens`, and `agent` when the agent reported the figure
    // itself through report_spend (docs/compiler/tools.md#spend-tool).
    pub cached: Option<u64>,
    pub tokens_source: Option<String>,
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

    pub fn session(batch: Vec<String>, rounds: u32, tokens: Option<u64>) -> Commit {
        Commit {
            kind: "session".to_string(),
            batch,
            resolved: Vec::new(),
            rounds,
            tokens,
            cached: None,
            tokens_source: None,
            all_or_nothing: false,
        }
    }
}

// The change records one generation writes, numbered as they are pushed. One record
// per kind and subject (and limit, for threshold crossings): the first push wins.
pub struct RecordBatch {
    generation: u64,
    records: Vec<ChangeRecord>,
    // The highest index this generation already holds outside the batch: a second
    // batch in one generation numbers after it, so record ids stay unique.
    offset: usize,
}

impl RecordBatch {
    pub fn new(generation: u64) -> RecordBatch {
        RecordBatch {
            generation,
            records: Vec::new(),
            offset: 0,
        }
    }

    // A batch that continues numbering after the records of its generation already in
    // `existing`, for a generation that writes more than one batch.
    pub fn after(generation: u64, existing: &[ChangeRecord]) -> RecordBatch {
        let prefix = format!("c{}-", generation);
        let offset = existing
            .iter()
            .filter(|c| c.generation == generation)
            .filter_map(|c| c.id.strip_prefix(&prefix)?.parse::<usize>().ok())
            .max()
            .unwrap_or(0);
        RecordBatch {
            generation,
            records: Vec::new(),
            offset,
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
        let index = self.offset + self.records.len() + 1;
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
    // A text with the replaced run swapped for the new one, when the run locates in it
    // (whitespace-insensitively).
    pub fn splice(&self, text: &str) -> Option<String> {
        md::locate_bytes(text, &self.old_text)
            .map(|(b, e)| format!("{}{}{}", &text[..b], self.new_text, &text[e..]))
    }

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
        // The parser joins a section's lines with LF; a CRLF document holds the same
        // section with CRLF, and the edit keeps the document's line endings.
        let crlf = old_full.contains("\r\n");
        let eol = |t: &str| {
            if crlf {
                t.replace("\r\n", "\n").replace('\n', "\r\n")
            } else {
                t.to_string()
            }
        };
        // A document mixing line endings holds a section in neither uniform form; the
        // section then locates by its tokens, any whitespace between.
        let window = section_raw
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .and_then(|raw| {
                let uniform = eol(raw);
                old_full
                    .find(&uniform)
                    .map(|b| (b, b + uniform.len()))
                    .or_else(|| md::locate_bytes(old_full, raw))
            });
        let doc_new = eol(new_text);
        let full = if old_text.trim().is_empty() {
            let Some((_, end)) = window else {
                return Err(format!(
                    "section {}#{} drifted from the document on disk; compile first",
                    doc, section
                ));
            };
            format!(
                "{}{}{}{}",
                old_full[..end].trim_end_matches(['\r', '\n']),
                eol("\n\n"),
                doc_new.trim(),
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
            format!("{}{}{}", &old_full[..b], doc_new, &old_full[e..])
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

// Cheap generation read: one line of build.yaml, no node parsing. A checkout without
// build.yaml reads 0 here until its first commit writes the floor
// (docs/compiler/graph.md#the-generation).
pub fn read_generation(out: &Path) -> u64 {
    crate::vfs::read_to_string(out.join(BUILD_FILE))
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("generation:"))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|v| v.trim().parse::<u64>().ok())
        })
        .unwrap_or(0)
}

// build.yaml: the local half of the status, what one checkout's runs leave behind.
// Mirrors docs/compiler/graph.md#storage-layout and #committed-and-local-state.
pub const BUILD_FILE: &str = "build.yaml";

// The status fields that live in build.yaml, never in the committed status.yaml.
const LOCAL_STATUS_KEYS: [&str; 5] = ["generation", "verdict", "costs", "spent", "diagnostics"];

#[derive(Serialize, Deserialize, Default)]
struct BuildFile {
    #[serde(default)]
    generation: u64,
    #[serde(default, skip_serializing_if = "Verdict::is_empty")]
    verdict: Verdict,
    #[serde(default, skip_serializing_if = "Costs::is_empty")]
    costs: Costs,
    #[serde(default)]
    spent: Spent,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    diagnostics: BTreeMap<String, u64>,
    // Document path to the content hash of the source file at last reconcile.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    docs: BTreeMap<String, String>,
}

// The highest generation the committed status names: change records and lookalike
// verdicts. A load never starts below it, so ids minted from the generation never
// repeat ones a merged branch committed. Mirrors docs/compiler/graph.md#the-generation.
pub fn generation_floor(status: &Status) -> u64 {
    status
        .changes
        .iter()
        .map(|c| c.generation)
        .chain(status.lookalikes.iter().map(|l| l.generation))
        .max()
        .unwrap_or(0)
}

// The committed status.yaml: the status without its local fields and without the
// records, which live one per file. Mirrors docs/compiler/graph.md#storage-layout.
fn committed_status(status: &Status) -> serde_norway::Value {
    let mut v = serde_norway::to_value(status).unwrap_or(serde_norway::Value::Null);
    if let serde_norway::Value::Mapping(m) = &mut v {
        for k in LOCAL_STATUS_KEYS.iter().chain(STATUS_DIRS.iter()) {
            m.shift_remove(*k);
        }
    }
    v
}

// The generation on disk as a load computes it: build.yaml raised to the floor of
// status.yaml; None for a directory holding neither. Errors name a file that exists
// and does not parse.
fn disk_generation(out: &Path) -> Result<Option<u64>, String> {
    let local = yaml_read::<BuildFile>(&out.join(BUILD_FILE))?.map(|b| b.generation);
    let floor = match yaml_read::<Status>(&out.join("status.yaml"))? {
        Some(_) => {
            let mut errors = Vec::new();
            let status = read_status(out, &mut errors);
            if let Some(e) = errors.into_iter().next() {
                return Err(e);
            }
            Some(generation_floor(&status))
        }
        None => None,
    };
    Ok(match (local, floor) {
        (None, None) => None,
        (l, f) => Some(l.unwrap_or(0).max(f.unwrap_or(0))),
    })
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Store {
    pub out: PathBuf,
    pub graph: Graph,
    pub docs: BTreeMap<String, DocRecord>,
    pub status: Status,
    // Alignment thresholds, registry constants (docs/compiler/graph.md#budgets-and-thresholds).
    #[serde(skip)]
    pub align: crate::align::Thresholds,
    // Files that existed but did not parse at load, each with its error. Non-empty
    // makes the store read-only. Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
    pub load_errors: Vec<String>,
}

// The read indices of one derivation (docs/compiler/graph.md#derived-data). While a
// `ReadScope` borrows a store, nothing can change the store, so the whole-graph scans
// that a board derivation repeats per goal (`requirements_referencing`,
// `derive::instance_types`, `board::level_members`) answer from indices built on
// first use and dropped with the scope. Each index holds what the scan it replaces
// computes.
pub struct ReadScope<'a> {
    prev: Option<std::rc::Rc<ScopeIndex>>,
    _store: std::marker::PhantomData<&'a Store>,
}

#[derive(Default)]
pub(crate) struct ScopeIndex {
    store: usize,
    requirements_of: std::cell::OnceCell<BTreeMap<String, Vec<String>>>,
    instance_types: std::cell::OnceCell<BTreeMap<String, String>>,
    children: std::cell::OnceCell<BTreeMap<String, Vec<String>>>,
    scope_roots: std::cell::OnceCell<BTreeMap<String, Vec<String>>>,
}

thread_local! {
    static READ_SCOPE: std::cell::RefCell<Option<std::rc::Rc<ScopeIndex>>> =
        const { std::cell::RefCell::new(None) };
}

impl<'a> ReadScope<'a> {
    pub fn enter(store: &'a Store) -> ReadScope<'a> {
        let index = std::rc::Rc::new(ScopeIndex {
            store: store as *const Store as usize,
            ..Default::default()
        });
        let prev = READ_SCOPE.with(|s| s.borrow_mut().replace(index));
        ReadScope {
            prev,
            _store: std::marker::PhantomData,
        }
    }
}

impl Drop for ReadScope<'_> {
    fn drop(&mut self) {
        let prev = self.prev.take();
        READ_SCOPE.with(|s| *s.borrow_mut() = prev);
    }
}

impl ScopeIndex {
    // The index of the scope open over this very store, if one is.
    pub(crate) fn of(store: &Store) -> Option<std::rc::Rc<ScopeIndex>> {
        READ_SCOPE.with(|s| {
            s.borrow()
                .as_ref()
                .filter(|ix| ix.store == store as *const Store as usize)
                .cloned()
        })
    }

    // Entity id (redirects followed) to the requirements naming it, by id.
    pub(crate) fn requirements_of(&self, store: &Store) -> &BTreeMap<String, Vec<String>> {
        self.requirements_of.get_or_init(|| {
            let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (rid, r) in &store.graph.requirements {
                for e in &r.entities {
                    let list = out.entry(store.resolve_id(e).to_string()).or_default();
                    if list.last() != Some(rid) {
                        list.push(rid.clone());
                    }
                }
            }
            out
        })
    }

    pub(crate) fn instance_types(
        &self,
        compute: impl FnOnce() -> BTreeMap<String, String>,
    ) -> &BTreeMap<String, String> {
        self.instance_types.get_or_init(compute)
    }

    // Parent id to its direct children, by id.
    pub(crate) fn children(&self, store: &Store) -> &BTreeMap<String, Vec<String>> {
        self.children.get_or_init(|| {
            let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (id, e) in &store.graph.entities {
                if let Some(p) = &e.parent {
                    out.entry(p.clone()).or_default().push(id.clone());
                }
            }
            out
        })
    }

    // Scope to its parentless entities, by id.
    pub(crate) fn scope_roots(&self, store: &Store) -> &BTreeMap<String, Vec<String>> {
        self.scope_roots.get_or_init(|| {
            let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (id, e) in &store.graph.entities {
                if e.parent.is_none() {
                    out.entry(e.scope.clone()).or_default().push(id.clone());
                }
            }
            out
        })
    }
}

pub(crate) fn normalize(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

// The function words pair selection ignores: articles, prepositions, pronouns,
// auxiliaries, quantifiers. Shared by the store's scoring and the hints' word lists.
// Mirrors docs/compiler/reconciler.md#pairs.
pub(crate) const CONTENT_STOP: [&str; 113] = [
    "the", "a", "an", "shall", "to", "of", "in", "on", "for", "is", "are", "be", "or", "and", "if",
    "with", "by", "it", "its", "when", "then", "that", "which", "this", "system", "not", "no",
    "only", "all", "each", "at", "as", "from", "into", "than", "has", "have", "had", "having",
    "their", "they", "them", "there", "these", "those", "once", "need", "needs", "must", "may",
    "can", "will", "would", "should", "could", "do", "does", "did", "done", "any", "some", "one",
    "two", "per", "before", "after", "within", "without", "every", "more", "less", "so", "up",
    "down", "out", "over", "under", "about", "also", "both", "either", "other", "such", "own",
    "same", "here", "where", "how", "what", "while", "until", "since", "because", "but", "however",
    "yet", "who", "whose", "whom", "was", "were", "been", "being", "get", "gets", "got", "make",
    "makes", "made", "use", "uses", "used", "via",
];

// A content token: not a stop word, and three letters or more unless it is a number.
// A token is an entity's boilerplate from this many statements naming the entity
// (and at least half of them). Mirrors docs/compiler/reconciler.md#pairs.
const BOILERPLATE_MIN: usize = 4;

// The one stemmer, in two stages. The suffix stage drops one of "ing", "ed", "s"
// from a word long enough to keep three letters: the vocabulary of the goal hints and
// the lookalike name tokens, whose stems ride in change records and remembered
// lookalike scores. Mirrors docs/compiler/reconciler.md#pairs.
pub(crate) fn suffix_stem(t: &str) -> String {
    for suffix in ["ing", "ed", "s"] {
        if t.len() > suffix.len() + 2 && t.ends_with(suffix) {
            return t[..t.len() - suffix.len()].to_string();
        }
    }
    t.to_string()
}

// The pair scorer's stem: the suffix stage, then a trailing "e", so "files" and
// "filed" both land on "fil" and "reverses" meets "reversed": the one stem per word
// the pairing rules count on.
pub(crate) fn content_stem(t: &str) -> String {
    let mut s = suffix_stem(t);
    if s.len() > 3 && s.ends_with('e') {
        s.pop();
    }
    s
}

pub(crate) fn is_content_token(t: &str) -> bool {
    !t.is_empty()
        && !CONTENT_STOP.contains(&t)
        && (t.len() >= 3 || t.chars().all(|c| c.is_ascii_digit()))
}

// Punctuation-insensitive statement normalization: the requirement natural key. A comma
// or spacing edit to a sentence keeps matching its existing requirement.
// The punctuation-insensitive form of a statement (graph.md#mutations): case,
// spacing, and punctuation fall away, but a comparison operator, a sign, and a
// decimal point stay, since `x > 30` and `x < 30` are different facts.
pub(crate) fn normalize_statement(s: &str) -> String {
    let chars: Vec<char> = s.to_lowercase().chars().collect();
    let mut tokens: Vec<String> = Vec::new();
    let mut tok = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let prev = i.checked_sub(1).map(|j| chars[j]);
        let next = chars.get(i + 1).copied();
        let digit = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit());
        if c.is_alphanumeric() || (c == '.' && digit(prev) && digit(next)) {
            tok.push(c);
            continue;
        }
        // A sign is a minus right before a digit that does not follow a word (`-1`,
        // not `2024-09` or `read-only`).
        let sign = c == '-' && digit(next) && !prev.is_some_and(|p| p.is_alphanumeric());
        if !tok.is_empty() {
            tokens.push(std::mem::take(&mut tok));
        }
        if sign {
            tok.push('-');
        } else if "<>=!≤≥≠+*/%".contains(c) {
            tokens.push(c.to_string());
        }
    }
    if !tok.is_empty() {
        tokens.push(tok);
    }
    tokens.join(" ")
}

// Same fact reworded: one statement's content tokens contained in the other's. A
// resumed build often re-extracts "X" as "X using Y" from the same sentence; that is
// one fact, not two. The loose form serves the stale-anchor path, where the dead quote
// is the signal that a statement is being re-recorded.
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

// The tight form for two statements quoting the same live sentence: two facts from one
// sentence share most of their words too ("An Order is paid within 21 days of
// placement" inside "When an Order is not paid within 21 days of placement, Orderly
// cancels it"), so a rewording may add at most two tokens and no negation.
// Mirrors docs/compiler/tools.md#write-tools.
// The obligation modals are set aside and an `-s` or `-ies` ending reduced, so "empties"
// restates "shall empty"; `can`, `should`, and `may` change what is required and count.
pub(crate) fn statement_rewords(a: &str, b: &str) -> bool {
    const NEGATIONS: [&str; 12] = [
        "not",
        "never",
        "no",
        "unless",
        "except",
        "otherwise",
        "cannot",
        // A permission or a recommendation is not the obligation.
        "can",
        "may",
        "should",
        "might",
        "could",
    ];
    let toks = |s: &str| -> std::collections::BTreeSet<String> {
        normalize_statement(s)
            .split(' ')
            .filter(|w| !["shall", "must", "will"].contains(w))
            .map(|w| {
                if w.len() > 4 && w.ends_with("ies") {
                    format!("{}y", &w[..w.len() - 3])
                } else if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") {
                    w[..w.len() - 1].to_string()
                } else {
                    w.to_string()
                }
            })
            .collect()
    };
    let (ta, tb) = (toks(a), toks(b));
    if ta.is_empty() || tb.is_empty() {
        return false;
    }
    let (small, large) = if ta.len() <= tb.len() {
        (&ta, &tb)
    } else {
        (&tb, &ta)
    };
    small.is_subset(large)
        && large.len() - small.len() <= 2
        && !large
            .difference(small)
            .any(|t| NEGATIONS.contains(&t.as_str()))
}

// Whitespace-insensitive containment: a quote wrapped across source lines still locates.
pub fn text_contains(hay: &str, needle: &str) -> bool {
    let h = hay.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = needle.split_whitespace().collect::<Vec<_>>().join(" ");
    !n.is_empty() && h.contains(&n)
}

fn yaml_to<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    yaml_read(path).ok().flatten()
}

// A missing file reads as `None`; a file that exists but does not parse is an error
// naming it, never an empty value.
// Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
fn yaml_read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let text = match crate::vfs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {}", path.display(), e)),
    };
    if text.trim().is_empty() {
        return Err(format!("{}: empty file", path.display()));
    }
    serde_norway::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{}: {}", path.display(), e))
}

// A shard read at load: missing reads as empty, unparseable is recorded and reads as
// empty for this load only (the store refuses to write while `errors` is non-empty).
fn shard<T: serde::de::DeserializeOwned + Default>(path: &Path, errors: &mut Vec<String>) -> T {
    match yaml_read::<T>(path) {
        Ok(Some(v)) => v,
        Ok(None) => T::default(),
        Err(e) => {
            errors.push(e);
            T::default()
        }
    }
}

// ---- node files ----
// One file per node under graph/<kind>/, named by its file id.
// Mirrors docs/compiler/graph.md#node-files.

// The node kinds a repository commits, as directory names under graph/.
pub const COMMITTED_KINDS: [&str; 6] = [
    "entities",
    "requirements",
    "criteria",
    "views",
    "diagnostics",
    "redirects",
];

// The derived kinds: local, recomputed at load when missing and on every commit.
pub const DERIVED_KINDS: [&str; 3] = ["default-views", "relationships", "state-machines"];

// The file id of a node id: `:` as `~`, `/` as `+`, lowercase ASCII letters, digits,
// `-`, `_`, `.` kept, every other byte (and a leading `.`) as `%XX` in uppercase hex.
// Reversible, and no two ids fold into one name on a case-insensitive file system.
pub fn file_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for (i, b) in id.bytes().enumerate() {
        match b {
            b':' => out.push('~'),
            b'/' => out.push('+'),
            b'.' if i == 0 => out.push_str("%2E"),
            b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// The node id a file id spells; None for a name `file_id` never writes.
pub fn id_from_file(stem: &str) -> Option<String> {
    let bytes = stem.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'~' => out.push(b':'),
            b'+' => out.push(b'/'),
            b'%' => {
                let hex = stem.get(i + 1..i + 3)?;
                if !hex
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
                {
                    return None;
                }
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            b @ (b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.') => out.push(b),
            _ => return None,
        }
        i += 1;
    }
    let id = String::from_utf8(out).ok()?;
    (!id.is_empty() && file_id(&id) == stem).then_some(id)
}

// The longest file id written whole; a longer one is shortened to a prefix, `%~`, and
// a hash, and its file carries the full id under `id`.
pub const FILE_ID_MAX: usize = 180;
const FILE_ID_PREFIX: usize = 160;

// 64-bit FNV-1a: fixed by the docs, so a name never changes between builds.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// The file stem of a node: its file id, or the shortened form past FILE_ID_MAX.
// The second value says whether the file must carry its id.
// Mirrors docs/compiler/graph.md#node-files.
pub fn node_stem(id: &str) -> (String, bool) {
    let full = file_id(id);
    if full.len() <= FILE_ID_MAX {
        return (full, false);
    }
    let b = full.as_bytes();
    let mut cut = FILE_ID_PREFIX;
    if b[cut - 1] == b'%' {
        cut -= 1;
    } else if b[cut - 2] == b'%' {
        cut -= 2;
    }
    (
        format!("{}%~{:016x}", &full[..cut], fnv1a64(id.as_bytes())),
        true,
    )
}

// The content of one redirect file.
#[derive(Clone, Serialize, Deserialize)]
struct RedirectFile {
    to: String,
}

// The id a shortened file carries, read from its content.
fn carried_id(path: &Path) -> Result<String, String> {
    let v: serde_norway::Value =
        yaml_read(path)?.ok_or_else(|| format!("{}: missing", path.display()))?;
    v.get("id")
        .and_then(|i| i.as_str())
        .map(String::from)
        .ok_or_else(|| format!("{}: a shortened file name carries no id", path.display()))
}

// The node files of one kind directory, `(id, path)` in id order. A plain name decodes
// to its id; a shortened one (`%~`) carries it inside and must match its own name. Any
// other name is an error naming the file. Temporary and hidden files are skipped.
fn node_files(dir: &Path, errors: &mut Vec<String>) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(rd) = crate::vfs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Some(stem) = name.strip_suffix(".yaml") else {
            continue;
        };
        if let Some(id) = id_from_file(stem) {
            out.push((id, e.path()));
            continue;
        }
        if stem.contains("%~") {
            match carried_id(&e.path()) {
                Ok(id) if node_stem(&id).0 == stem => out.push((id, e.path())),
                Ok(id) => errors.push(format!(
                    "{}: carries id {} whose file name differs",
                    e.path().display(),
                    id
                )),
                Err(err) => errors.push(err),
            }
            continue;
        }
        errors.push(format!(
            "{}: the file name spells no node id",
            e.path().display()
        ));
    }
    out.sort();
    out
}

// Every node of one kind directory. Missing reads as empty; a file that does not parse
// is recorded in `errors` (the store then refuses to write). A file starts with the id
// it is named by; one whose `id` names another node is an error too.
// Mirrors docs/compiler/graph.md#node-files.
fn load_nodes<T: serde::de::DeserializeOwned>(
    dir: &Path,
    errors: &mut Vec<String>,
) -> BTreeMap<String, T> {
    let mut nodes = BTreeMap::new();
    for (id, path) in node_files(dir, errors) {
        let read = yaml_read::<serde_norway::Value>(&path).and_then(|v| {
            let Some(mut v) = v else {
                return Ok(None);
            };
            if let serde_norway::Value::Mapping(m) = &mut v {
                if let Some(carried) = m.remove("id") {
                    if carried.as_str() != Some(id.as_str()) {
                        return Err(format!(
                            "{}: carries id {} but its file name spells {}",
                            path.display(),
                            carried.as_str().unwrap_or("?"),
                            id
                        ));
                    }
                }
            }
            serde_norway::from_value::<T>(v)
                .map(Some)
                .map_err(|e| format!("{}: {}", path.display(), e))
        });
        match read {
            Ok(Some(v)) => {
                nodes.insert(id, v);
            }
            Ok(None) => {}
            Err(e) => errors.push(e),
        }
    }
    nodes
}

// The nodes of one kind directory as JSON values, for readers outside the store
// (replays seed from a snapshot's graph).
pub fn read_node_values(dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    let mut errors = Vec::new();
    load_nodes::<serde_json::Value>(dir, &mut errors)
        .into_iter()
        .collect()
}

// A node as its file holds it: `id` first, then its own fields, the generation markers
// taken out into `markers`. Mirrors docs/compiler/graph.md#node-files.
fn node_value<T: Serialize>(id: &str, node: &T, markers: &mut Markers) -> serde_norway::Value {
    let mut m = serde_norway::Mapping::new();
    m.insert("id".into(), id.into());
    if let Ok(serde_norway::Value::Mapping(fields)) = serde_norway::to_value(node) {
        let mut fields = fields;
        let created = fields
            .shift_remove("created")
            .and_then(|v| v.as_str().map(String::from));
        let updated = fields
            .shift_remove("updated")
            .and_then(|v| v.as_str().map(String::from));
        if created.is_some() || updated.is_some() {
            markers.insert(id.to_string(), (created, updated));
        }
        m.extend(fields);
    }
    serde_norway::Value::Mapping(m)
}

// The generation markers a node file leaves to `markers.yaml`.
trait Marked {
    fn marks(&self) -> (Option<&String>, Option<&String>) {
        (None, None)
    }
}

macro_rules! marked {
    ($($t:ty),*) => {$(
        impl Marked for $t {
            fn marks(&self) -> (Option<&String>, Option<&String>) {
                (self.created.as_ref(), self.updated.as_ref())
            }
        }
    )*};
}
marked!(Entity, Requirement, Criterion, View, Diagnostic);
impl Marked for Relationship {}
impl Marked for StateMachine {}
impl Marked for RedirectFile {}

// Write every node of one kind whose bytes changed; return the files of nodes that
// left the graph, for the caller to delete after every write landed. The markers are
// taken out as `node_value` takes them.
fn save_kind<'a, T: Serialize + Marked + Clone + 'a>(
    dir: &Path,
    nodes: impl Iterator<Item = (&'a String, std::borrow::Cow<'a, T>)>,
    markers: &mut Markers,
) -> Vec<PathBuf> {
    let mut ids: BTreeSet<&String> = BTreeSet::new();
    for (id, node) in nodes {
        let (created, updated) = node.marks();
        if created.is_some() || updated.is_some() {
            markers.insert(id.clone(), (created.cloned(), updated.cloned()));
        }
        let path = dir.join(format!("{}.yaml", node_stem(id).0));
        write_tracked(&path, id, node.as_ref(), || {
            node_value(id, node.as_ref(), &mut Markers::new())
        });
        ids.insert(id);
    }
    node_files(dir, &mut Vec::new())
        .into_iter()
        .filter(|(id, _)| !ids.contains(id))
        .map(|(_, path)| path)
        .collect()
}

fn borrowed<'a, T: Clone>((id, n): (&'a String, &'a T)) -> (&'a String, Cow<'a, T>) {
    (id, Cow::Borrowed(n))
}

// Write a node file or doc record through `write_yaml`, unless the file still holds
// what this process last wrote or verified there from a value with the same
// fingerprint: then its bytes already equal the serialized value, and the save skips
// serializing it. Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
fn write_tracked<T: Serialize, V: Serialize>(
    path: &Path,
    key: &str,
    source: &T,
    value: impl FnOnce() -> V,
) {
    let fp = written::fingerprint(key, source);
    if fp.is_some_and(|fp| written::holds(path, fp)) {
        return;
    }
    #[cfg(test)]
    SERIALIZED.with(|n| n.set(n.get() + 1));
    let r = write_yaml(path, &value());
    match (&r, fp) {
        (Ok(()), Some(fp)) => written::remember(path, fp),
        _ => written::forget(path),
    }
    report_write(path, r);
}

// The node files and doc records this thread serialized, for the tests to count.
#[cfg(test)]
thread_local! {
    static SERIALIZED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// What this process last wrote to or found in each node file and doc record: the
// fingerprint of the value the bytes serialize, and the file's identity (length,
// modification time, inode) then. Any other write to the file replaces its inode or
// moves its time, so a file whose identity still matches holds those bytes. An
// in-memory tree keeps none: its reads cost no disk.
mod written {
    use serde::Serialize;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{LazyLock, Mutex};

    #[derive(Clone, Copy, PartialEq)]
    struct Identity {
        len: u64,
        modified: std::time::SystemTime,
        inode: u64,
    }

    static SEEN: LazyLock<Mutex<HashMap<PathBuf, (u128, Identity)>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    fn tracked() -> bool {
        cfg!(not(target_arch = "wasm32")) && !crate::vfs::is_scoped()
    }

    fn identity(path: &Path) -> Option<Identity> {
        let m = std::fs::metadata(path).ok()?;
        #[cfg(unix)]
        let inode = std::os::unix::fs::MetadataExt::ino(&m);
        #[cfg(not(unix))]
        let inode = 0;
        Some(Identity {
            len: m.len(),
            modified: m.modified().ok()?,
            inode,
        })
    }

    struct HashWriter(siphasher::sip128::SipHasher13);
    impl std::io::Write for HashWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            std::hash::Hasher::write(&mut self.0, buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // A 128-bit hash of the key and the value's JSON form: equal fingerprints mean
    // equal values, so equal serialized bytes.
    pub(super) fn fingerprint<T: Serialize>(key: &str, value: &T) -> Option<u128> {
        use siphasher::sip128::Hasher128;
        if !tracked() {
            return None;
        }
        let mut w = HashWriter(siphasher::sip128::SipHasher13::new());
        std::hash::Hasher::write(&mut w.0, key.as_bytes());
        std::hash::Hasher::write_u8(&mut w.0, 0);
        serde_json::to_writer(&mut w, value).ok()?;
        Some(w.0.finish128().as_u128())
    }

    pub(super) fn holds(path: &Path, fp: u128) -> bool {
        let Some(seen) = SEEN.lock().unwrap().get(path).copied() else {
            return false;
        };
        seen.0 == fp && identity(path) == Some(seen.1)
    }

    pub(super) fn remember(path: &Path, fp: u128) {
        match identity(path) {
            Some(id) => {
                SEEN.lock().unwrap().insert(path.to_path_buf(), (fp, id));
            }
            None => forget(path),
        }
    }

    pub(super) fn forget(path: &Path) {
        if tracked() {
            SEEN.lock().unwrap().remove(path);
        }
    }
}

// The local generation markers: node id to `[created, updated]`.
// Mirrors docs/compiler/graph.md#storage-layout.
pub const MARKERS_FILE: &str = "markers.yaml";
type Markers = BTreeMap<String, (Option<String>, Option<String>)>;

// The local causes of the open change records: `<kind> <subject>` to the id,
// generation, and mutation of each record in that file, in file order.
// Mirrors docs/compiler/graph.md#storage-layout.
pub const CAUSES_FILE: &str = "causes.yaml";
type Causes = BTreeMap<String, Vec<(String, u64, usize)>>;

fn cause_key(kind: &str, subject: &str) -> String {
    format!("{} {}", kind, subject)
}

// A change record as its file holds it: no local cause fields.
fn record_file_value(r: &ChangeRecord) -> serde_norway::Value {
    let mut v = serde_norway::to_value(r).unwrap_or(serde_norway::Value::Null);
    if let serde_norway::Value::Mapping(m) = &mut v {
        for k in ["id", "generation", "mutation"] {
            m.shift_remove(k);
        }
    }
    v
}

// The status record directories, committed. Mirrors docs/compiler/graph.md#storage-layout.
pub const STATUS_DIRS: [&str; 5] = ["changes", "parked", "failed", "alignment", "lookalikes"];

// One record file: `id` first, then the record's fields.
fn record_value<T: Serialize>(id: &str, record: &T) -> serde_norway::Value {
    let mut m = serde_norway::Mapping::new();
    m.insert("id".into(), id.into());
    if let Ok(serde_norway::Value::Mapping(fields)) = serde_norway::to_value(record) {
        for (k, v) in fields {
            if k.as_str() != Some("id") {
                m.insert(k, v);
            }
        }
    }
    serde_norway::Value::Mapping(m)
}

// Every YAML file under a directory, recursively, in path order.
fn yaml_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_yaml(dir, &mut out);
    out.retain(|p| {
        !p.file_name()
            .map(|n| n.to_string_lossy().starts_with('.'))
            .unwrap_or(true)
    });
    out.sort();
    out
}

// The status as the disk holds it: status.yaml, then every record file. A version 3
// status.yaml carries the records inline; both forms read into one status.
// Mirrors docs/compiler/graph.md#storage-layout and #store-version.
fn read_status(out: &Path, errors: &mut Vec<String>) -> Status {
    let mut status: Status = shard(&out.join("status.yaml"), errors);
    let causes: Causes = shard(&out.join(CAUSES_FILE), errors);
    let mut changes: Vec<ChangeRecord> = Vec::new();
    for p in yaml_files(&out.join("changes")) {
        let items = match yaml_read::<serde_norway::Value>(&p) {
            Ok(Some(serde_norway::Value::Sequence(items))) => items,
            Ok(Some(v)) => vec![v],
            Ok(None) => Vec::new(),
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let mut file: Vec<ChangeRecord> = Vec::new();
        for item in items {
            match serde_norway::from_value::<ChangeRecord>(item) {
                Ok(r) => file.push(r),
                Err(e) => errors.push(format!("{}: {}", p.display(), e)),
            }
        }
        // The cause is local: a record another checkout wrote reads with generation 0.
        let local = file
            .first()
            .and_then(|r| causes.get(&cause_key(&r.kind, &r.subject)))
            .filter(|c| c.len() == file.len());
        for (i, r) in file.iter_mut().enumerate() {
            match local {
                Some(c) => {
                    r.id = c[i].0.clone();
                    r.generation = c[i].1;
                    r.mutation = c[i].2;
                }
                None => {
                    r.id = format!("c0-{}", crate::ids::hash(&cause_key(&r.kind, &r.subject)));
                    r.generation = 0;
                    r.mutation = 0;
                }
            }
        }
        changes.extend(file);
    }
    // Records in the order they were written: by generation, then by file.
    changes.sort_by_key(|r| r.generation);
    status.changes.extend(changes);
    fn records<T: serde::de::DeserializeOwned>(dir: &Path, errors: &mut Vec<String>) -> Vec<T> {
        let mut out = Vec::new();
        for p in yaml_files(dir) {
            let read = yaml_read::<serde_norway::Value>(&p).and_then(|v| {
                let Some(mut v) = v else { return Ok(None) };
                if let serde_norway::Value::Mapping(m) = &mut v {
                    // The parked form carries its own id; the others are named by it.
                    if !dir.ends_with("parked") {
                        m.remove("id");
                    }
                }
                serde_norway::from_value::<T>(v)
                    .map(Some)
                    .map_err(|e| format!("{}: {}", p.display(), e))
            });
            match read {
                Ok(Some(r)) => out.push(r),
                Ok(None) => {}
                Err(e) => errors.push(e),
            }
        }
        out
    }
    status
        .parked
        .extend(records::<ParkedGoal>(&out.join("parked"), errors));
    status
        .failed
        .extend(records::<FailedGoal>(&out.join("failed"), errors));
    status
        .alignment
        .extend(records::<DocAlignment>(&out.join("alignment"), errors));
    status
        .lookalikes
        .extend(records::<LookalikeVerdict>(&out.join("lookalikes"), errors));
    status
}

// Write one file per status record and delete the files of records that cleared.
// Mirrors docs/compiler/graph.md#storage-layout.
fn save_status_records(out: &Path, status: &Status) {
    let mut written: BTreeSet<PathBuf> = BTreeSet::new();
    let mut put = |path: PathBuf, value: serde_norway::Value| {
        report_write(&path, write_yaml(&path, &value));
        written.insert(path);
    };
    let mut by_subject: BTreeMap<(String, String), Vec<&ChangeRecord>> = BTreeMap::new();
    for r in &status.changes {
        by_subject
            .entry((r.kind.clone(), r.subject.clone()))
            .or_default()
            .push(r);
    }
    let mut causes = Causes::new();
    for ((kind, subject), rs) in by_subject {
        let path = out
            .join("changes")
            .join(file_id(&kind))
            .join(format!("{}.yaml", node_stem(&subject).0));
        let value = if rs.len() == 1 {
            record_file_value(rs[0])
        } else {
            serde_norway::Value::Sequence(rs.iter().map(|r| record_file_value(r)).collect())
        };
        causes.insert(
            cause_key(&kind, &subject),
            rs.iter()
                .map(|r| (r.id.clone(), r.generation, r.mutation))
                .collect(),
        );
        put(path, value);
    }
    let causes_path = out.join(CAUSES_FILE);
    report_write(&causes_path, write_yaml(&causes_path, &causes));
    let name = |dir: &str, id: &str| out.join(dir).join(format!("{}.yaml", node_stem(id).0));
    for p in &status.parked {
        put(
            name("parked", &p.goal.id),
            serde_norway::to_value(p).unwrap_or(serde_norway::Value::Null),
        );
    }
    for f in &status.failed {
        put(name("failed", &f.goal.id), record_value(&f.goal.id, f));
    }
    for a in &status.alignment {
        put(name("alignment", &a.doc), record_value(&a.doc, a));
    }
    for l in &status.lookalikes {
        put(name("lookalikes", &l.pair), record_value(&l.pair, l));
    }
    for dir in STATUS_DIRS {
        for p in yaml_files(&out.join(dir)) {
            if !written.contains(&p) {
                if let Err(e) = crate::vfs::remove_file(&p) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        eprintln!("[jazyk] error: cannot remove {}: {}", p.display(), e);
                    }
                }
            }
        }
    }
}

// A document record as its file holds it: no whole-file hash, no line numbers.
// Mirrors docs/compiler/graph.md#storage-layout.
fn doc_record_value(rec: &DocRecord) -> serde_norway::Value {
    let mut v = serde_norway::to_value(rec).unwrap_or(serde_norway::Value::Null);
    if let serde_norway::Value::Mapping(m) = &mut v {
        m.shift_remove("contentHash");
        if let Some(serde_norway::Value::Mapping(sections)) = m.get_mut("sections") {
            for (_, sec) in sections.iter_mut() {
                if let serde_norway::Value::Mapping(sm) = sec {
                    sm.shift_remove("lines");
                }
            }
        }
    }
    v
}

// Recompute each section's line range from the raw texts in document order: the
// heading kinds tile the document in tree order, a fenced block sits inside its holder.
// A record that still carries its lines (version 3) keeps them.
// Mirrors docs/compiler/graph.md#storage-layout and docs/compiler/parsing.md#section-tree.
pub fn fill_section_lines(rec: &mut DocRecord) {
    let holder = |k: &str| matches!(k, "preamble" | "root" | "heading");
    if rec.sections.values().all(|s| s.lines[1] > 0) {
        return;
    }
    let mut children: BTreeMap<Option<String>, Vec<(usize, bool, String)>> = BTreeMap::new();
    for (r, s) in &rec.sections {
        if holder(&s.kind) {
            children.entry(s.parent.clone()).or_default().push((
                s.order,
                s.kind != "preamble",
                r.clone(),
            ));
        }
    }
    for list in children.values_mut() {
        list.sort_by_key(|a| (a.1, a.0));
    }
    let mut order: Vec<String> = Vec::new();
    let mut stack: Vec<String> = children
        .get(&None)
        .map(|l| l.iter().rev().map(|(_, _, r)| r.clone()).collect())
        .unwrap_or_default();
    while let Some(r) = stack.pop() {
        if let Some(kids) = children.get(&Some(r.clone())) {
            for (_, _, k) in kids.iter().rev() {
                stack.push(k.clone());
            }
        }
        order.push(r);
    }
    let mut line = 0usize;
    let mut starts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &order {
        if let Some(s) = rec.sections.get_mut(r) {
            let n = s.raw.split('\n').count();
            s.lines = [line, line + n];
            starts.insert(r.clone(), line);
            line += n;
        }
    }
    let blocks: Vec<String> = rec
        .sections
        .iter()
        .filter(|(_, s)| !holder(&s.kind))
        .map(|(r, _)| r.clone())
        .collect();
    for r in blocks {
        let (parent, raw) = {
            let s = &rec.sections[&r];
            (s.parent.clone(), s.raw.clone())
        };
        let Some(p) =
            parent.and_then(|p| rec.sections.get(&p).map(|ps| (ps.lines[0], ps.raw.clone())))
        else {
            continue;
        };
        let at =
            p.1.find(&raw)
                .map(|i| p.1[..i].matches('\n').count())
                .unwrap_or(0);
        let n = raw.split('\n').count();
        if let Some(s) = rec.sections.get_mut(&r) {
            s.lines = [p.0 + at, p.0 + at + n];
        }
    }
}

// Copy a directory tree, for snapshots.
fn copy_tree(from: &Path, to: &Path) {
    let Ok(rd) = crate::vfs::read_dir(from) else {
        return;
    };
    if crate::vfs::create_dir_all(to).is_err() {
        return;
    }
    for e in rd.flatten() {
        let p = e.path();
        let target = to.join(e.file_name());
        if p.vis_dir() {
            copy_tree(&p, &target);
        } else if p.extension().and_then(|x| x.to_str()) == Some("yaml") {
            crate::vfs::copy(&p, &target).ok();
        }
    }
}

// A file whose bytes already match is left in place, so a save rewrites only what
// changed. The bytes end in one newline. Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
fn write_yaml<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let mut text = serde_norway::to_string(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if crate::vfs::read(path).is_ok_and(|on_disk| on_disk == text.as_bytes()) {
        return Ok(());
    }
    write_atomic(path, text.as_bytes())
}

// Write through a sibling temporary file renamed into place, so a reader or a crash
// never sees a half-written file. Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        crate::vfs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{}.{}.tmp", name, crate::sys::process_id()));
    write_durable(&tmp, bytes)?;
    // Native executable and private files retain their mode across replacement.
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        if let Ok(metadata) = std::fs::metadata(path) {
            if let Err(e) = std::fs::set_permissions(&tmp, metadata.permissions()) {
                crate::vfs::remove_file(&tmp).ok();
                return Err(e);
            }
        }
    }
    if let Err(e) = crate::vfs::rename(&tmp, path) {
        crate::vfs::remove_file(&tmp).ok();
        return Err(e);
    }
    if let Some(parent) = path.parent() {
        TOUCHED_DIRS.with(|t| t.borrow_mut().insert(parent.to_path_buf()));
    }
    Ok(())
}

// The directories renames landed in since the last flush.
thread_local! {
    static TOUCHED_DIRS: std::cell::RefCell<BTreeSet<PathBuf>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

// A temporary file's bytes reach the disk before the rename publishes them, so a crash
// never leaves a renamed file empty. An in-memory tree has no disk.
// Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
fn write_durable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        use std::io::Write;
        let mut f = std::fs::File::create(path)?;
        let written = f.write_all(bytes).and_then(|()| sync_file(&f));
        if let Err(e) = written {
            drop(f);
            std::fs::remove_file(path).ok();
            return Err(e);
        }
        return Ok(());
    }
    crate::vfs::write(path, bytes)
}

// fsync. On Apple platforms `sync_all` issues F_FULLFSYNC, a drive cache flush per
// file; plain fsync gives the same ordering against a crash of the system.
#[cfg(not(target_arch = "wasm32"))]
fn sync_file(f: &std::fs::File) -> std::io::Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::fsync(f.as_raw_fd()) } == 0 {
            return Ok(());
        }
        Err(std::io::Error::last_os_error())
    }
    #[cfg(not(target_vendor = "apple"))]
    f.sync_all()
}

// Flush the directories renames landed in, so the renames themselves survive a crash.
// Windows has no directory handle to flush; an in-memory tree has no disk.
fn sync_touched_dirs() {
    let dirs = TOUCHED_DIRS.with(|t| std::mem::take(&mut *t.borrow_mut()));
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    if !crate::vfs::is_scoped() {
        for d in dirs {
            if let Ok(f) = std::fs::File::open(&d) {
                if let Err(e) = sync_file(&f) {
                    eprintln!("[jazyk] error: cannot flush {}: {}", d.display(), e);
                }
            }
        }
    }
    #[cfg(not(all(unix, not(target_arch = "wasm32"))))]
    drop(dirs);
}

// The stored record of one document, as the last build left it; None when the
// document never reconciled or its record does not read.
pub fn stored_doc(out: &Path, rel: &str) -> Option<DocRecord> {
    let mut rec: DocRecord = yaml_to(&out.join("docs").join(format!("{}.yaml", rel)))?;
    if let Ok(Some(build)) = yaml_read::<BuildFile>(&out.join(BUILD_FILE)) {
        if let Some(h) = build.docs.get(rel) {
            rec.content_hash = h.clone();
        }
    }
    fill_section_lines(&mut rec);
    Some(rec)
}

// The sources of the requirements naming each entity: the mentions a commit adds by
// reference. Mirrors docs/compiler/graph.md#derived-data.
fn reference_sources(graph: &Graph) -> BTreeMap<String, Vec<SourceRef>> {
    let mut out: BTreeMap<String, Vec<SourceRef>> = BTreeMap::new();
    for r in graph.requirements.values() {
        let Some(src) = &r.source else { continue };
        for e in &r.entities {
            let list = out.entry(e.clone()).or_default();
            if !list.contains(src) {
                list.push(src.clone());
            }
        }
    }
    out
}

// The canonical order of an entity's mentions: the ones it states itself first, in
// their stated order, then the ones requirements add by reference, in document order
// (link level from the stamped roots, then path, then position).
// The same order holds in memory and after a load. With `add`, the by-reference
// mentions an entity file leaves out come back; a source whose section is known and no
// longer holds its quote is stale and stays out, as the sweep would prune it.
// Mirrors docs/compiler/graph.md#node-files.
fn normalize_mentions(
    graph: &mut Graph,
    docs: &BTreeMap<String, DocRecord>,
    roots: &[String],
    add: bool,
) {
    let stale = |m: &SourceRef| {
        docs.get(&m.doc)
            .and_then(|d| d.sections.get(&m.section))
            .is_some_and(|s| !text_contains(&s.raw, &m.quote))
    };
    // Link levels cost a pass over every section's links: computed once, and only when
    // some entity holds by-reference mentions from more than one document.
    let levels: std::cell::OnceCell<BTreeMap<String, usize>> = std::cell::OnceCell::new();
    let level = |doc: &str| -> usize {
        levels
            .get_or_init(|| crate::derive::doc_levels_in(docs, roots))
            .get(doc)
            .copied()
            .unwrap_or(usize::MAX)
    };
    let place = |m: &SourceRef, several_docs: bool| -> (usize, String, usize, usize, String) {
        let sec = docs.get(&m.doc).and_then(|d| d.sections.get(&m.section));
        let line = sec.map(|s| s.lines[0]).unwrap_or(usize::MAX);
        let at = sec
            .and_then(|s| {
                let hay = s.raw.split_whitespace().collect::<Vec<_>>().join(" ");
                let needle = m.quote.split_whitespace().collect::<Vec<_>>().join(" ");
                hay.find(&needle)
            })
            .unwrap_or(usize::MAX);
        let lv = if several_docs { level(&m.doc) } else { 0 };
        (lv, m.doc.clone(), line, at, m.quote.clone())
    };
    let by_reference = reference_sources(graph);
    for (id, e) in graph.entities.iter_mut() {
        let derived = by_reference.get(id).map(Vec::as_slice).unwrap_or(&[]);
        let (own, mut by_ref): (Vec<SourceRef>, Vec<SourceRef>) = std::mem::take(&mut e.mentions)
            .into_iter()
            .partition(|m| !derived.contains(m));
        if add {
            for src in derived {
                if !by_ref.contains(src) && !stale(src) {
                    by_ref.push(src.clone());
                }
            }
        }
        let several_docs = by_ref.iter().any(|m| m.doc != by_ref[0].doc);
        by_ref.sort_by_cached_key(|m| place(m, several_docs));
        e.mentions = own;
        e.mentions.extend(by_ref);
    }
}

// Set the local generation markers on every node that has an entry.
fn apply_markers(graph: &mut Graph, markers: &Markers) {
    macro_rules! mark {
        ($map:expr) => {
            for (id, n) in $map.iter_mut() {
                if let Some((c, u)) = markers.get(id) {
                    n.created = c.clone();
                    n.updated = u.clone();
                }
            }
        };
    }
    mark!(graph.entities);
    mark!(graph.requirements);
    mark!(graph.criteria);
    mark!(graph.views);
    mark!(graph.diagnostics);
}

// Drop repeated ids, keeping each first occurrence in place. A rewire or resolve can
// make two ids equal anywhere in the list, so adjacent-only `Vec::dedup` is not enough.
fn dedup_keep_order(v: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    v.retain(|x| seen.insert(x.clone()));
}

// A store write that failed is reported with its path, never dropped.
fn report_write(path: &Path, r: std::io::Result<()>) {
    if let Err(e) = r {
        eprintln!("[jazyk] error: cannot write {}: {}", path.display(), e);
    }
}

// The kind of a node id by prefix.
fn id_kind(id: &str) -> &'static str {
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

fn provenance_is_pending(p: &Provenance) -> bool {
    !matches!(p, Provenance::Quote(_))
}

// The parent moves the journal entries record, oldest first. A mutation carrying both
// `parent` and `prior.parent` (null for parentless) moved its `id`; a `dissolve_entity`
// moved each of its `children` to its `parent`. Mirrors docs/compiler/graph.md#journal.
pub fn parent_moves_in(entries: &[JournalEntry]) -> Vec<ParentMove> {
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
            // A clear journals `clear_parent: true` and no `parent`: a move to none.
            let cleared = mv["clear_parent"] == true;
            if (has(mv, "parent") || cleared) && has(&mv["prior"], "parent") {
                out.push(ParentMove {
                    generation: entry.generation,
                    child: id.to_string(),
                    from: id_or_none(&mv["prior"]["parent"]),
                    to: if cleared {
                        None
                    } else {
                        id_or_none(&mv["parent"])
                    },
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

// The graph as a batch saw it: the shards and status.yaml copied under
// <out>/snapshots/<label>/ once per batch, for replays to seed from.
// Mirrors docs/compiler/sessions.md#snapshots. A hosted workspace keeps none
// (docs/compiler/hosted-runtime.md#workspace-snapshots).
pub fn snapshot_graph(out: &Path, label: &str) {
    if crate::vfs::is_scoped() {
        return;
    }
    let dir = out.join("snapshots").join(label.replace(['/', ':'], "_"));
    if dir.vexists() {
        return;
    }
    let g = out.join("graph");
    if !g.vis_dir() {
        return;
    }
    copy_tree(&g, &dir.join("graph"));
    crate::vfs::copy(out.join("status.yaml"), dir.join("status.yaml")).ok();
    for d in STATUS_DIRS {
        if out.join(d).vis_dir() {
            copy_tree(&out.join(d), &dir.join(d));
        }
    }
    crate::vfs::copy(out.join(BUILD_FILE), dir.join(BUILD_FILE)).ok();
}

impl Store {
    pub fn load(out: &Path) -> Store {
        // The browser engine's tree changes only through its own writes: a load of an
        // unchanged tree is the last one's copy (docs/frontends/wasm.md#filesystem).
        #[cfg(target_arch = "wasm32")]
        {
            thread_local! {
                static LAST: std::cell::RefCell<std::collections::BTreeMap<PathBuf, (u64, Store)>> =
                    const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
            }
            let now = crate::vfs::memory_generation();
            if let Some(store) = LAST.with(|l| {
                l.borrow()
                    .get(out)
                    .filter(|(g, _)| *g == now)
                    .map(|(_, s)| s.clone())
            }) {
                return store;
            }
            let store = Self::load_uncached(out);
            let now = crate::vfs::memory_generation();
            LAST.with(|l| {
                let mut l = l.borrow_mut();
                l.retain(|_, (g, _)| *g == now);
                l.insert(out.to_path_buf(), (now, store.clone()));
            });
            store
        }
        #[cfg(not(target_arch = "wasm32"))]
        Self::load_uncached(out)
    }

    fn load_uncached(out: &Path) -> Store {
        // Readers never take the lock. A read counts when no commit was in progress at
        // its start or its end and the generation held still across it; otherwise it
        // waits and retries. A store that never holds still within the wait loads
        // read-only with a `store-busy` error, never as a mix of two commits.
        // A store of another version reads as empty; only a build archives it.
        // Mirrors docs/compiler/graph.md#concurrency and #store-version.
        let started = crate::sys::Instant::now();
        let mut attempts = 0u32;
        let store = loop {
            if !commit_in_progress(out) {
                let before = read_generation(out);
                let store = Self::load_once(out);
                if !commit_in_progress(out) && read_generation(out) == before {
                    break store;
                }
            }
            attempts += 1;
            let wait = read_wait();
            if started.elapsed() >= wait || (cfg!(target_arch = "wasm32") && attempts >= 4) {
                let mut store = Self::load_once(out);
                store.load_errors.insert(
                    0,
                    format!(
                        "store-busy: {} kept changing for {} ms of reads ({} attempts); retry once its commit finishes",
                        out.display(),
                        wait.as_millis(),
                        attempts
                    ),
                );
                break store;
            }
            crate::sys::sleep(std::time::Duration::from_millis(
                u64::from(attempts.min(10)) * 10,
            ));
        };
        for e in &store.load_errors {
            eprintln!("[jazyk] error: {} did not load: {}", out.display(), e);
        }
        if !readable_version(store.status.version) {
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
        if status_path.vexists() {
            // A status.yaml that does not parse is a broken file, not another version:
            // the store is read-only and nothing is archived.
            // Mirrors docs/compiler/graph.md#store-version.
            let version = match yaml_read::<Status>(&status_path) {
                Ok(Some(s)) => s.version,
                Ok(None) => 0,
                Err(e) => {
                    eprintln!("[jazyk] error: {}; the store stays read-only", e);
                    return Store {
                        out: out.to_path_buf(),
                        load_errors: vec![e],
                        ..Default::default()
                    };
                }
            };
            if !readable_version(version) {
                let name = out
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".jazyk".to_string());
                let bak = out.with_file_name(format!("{}.bak", name));
                crate::vfs::remove_dir_all(&bak).ok();
                match crate::vfs::rename(out, &bak) {
                    Ok(()) => eprintln!(
                        "[jazyk] store version {} is not {}; archived {} to {} and starting empty",
                        version,
                        STORE_VERSION,
                        out.display(),
                        bak.display()
                    ),
                    Err(e) => {
                        let why = format!(
                            "could not archive {} to {}: {}",
                            out.display(),
                            bak.display(),
                            e
                        );
                        eprintln!("[jazyk] error: {}; the store stays read-only", why);
                        return Store {
                            out: out.to_path_buf(),
                            load_errors: vec![why],
                            ..Default::default()
                        };
                    }
                }
                return Store {
                    out: out.to_path_buf(),
                    ..Default::default()
                };
            }
        }
        let mut store = Self::load(out);
        // A version 3 store is rewritten whole in the version 4 layout before the build
        // begins, ids and facts kept, so even a build with nothing to do leaves no file
        // in the old form. Mirrors docs/compiler/graph.md#store-version.
        if store.status.version == UPGRADABLE_VERSION && store.load_errors.is_empty() {
            let _flock = FileLock::acquire(out);
            store.save();
            let _ledger_lock = crate::gen::Ledger::lock(out);
            let ledger = crate::gen::Ledger::load(out);
            if ledger.unreadable.is_none() {
                // A failed split is reported by save; the inline ledger still loads.
                let _ = ledger.save(out);
            }
            store.status.version = STORE_VERSION;
            eprintln!(
                "[jazyk] upgraded {} from store version {} to {}",
                out.display(),
                UPGRADABLE_VERSION,
                STORE_VERSION
            );
        }
        store
    }

    fn load_once(out: &Path) -> Store {
        let g = out.join("graph");
        let mut errors: Vec<String> = Vec::new();
        let mut views: BTreeMap<String, View> = load_nodes(&g.join("views"), &mut errors);
        views.extend(load_nodes::<View>(&g.join("default-views"), &mut errors));
        let mut store = Store {
            out: out.to_path_buf(),
            graph: Graph {
                entities: load_nodes(&g.join("entities"), &mut errors),
                requirements: load_nodes(&g.join("requirements"), &mut errors),
                criteria: load_nodes(&g.join("criteria"), &mut errors),
                views,
                relationships: load_nodes(&g.join("relationships"), &mut errors),
                state_machines: load_nodes(&g.join("state-machines"), &mut errors),
                diagnostics: load_nodes(&g.join("diagnostics"), &mut errors),
                redirects: load_nodes::<RedirectFile>(&g.join("redirects"), &mut errors)
                    .into_iter()
                    .map(|(id, r)| (id, r.to))
                    .collect(),
            },
            docs: BTreeMap::new(),
            status: read_status(out, &mut errors),
            align: Default::default(),
            load_errors: Vec::new(),
        };
        // The local half of the status, raised to the committed floor.
        // Mirrors docs/compiler/graph.md#the-generation.
        let build: BuildFile = shard(&out.join(BUILD_FILE), &mut errors);
        let docs_dir = out.join("docs");
        let mut files = Vec::new();
        collect_yaml(&docs_dir, &mut files);
        for f in files {
            if let Ok(rel) = f.strip_prefix(&docs_dir) {
                let doc = rel.to_string_lossy().replace('\\', "/");
                let doc = doc.strip_suffix(".yaml").unwrap_or(&doc).to_string();
                match yaml_read::<DocRecord>(&f) {
                    Ok(Some(mut rec)) => {
                        // The content hash is local; a version 3 record still carries one.
                        if let Some(h) = build.docs.get(&doc) {
                            rec.content_hash = h.clone();
                        }
                        fill_section_lines(&mut rec);
                        store.docs.insert(doc, rec);
                    }
                    Ok(None) => {}
                    Err(e) => errors.push(e),
                }
            }
        }
        store.status.generation = build.generation.max(generation_floor(&store.status));
        store.status.verdict = build.verdict;
        store.status.costs = build.costs;
        store.status.spent = build.spent;
        store.status.diagnostics = build.diagnostics;
        // The generation markers are local; a version 3 node file still carries its own.
        // Mirrors docs/compiler/graph.md#storage-layout.
        let markers: Markers = shard(&out.join(MARKERS_FILE), &mut errors);
        apply_markers(&mut store.graph, &markers);
        let roots = store.status.roots.clone();
        normalize_mentions(&mut store.graph, &store.docs, &roots, true);
        // The derived kinds are local: a checkout carries none, so a load that finds
        // any of their directories missing recomputes them in memory and the next
        // commit writes them. Mirrors docs/compiler/graph.md#a-fresh-checkout.
        if DERIVED_KINDS.iter().any(|k| !g.join(k).vis_dir()) {
            crate::derive::recompute_relationships(&mut store);
            crate::derive::recompute_state_machines(&mut store);
            let build = format!("g{}", store.status.generation);
            let mut batch = RecordBatch::new(store.status.generation);
            crate::derive::recompute_default_views(&mut store, &build, &mut batch);
        }
        store.load_errors = errors;
        store
    }

    // Why this store must not write: a file that did not parse at load.
    // Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
    fn unreadable(&self) -> Option<String> {
        self.load_errors.first().map(|e| {
            if e.starts_with("store-busy:") {
                return e.clone();
            }
            format!(
                "store-unreadable: {} (repair or remove the file, then retry)",
                e
            )
        })
    }

    // Why this store must not commit: it did not load cleanly, the directory holds a
    // store of another version, or another writer landed a generation since it loaded.
    // Called under the lock. Mirrors docs/compiler/graph.md#concurrency.
    fn refusal(&self) -> Option<String> {
        if let Some(why) = self.unreadable() {
            return Some(why);
        }
        match yaml_read::<Status>(&self.out.join("status.yaml")) {
            Ok(Some(disk)) if !readable_version(disk.version) => Some(format!(
                "store-version: {} holds a version {} store and this is version {}; a build archives it",
                self.out.display(),
                disk.version,
                STORE_VERSION
            )),
            Err(e) => Some(format!(
                "store-unreadable: {} (repair or remove the file, then retry)",
                e
            )),
            _ => match disk_generation(&self.out) {
                Ok(Some(g)) if g != self.status.generation => Some(format!(
                    "store-moved: {} is at g{} on disk and g{} in memory; reload and restage",
                    self.out.display(),
                    g,
                    self.status.generation
                )),
                Ok(_) => None,
                Err(e) => Some(format!(
                    "store-unreadable: {} (repair or remove the file, then retry)",
                    e
                )),
            },
        }
    }

    // The refusal, reported. Every writer calls this under the lock before it mutates.
    fn refuse(&self, what: &str) -> Option<String> {
        let why = self.refusal()?;
        eprintln!("[jazyk] error: {} refused: {}", what, why);
        Some(why)
    }

    // Node files and doc records first, then the files of removed nodes, the status
    // last: the generation in `build.yaml` is the commit point a reader checks. The
    // commit marker spans every write. Callers hold the lock and checked the refusal;
    // outside this module `write_status` is the only way to write.
    // Mirrors docs/compiler/graph.md#writes-and-unreadable-files and #node-files.
    fn save(&self) {
        if let Some(why) = self.unreadable() {
            eprintln!(
                "[jazyk] error: refusing to write {}: {}",
                self.out.display(),
                why
            );
            return;
        }
        let _marker = CommitMarker::begin(&self.out);
        self.save_nodes();
        self.save_status_files();
    }

    // A test's seed of a store on disk, written as a commit writes it.
    #[cfg(test)]
    pub(crate) fn save_for_test(&self) {
        self.save();
    }

    fn save_nodes(&self) {
        let g = self.out.join("graph");
        let mut markers = Markers::new();
        let redirects: BTreeMap<String, RedirectFile> = self
            .graph
            .redirects
            .iter()
            .map(|(from, to)| (from.clone(), RedirectFile { to: to.clone() }))
            .collect();
        // An entity file holds only the mentions its own facts state: a mention that
        // is the source of a requirement naming the entity is derived and comes back at
        // load. Mirrors docs/compiler/graph.md#node-files.
        let by_reference = reference_sources(&self.graph);
        let entities = self
            .graph
            .entities
            .iter()
            .map(|(id, e)| match by_reference.get(id) {
                Some(d) if e.mentions.iter().any(|m| d.contains(m)) => {
                    let mut own = e.clone();
                    own.mentions.retain(|m| !d.contains(m));
                    (id, Cow::Owned(own))
                }
                _ => (id, Cow::Borrowed(e)),
            });
        let mut gone = Vec::new();
        gone.extend(save_kind(&g.join("entities"), entities, &mut markers));
        gone.extend(save_kind(
            &g.join("requirements"),
            self.graph.requirements.iter().map(borrowed),
            &mut markers,
        ));
        gone.extend(save_kind(
            &g.join("criteria"),
            self.graph.criteria.iter().map(borrowed),
            &mut markers,
        ));
        gone.extend(save_kind(
            &g.join("views"),
            self.graph
                .views
                .iter()
                .filter(|(_, v)| !v.default)
                .map(borrowed),
            &mut markers,
        ));
        gone.extend(save_kind(
            &g.join("default-views"),
            self.graph
                .views
                .iter()
                .filter(|(_, v)| v.default)
                .map(borrowed),
            &mut markers,
        ));
        gone.extend(save_kind(
            &g.join("diagnostics"),
            self.graph.diagnostics.iter().map(borrowed),
            &mut markers,
        ));
        gone.extend(save_kind(
            &g.join("redirects"),
            redirects.iter().map(borrowed),
            &mut Markers::new(),
        ));
        gone.extend(save_kind(
            &g.join("relationships"),
            self.graph.relationships.iter().map(borrowed),
            &mut Markers::new(),
        ));
        gone.extend(save_kind(
            &g.join("state-machines"),
            self.graph.state_machines.iter().map(borrowed),
            &mut Markers::new(),
        ));
        // A derived directory exists once saved, even empty, so a load recomputes the
        // derived kinds only on a checkout that lacks them.
        for kind in DERIVED_KINDS {
            crate::vfs::create_dir_all(g.join(kind)).ok();
        }
        for (doc, rec) in &self.docs {
            let p = self.out.join("docs").join(format!("{}.yaml", doc));
            write_tracked(&p, doc, rec, || doc_record_value(rec));
        }
        for p in gone {
            if let Err(e) = crate::vfs::remove_file(&p) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("[jazyk] error: cannot remove {}: {}", p.display(), e);
                }
            }
        }
        let path = self.out.join(MARKERS_FILE);
        report_write(&path, write_yaml(&path, &markers));
        // A `.jazyk` folder never exists without its ignore file.
        // Mirrors docs/compiler/document-sets.md#persistence-and-identity.
        if let Some(jazyk) = self
            .out
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == ".jazyk"))
        {
            if let Some(root) = jazyk.parent() {
                if let Err(e) = crate::document_sets::ensure_ignore(root) {
                    eprintln!(
                        "[jazyk] error: cannot write {}/.gitignore: {}",
                        jazyk.display(),
                        e
                    );
                }
            }
        }
    }

    // A commit's write: the mentions in their canonical order, then the files.
    fn persist(&mut self) {
        let roots = self.status.roots.clone();
        normalize_mentions(&mut self.graph, &self.docs, &roots, false);
        self.save();
    }

    // The status alone, inside the commit marker. Callers hold the lock and checked
    // the refusal. Every status write stamps the store version.
    fn save_status(&self) {
        if let Some(why) = self.unreadable() {
            eprintln!(
                "[jazyk] error: refusing to write {}: {}",
                self.out.display(),
                why
            );
            return;
        }
        let _marker = CommitMarker::begin(&self.out);
        self.save_status_files();
    }

    // A status write outside a changeset (a build's opening, absorbed derivations,
    // parked and failed goals, the verdict): under the lock and refused like a commit,
    // so a store that is unreadable, of another version, or behind the disk never
    // overwrites what it has not seen. Mirrors docs/compiler/graph.md#concurrency.
    pub fn write_status(&self, what: &str) -> Result<(), String> {
        let _flock = FileLock::acquire(&self.out);
        if let Some(why) = self.refuse(what) {
            return Err(why);
        }
        self.save_status();
        Ok(())
    }

    fn save_status_files(&self) {
        // status.yaml, then build.yaml: the generation there is the commit point.
        // Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
        let mut status = self.status.clone();
        status.version = STORE_VERSION;
        save_status_records(&self.out, &status);
        let path = self.out.join("status.yaml");
        report_write(&path, write_yaml(&path, &committed_status(&status)));
        let build = BuildFile {
            generation: status.generation,
            verdict: status.verdict,
            costs: status.costs,
            spent: status.spent,
            diagnostics: status.diagnostics,
            docs: self
                .docs
                .iter()
                .filter(|(_, r)| !r.content_hash.is_empty())
                .map(|(d, r)| (d.clone(), r.content_hash.clone()))
                .collect(),
        };
        // Every file the commit point covers is on disk before it moves.
        sync_touched_dirs();
        let path = self.out.join(BUILD_FILE);
        report_write(&path, write_yaml(&path, &build));
        sync_touched_dirs();
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
            cached_tokens: commit.cached,
            tokens_source: commit.tokens_source.clone(),
            ..Default::default()
        }
    }

    // Stamp the session's measured spend onto the entries a session just wrote: the
    // entry commits inside the serving, before the runner knows what the turn cost.
    // `cached` is the input among `tokens` the endpoint served from its prompt cache,
    // split the same way; 0 leaves the field absent.
    // Mirrors docs/compiler/graph.md#journal.
    pub fn stamp_tokens(out: &Path, from: u64, to: u64, tokens: u64, cached: u64) {
        if tokens == 0 || to <= from {
            return;
        }
        // A journal rewrite is a store write: under the lock, through the atomic
        // write. Mirrors docs/compiler/graph.md#concurrency.
        let _flock = FileLock::acquire(out);
        // Only the session entries the turn wrote take a share, so the parts sum to
        // the spend: a checks or settle entry between them bills nothing.
        let entries: Vec<(PathBuf, JournalEntry)> = (from + 1..=to)
            .filter_map(|g| {
                let p = out.join("journal").join(format!("g{}.yaml", g));
                let text = crate::vfs::read_to_string(&p).ok()?;
                let entry = serde_norway::from_str::<JournalEntry>(&text).ok()?;
                (entry.kind == "session" && entry.tokens.is_none()).then_some((p, entry))
            })
            .collect();
        let n = entries.len() as u64;
        if n == 0 {
            return;
        }
        // The remainder rides on the last entry.
        let split = |total: u64, i: usize| {
            let share = total / n;
            if i as u64 + 1 == n {
                total - share * (n - 1)
            } else {
                share
            }
        };
        for (i, (p, mut entry)) in entries.into_iter().enumerate() {
            entry.tokens = Some(split(tokens, i));
            entry.cached_tokens = (cached > 0).then(|| split(cached, i));
            report_write(&p, write_yaml(&p, &entry));
        }
    }

    // Stamp an external agent's own report of its bill onto one committed session
    // entry (`jazyk spend`). Refuses a generation the journal does not hold, an entry
    // that is not a session, and an entry that already carries a figure, measured or
    // reported: a stamp never overwrites what the harness measured.
    // Mirrors docs/frontends/cli.md#jazyk-spend.
    pub fn stamp_reported(
        out: &Path,
        generation: u64,
        tokens: u64,
        cached: Option<u64>,
    ) -> Result<JournalEntry, String> {
        if tokens == 0 {
            return Err("tokens must be a whole number above zero".to_string());
        }
        if cached.is_some_and(|c| c > tokens) {
            return Err(format!(
                "cached ({}) cannot exceed tokens ({}): the cached input is a share of the bill",
                cached.unwrap_or(0),
                tokens
            ));
        }
        let p = out.join("journal").join(format!("g{}.yaml", generation));
        if !p.vis_file() {
            return Err(format!(
                "no journal entry g{} under {}",
                generation,
                out.display()
            ));
        }
        // The read, the check, and the write hold the lock, so two stamps never both
        // pass the no-figure check. Mirrors docs/compiler/graph.md#concurrency.
        let _flock = FileLock::acquire(out);
        let text = crate::vfs::read_to_string(&p)
            .map_err(|_| format!("no journal entry g{} under {}", generation, out.display()))?;
        let mut entry = serde_norway::from_str::<JournalEntry>(&text)
            .map_err(|e| format!("journal entry g{} does not parse: {}", generation, e))?;
        if entry.kind != "session" {
            return Err(format!(
                "g{} is a {} entry, not a session; only a session entry carries a spend",
                generation, entry.kind
            ));
        }
        if let Some(t) = entry.tokens {
            let source = match entry.tokens_source.as_deref() {
                Some(crate::model::TOKENS_SOURCE_AGENT) => "reported by the agent",
                _ => "measured by the harness",
            };
            return Err(format!(
                "g{} already carries {} tokens{} ({}); a stamp never overwrites a figure",
                generation,
                t,
                entry
                    .cached_tokens
                    .map(|c| format!(", {} cached", c))
                    .unwrap_or_default(),
                source
            ));
        }
        entry.tokens = Some(tokens);
        entry.cached_tokens = cached.filter(|c| *c > 0);
        entry.tokens_source = Some(crate::model::TOKENS_SOURCE_AGENT.to_string());
        write_yaml(&p, &entry).map_err(|e| format!("write {}: {}", p.display(), e))?;
        Ok(entry)
    }

    fn journal_path(&self, generation: u64) -> PathBuf {
        self.out
            .join("journal")
            .join(format!("g{}.yaml", generation))
    }

    fn write_journal(&self, entry: &JournalEntry) {
        let path = self.journal_path(entry.generation);
        report_write(&path, write_yaml(&path, entry));
    }

    // The latest journal mutation that decreed over the given fact and recorded the
    // prior value it replaced. Reads newest first and stops at the first hit.
    // Mirrors docs/compiler/graph.md#journal: a decree entry carries, on a mutation
    // written over a quoted value, the prior value and source.
    fn decree_prior(&self, id: &str, op: &str) -> Option<serde_json::Value> {
        for entry in self.journal_newest_first() {
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

    // The generations the journal holds on disk, newest first.
    fn journal_generations(&self) -> Vec<u64> {
        let dir = self.out.join("journal");
        let Ok(read) = crate::vfs::read_dir(&dir) else {
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
        gens.sort_unstable_by(|a, b| b.cmp(a));
        gens
    }

    // Every journal entry on disk, oldest first.
    fn journal_entries(&self) -> Vec<JournalEntry> {
        let mut gens = self.journal_generations();
        gens.reverse();
        gens.into_iter()
            .filter_map(|g| crate::goals::read_journal_entry(&self.journal_path(g), g))
            .collect()
    }

    // The journal entries newest first, each parsed when the scan reaches it, so a
    // lookup that stops at its first hit parses no more than it needs.
    // Mirrors docs/compiler/graph.md#journal.
    fn journal_newest_first(&self) -> impl Iterator<Item = JournalEntry> + '_ {
        self.journal_generations()
            .into_iter()
            .filter_map(move |g| crate::goals::read_journal_entry(&self.journal_path(g), g))
    }

    // Every journaled parent move, oldest first: an `update_entity` or `retract_decree`
    // carrying `parent` and `prior.parent`, and each child of a `dissolve_entity` (a
    // tool's or the sweep's), which moved from the dissolved entity to its parent.
    // Mirrors docs/compiler/graph.md#journal.
    pub fn journaled_parent_moves(&self) -> Vec<ParentMove> {
        parent_moves_in(&self.journal_entries())
    }

    // The last journaled parent move of each child in `moves`, and the name and scope
    // the journal created for every parent those moves and the last ones name that
    // the graph no longer holds. The scan reads newest first and stops once every
    // child has its last move and every dead parent its create (an id is minted once,
    // so the first create found is the only one); a child never moved before and a
    // parent the journal never created run the scan to its end.
    // Mirrors docs/compiler/graph.md#journal.
    pub fn parent_history(
        &self,
        moves: &[(String, Option<String>, Option<String>, usize)],
    ) -> (
        BTreeMap<String, ParentMove>,
        BTreeMap<String, (String, String)>,
    ) {
        let mut pending: BTreeSet<String> = moves.iter().map(|(c, _, _, _)| c.clone()).collect();
        let mut last: BTreeMap<String, ParentMove> = BTreeMap::new();
        let mut names: BTreeMap<String, (String, String)> = BTreeMap::new();
        let mut dead: BTreeSet<String> = BTreeSet::new();
        let want_name = |p: &Option<String>, dead: &mut BTreeSet<String>| {
            if let Some(p) = p {
                if !self.graph.entities.contains_key(p) {
                    dead.insert(p.clone());
                }
            }
        };
        for (_, from, to, _) in moves {
            want_name(from, &mut dead);
            want_name(to, &mut dead);
        }
        for entry in self.journal_newest_first() {
            if pending.is_empty() && dead.is_empty() {
                break;
            }
            let one = std::slice::from_ref(&entry);
            // The last move within the entry is the latest, so the reversed walk's
            // first hit per child stands.
            for mv in parent_moves_in(one).into_iter().rev() {
                if pending.remove(&mv.child) {
                    want_name(&mv.from, &mut dead);
                    want_name(&mv.to, &mut dead);
                    last.insert(mv.child.clone(), mv);
                }
            }
            for (id, name) in created_names_in(one) {
                if dead.remove(&id) {
                    names.insert(id, name);
                }
            }
        }
        (last, names)
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
        let (history, names) = self.parent_history(moves);
        for (child, from, to, m) in moves {
            let Some(e) = self.graph.entities.get(child) else {
                continue;
            };
            let scope = e.scope.clone();
            let key = |p: Option<&str>| self.parent_key(p, &scope, &names);
            let Some(last) = history.get(child) else {
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
        report_write(&path, write_yaml(&path, &entry));
    }

    // Resolving a goal clears the records it stood on, except the created and revised
    // records a pair stands on: those keep their other candidates, the commit marked
    // the partner judged, and the sweep clears them when none remains.
    // Mirrors docs/compiler/graph.md#change-records.
    pub fn clear_resolved(&mut self, ids: &[String]) {
        let ids: Vec<String> = ids
            .iter()
            .filter(|id| {
                !self.status.changes.iter().any(|c| {
                    c.id == **id
                        && matches!(c.kind.as_str(), CHANGE_REQ_CREATED | CHANGE_REQ_REVISED)
                })
            })
            .cloned()
            .collect();
        self.clear_changes(&ids);
    }

    // Clear the named records. Persists at once.
    pub fn clear_changes(&mut self, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        let _flock = FileLock::acquire(&self.out);
        if self.refuse("clear-changes").is_some() {
            return;
        }
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
    // A chain a store written before compaction still holds is followed to its end;
    // the hop bound only stops a cycle.
    pub fn resolve_id<'a>(&'a self, id: &'a str) -> &'a str {
        let mut cur = id;
        let mut hops = 0;
        while let Some(next) = self.graph.redirects.get(cur) {
            if next.is_empty() || hops >= self.graph.redirects.len() {
                return cur;
            }
            cur = next;
            hops += 1;
        }
        cur
    }

    // ---- id minting ----

    // Ids carry no counter: a slug of the natural key and, where the kind needs one or
    // the plain id is taken, a short hash of the key, so two branches never mint one id
    // for two nodes. Mirrors docs/compiler/model.md#identifiers.
    fn id_taken(&self, id: &str, taken: &BTreeSet<String>) -> bool {
        self.graph.entities.contains_key(id)
            || self.graph.requirements.contains_key(id)
            || self.graph.criteria.contains_key(id)
            || self.graph.views.contains_key(id)
            || self.graph.diagnostics.contains_key(id)
            || self.graph.redirects.contains_key(id)
            || taken.contains(id)
    }

    pub fn mint_entity_id(&self, name: &str, taken: &BTreeSet<String>) -> String {
        let plain = format!("ent:{}", crate::ids::slug(name));
        crate::ids::plain_or_unique(&plain, name, |id| self.id_taken(id, taken))
    }

    // The stem of a requirement id: the id slug of its document's file stem and the
    // last segment of its section reference (dropped for a root or preamble section),
    // `x` without a document. Mirrors docs/compiler/model.md#identifiers.
    pub fn req_id_stem(source: Option<&crate::model::SourceRef>) -> String {
        let Some(src) = source else {
            return "x".to_string();
        };
        let file = src.doc.rsplit('/').next().unwrap_or(&src.doc);
        let file = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        let segments: Vec<&str> = src.section.split('/').filter(|s| !s.is_empty()).collect();
        let text = if segments.len() >= 2 {
            format!("{} {}", file, segments[segments.len() - 1])
        } else {
            file.to_string()
        };
        crate::ids::slug(&text)
    }

    // `req:<stem>-<hash>`, the hash of the document, section, and statement, or of the
    // sorted `from` ids and the statement for a derived or decreed requirement.
    pub fn mint_req_id(&self, r: &Requirement, taken: &BTreeSet<String>) -> String {
        let statement = normalize_statement(&r.statement);
        let key = match &r.source {
            Some(src) => format!("{}#{}\n{}", src.doc, src.section, statement),
            None => {
                let mut from: Vec<String> = match &r.provenance {
                    Some(Provenance::Derived { from, .. }) => from.clone(),
                    _ => Vec::new(),
                };
                from.sort();
                format!("x\n{}\n{}", from.join(","), statement)
            }
        };
        let base = format!("req:{}", Self::req_id_stem(r.source.as_ref()));
        crate::ids::unique(&base, &key, |id| self.id_taken(id, taken))
    }

    // The id stem of a requirement: its source document, or `x` without a quote.
    pub fn req_stem(r: &Requirement) -> &str {
        r.source.as_ref().map(|s| s.doc.as_str()).unwrap_or("x")
    }

    // `crit:<stem>-<hash>`: the stem of its requirement (else of its own quote, else
    // `x`), the hash of the requirement id and the statement.
    // Mirrors docs/compiler/model/criterion.md#identity.
    // `req_source` is the source of a requirement staged beside the criterion, not yet
    // in the graph.
    pub fn mint_crit_id(
        &self,
        c: &Criterion,
        req_source: Option<&crate::model::SourceRef>,
        taken: &BTreeSet<String>,
    ) -> String {
        let source = self
            .graph
            .requirements
            .get(&c.requirement)
            .and_then(|r| r.source.as_ref())
            .or(req_source)
            .or(c.source.as_ref());
        let key = format!("{}\n{}", c.requirement, normalize_statement(&c.statement));
        let base = format!("crit:{}", Self::req_id_stem(source));
        crate::ids::unique(&base, &key, |id| self.id_taken(id, taken))
    }

    // The id stem of a criterion: its quote's document, else its requirement's, so a
    // derived criterion reads as its requirement's kin (crit:slide-about-3), and `x`
    // only when neither has a document (docs/compiler/model/criterion.md#identity).
    pub fn crit_stem(&self, c: &Criterion) -> String {
        self.graph
            .requirements
            .get(&c.requirement)
            .and_then(|r| r.source.as_ref())
            .map(|s| s.doc.clone())
            .or_else(|| c.source.as_ref().map(|s| s.doc.clone()))
            .unwrap_or_else(|| "x".to_string())
    }

    // The criteria of one requirement, in id order (docs/compiler/model/criterion.md).
    pub fn criteria_of(&self, rid: &str) -> Vec<String> {
        let rid = self.resolve_id(rid);
        self.graph
            .criteria
            .iter()
            .filter(|(_, c)| c.requirement == rid)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn mint_view_id(&self, kind: &str, title: &str, taken: &BTreeSet<String>) -> String {
        let plain = format!("view:{}/{}", view_kind_slug(kind), crate::ids::slug(title));
        let key = format!("{}\n{}", kind, title);
        crate::ids::plain_or_unique(&plain, &key, |id| self.id_taken(id, taken))
    }

    // `diag:<rule>-<hash>`, the hash of the rule and the sorted subjects.
    pub fn mint_diag_id(
        &self,
        rule: &str,
        subjects: &[String],
        taken: &BTreeSet<String>,
    ) -> String {
        let mut sorted: Vec<&str> = subjects.iter().map(String::as_str).collect();
        sorted.sort();
        let key = format!("{}\n{}", rule, sorted.join("\n"));
        let base = format!("diag:{}", crate::ids::slug(rule));
        crate::ids::unique(&base, &key, |id| self.id_taken(id, taken))
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
        self.redirect(id, parent.as_deref().unwrap_or_default());
        (parent, children)
    }

    // Redirect `from` to the surviving `to`, one hop: every redirect that named `from`
    // as its survivor now names `to`, so no chain grows across merges. A tombstone
    // (empty `to`) leaves the redirects into `from` alone; they resolve to the dead id.
    // Mirrors docs/compiler/graph.md#storage-layout.
    fn redirect(&mut self, from: &str, to: &str) {
        if !to.is_empty() {
            for target in self.graph.redirects.values_mut() {
                if target == from {
                    *target = to.to_string();
                }
            }
        }
        self.graph
            .redirects
            .insert(from.to_string(), to.to_string());
    }

    // Whether a node id names an existing entity or requirement.
    fn node_exists(&self, id: &str) -> bool {
        self.graph.entities.contains_key(id)
            || self.graph.requirements.contains_key(id)
            || self.graph.criteria.contains_key(id)
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
            .map(|s| s.kind != "blob" && text_contains(&s.raw, quote))
            .unwrap_or(false)
    }

    pub fn requirements_referencing(&self, entity_id: &str) -> Vec<String> {
        if let Some(ix) = ScopeIndex::of(self) {
            return ix
                .requirements_of(self)
                .get(entity_id)
                .cloned()
                .unwrap_or_default();
        }
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
        self.content_token_words(statement, entities, content_stem)
            .into_iter()
            .map(|(stemmed, _)| stemmed)
            .collect()
    }

    // The one content tokenizer: the statement's normalized words that are content
    // tokens, each stemmed by `stem` and paired with the word as written, minus the
    // given entities' own name tokens under the same stem. The scorer stems with
    // `content_stem`, the hints with `suffix_stem`.
    pub(crate) fn content_token_words(
        &self,
        statement: &str,
        entities: &[String],
        stem: fn(&str) -> String,
    ) -> Vec<(String, String)> {
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
            .filter(|t| is_content_token(t))
            .map(|t| (stem(t), t.to_string()))
            .filter(|(stemmed, _)| is_content_token(stemmed) && !name_toks.contains(stemmed))
            .collect()
    }

    // The scorer's inputs over the whole graph, one walk of the requirements: the
    // hub; per entity (resolved id) how many requirements name it and how many of
    // those carry each content token (the boilerplate source) and which they are;
    // and each requirement's content tokens. Computed once per pass and shared by
    // every requirement the pass scores. Mirrors docs/compiler/reconciler.md#pairs.
    pub fn pair_scoring(&self) -> PairScoring {
        #[cfg(test)]
        PAIR_SCORINGS.with(|n| n.set(n.get() + 1));
        let mut counts: BTreeMap<String, (usize, BTreeMap<String, usize>)> = BTreeMap::new();
        let mut by_entity: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut tokens: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (rid, r) in &self.graph.requirements {
            let toks = self.content_tokens(&r.statement, &r.entities);
            let ids: BTreeSet<&str> = r.entities.iter().map(|e| self.resolve_id(e)).collect();
            for id in ids {
                let e = counts.entry(id.to_string()).or_default();
                e.0 += 1;
                for t in &toks {
                    *e.1.entry(t.clone()).or_default() += 1;
                }
                by_entity
                    .entry(id.to_string())
                    .or_default()
                    .push(rid.clone());
            }
            tokens.insert(rid.clone(), toks);
        }
        PairScoring {
            hub: self.hub_entity(),
            counts,
            by_entity,
            tokens,
        }
    }

    // An entity's boilerplate: the content tokens carried by at least four of the
    // statements naming it and by at least half of them, the wording every statement
    // of that entity shares. Mirrors docs/compiler/reconciler.md#pairs.
    fn boilerplate_of(
        counts: &BTreeMap<String, (usize, BTreeMap<String, usize>)>,
        entities: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for e in entities {
            if let Some((n, toks)) = counts.get(e.as_ref()) {
                for (t, c) in toks {
                    if *c >= BOILERPLATE_MIN && *c * 2 >= *n {
                        out.insert(t.clone());
                    }
                }
            }
        }
        out
    }

    // The boilerplate of the entities two requirements share, as content stems
    // (`content_stem`), for a hint that drops the same words the scorer drops.
    pub fn pair_boilerplate(&self, a: &Requirement, b: &Requirement) -> BTreeSet<String> {
        self.pair_boilerplate_with(a, b, &self.pair_scoring())
    }

    // `pair_boilerplate` over scorer inputs computed once for a pass.
    pub fn pair_boilerplate_with(
        &self,
        a: &Requirement,
        b: &Requirement,
        scoring: &PairScoring,
    ) -> BTreeSet<String> {
        let bs: BTreeSet<&str> = b.entities.iter().map(|e| self.resolve_id(e)).collect();
        let shared: Vec<&str> = a
            .entities
            .iter()
            .map(|e| self.resolve_id(e))
            .filter(|e| bs.contains(e))
            .collect();
        if shared.is_empty() {
            return BTreeSet::new();
        }
        Self::boilerplate_of(&scoring.counts, shared)
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
        self.requirement_neighbors_scored(rid)
            .into_iter()
            .map(|(_, id)| id)
            .collect()
    }

    // The neighbor set with each neighbor's score, best first. The score is
    // symmetric (shared tokens, shared entities, the transition and intro boosts
    // read the same from either side), so a pair scores the same whichever side
    // derived it. Mirrors docs/compiler/reconciler.md#pairs.
    pub fn requirement_neighbors_scored(&self, rid: &str) -> Vec<(usize, String)> {
        self.requirement_neighbors_scored_with(rid, &self.pair_scoring())
    }

    // `requirement_neighbors_scored` over scorer inputs computed once for a pass:
    // the sweep and the goal derivation score every changed requirement against
    // the same graph, so the hub and the token counts are computed once, not per
    // record. Mirrors docs/compiler/reconciler.md#pairs.
    pub fn requirement_neighbors_scored_with(
        &self,
        rid: &str,
        scoring: &PairScoring,
    ) -> Vec<(usize, String)> {
        let Some(req) = self.graph.requirements.get(rid) else {
            return Vec::new();
        };
        let subject_entities: BTreeSet<&str> =
            req.entities.iter().map(|e| self.resolve_id(e)).collect();
        let PairScoring {
            hub,
            counts,
            by_entity,
            tokens,
        } = scoring;
        let toks: BTreeSet<String> = tokens
            .get(rid)
            .cloned()
            .unwrap_or_else(|| self.content_tokens(&req.statement, &req.entities));
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
        // The candidates: every requirement naming one of the subject's entities. The
        // final sort is total (score, then id), so the walk's order does not matter.
        let mut candidates: BTreeSet<&String> = BTreeSet::new();
        for e in &subject_entities {
            if let Some(list) = by_entity.get(*e) {
                candidates.extend(list.iter());
            }
        }
        let mut scored: Vec<(usize, &String)> = Vec::new();
        for oid in candidates {
            if oid == rid {
                continue;
            }
            let Some(other) = self.graph.requirements.get(oid) else {
                continue;
            };
            let other_entities: BTreeSet<&str> =
                other.entities.iter().map(|e| self.resolve_id(e)).collect();
            let shared_entities = other_entities.intersection(&subject_entities).count();
            if shared_entities == 0 {
                continue;
            }
            // The shared entities' boilerplate leaves the pool: "supports" and
            // "operation" on every operation bullet of one page say nothing about
            // any two of them.
            let boilerplate =
                Self::boilerplate_of(counts, other_entities.intersection(&subject_entities));
            let computed;
            let other_toks: &BTreeSet<String> = match tokens.get(oid) {
                Some(t) => t,
                None => {
                    computed = self.content_tokens(&other.statement, &other.entities);
                    &computed
                }
            };
            let shared = other_toks
                .intersection(&toks)
                .filter(|t| !boilerplate.contains(*t))
                .count();
            // The hub never counts as a shared entity: sharing it says little (a
            // hub-sharing flood once derived forty-nine pair goals with one real pair
            // among them, and the hub plus one entity paired every statement of a shop
            // with no word in common). A pair sharing only the hub needs three tokens.
            let hub_shared = hub
                .as_deref()
                .map(|h| subject_entities.contains(h) && other_entities.contains(h))
                .unwrap_or(false);
            let shared_entities = shared_entities - usize::from(hub_shared);
            let hub_only = hub_shared && shared_entities == 0;
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
            // Two transitions of one subject with different ends are siblings of one
            // machine, distinct by construction: the state machine checks judge their
            // conflicts (nondeterministic-transition), never a pair. Six transition
            // bullets once opened twenty-eight pairs, every one consistent.
            let sibling_transitions = match (&transition, &other.transition) {
                (Some((subj, _, _)), Some(t)) => {
                    self.resolve_id(&t.subject) == *subj && !same_transition
                }
                _ => false,
            };
            if sibling_transitions {
                continue;
            }
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
            // Two shared content tokens qualify, and so do two shared entities with
            // one token: a restatement built from the same entities shares few other
            // words, since the shared names leave the token pool, but two entities
            // alone are what every statement of one page shares, and a same-entity
            // restatement with no word in common is review-entity's catch.
            // Mirrors docs/compiler/reconciler.md#pairs.
            let qualifies = shared >= token_bar
                || (shared_entities >= 2 && shared >= 1)
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
            .map(|(score, id)| (score, id.clone()))
            .collect()
    }

    // A created or revised record's candidates: the requirement's neighbors and
    // sticky partners minus the partners `detail.judged` lists, each with its score
    // (a sticky partner outside the neighbor set scores zero; it is pinned anyway).
    // Mirrors docs/compiler/reconciler.md#pairs.
    pub fn pair_review_candidates(&self, rec: &ChangeRecord) -> Vec<(String, usize)> {
        self.pair_review_candidates_with(rec, &self.pair_scoring())
    }

    // `pair_review_candidates` over scorer inputs computed once for a pass.
    pub fn pair_review_candidates_with(
        &self,
        rec: &ChangeRecord,
        scoring: &PairScoring,
    ) -> Vec<(String, usize)> {
        let judged = rec.judged();
        let mut out: Vec<(String, usize)> = self
            .requirement_neighbors_scored_with(&rec.subject, scoring)
            .into_iter()
            .map(|(score, id)| (id, score))
            .collect();
        for partner in self.sticky_partners(&rec.subject) {
            if !out.iter().any(|(id, _)| *id == partner) {
                out.push((partner, 0));
            }
        }
        out.retain(|(id, _)| !judged.contains(id));
        out
    }

    // The pair set: computed neighbors plus sticky partners. Open contradiction and
    // duplicate-requirement diagnostics tie pairs; editing one side of a known pair
    // always re-judges the other.
    pub fn pair_review_neighbors(&self, rid: &str) -> Vec<String> {
        let mut nbrs = self.requirement_neighbors(rid);
        for partner in self.sticky_partners(rid) {
            if !nbrs.contains(&partner) {
                nbrs.push(partner);
            }
        }
        nbrs
    }

    // The sticky partners of a requirement: the other living subjects of every open
    // contradiction or duplicate-requirement diagnostic naming it, in diagnostic order.
    fn sticky_partners(&self, rid: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
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
                if sub != rid && self.graph.requirements.contains_key(sub) && !out.contains(sub) {
                    out.push(sub.clone());
                }
            }
        }
        out
    }

    // Open diagnostic counts by severity, suppressed excluded: the health line that
    // rides beside every verdict. Mirrors docs/compiler/compilation.md#convergence.
    pub fn open_diag_counts(&self) -> BTreeMap<String, u64> {
        let mut m: BTreeMap<String, u64> = BTreeMap::new();
        for d in self.graph.diagnostics.values() {
            // Proposals are questions, counted on their own line
            // (docs/compiler/model/diagnostic.md#ratification-proposals).
            if d.lifecycle == "open"
                && d.triage.as_deref() != Some("suppressed")
                && d.rule != "ratification-pending"
            {
                *m.entry(d.severity.clone()).or_insert(0) += 1;
            }
        }
        m
    }

    // Open ratification proposals: one per derived or decreed fact that stands.
    pub fn pending_proposals(&self) -> u64 {
        self.graph
            .diagnostics
            .values()
            .filter(|d| d.lifecycle == "open" && d.rule == "ratification-pending")
            .count() as u64
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

    // The lookalike memory and the name index, settled at every commit. A `separate`
    // verdict (a resolved `dedupe-candidates` goal, or one entry of a resolved
    // `review-entity` goal's `lookalikes` evidence) writes the pair's entry with the
    // formula's value over the graph as it stands and clears the pair's record. An
    // entry whose pair lost an id drops. Then the name index: every scored pair with no
    // standing verdict takes a `lookalike` record, a remembered pair whose score moved
    // drops its entry and takes one too, and a record whose pair is no longer pending
    // clears. Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
    fn settle_lookalikes(
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
    fn mark_pairs_judged(&mut self, resolved: &[Resolved]) {
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
    fn mark_answered_pairs_judged(&mut self, applied: &[Op]) {
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
    fn prune_records(&mut self) {
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
                    if !self.status.parked.iter().any(|p| &p.goal.target == subject) {
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
    fn combine_criterion_proposals(&mut self, build: &str) -> Vec<Op> {
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

    // Apply a staged changeset atomically: reconcile creates by natural key against nodes
    // committed concurrently, apply ops in order under the commit-time gates, propagate
    // deletions, recompute derived data, write the change records, journal, bump the
    // generation, write shards. Mirrors docs/compiler/graph.md#changesets.
    pub fn apply(&mut self, ops: Vec<Op>, commit: &Commit) -> CommitReport {
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

fn collect_yaml(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = crate::vfs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.vis_dir() {
                collect_yaml(&p, out);
            } else if p.extension().map(|e| e == "yaml").unwrap_or(false) {
                out.push(p);
            }
        }
    }
}

// Cross-process single-writer lock: an advisory lock on `.lock` the operating system
// releases when its holder exits, so a crashed writer leaves nothing stale and a waiter
// never overrides a live holder. Two handles in one process exclude each other too.
// A scoped (in-memory) store belongs to one process and keeps a marker file instead.
// Mirrors docs/compiler/graph.md#concurrency.
struct FileLock {
    #[cfg(not(target_arch = "wasm32"))]
    _file: Option<std::fs::File>,
    marker: Option<PathBuf>,
}

impl FileLock {
    fn acquire(out: &Path) -> FileLock {
        let path = out.join(".lock");
        crate::vfs::create_dir_all(out).ok();
        #[cfg(not(target_arch = "wasm32"))]
        if !crate::vfs::is_scoped() {
            let opened = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path);
            let file = match opened {
                Ok(f) => f,
                Err(e) => {
                    eprintln!(
                        "[jazyk] warning: cannot open {}: {}; writing without the lock",
                        path.display(),
                        e
                    );
                    return FileLock {
                        _file: None,
                        marker: None,
                    };
                }
            };
            let started = std::time::Instant::now();
            let mut warned = false;
            loop {
                if fs2::FileExt::try_lock_exclusive(&file).is_ok() {
                    return FileLock {
                        _file: Some(file),
                        marker: None,
                    };
                }
                if !warned && started.elapsed() >= std::time::Duration::from_secs(10) {
                    eprintln!("[jazyk] waiting for the writer holding {}", path.display());
                    warned = true;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        // An in-memory tree belongs to one thread, so no other party can hold its
        // marker and waiting never frees it. A marker this thread holds is a nested
        // acquire: it proceeds without taking the marker, which stays its holder's. Any
        // other marker has no live holder (a snapshot carried it in) and is taken over.
        let nested = SCOPED_LOCKS.with(|held| held.borrow().contains(&path));
        if nested {
            return FileLock {
                #[cfg(not(target_arch = "wasm32"))]
                _file: None,
                marker: None,
            };
        }
        if let Err(e) = crate::vfs::write(&path, crate::sys::process_id().to_string()) {
            eprintln!(
                "[jazyk] warning: cannot write {}: {}; writing without the lock",
                path.display(),
                e
            );
            return FileLock {
                #[cfg(not(target_arch = "wasm32"))]
                _file: None,
                marker: None,
            };
        }
        SCOPED_LOCKS.with(|held| held.borrow_mut().insert(path.clone()));
        FileLock {
            #[cfg(not(target_arch = "wasm32"))]
            _file: None,
            marker: Some(path),
        }
    }
}

// How many pair scorings this thread computed: a test's measure of passes.
#[cfg(test)]
thread_local! {
    static PAIR_SCORINGS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// How long a reader waits for a consistent read before it reports `store-busy`.
const READ_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(test)]
thread_local! {
    static READ_WAIT_OVERRIDE: std::cell::Cell<Option<std::time::Duration>> =
        const { std::cell::Cell::new(None) };
}

fn read_wait() -> std::time::Duration {
    #[cfg(test)]
    if let Some(w) = READ_WAIT_OVERRIDE.with(|o| o.get()) {
        return w;
    }
    READ_WAIT
}

// The commit marker: present while a writer rewrites the store's files, removed once
// the commit point is written. Mirrors docs/compiler/graph.md#concurrency.
pub const COMMIT_MARKER: &str = ".committing";

struct CommitMarker {
    path: Option<PathBuf>,
}

impl CommitMarker {
    // A nested begin (a save inside a save) leaves the marker to the outer one.
    fn begin(out: &Path) -> CommitMarker {
        let path = out.join(COMMIT_MARKER);
        let outer = MARKING.with(|m| m.borrow_mut().insert(path.clone()));
        if !outer {
            return CommitMarker { path: None };
        }
        if let Err(e) = crate::vfs::create_dir_all(out)
            .and_then(|()| write_durable(&path, crate::sys::process_id().to_string().as_bytes()))
        {
            eprintln!("[jazyk] error: cannot write {}: {}", path.display(), e);
        }
        CommitMarker { path: Some(path) }
    }
}

impl Drop for CommitMarker {
    fn drop(&mut self) {
        if let Some(p) = self.path.take() {
            if let Err(e) = crate::vfs::remove_file(&p) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("[jazyk] error: cannot remove {}: {}", p.display(), e);
                }
            }
            MARKING.with(|m| m.borrow_mut().remove(&p));
        }
    }
}

thread_local! {
    static MARKING: std::cell::RefCell<BTreeSet<PathBuf>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

// Whether a writer is rewriting the store now: the marker is present and a writer holds
// `.lock`. A marker no writer holds is left by a writer that stopped mid-commit; the
// next commit replaces it, and reads proceed on what the disk holds. An in-memory tree
// belongs to one thread, which cannot read while it writes, so its marker is always a
// leftover. Mirrors docs/compiler/graph.md#concurrency.
fn commit_in_progress(out: &Path) -> bool {
    if !out.join(COMMIT_MARKER).vexists() {
        return false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        // A shared probe of the writers' advisory lock: it fails only while a writer
        // holds it, and it creates nothing.
        let Ok(file) = std::fs::File::open(out.join(".lock")) else {
            return false;
        };
        return match fs2::FileExt::try_lock_shared(&file) {
            Ok(()) => {
                fs2::FileExt::unlock(&file).ok();
                false
            }
            Err(_) => true,
        };
    }
    false
}

// The markers this thread holds on in-memory trees.
thread_local! {
    static SCOPED_LOCKS: std::cell::RefCell<BTreeSet<PathBuf>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

impl Drop for FileLock {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(f) = &self._file {
            fs2::FileExt::unlock(f).ok();
        }
        if let Some(p) = &self.marker {
            crate::vfs::remove_file(p).ok();
            SCOPED_LOCKS.with(|held| held.borrow_mut().remove(p));
        }
    }
}

#[cfg(test)]
mod concurrency_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_statement_keeps_operators_signs_and_decimals() {
        assert_eq!(
            normalize_statement("The reply arrives, under 30 s."),
            "the reply arrives under 30 s"
        );
        assert_ne!(
            normalize_statement("latency > 30 s"),
            normalize_statement("latency < 30 s")
        );
        assert_ne!(
            normalize_statement("under 30 s"),
            normalize_statement("under 3.0 s")
        );
        assert_ne!(
            normalize_statement("an offset of -1"),
            normalize_statement("an offset of 1")
        );
        assert_eq!(
            normalize_statement("Read-only mode."),
            normalize_statement("read only mode")
        );
        assert_eq!(normalize_statement("on 2024-09-27"), "on 2024 09 27");
        assert_eq!(normalize_statement("x >= 5"), "x > = 5");
    }

    // A fresh directory per call: parallel tests each hold a store of their own, so a
    // commit never finds another test's generation on disk (`store-moved`).
    fn tmp() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "jazyk-store-test-{}-{}",
            crate::sys::process_id(),
            n
        ));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(&dir).ok();
        dir
    }

    // A directory of its own for a test that reloads from disk (the shared tmp dir is
    // stomped by parallel tests).
    fn own_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jazyk-{}-test-{}", label, crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(&dir).ok();
        dir
    }

    // A save rewrites only the files whose bytes changed: a second save with nothing
    // new replaces no file, and a commit leaves untouched shards in place while
    // status.yaml, the commit point, is rewritten. A rewrite renames a new file into
    // place, so an unchanged inode means no write.
    // Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
    #[cfg(unix)]
    #[test]
    fn save_leaves_files_with_unchanged_bytes_in_place() {
        use std::os::unix::fs::MetadataExt;
        let out = own_dir("unchanged-save");
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        let r = s.apply(vec![create("ent:a", "A")], &session());
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        let inodes = |dir: &Path| -> BTreeMap<PathBuf, u64> {
            let mut all = BTreeMap::new();
            for sub in ["graph/entities", "graph/diagnostics", "docs", ""] {
                let Ok(rd) = std::fs::read_dir(dir.join(sub)) else {
                    continue;
                };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("yaml") {
                        all.insert(p.clone(), std::fs::metadata(&p).unwrap().ino());
                    }
                }
            }
            all
        };
        let before = inodes(&out);
        assert!(before.contains_key(&out.join("status.yaml")));
        assert!(before.contains_key(&out.join("graph/entities/ent~a.yaml")));
        let again = Store::load(&out);
        again.save();
        assert_eq!(inodes(&out), before, "a save with no change writes nothing");

        let mut s = Store::load(&out);
        let r = s.apply(vec![create("ent:b", "B")], &session());
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        let after = inodes(&out);
        assert!(
            after.contains_key(&out.join("graph/entities/ent~b.yaml")),
            "the new node is written"
        );
        // The records live one per file: status.yaml stays, the commit point moves.
        assert_eq!(
            after[&out.join("status.yaml")],
            before[&out.join("status.yaml")]
        );
        assert_ne!(after[&out.join(BUILD_FILE)], before[&out.join(BUILD_FILE)]);
        let a = out.join("graph/entities/ent~a.yaml");
        assert_eq!(after[&a], before[&a], "an unchanged node file stays");
        assert_eq!(Store::load(&out).status.generation, 2);
    }

    // A save serializes only the nodes whose value moved since this process last
    // wrote or verified their files; a file another writer changed is compared and
    // rewritten as before. Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
    #[cfg(unix)]
    #[test]
    fn a_save_serializes_only_nodes_whose_value_moved() {
        let out = own_dir("serialized-save");
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        let ops: Vec<Op> = (0..20)
            .map(|i| create(&format!("ent:n{}", i), &format!("N{}", i)))
            .collect();
        let r = s.apply(ops, &session());
        assert_eq!(r.applied, 20, "{:?}", r.skipped);
        let serialized = |s: &Store| {
            let before = SERIALIZED.with(|n| n.get());
            s.save();
            SERIALIZED.with(|n| n.get()) - before
        };
        let again = Store::load(&out);
        assert_eq!(serialized(&again), 0, "nothing moved, nothing serializes");

        let mut s = Store::load(&out);
        s.graph.entities.get_mut("ent:n4").unwrap().definition = Some("moved".into());
        assert_eq!(serialized(&s), 1, "one node moved");

        let shard = out.join("graph/entities/ent~n5.yaml");
        let canonical = std::fs::read_to_string(&shard).unwrap();
        std::fs::write(&shard, format!("{}# a hand edit\n", canonical)).unwrap();
        let again = Store::load(&out);
        assert_eq!(serialized(&again), 1, "the edited file is compared again");
        assert_eq!(std::fs::read_to_string(&shard).unwrap(), canonical);
        crate::vfs::remove_dir_all(&out).ok();
    }

    // Inside a read scope the whole-graph lookups answer from indices, and answer
    // what the scans answer: redirects followed, each requirement once, by id.
    // Mirrors docs/compiler/graph.md#derived-data.
    #[test]
    fn a_read_scope_answers_what_the_scans_answer() {
        let mut s = Store::default();
        for (id, parent, scope) in [
            ("ent:a", None, "public"),
            ("ent:b", Some("ent:a"), "public"),
            ("ent:c", Some("ent:a"), "public"),
            ("ent:d", None, "internal"),
            ("ent:e", None, "public"),
        ] {
            let mut e = entity(id);
            e.parent = parent.map(String::from);
            e.scope = scope.into();
            s.graph.entities.insert(id.into(), e);
        }
        s.graph.redirects.insert("ent:old".into(), "ent:b".into());
        for (rid, ents) in [
            ("req:1", vec!["ent:a", "ent:b"]),
            ("req:2", vec!["ent:old", "ent:b"]),
            ("req:3", vec!["ent:c", "ent:old", "ent:a"]),
            ("req:4", vec!["ent:gone"]),
        ] {
            s.graph.requirements.insert(
                rid.into(),
                Requirement {
                    statement: rid.into(),
                    entities: ents.into_iter().map(String::from).collect(),
                    ..Default::default()
                },
            );
        }
        let ids = [
            "ent:a",
            "ent:b",
            "ent:c",
            "ent:d",
            "ent:e",
            "ent:old",
            "ent:gone",
            "scope:public",
            "scope:internal",
            "scope:none",
        ];
        let answers = |s: &Store| -> Vec<(Vec<String>, Vec<String>)> {
            ids.iter()
                .map(|id| {
                    (
                        s.requirements_referencing(id),
                        crate::board::level_members(s, id),
                    )
                })
                .collect()
        };
        let scanned = answers(&s);
        assert_eq!(scanned[1].0, vec!["req:1", "req:2", "req:3"]);
        let scope = ReadScope::enter(&s);
        assert!(ScopeIndex::of(&s).is_some());
        assert_eq!(answers(&s), scanned);
        let other = s.clone();
        assert!(ScopeIndex::of(&other).is_none(), "another store scans");
        drop(scope);
        assert!(ScopeIndex::of(&s).is_none());
    }

    #[test]
    fn an_unparseable_node_file_makes_the_store_read_only() {
        let out = own_dir("unreadable");
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        let r = s.apply(vec![create("ent:a", "A")], &session());
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        let shard = out.join("graph/entities/ent~a.yaml");
        let broken = "name: [this is not an entity\n";
        crate::vfs::write(&shard, broken).unwrap();
        let mut again = Store::load(&out);
        assert!(
            again.load_errors.iter().any(|e| e.contains("ent~a.yaml")),
            "{:?}",
            again.load_errors
        );
        let r = again.apply(vec![create("ent:b", "B")], &session());
        assert_eq!(r.applied, 0);
        assert!(
            r.skipped[0].starts_with("store-unreadable:"),
            "{:?}",
            r.skipped
        );
        assert!(again.sweep().is_empty());
        again.save();
        assert_eq!(crate::vfs::read_to_string(&shard).unwrap(), broken);
        assert!(!out.join("journal").join("g2.yaml").vexists());
        assert_eq!(Store::load(&out).status.generation, 1);
    }

    #[test]
    fn a_commit_against_a_moved_store_is_refused() {
        let out = own_dir("moved");
        let mut a = Store {
            out: out.clone(),
            ..Default::default()
        };
        assert_eq!(a.apply(vec![create("ent:a", "A")], &session()).applied, 1);
        let mut b = Store::load(&out);
        assert_eq!(b.apply(vec![create("ent:b", "B")], &session()).applied, 1);
        let r = a.apply(vec![create("ent:c", "C")], &session());
        assert_eq!(r.applied, 0);
        assert!(r.skipped[0].starts_with("store-moved:"), "{:?}", r.skipped);
        let disk = Store::load(&out);
        assert_eq!(disk.status.generation, 2);
        assert!(disk.graph.entities.contains_key("ent:b"));
        assert!(!disk.graph.entities.contains_key("ent:c"));
        let mut a = Store::load(&out);
        assert_eq!(a.apply(vec![create("ent:c", "C")], &session()).applied, 1);
        assert_eq!(Store::load(&out).status.generation, 3);
    }

    #[test]
    fn writes_land_whole_and_leave_no_temporary_files() {
        let out = own_dir("atomic");
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        assert_eq!(s.apply(vec![create("ent:a", "A")], &session()).applied, 1);
        fn walk(dir: &Path, into: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, into);
                } else {
                    into.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(&out, &mut files);
        let leftover: Vec<_> = files
            .iter()
            .filter(|p| p.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftover.is_empty(), "{:?}", leftover);
        assert!(out.join("status.yaml").vexists());
        assert!(out.join("graph/entities/ent~a.yaml").vexists());
        assert!(out.join("journal").join("g1.yaml").vexists());
    }

    // File ids: `:` as `~`, `/` as `+`, everything outside the plain set as `%XX`, and
    // only the canonical spelling decodes. Mirrors docs/compiler/graph.md#node-files.
    #[test]
    fn file_ids_round_trip_and_never_fold_under_case() {
        for (id, name) in [
            ("ent:order", "ent~order"),
            ("view:class/shop", "view~class+shop"),
            ("ent:čaj", "ent~%C4%8Daj"),
            ("rel:order~payment", "rel~order%7Epayment"),
            ("ent:Order", "ent~%4Frder"),
            (".hidden", "%2Ehidden"),
            ("req:a.b_c-1", "req~a.b_c-1"),
            ("x+y%z", "x%2By%25z"),
        ] {
            assert_eq!(file_id(id), name, "{id}");
            assert_eq!(id_from_file(name).as_deref(), Some(id), "{name}");
        }
        for bad in [
            "", "ent~%61", "ENT~a", "ent~%4", "ent~%zz", "ent~%4f", "a b", ".x",
        ] {
            assert_eq!(id_from_file(bad), None, "{bad}");
        }
        let names: BTreeSet<String> = [
            "ent:ab", "ent:Ab", "ent:AB", "ent:%41b", "ent:a~b", "ent:a:b",
        ]
        .iter()
        .map(|id| file_id(id).to_lowercase())
        .collect();
        assert_eq!(names.len(), 6, "{names:?}");
    }

    fn every_kind_graph() -> Store {
        let mut s = Store::default();
        s.graph.entities.insert("ent:order".into(), entity("Order"));
        s.graph
            .entities
            .insert("ent:payment".into(), entity("Payment"));
        s.graph.entities.insert("ent:čaj".into(), entity("Čaj"));
        let mut r = seeded_req("orders.md", "An order holds a payment.");
        r.entities = vec!["ent:order".into(), "ent:payment".into()];
        r.source = Some(mention("orders.md", "/orders", "An order holds a payment."));
        // As a commit leaves it: the requirement's source is a mention on its entities.
        for e in ["ent:order", "ent:payment"] {
            s.graph
                .entities
                .get_mut(e)
                .unwrap()
                .mentions
                .push(r.source.clone().unwrap());
        }
        r.edges = vec![ReqEdge {
            a: "ent:order".into(),
            b: "ent:payment".into(),
            rel_type: Some("association".into()),
            cardinality: None,
        }];
        r.transition = Some(Transition {
            subject: "ent:order".into(),
            from: "open".into(),
            to: "paid".into(),
            trigger: None,
            guard: None,
        });
        s.graph.requirements.insert("req:orders-1".into(), r);
        s.graph.criteria.insert(
            "crit:orders-1".into(),
            Criterion {
                requirement: "req:orders-1".into(),
                statement: "A paid order shows its payment.".into(),
                ..Default::default()
            },
        );
        s.graph.views.insert(
            "view:class/shop".into(),
            View {
                kind: "class".into(),
                title: "Shop".into(),
                members: vec!["ent:order".into(), "ent:payment".into()],
                ..Default::default()
            },
        );
        s.graph
            .diagnostics
            .insert("diag:lint-1".into(), diag("lint", vec!["req:orders-1"]));
        s.graph
            .redirects
            .insert("ent:purchase".into(), "ent:order".into());
        crate::derive::recompute_relationships(&mut s);
        crate::derive::recompute_state_machines(&mut s);
        s
    }

    fn tree_bytes(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
            let Ok(rd) = std::fs::read_dir(dir) else {
                return;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(root, &p, into);
                } else {
                    let rel = p
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    into.insert(rel, std::fs::read(&p).unwrap());
                }
            }
        }
        let mut all = BTreeMap::new();
        walk(dir, dir, &mut all);
        all
    }

    // Every kind lands one file per node and loads back whole.
    #[test]
    fn node_files_round_trip_every_kind() {
        let out = own_dir("node-round-trip");
        let mut s = every_kind_graph();
        s.out = out.clone();
        s.save();
        let files = tree_bytes(&out);
        for name in [
            "graph/entities/ent~order.yaml",
            "graph/entities/ent~%C4%8Daj.yaml",
            "graph/requirements/req~orders-1.yaml",
            "graph/criteria/crit~orders-1.yaml",
            "graph/views/view~class+shop.yaml",
            "graph/diagnostics/diag~lint-1.yaml",
            "graph/redirects/ent~purchase.yaml",
            "graph/relationships/rel~order%7Epayment.yaml",
            "graph/state-machines/sm~order.yaml",
            "status.yaml",
        ] {
            assert!(
                files.contains_key(name),
                "{name} missing from {:?}",
                files.keys()
            );
        }
        assert_eq!(
            files["graph/redirects/ent~purchase.yaml"],
            b"id: ent:purchase\nto: ent:order\n"
        );
        assert!(files.values().all(|b| b.ends_with(b"\n")));
        // Every node file starts with its id and holds only the node's own facts: no
        // generation markers, no mention a requirement adds by reference.
        let order = String::from_utf8(files["graph/entities/ent~order.yaml"].clone()).unwrap();
        assert!(order.starts_with("id: ent:order\n"), "{order}");
        assert!(
            !order.contains("mentions") && !order.contains("created"),
            "{order}"
        );
        let req = String::from_utf8(files["graph/requirements/req~orders-1.yaml"].clone()).unwrap();
        assert!(
            req.starts_with("id: req:orders-1\n") && !req.contains("updated:"),
            "{req}"
        );
        assert!(files.contains_key("markers.yaml"));
        let back = Store::load(&out);
        assert!(back.load_errors.is_empty(), "{:?}", back.load_errors);
        let json = |g: &Graph| serde_json::to_value(g).unwrap();
        assert_eq!(json(&back.graph), json(&s.graph));
    }

    // The same graph always writes the same bytes, wherever and however often it saves.
    #[test]
    fn two_saves_of_one_graph_write_identical_bytes() {
        let one = own_dir("deterministic-one");
        let two = own_dir("deterministic-two");
        let mut s = every_kind_graph();
        s.out = one.clone();
        s.save();
        let first = tree_bytes(&one);
        s.save();
        assert_eq!(tree_bytes(&one), first);
        let mut again = Store::load(&one);
        again.out = two.clone();
        again.save();
        assert_eq!(tree_bytes(&two), first);
    }

    // A node that leaves the graph takes its file along; the others stay.
    #[test]
    fn a_removed_node_deletes_its_file() {
        let out = own_dir("node-delete");
        let mut s = every_kind_graph();
        s.out = out.clone();
        s.save();
        s.graph.entities.remove("ent:čaj");
        s.graph.redirects.clear();
        s.graph.diagnostics.clear();
        s.save();
        let files = tree_bytes(&out);
        assert!(!files.contains_key("graph/entities/ent~%C4%8Daj.yaml"));
        assert!(!files.keys().any(|k| k.starts_with("graph/redirects/")));
        assert!(!files.keys().any(|k| k.starts_with("graph/diagnostics/")));
        assert!(files.contains_key("graph/entities/ent~order.yaml"));
        let back = Store::load(&out);
        assert!(!back.graph.entities.contains_key("ent:čaj"));
        assert!(back.graph.redirects.is_empty());
    }

    // A file name that spells no id is unreadable, never dropped.
    #[test]
    fn a_node_file_name_that_does_not_decode_makes_the_store_read_only() {
        let out = own_dir("node-bad-name");
        let mut s = every_kind_graph();
        s.out = out.clone();
        s.save();
        crate::vfs::write(out.join("graph/entities/Order.yaml"), "name: Order\n").unwrap();
        let back = Store::load(&out);
        assert!(
            back.load_errors.iter().any(|e| e.contains("Order.yaml")),
            "{:?}",
            back.load_errors
        );
        back.save();
        assert!(out.join("graph/entities/Order.yaml").vexists());
    }

    // A fresh checkout: only the committed state is present. The derived kinds are
    // recomputed at load, a missing journal reads as empty history, and only the
    // document whose bytes changed outside Jazyk is dirty.
    // Mirrors docs/compiler/graph.md#a-fresh-checkout.
    #[test]
    fn a_fresh_checkout_loads_committed_state_and_dirties_only_the_edited_document() {
        let out = own_dir("fresh-checkout");
        let mut s = every_kind_graph();
        s.out = out.clone();
        let a = "# Orders\nAn order holds a payment.\n\n## Paying\nAn order is paid.\n";
        let b = "# Payments\nA payment settles an order.\n";
        let parse = |docs: &[(&str, &str)]| -> crate::reconcile::ParsedDocs {
            docs.iter()
                .map(|(d, t)| (d.to_string(), (hash_hex(t), crate::md::parse_sections(t))))
                .collect()
        };
        s.sync_docs(&parse(&[("orders.md", a), ("payments.md", b)]));
        s.save();
        let derived =
            serde_json::to_value((&s.graph.relationships, &s.graph.state_machines)).unwrap();
        assert!(!s.graph.relationships.is_empty() && !s.graph.state_machines.is_empty());
        // What a clone carries: the committed paths only.
        for local in [
            "graph/default-views",
            "graph/relationships",
            "graph/state-machines",
            "journal",
            "diagrams",
            "trace",
        ] {
            crate::vfs::remove_dir_all(out.join(local)).ok();
        }
        crate::vfs::remove_file(out.join(".lock")).ok();
        crate::vfs::remove_file(out.join(BUILD_FILE)).ok();
        crate::vfs::remove_file(out.join(MARKERS_FILE)).ok();
        crate::vfs::remove_file(out.join(CAUSES_FILE)).ok();
        let committed: Vec<String> = tree_bytes(&out).into_keys().collect();
        assert!(
            committed
                .iter()
                .all(|p| crate::document_sets::committed(&format!(".jazyk/ds_main/{p}"))),
            "{committed:?}"
        );
        let mut opened = Store::load(&out);
        assert!(opened.load_errors.is_empty(), "{:?}", opened.load_errors);
        // The generation is local: a clone starts at the committed floor, and the
        // records it carries name no generation of this checkout.
        assert_eq!(opened.status.generation, generation_floor(&opened.status));
        assert!(opened.status.changes.iter().all(|c| c.generation == 0));
        assert_eq!(
            serde_json::to_value((&opened.graph.relationships, &opened.graph.state_machines))
                .unwrap(),
            derived
        );
        assert!(opened.journal_entries().is_empty());
        // The prose of one document changed on another branch.
        let b2 = "# Payments\nA payment settles an order within a day.\n";
        let dirty = opened.sync_docs_in_memory(&parse(&[("orders.md", a), ("payments.md", b2)]));
        assert_eq!(
            dirty.iter().map(|d| d.doc.as_str()).collect::<Vec<_>>(),
            vec!["payments.md"]
        );
        // A commit on the checkout continues the generation and writes the derived kinds.
        let mut opened = Store::load(&out);
        let generation = opened.status.generation;
        let r = opened.apply(vec![create("ent:refund", "Refund")], &session());
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        assert_eq!(opened.status.generation, generation + 1);
        assert!(out
            .join("graph/relationships/rel~order%7Epayment.yaml")
            .vexists());
        assert!(out.join("graph/state-machines/sm~order.yaml").vexists());
    }

    // Ids past the cap shorten to a prefix and a hash, carry the full id inside, load
    // back whole, and never share a name. Mirrors docs/compiler/graph.md#node-files.
    #[test]
    fn long_ids_shorten_carry_their_id_and_never_collide() {
        let long = |tail: &str| format!("ent:{}{}", "č".repeat(40), tail);
        let ids: Vec<String> = (0..200).map(|n| long(&format!("-{n}"))).collect();
        let mut names = BTreeSet::new();
        for id in &ids {
            let (stem, carries) = node_stem(id);
            assert!(carries && stem.len() <= FILE_ID_MAX, "{stem}");
            assert!(stem.contains("%~") && id_from_file(&stem).is_none());
            assert!(names.insert(stem.to_lowercase()), "collision on {id}");
        }
        // An id just at the cap stays plain; one byte more shortens.
        let at = format!("ent:{}", "a".repeat(FILE_ID_MAX - 4));
        assert_eq!(node_stem(&at), (file_id(&at), false));
        let over = format!("{at}a");
        assert!(node_stem(&over).1);
        // A cut never splits an escape.
        for pad in 0..3 {
            let id = format!("ent:{}{}", "a".repeat(pad), "č".repeat(80));
            let stem = node_stem(&id).0;
            let prefix = stem.split("%~").next().unwrap();
            assert!(
                !prefix.ends_with('%') && prefix.as_bytes()[prefix.len() - 2] != b'%',
                "{stem}"
            );
        }

        let out = own_dir("long-ids");
        let mut s = every_kind_graph();
        s.out = out.clone();
        for id in &ids {
            s.graph.entities.insert(id.clone(), entity("Long"));
        }
        s.graph.redirects.insert(long("-old"), "ent:order".into());
        s.save();
        let stem = node_stem(&ids[0]).0;
        let text =
            crate::vfs::read_to_string(out.join(format!("graph/entities/{stem}.yaml"))).unwrap();
        assert!(text.starts_with(&format!("id: {}\n", ids[0])), "{text}");
        let back = Store::load(&out);
        assert!(back.load_errors.is_empty(), "{:?}", back.load_errors);
        let json = |g: &Graph| serde_json::to_value(g).unwrap();
        assert_eq!(json(&back.graph), json(&s.graph));
        // Removing a long node removes its file.
        let mut s = back;
        s.graph.entities.remove(&ids[0]);
        s.save();
        assert!(!out.join(format!("graph/entities/{stem}.yaml")).vexists());
        // A shortened name whose carried id spells another name is unreadable.
        crate::vfs::write(
            out.join(format!("graph/entities/{stem}.yaml")),
            format!("id: {}\nname: Wrong\n", ids[1]),
        )
        .unwrap();
        assert!(!Store::load(&out).load_errors.is_empty());
    }

    // The generation is local: a load raises build.yaml to the highest generation the
    // committed status names. Mirrors docs/compiler/graph.md#the-generation.
    #[test]
    fn the_generation_starts_at_the_committed_floor() {
        let out = own_dir("generation-floor");
        let mut s = every_kind_graph();
        s.out = out.clone();
        s.status.generation = 12;
        s.status.changes.push(ChangeRecord::new(
            12,
            1,
            1,
            CHANGE_ENTITY,
            "ent:order",
            "name",
        ));
        s.save();
        let status = crate::vfs::read_to_string(out.join("status.yaml")).unwrap();
        for key in ["generation:", "verdict:", "costs:", "spent:"] {
            assert!(
                !status.lines().any(|l| l.starts_with(key)),
                "{key} in {status}"
            );
        }
        assert_eq!(read_generation(&out), 12);
        crate::vfs::remove_file(out.join(BUILD_FILE)).unwrap();
        assert_eq!(
            Store::load(&out).status.generation,
            12,
            "a checkout starts at the floor"
        );
        crate::vfs::write(out.join(BUILD_FILE), "generation: 3\n").unwrap();
        let mut low = Store::load(&out);
        assert_eq!(
            low.status.generation, 12,
            "a stale local counter rises to the floor"
        );
        let r = low.apply(vec![create("ent:refund", "Refund")], &session());
        assert_eq!((r.applied, r.generation), (1, 13), "{:?}", r.skipped);
        assert_eq!(
            read_generation(&out),
            low.status.generation,
            "the commit wrote the raised counter"
        );
        crate::vfs::write(out.join(BUILD_FILE), "generation: 20\n").unwrap();
        assert_eq!(Store::load(&out).status.generation, 20);
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                copy_dir(&p, &to.join(e.file_name()));
            } else {
                std::fs::copy(&p, to.join(e.file_name())).unwrap();
            }
        }
    }

    // Two branches from one base each commit and build. The committed status.yaml
    // merges as text with no conflict, where the local build.yaml (both bumped the
    // generation) would conflict. A run that changes nothing in the graph leaves the
    // committed status.yaml bytes alone. Mirrors docs/compiler/graph.md#the-generation.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parallel_branches_merge_status_without_conflict() {
        let root = own_dir("status-merge");
        let (base, a, b) = (root.join("base"), root.join("a"), root.join("b"));
        let mut s = every_kind_graph();
        s.out = base.clone();
        s.save();
        let base_status = std::fs::read(base.join("status.yaml")).unwrap();
        // A run that only spends and judges convergence.
        let mut run = Store::load(&base);
        run.status.costs.sessions = 3;
        run.status.costs.tokens = 9000;
        run.status.spent.rounds = 7;
        run.status.verdict.state = "converged".into();
        run.save_status();
        assert_eq!(
            std::fs::read(base.join("status.yaml")).unwrap(),
            base_status
        );
        copy_dir(&base, &a);
        copy_dir(&base, &b);
        // Branch a commits once and keeps its open change records.
        let mut sa = Store::load(&a);
        assert_eq!(
            sa.apply(vec![create("ent:alpha", "Alpha")], &session())
                .applied,
            1
        );
        sa.status.costs.tokens = 111;
        sa.save_status();
        // Branch b commits twice, then converges: its records clear.
        let mut sb = Store::load(&b);
        assert_eq!(
            sb.apply(vec![create("ent:beta", "Beta")], &session())
                .applied,
            1
        );
        assert_eq!(
            sb.apply(vec![create("ent:gamma", "Gamma")], &session())
                .applied,
            1
        );
        let ids: Vec<String> = sb.status.changes.iter().map(|c| c.id.clone()).collect();
        sb.clear_changes(&ids);
        sb.status.costs.tokens = 222;
        sb.save_status();
        let merge = |file: &str| {
            std::process::Command::new("git")
                .args(["merge-file", "-p"])
                .arg(a.join(file))
                .arg(base.join(file))
                .arg(b.join(file))
                .output()
        };
        let Ok(merged) = merge("status.yaml") else {
            eprintln!("no git binary; the status split was checked without a text merge");
            return;
        };
        assert!(
            merged.status.success(),
            "status.yaml conflicts:\n{}",
            String::from_utf8_lossy(&merged.stdout)
        );
        // b ended where the base was, so the merge is a's status.
        assert_eq!(merged.stdout, std::fs::read(a.join("status.yaml")).unwrap());
        assert_eq!(std::fs::read(b.join("status.yaml")).unwrap(), base_status);
        // The local half both branches bumped is exactly what would have conflicted.
        let local = merge(BUILD_FILE).unwrap();
        assert!(!local.status.success(), "build.yaml merged cleanly");
        // The merged checkout: a's tree, b's nodes, the merged status, a's local file.
        copy_dir(&b.join("graph"), &a.join("graph"));
        std::fs::write(a.join("status.yaml"), &merged.stdout).unwrap();
        let m = Store::load(&a);
        assert!(m.load_errors.is_empty(), "{:?}", m.load_errors);
        for id in ["ent:alpha", "ent:beta", "ent:gamma"] {
            assert!(m.graph.entities.contains_key(id), "{id}");
        }
        assert!(m.status.generation >= generation_floor(&m.status));
    }

    // An entity no document states takes its mentions from the requirements naming it.
    // The first is the most upstream one by link level from the roots, not the first by
    // path: `catalog.md` sorts before `system.md`, but the root `system.md` links to it.
    // Mirrors docs/compiler/graph.md#node-files.
    #[test]
    fn by_reference_mentions_follow_link_levels_from_the_roots() {
        let out = own_dir("mention-levels").join(".jazyk/ds_main");
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        seed_doc(
            &mut s,
            "system.md",
            "# System\nThe system serves a customer. See [the catalog](catalog.md).\n",
        );
        seed_doc(
            &mut s,
            "catalog.md",
            "# Catalog\nA customer browses the catalog.\n",
        );
        s.status.roots = vec!["system.md".into()];
        let mut customer = entity("Customer");
        customer.provenance = Some(derived(&["req:x"]));
        let req = |doc: &str, sec: &str, quote: &str| Op::CreateRequirement {
            id: String::new(),
            requirement: Requirement {
                statement: quote.into(),
                entities: vec!["ent:customer".into()],
                source: Some(mention(doc, sec, quote)),
                facets: vec![Facet {
                    facet: "behavior".into(),
                    reasoning: "stated".into(),
                    measure: None,
                }],
                ..Default::default()
            },
        };
        // The catalog requirement lands first, so neither commit order nor path order
        // would put the root first.
        let r = s.apply(
            vec![
                Op::CreateEntity {
                    id: "ent:customer".into(),
                    entity: customer,
                },
                req("catalog.md", "/catalog", "A customer browses the catalog."),
            ],
            &session(),
        );
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        let r = s.apply(
            vec![req("system.md", "/system", "The system serves a customer.")],
            &session(),
        );
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        let docs = |st: &Store| -> Vec<String> {
            st.graph.entities["ent:customer"]
                .mentions
                .iter()
                .map(|m| m.doc.clone())
                .collect()
        };
        assert_eq!(docs(&s), ["system.md", "catalog.md"]);
        let loaded = Store::load(&out);
        assert!(loaded.load_errors.is_empty(), "{:?}", loaded.load_errors);
        assert_eq!(docs(&loaded), ["system.md", "catalog.md"]);
        // With no roots stamped, every document is its own level in path order.
        let mut no_roots = loaded;
        no_roots.status.roots.clear();
        let roots: Vec<String> = Vec::new();
        normalize_mentions(&mut no_roots.graph, &no_roots.docs, &roots, false);
        assert_eq!(docs(&no_roots), ["catalog.md", "system.md"]);
    }

    // Two branches from one base each extract a requirement about the same entity from
    // different sections and commit. Git merges the whole store with no conflict: the
    // ids carry no counter, the entity file holds no by-reference mention, and every
    // change record is a file of its own. Mirrors docs/compiler/graph.md#node-files.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parallel_branches_merge_the_whole_store_through_git() {
        let root = own_dir("git-merge");
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(&root)
                .output()
        };
        if git(&["init", "-q", "-b", "base"])
            .map(|o| !o.status.success())
            .unwrap_or(true)
        {
            eprintln!("no git binary; the merge was not exercised");
            return;
        }
        let out = root.join(".jazyk/ds_main");
        let text = "# Shop\nThe shop sells tea.\n\n## Orders\nAn order holds items.\n\n## Payments\nA payment settles an order.\n";
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        seed_doc(&mut s, "shop.md", text);
        let r = s.apply(vec![create("ent:order", "Order")], &session());
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        assert!(
            root.join(".jazyk/.gitignore").vexists(),
            "a save writes the ignore file"
        );
        let req = |section: &str, quote: &str| Op::CreateRequirement {
            id: String::new(),
            requirement: Requirement {
                statement: quote.into(),
                entities: vec!["ent:order".into()],
                source: Some(mention("shop.md", section, quote)),
                facets: vec![Facet {
                    facet: "behavior".into(),
                    reasoning: "stated".into(),
                    measure: None,
                }],
                ..Default::default()
            },
        };
        let commit = |msg: &str| {
            let add = git(&["add", "-A"]).unwrap();
            assert!(
                add.status.success(),
                "{}",
                String::from_utf8_lossy(&add.stderr)
            );
            let c = git(&["commit", "-q", "-m", msg]).unwrap();
            assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
        };
        commit("base");
        assert!(git(&["checkout", "-q", "-b", "a"])
            .unwrap()
            .status
            .success());
        let mut sa = Store::load(&out);
        let r = sa.apply(
            vec![req("/shop/orders", "An order holds items.")],
            &session(),
        );
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        commit("a");
        assert!(git(&["checkout", "-q", "base"]).unwrap().status.success());
        assert!(git(&["checkout", "-q", "-b", "b"])
            .unwrap()
            .status
            .success());
        let mut sb = Store::load(&out);
        let r = sb.apply(
            vec![req("/shop/payments", "A payment settles an order.")],
            &session(),
        );
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        commit("b");
        let merged = git(&["merge", "-q", "--no-ff", "--no-edit", "a"]).unwrap();
        assert!(
            merged.status.success(),
            "the store conflicted:\n{}{}",
            String::from_utf8_lossy(&merged.stdout),
            String::from_utf8_lossy(&merged.stderr)
        );
        let m = Store::load(&out);
        assert!(m.load_errors.is_empty(), "{:?}", m.load_errors);
        assert_eq!(
            m.graph.requirements.len(),
            2,
            "{:?}",
            m.graph.requirements.keys()
        );
        let ids: BTreeSet<&String> = m.graph.requirements.keys().collect();
        assert!(
            ids.iter().any(|i| i.starts_with("req:shop-orders-")),
            "{ids:?}"
        );
        assert!(
            ids.iter().any(|i| i.starts_with("req:shop-payments-")),
            "{ids:?}"
        );
        // Both sources come back as mentions on the entity both branches named.
        assert_eq!(
            m.graph.entities["ent:order"].mentions.len(),
            2,
            "{:?}",
            m.graph.entities["ent:order"].mentions
        );
        let order = std::fs::read_to_string(out.join("graph/entities/ent~order.yaml")).unwrap();
        assert!(!order.contains("mentions"), "{order}");
        // Each branch's change records survive the merge.
        for id in &ids {
            assert!(
                m.status.has_change(CHANGE_REQ_CREATED, id),
                "{id}: {:?}",
                m.status.changes
            );
        }
        // Nothing local was committed.
        let tracked = git(&["ls-files"]).unwrap();
        let tracked = String::from_utf8_lossy(&tracked.stdout);
        for local in [
            "build.yaml",
            "markers.yaml",
            "causes.yaml",
            "journal/",
            "relationships/",
            "default-views/",
        ] {
            assert!(!tracked.contains(local), "{local} tracked:\n{tracked}");
        }
        assert!(tracked.contains(".jazyk/ds_main/changes/"), "{tracked}");
    }

    // A version 3 store loads as it is, and its next save writes the version 4 layout
    // with every id and fact kept. Mirrors docs/compiler/graph.md#store-version.
    #[test]
    fn a_version_3_store_upgrades_in_place_at_its_next_save() {
        let out = own_dir("v3-upgrade");
        let w = |rel: &str, text: &str| {
            let p = out.join(rel);
            crate::vfs::create_dir_all(p.parent().unwrap()).unwrap();
            crate::vfs::write(p, text).unwrap();
        };
        w("status.yaml", "version: 3\nchanges:\n- id: c4-1\n  generation: 4\n  mutation: 1\n  kind: entity-changed\n  subject: ent:order\n  via: name\nlookalikes:\n- pair: ent:order~ent:purchase\n  verdict: separate\n  score: 0.6\n  generation: 3\nroots:\n- shop.md\n");
        w("build.yaml", "generation: 4\n");
        w("graph/entities/ent~order.yaml", "name: Order\nmentions:\n- doc: shop.md\n  section: /shop\n  quote: An order holds items.\ncreated: g1\nupdated: g4\n");
        w("graph/requirements/req~shop-1.yaml", "statement: An order holds items.\nentities:\n- ent:order\nfacets:\n- facet: behavior\n  reasoning: stated\nsource:\n  doc: shop.md\n  section: /shop\n  quote: An order holds items.\ncreated: g2\nupdated: g2\n");
        w("graph/views/view~class+shop.yaml", "kind: class\ntitle: Shop\nmembers:\n- ent:order\nprovenance:\n  derived:\n    from:\n    - ent:order\n    reasoning: 'default view: x'\ndefault: true\n");
        w("graph/diagnostics/diag~lint-1.yaml", "rule: lint\nseverity: info\nsubjects:\n- req:shop-1\nmessage: m\nlifecycle: open\ncreated: g3\nupdated: g3\n");
        w("docs/shop.md.yaml", "contentHash: abc\nsections:\n  /shop:\n    title: Shop\n    kind: root\n    order: 0\n    raw: |-\n      # Shop\n      An order holds items.\n    hash: h1\n    lines:\n    - 0\n    - 2\n");
        w(
            "gen/ledger.yaml",
            "entities:\n  order:\n    factHash: f\n    requirements:\n    - req:shop-1\n",
        );
        let s = Store::load(&out);
        assert!(s.load_errors.is_empty(), "{:?}", s.load_errors);
        assert_eq!(s.graph.entities["ent:order"].updated.as_deref(), Some("g4"));
        assert_eq!(s.graph.entities["ent:order"].mentions.len(), 1);
        assert!(s.status.has_change(CHANGE_ENTITY, "ent:order"));
        assert_eq!(s.status.lookalikes.len(), 1);
        assert_eq!(s.docs["shop.md"].content_hash, "abc");
        assert_eq!(s.docs["shop.md"].sections["/shop"].lines, [0, 2]);
        let ledger = crate::gen::Ledger::load(&out);
        assert!(ledger.entities.contains_key("order"));
        let before =
            serde_json::to_value((&s.graph, &s.status.changes, &s.status.lookalikes)).unwrap();
        s.save();
        ledger.save(&out).unwrap();
        let status = std::fs::read_to_string(out.join("status.yaml")).unwrap();
        assert!(
            status.contains("version: 4")
                && !status.contains("changes")
                && !status.contains("lookalikes"),
            "{status}"
        );
        assert!(out.join("changes/entity-changed/ent~order.yaml").vexists());
        assert!(out
            .join("lookalikes/ent~order%7Eent~purchase.yaml")
            .vexists());
        let order = std::fs::read_to_string(out.join("graph/entities/ent~order.yaml")).unwrap();
        assert_eq!(order, "id: ent:order\nname: Order\n");
        // A default view is never committed: the recompute at load owns it.
        assert!(!out.join("graph/views/view~class+shop.yaml").vexists());
        let doc = std::fs::read_to_string(out.join("docs/shop.md.yaml")).unwrap();
        assert!(
            !doc.contains("contentHash") && !doc.contains("lines"),
            "{doc}"
        );
        assert!(std::fs::read_to_string(out.join(BUILD_FILE))
            .unwrap()
            .contains("shop.md: abc"));
        let wide = std::fs::read_to_string(out.join("gen/ledger.yaml")).unwrap();
        assert!(!wide.contains("factHash"), "{wide}");
        assert!(std::fs::read_to_string(out.join("gen/entities/order.yaml"))
            .unwrap()
            .starts_with("id: order\n"));
        // The upgraded store reads back the same graph, markers and all.
        let back = Store::load(&out);
        assert!(back.load_errors.is_empty(), "{:?}", back.load_errors);
        assert_eq!(back.status.version, STORE_VERSION);
        assert_eq!(
            serde_json::to_value((&back.graph, &back.status.changes, &back.status.lookalikes))
                .unwrap(),
            before
        );
        assert_eq!(back.docs["shop.md"].sections["/shop"].lines, [0, 2]);
        assert_eq!(back.docs["shop.md"].content_hash, "abc");
        assert_eq!(
            crate::gen::Ledger::load(&out).entities["order"].fact_hash,
            "f"
        );
        // A cleared record deletes its file.
        let mut s = back;
        s.status.changes.clear();
        s.save_status();
        assert!(!out.join("changes/entity-changed/ent~order.yaml").vexists());
    }

    // A status.yaml that does not parse is a broken file, not another version: a build
    // leaves it in place and the store is read-only.
    // Mirrors docs/compiler/graph.md#store-version.
    #[test]
    fn a_status_that_does_not_parse_is_never_archived() {
        let out = own_dir("status-torn");
        crate::vfs::create_dir_all(out.join("graph/entities")).unwrap();
        crate::vfs::write(out.join("status.yaml"), "version: [4\n").unwrap();
        crate::vfs::write(
            out.join("graph/entities/ent~order.yaml"),
            "id: ent:order\nname: Order\n",
        )
        .unwrap();
        let s = Store::open_for_build(&out);
        assert!(!s.load_errors.is_empty());
        assert!(out.join("graph/entities/ent~order.yaml").vexists());
        assert!(!out
            .with_file_name(format!(
                "{}.bak",
                out.file_name().unwrap().to_string_lossy()
            ))
            .vexists());
        s.save();
        assert_eq!(
            std::fs::read_to_string(out.join("status.yaml")).unwrap(),
            "version: [4\n"
        );
    }

    // Section line ranges recompute from the raw texts exactly as the parse gave them.
    #[test]
    fn section_lines_recompute_from_raw_texts() {
        let text = "Intro line.\n\n# Top\nBody.\n\n```mermaid\na\n```\n\n## Child\nMore.\n\n# Second\nEnd.\n";
        let parsed = crate::md::parse_sections(text);
        let mut rec = DocRecord {
            content_hash: String::new(),
            sections: parsed.clone(),
            coverage: BTreeMap::new(),
        };
        for s in rec.sections.values_mut() {
            s.lines = [0, 0];
        }
        fill_section_lines(&mut rec);
        for (r, s) in &parsed {
            assert_eq!(rec.sections[r].lines, s.lines, "{r}");
        }
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
        wi().commit(1, None)
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

    // Mirrors docs/compiler/model/criterion.md: a criterion keys on its requirement plus
    // statement, a restatement is refused, a statement change on the requirement marks
    // its derived criteria revised, and a deleted requirement takes its criteria along.
    #[test]
    fn criteria_key_on_their_requirement_and_follow_it() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(
            &mut s,
            "t.md",
            "# T\nThe sort utility outputs its lines in ascending order.\n\nGiven the lines 321, 654, 453 the utility outputs 321, 453, 654.\n",
        );
        let quoted = |st: &str| Criterion {
            requirement: "req:t-1".into(),
            statement: st.into(),
            kind: "positive".into(),
            source: Some(mention("t.md", "/t", "Given the lines 321, 654, 453")),
            ..Default::default()
        };
        let ops = vec![
            create("prov:1", "Sort"),
            Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    statement: "The sort utility outputs its lines in ascending order.".into(),
                    entities: vec!["prov:1".into()],
                    source: Some(mention(
                        "t.md",
                        "/t",
                        "outputs its lines in ascending order",
                    )),
                    ..Default::default()
                },
            },
            Op::CreateCriterion {
                id: "crit:new-1".into(),
                criterion: quoted(
                    "Given the lines 321, 654, 453, the utility outputs 321, 453, 654.",
                ),
            },
            Op::CreateCriterion {
                id: "crit:new-2".into(),
                criterion: Criterion {
                    requirement: "req:t-1".into(),
                    statement: "Given one line, the utility outputs that line unchanged.".into(),
                    kind: "boundary".into(),
                    provenance: Some(Provenance::Derived {
                        from: vec!["req:t-1".into()],
                        reasoning: "the edge of the ordering rule".into(),
                    }),
                    ..Default::default()
                },
            },
            Op::CreateCriterion {
                id: "crit:new-3".into(),
                criterion: quoted("The sort utility outputs its lines in ascending order"),
            },
            Op::CreateCriterion {
                id: "crit:new-4".into(),
                criterion: quoted(
                    "Given the lines 321, 654, 453, the utility outputs 321, 453, 654",
                ),
            },
        ];
        let r = s.apply(ops, &wi().commit(3, Some(100)));
        assert!(
            r.skipped.iter().any(|x| x.contains("criterion-restates")),
            "{:?}",
            r.skipped
        );
        let ids: Vec<&String> = s.graph.criteria.keys().collect();
        assert_eq!(ids.len(), 2, "{:?}", ids);
        assert!(ids.iter().all(|id| id.starts_with("crit:t-")), "{:?}", ids);
        // The quoted criterion and the derived one, by provenance, not by mint order.
        let quoted_id = s
            .graph
            .criteria
            .iter()
            .find(|(_, c)| c.provenance.is_none())
            .map(|(id, _)| id.clone())
            .unwrap();
        let derived_id = s
            .graph
            .criteria
            .iter()
            .find(|(_, c)| c.provenance.is_some())
            .map(|(id, _)| id.clone())
            .unwrap();
        assert_eq!(s.criteria_of("req:t-1").len(), 2);
        assert!(s.status.has_change(CHANGE_CRIT_CREATED, &quoted_id));
        assert!(s.status.has_change(CHANGE_CRIT_CREATED, &derived_id));
        // The requirement's statement changes: the derived criterion is revised, the
        // quoted one keeps its own quote.
        s.apply(
            vec![Op::UpdateRequirement {
                id: "req:t-1".into(),
                statement: Some(
                    "The sort utility outputs its lines in ascending order by default.".into(),
                ),
                entities: None,
                edges: None,
                transition: None,
                facets: None,
                source: None,
                provenance: None,
            }],
            &wi().commit(4, Some(100)),
        );
        assert!(s.status.has_change(CHANGE_CRIT_REVISED, &derived_id));
        assert!(!s.status.has_change(CHANGE_CRIT_REVISED, &quoted_id));
        // The requirement goes: its criteria go with it.
        s.apply(
            vec![Op::DeleteRequirement {
                id: "req:t-1".into(),
                reason: "gone".into(),
            }],
            &wi().commit(5, Some(100)),
        );
        assert!(s.graph.criteria.is_empty());
        assert!(s.status.has_change(CHANGE_CRIT_DELETED, &quoted_id));
        assert!(s.status.has_change(CHANGE_CRIT_DELETED, &derived_id));
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
        serde_norway::from_str(&crate::vfs::read_to_string(&path).unwrap()).unwrap()
    }

    // A dual write lands the prose and the graph as one changeset, absorbs its own
    // hashes so the document is not dirty against the graph, re-anchors the quoted
    // facts in the section, and puts the file back when the commit skips.
    // Mirrors docs/compiler/compilation.md#edit-paths.
    // An answer whose option carried a suggested edit is a human edit too: the commit
    // lands as `answer` with the prose edit and writes no `entity-changed`.
    // Mirrors docs/compiler/compilation.md#edit-paths.
    #[test]
    fn an_answer_carrying_an_edit_writes_no_entity_changed() {
        let root = own_dir("answer-edit");
        let text = "# Pay\n\nAn Order is paid within 30 days.\n";
        crate::vfs::create_dir_all(root.join("docs")).unwrap();
        crate::vfs::write(root.join("docs/pay.md"), text).unwrap();
        let mut s = Store {
            out: root.join(".jazyk"),
            ..Default::default()
        };
        let mut parsed = BTreeMap::new();
        parsed.insert(
            "docs/pay.md".to_string(),
            (hash_hex(text), crate::md::parse_sections(text)),
        );
        s.sync_docs(&parsed);
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
        let setup: Vec<String> = s.status.changes.iter().map(|c| c.id.clone()).collect();
        s.clear_changes(&setup);
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
                &Commit::store("answer"),
                None,
            )
            .unwrap();
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        assert!(
            !s.status
                .changes
                .iter()
                .any(|c| c.kind == CHANGE_ENTITY && c.subject == "ent:order"),
            "{:?}",
            s.status.changes
        );
        assert!(
            s.status
                .changes
                .iter()
                .any(|c| c.kind == CHANGE_REQ_REVISED && c.subject == "req:pay-1"),
            "{:?}",
            s.status.changes
        );
    }

    #[test]
    fn dual_write_absorbs_its_own_hashes_and_rolls_back_a_stale_edit() {
        let root = own_dir("dual-write");
        let text = "# Pay\n\nAn Order is paid within 30 days.\n";
        crate::vfs::create_dir_all(root.join("docs")).unwrap();
        crate::vfs::write(root.join("docs/pay.md"), text).unwrap();
        let mut s = Store {
            out: root.join(".jazyk"),
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
        // The setup's own records go, so what the dual write writes stands alone.
        let setup: Vec<String> = s.status.changes.iter().map(|c| c.id.clone()).collect();
        s.clear_changes(&setup);
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
        let on_disk = crate::vfs::read_to_string(root.join("docs/pay.md")).unwrap();
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
        // A human edit is the review: the mention opens no review of the Order, while
        // the revised statement keeps its pair judgment against other requirements.
        // Mirrors docs/compiler/compilation.md#edit-paths.
        assert!(
            !s.status.has_change(CHANGE_ENTITY, "ent:order"),
            "{:?}",
            s.status.changes
        );
        assert!(
            s.status.has_change(CHANGE_REQ_REVISED, "req:pay-1"),
            "{:?}",
            s.status.changes
        );
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
            crate::vfs::read_to_string(root.join("docs/pay.md")).unwrap(),
            on_disk
        );
        assert!(s.graph.requirements.contains_key("req:pay-1"));
        // A prose edit never lands alone.
        assert!(s
            .dual_write(&root, &edit, Vec::new(), &Commit::store("dual-write"), None)
            .is_err());
        // An empty old_text appends the sentence to the section's body.
        let on_disk = crate::vfs::read_to_string(root.join("docs/pay.md")).unwrap();
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
        crate::vfs::remove_dir_all(&root).ok();
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
        let mut commit = wi().commit(2, Some(10));
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
        // The created record fed no pair (the requirement has no neighbor), so the
        // sweep behind the second commit cleared it; the entity record stood until
        // cleared by id, persisted. Mirrors docs/compiler/reconciler.md#pairs.
        assert!(!s.status.has_change(CHANGE_REQ_CREATED, "req:t-1"));
        let ids = s.status.change_ids(&[CHANGE_ENTITY], "ent:cart");
        assert_eq!(ids.len(), 1);
        s.clear_changes(&ids);
        assert!(!s.status.has_change(CHANGE_ENTITY, "ent:cart"));
        assert_eq!(Store::load(&s.out).status.changes.len(), 0);
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
            &wi().commit(1, Some(10)),
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
            &wi().commit(1, Some(10)),
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
        // A second fact that repeats the first's words and adds a clause is not a
        // rewording: the deadline and the cancellation it triggers are two statements.
        assert!(!statement_rewords(
            "An Order is paid within 21 days of placement.",
            "When an Order is not paid within 21 days of placement, Orderly cancels it."
        ));
        assert!(!statement_rewords(
            "The Shop sends a confirmation email.",
            "The Shop does not send a confirmation email."
        ));
        assert!(statement_rewords(
            "The Admin CLI creates a user.",
            "The Admin CLI creates a user using createUser."
        ));
        // An obligation modal and a verb ending restate the same fact; a local model
        // minted "the system empties the Cart" beside "shall empty" off one quote.
        assert!(statement_rewords(
            "When the customer checks out, the system shall empty the Cart.",
            "When the customer checks out, the system empties the Cart."
        ));
        // A permission is not the obligation.
        assert!(!statement_rewords(
            "The Customer must cancel the Order.",
            "The Customer can cancel the Order."
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
        let r = s.apply(ops, &wi().commit(3, Some(100)));
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

    // A rewire or resolve that makes two ids equal anywhere in a list leaves one, in
    // first-occurrence order, not only when the two sat side by side.
    #[test]
    fn merge_and_resolve_drop_non_adjacent_duplicates() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nbuyer, cart, and customer\n");
        s.apply(
            vec![
                create("ent:buyer", "Buyer"),
                create("ent:cart", "Cart"),
                create("ent:customer", "Customer"),
                Op::CreateRequirement {
                    id: "req:t-1".into(),
                    requirement: Requirement {
                        statement: "The buyer fills the cart for the customer.".into(),
                        entities: vec![
                            "ent:buyer".into(),
                            "ent:cart".into(),
                            "ent:customer".into(),
                        ],
                        source: Some(mention("t.md", "/t", "buyer, cart, and customer")),
                        ..Default::default()
                    },
                },
                Op::CreateView {
                    id: "view:class/shop".into(),
                    view: View {
                        kind: "class".into(),
                        title: "Shop".into(),
                        members: vec!["ent:buyer".into(), "ent:cart".into(), "ent:customer".into()],
                        provenance: Some(derived(&["ent:buyer", "ent:cart", "ent:customer"])),
                        ..Default::default()
                    },
                },
            ],
            &session(),
        );
        let r = s.apply(
            vec![Op::MergeEntities {
                keep: "ent:buyer".into(),
                absorb: "ent:customer".into(),
                reason: "same concept".into(),
            }],
            &session(),
        );
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        assert_eq!(
            s.graph.requirements["req:t-1"].entities,
            vec!["ent:buyer", "ent:cart"]
        );
        assert_eq!(
            s.graph.views["view:class/shop"].members,
            vec!["ent:buyer", "ent:cart"]
        );
        // A staged list naming the absorbed id and its survivor apart resolves to one.
        let r = s.apply(
            vec![Op::CreateView {
                id: "view:class/again".into(),
                view: View {
                    kind: "class".into(),
                    title: "Again".into(),
                    members: vec!["ent:customer".into(), "ent:cart".into(), "ent:buyer".into()],
                    collapse: vec!["ent:customer".into(), "ent:cart".into(), "ent:buyer".into()],
                    provenance: Some(derived(&["ent:buyer"])),
                    ..Default::default()
                },
            }],
            &session(),
        );
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        let v = s.graph.views.values().find(|v| v.title == "Again").unwrap();
        assert_eq!(v.members, vec!["ent:buyer", "ent:cart"]);
        assert_eq!(v.collapse, vec!["ent:buyer", "ent:cart"]);
    }

    // A merge carries the absorbed entity's `not_about`, `distinct_from`, and `limits`
    // (the higher bump on a clash) and drops the pair's own note on each other.
    // Mirrors docs/compiler/graph.md#mutations.
    #[test]
    fn merge_unions_not_about_distinct_from_and_limits() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nbuyer and customer\n");
        let with = |name: &str,
                    not_about: &[&str],
                    distinct: &[(&str, &str)],
                    limits: &[(&str, u64)]| Entity {
            not_about: not_about.iter().map(|x| x.to_string()).collect(),
            distinct_from: distinct
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            limits: limits
                .iter()
                .map(|(k, v)| (k.to_string(), LimitBump { value: *v }))
                .collect(),
            ..entity(name)
        };
        s.apply(
            vec![
                create("ent:payment", "Payment"),
                Op::CreateEntity {
                    id: "ent:customer".into(),
                    entity: with(
                        "Customer",
                        &["req:a-1"],
                        &[("ent:buyer", "told apart")],
                        &[
                            ("requirements-per-entity", 70),
                            ("attributes-per-entity", 40),
                        ],
                    ),
                },
                Op::CreateEntity {
                    id: "ent:buyer".into(),
                    entity: with(
                        "Buyer",
                        &["req:b-1", "req:a-1"],
                        &[("ent:payment", "the payer, not the payment")],
                        &[("requirements-per-entity", 90), ("children-per-entity", 30)],
                    ),
                },
            ],
            &session(),
        );
        let r = s.apply(
            vec![Op::MergeEntities {
                keep: "ent:customer".into(),
                absorb: "ent:buyer".into(),
                reason: "same concept".into(),
            }],
            &session(),
        );
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        let k = &s.graph.entities["ent:customer"];
        assert_eq!(k.not_about, vec!["req:a-1", "req:b-1"]);
        assert_eq!(
            k.distinct_from.keys().collect::<Vec<_>>(),
            vec!["ent:payment"],
            "the pair's own note goes, the absorbed one's other notes stay"
        );
        assert_eq!(k.limits["requirements-per-entity"].value, 90);
        assert_eq!(k.limits["attributes-per-entity"].value, 40);
        assert_eq!(k.limits["children-per-entity"].value, 30);
    }

    // Redirects stay one hop: absorbing a survivor repoints the redirects into it, and
    // a chain an older store left behind still resolves past eight hops.
    // Mirrors docs/compiler/graph.md#storage-layout.
    #[test]
    fn merges_compact_redirect_chains() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nshop\n");
        s.apply(
            vec![
                create("ent:a", "A"),
                create("ent:b", "B"),
                create("ent:c", "C"),
            ],
            &session(),
        );
        for (keep, absorb) in [("ent:b", "ent:a"), ("ent:c", "ent:b")] {
            let r = s.apply(
                vec![Op::MergeEntities {
                    keep: keep.into(),
                    absorb: absorb.into(),
                    reason: "same".into(),
                }],
                &session(),
            );
            assert_eq!(r.applied, 1, "{:?}", r.skipped);
        }
        assert_eq!(s.graph.redirects["ent:a"], "ent:c");
        assert_eq!(s.graph.redirects["ent:b"], "ent:c");
        let mut old = Store::default();
        old.graph.entities.insert("ent:end".into(), entity("End"));
        for i in 0..12 {
            let to = if i == 11 {
                "ent:end".to_string()
            } else {
                format!("ent:x{}", i + 1)
            };
            old.graph.redirects.insert(format!("ent:x{}", i), to);
        }
        assert_eq!(old.resolve_id("ent:x0"), "ent:end");
        // A cycle stops instead of spinning.
        old.graph.redirects.insert("ent:p".into(), "ent:q".into());
        old.graph.redirects.insert("ent:q".into(), "ent:p".into());
        let _ = old.resolve_id("ent:p");
    }

    // Clearing a parent is a move: the journal records the prior parent, the parent
    // left gets its `parent` record, the child no `fields` record, and the reparent
    // history reads the clear. Mirrors docs/compiler/graph.md#journal.
    #[test]
    fn clearing_a_parent_is_journaled_as_a_move() {
        let mut s = Store {
            out: own_dir("clear-parent"),
            ..Default::default()
        };
        seed_doc(&mut s, "t.md", "# T\nshop\n");
        s.apply(
            vec![
                create("ent:orders", "Orders"),
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
        let r = s.apply(
            vec![Op::UpdateEntity {
                id: "ent:parcel".into(),
                name: None,
                definition: None,
                add_aliases: Vec::new(),
                remove_aliases: Vec::new(),
                clear_parent: true,
                not_about_add: Vec::new(),
                add_mention: None,
                stereotype: None,
                parent: None,
                set_attributes: None,
                add_attributes: Vec::new(),
                provenance: None,
            }],
            &session(),
        );
        assert_eq!(r.applied, 1, "{:?}", r.skipped);
        assert_eq!(s.graph.entities["ent:parcel"].parent, None);
        let records: Vec<(String, String)> = r
            .changes
            .iter()
            .filter(|c| c.kind == CHANGE_ENTITY)
            .map(|c| (c.subject.clone(), c.via.clone()))
            .collect();
        assert!(
            records.contains(&("ent:orders".into(), "parent".into())),
            "{:?}",
            records
        );
        assert!(
            !records.iter().any(|(s, _)| s == "ent:parcel"),
            "{:?}",
            records
        );
        let entry = journal_entry(&s, r.generation);
        let moves = parent_moves_in(std::slice::from_ref(&entry));
        assert_eq!(moves.len(), 1, "{:?}", entry.mutations);
        assert_eq!(moves[0].child, "ent:parcel");
        assert_eq!(moves[0].from.as_deref(), Some("ent:orders"));
        assert_eq!(moves[0].to, None);
    }

    // A deleted requirement's records name its own journal mutation; each cascaded
    // criterion delete names its own. Mirrors docs/compiler/graph.md#change-records.
    #[test]
    fn a_requirement_delete_names_its_own_mutation() {
        let mut s = Store {
            out: own_dir("delete-cascade"),
            ..Default::default()
        };
        seed_doc(
            &mut s,
            "t.md",
            "# T\nThe sort utility outputs its lines in ascending order.\n",
        );
        s.apply(
            vec![
                create("ent:sort", "Sort"),
                Op::CreateRequirement {
                    id: "req:t-1".into(),
                    requirement: Requirement {
                        statement: "The sort utility outputs its lines in ascending order.".into(),
                        entities: vec!["ent:sort".into()],
                        source: Some(mention(
                            "t.md",
                            "/t",
                            "outputs its lines in ascending order",
                        )),
                        ..Default::default()
                    },
                },
                Op::CreateCriterion {
                    id: "crit:new-1".into(),
                    criterion: Criterion {
                        requirement: "req:t-1".into(),
                        statement: "Given one line, the utility outputs that line unchanged."
                            .into(),
                        kind: "boundary".into(),
                        provenance: Some(derived(&["req:t-1"])),
                        ..Default::default()
                    },
                },
            ],
            &session(),
        );
        let r = s.apply(
            vec![Op::DeleteRequirement {
                id: "req:t-1".into(),
                reason: "gone".into(),
            }],
            &session(),
        );
        let entry = journal_entry(&s, r.generation);
        let op_at = |m: usize| {
            entry.mutations[m - 1]["op"]
                .as_str()
                .unwrap_or("")
                .to_string()
        };
        let req = r
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_REQ_DELETED && c.subject == "req:t-1")
            .expect("a requirement-deleted record");
        assert_eq!(
            op_at(req.mutation),
            "delete_requirement",
            "{:?}",
            entry.mutations
        );
        let crit = r
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_CRIT_DELETED)
            .expect("a criterion-deleted record");
        assert_eq!(
            op_at(crit.mutation),
            "delete_criterion",
            "{:?}",
            entry.mutations
        );
    }

    // A CRLF document locates its sections for a dual write the same way an LF one
    // does, and the edit keeps CRLF. Mirrors docs/compiler/graph.md#mutations.
    #[test]
    fn a_crlf_document_takes_a_dual_write_in_its_section() {
        let lf = "# T\n\n## Alpha\nThe cart holds items.\nIt is shared.\n\n## Beta\nThe cart holds items.\n";
        let crlf = lf.replace('\n', "\r\n");
        let sections = crate::md::parse_sections(&crlf);
        let beta = sections["/t/beta"].raw.clone();
        let alpha = sections["/t/alpha"].raw.clone();
        // A replacement scoped to Beta never lands in Alpha's identical sentence.
        let edit = ProseEdit::locate(
            "t.md",
            "/t/beta",
            Some(&beta),
            &crlf,
            "The cart holds items.",
            "The cart holds goods.",
        )
        .unwrap();
        assert_eq!(
            edit.full,
            crlf.replace(
                "## Beta\r\nThe cart holds items.",
                "## Beta\r\nThe cart holds goods."
            )
        );
        // An append after a multi-line section lands right after it, in CRLF.
        let edit = ProseEdit::locate(
            "t.md",
            "/t/alpha",
            Some(&alpha),
            &crlf,
            "",
            "A cart expires.\nAfter a day.",
        )
        .unwrap();
        assert!(
            edit.full
                .contains("It is shared.\r\n\r\nA cart expires.\r\nAfter a day.\r\n\r\n## Beta"),
            "{:?}",
            edit.full
        );
        assert!(
            !edit.full.replace("\r\n", "").contains('\n'),
            "{:?}",
            edit.full
        );
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
            remove_aliases: Vec::new(),
            clear_parent: false,
            not_about_add: Vec::new(),
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
        let update_parent = |id: &str, parent: &str| Op::UpdateEntity {
            id: id.into(),
            name: None,
            definition: None,
            add_aliases: Vec::new(),
            remove_aliases: Vec::new(),
            clear_parent: false,
            not_about_add: Vec::new(),
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
                remove_aliases: Vec::new(),
                clear_parent: false,
                not_about_add: Vec::new(),
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
            s.graph.views["view:usecase/t"].members,
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
                id: "view:usecase/t".into(),
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
        let v = &s.graph.views["view:usecase/t"];
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
        assert_eq!(s.graph.views["view:usecase/t"].members, vec!["req:t-1"]);
        let gone = s
            .status
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_VIEW_MEMBER_GONE)
            .expect("view-member-gone");
        assert_eq!(gone.subject, "view:usecase/t");
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
            remove_aliases: Vec::new(),
            clear_parent: false,
            not_about_add: Vec::new(),
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
        crate::vfs::write(dir.join("status.yaml"), "generation: 7\n").unwrap();
        let read = Store::load(&dir);
        assert!(read.graph.entities.is_empty());
        assert_eq!(read.status.generation, 0);
        assert!(dir.join("graph/entities/ent~cart.yaml").vexists());
        let bak = dir.with_file_name(format!(
            "{}.bak",
            dir.file_name().unwrap().to_string_lossy()
        ));
        crate::vfs::remove_dir_all(&bak).ok();
        let fresh = Store::open_for_build(&dir);
        assert!(fresh.graph.entities.is_empty());
        assert!(!dir.join("status.yaml").vexists());
        assert!(bak.join("graph/entities/ent~cart.yaml").vexists());
        // A second archive replaces the first.
        crate::vfs::create_dir_all(&dir).unwrap();
        crate::vfs::write(dir.join("status.yaml"), "version: 1\ngeneration: 2\n").unwrap();
        crate::vfs::write(dir.join("marker"), "second").unwrap();
        let fresh = Store::open_for_build(&dir);
        assert!(fresh.graph.entities.is_empty());
        assert!(bak.join("marker").vexists());
        assert!(!bak.join("graph").vexists());
        // A fresh directory is simply empty: nothing to archive.
        crate::vfs::remove_dir_all(&dir).ok();
        assert!(Store::open_for_build(&dir).graph.entities.is_empty());
        assert!(!dir.join("status.yaml").vexists());
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
                        subjects: vec!["prov:new".into()],
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
        // staged diagnostic on the derived one already stood, so no second one landed there.
        let id_of = |s: &Store, statement: &str| {
            s.graph
                .requirements
                .iter()
                .find(|(_, r)| r.statement == statement)
                .map(|(id, _)| id.clone())
                .expect("the requirement landed")
        };
        let x1 = id_of(&s, "The cart is split by category.");
        let x2 = id_of(&s, "The cart never exceeds ten categories.");
        assert!(x1.starts_with("req:x-") && x2.starts_with("req:x-") && x1 != x2);
        let proposal_id = s
            .graph
            .diagnostics
            .iter()
            .find(|(_, d)| d.rule == "ratification-pending" && d.subjects == vec![x2.clone()])
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
                .filter(|d| d.rule == "ratification-pending" && d.subjects == vec![x1.clone()])
                .count(),
            1
        );
        assert!(s.graph.requirements.contains_key(&x1));
        assert!(s.graph.requirements.contains_key(&x2));
        assert!(s.status.has_change(CHANGE_PROVENANCE_PENDING, &x1));
        assert!(s.status.has_change(CHANGE_PROVENANCE_PENDING, &x2));
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
        let setup: Vec<String> = s.status.changes.iter().map(|c| c.id.clone()).collect();
        s.clear_changes(&setup);
        let r = s.apply(
            vec![Op::RatifyProvenance {
                id: x1.clone(),
                source: mention("t.md", "/t", "The cart is split by category."),
            }],
            &Commit::store("ratify"),
        );
        assert_eq!(r.applied, 2, "{:?}", r.skipped);
        let q = &s.graph.requirements[&x1];
        assert!(q.source.is_some() && q.provenance.is_none());
        assert!(!s.status.has_change(CHANGE_PROVENANCE_PENDING, &x1));
        // An accepted proposal is a human edit: the mention the landed sentence adds
        // opens no review of the Cart. Mirrors docs/compiler/goals/ratify.md#accept.
        assert!(
            !s.status.has_change(CHANGE_ENTITY, "ent:cart"),
            "{:?}",
            s.status.changes
        );
        assert!(s
            .graph
            .diagnostics
            .values()
            .filter(|d| d.subjects.contains(&x1))
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
                    id: x2.clone(),
                    reason: "withdrawn".into(),
                },
                Op::RetractDecree {
                    id: x1.clone(),
                    reason: "withdrawn".into(),
                },
            ],
            &Commit::store("decree"),
        );
        assert_eq!(r.applied, 2, "{:?}", r.skipped);
        assert!(!s.graph.requirements.contains_key(&x2));
        assert!(s.status.has_change(CHANGE_REQ_DELETED, &x2));
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
        assert!(s
            .status
            .has_change(CHANGE_PROVENANCE_PENDING, "ent:selection"));
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
        assert!(!s
            .status
            .has_change(CHANGE_PROVENANCE_PENDING, "ent:selection"));
        // A retract is journaled as `ratify` but is no accepted sentence: the parent
        // that took the requirement and the children is reviewed as usual.
        // Mirrors docs/compiler/goals/ratify.md#retract.
        assert!(
            s.status.has_change(CHANGE_ENTITY, "ent:checkout"),
            "{:?}",
            s.status.changes
        );
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
                        error: None,
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
            .vexists());
    }

    // A save that dirtied sections is a generation of its own with its records.
    // Mirrors docs/compiler/graph.md#journal.
    // The journal's `tokens` is the session's measured spend, stamped after the turn:
    // the entry commits inside the serving, before the cost is known, and a zero
    // would price the build at nothing (docs/compiler/graph.md#journal).
    #[test]
    fn measured_spend_stamps_the_entries_a_session_wrote() {
        let out = own_dir("stamp-tokens");
        crate::vfs::create_dir_all(out.join("journal")).unwrap();
        let write = |g: u64, kind: &str, tokens: Option<u64>| {
            let entry = JournalEntry {
                build: format!("g{}", g),
                generation: g,
                kind: kind.to_string(),
                tokens,
                ..Default::default()
            };
            crate::vfs::write(
                out.join("journal").join(format!("g{}.yaml", g)),
                serde_norway::to_string(&entry).unwrap(),
            )
            .unwrap();
        };
        write(1, "edit", None);
        write(2, "session", None);
        write(3, "session", None);
        Store::stamp_tokens(&out, 1, 3, 901, 601);
        let read = |g: u64| -> JournalEntry {
            serde_norway::from_str(
                &crate::vfs::read_to_string(out.join("journal").join(format!("g{}.yaml", g)))
                    .unwrap(),
            )
            .unwrap()
        };
        // The two session entries carry the whole spend between them, the cached input
        // split the same way; the edit entry, which no session wrote, stays without one.
        assert_eq!(read(2).tokens, Some(450));
        assert_eq!(read(3).tokens, Some(451));
        assert_eq!(read(2).cached_tokens, Some(300));
        assert_eq!(read(3).cached_tokens, Some(301));
        assert_eq!(read(1).tokens, None);
        // A second stamp over the same generations leaves the first one's numbers.
        Store::stamp_tokens(&out, 1, 3, 100, 0);
        assert_eq!(read(2).tokens, Some(450));
        assert_eq!(read(2).cached_tokens, Some(300));
        // A checks entry after the session's own takes no share: the session entry
        // carries the whole spend (a split once dropped half a criteria session's).
        // An endpoint that reported no cache read leaves the field absent, not zero.
        write(4, "session", None);
        write(5, "checks", None);
        Store::stamp_tokens(&out, 3, 5, 12051, 0);
        assert_eq!(read(4).tokens, Some(12051));
        assert_eq!(read(4).cached_tokens, None);
        assert!(!crate::vfs::read_to_string(out.join("journal/g4.yaml"))
            .unwrap()
            .contains("cachedTokens"));
        assert_eq!(read(5).tokens, None);
    }

    // `jazyk spend` stamps an external agent's own report onto a committed session
    // entry lacking a figure, marked tokensSource: agent, and refuses an entry that
    // already carries one (measured or reported), a non-session entry, a missing
    // generation, and a cached figure above the bill.
    // Mirrors docs/frontends/cli.md#jazyk-spend.
    #[test]
    fn an_agent_report_stamps_one_unmeasured_session_entry_and_refuses_the_rest() {
        let out = own_dir("stamp-reported");
        crate::vfs::create_dir_all(out.join("journal")).unwrap();
        let write = |g: u64, kind: &str, tokens: Option<u64>| {
            let entry = JournalEntry {
                build: format!("g{}", g),
                generation: g,
                kind: kind.to_string(),
                tokens,
                ..Default::default()
            };
            crate::vfs::write(
                out.join("journal").join(format!("g{}.yaml", g)),
                serde_norway::to_string(&entry).unwrap(),
            )
            .unwrap();
        };
        write(1, "edit", None);
        write(2, "session", None);
        write(3, "session", Some(9800));
        let read = |g: u64| -> JournalEntry {
            serde_norway::from_str(
                &crate::vfs::read_to_string(out.join("journal").join(format!("g{}.yaml", g)))
                    .unwrap(),
            )
            .unwrap()
        };
        let stamped = Store::stamp_reported(&out, 2, 18400, Some(9100)).unwrap();
        assert_eq!(stamped.tokens, Some(18400));
        let e = read(2);
        assert_eq!(e.tokens, Some(18400));
        assert_eq!(e.cached_tokens, Some(9100));
        assert_eq!(e.tokens_source.as_deref(), Some("agent"));
        let text = crate::vfs::read_to_string(out.join("journal/g2.yaml")).unwrap();
        assert!(
            text.contains("tokensSource: agent") && text.contains("cachedTokens: 9100"),
            "{}",
            text
        );
        // A second report on the same entry is refused and names the standing figure.
        let err = Store::stamp_reported(&out, 2, 100, None).unwrap_err();
        assert!(
            err.contains("18400 tokens") && err.contains("reported by the agent"),
            "{}",
            err
        );
        assert_eq!(read(2).tokens, Some(18400));
        // A measured entry is never overwritten.
        let err = Store::stamp_reported(&out, 3, 100, None).unwrap_err();
        assert!(
            err.contains("9800 tokens") && err.contains("measured by the harness"),
            "{}",
            err
        );
        assert_eq!(read(3).tokens_source, None);
        // Not a session, not there, not a bill.
        assert!(Store::stamp_reported(&out, 1, 100, None)
            .unwrap_err()
            .contains("not a session"));
        assert!(Store::stamp_reported(&out, 9, 100, None)
            .unwrap_err()
            .contains("no journal entry g9"));
        assert!(Store::stamp_reported(&out, 2, 0, None).is_err());
        write(4, "session", None);
        assert!(Store::stamp_reported(&out, 4, 100, Some(101))
            .unwrap_err()
            .contains("cannot exceed"));
        assert_eq!(read(4).tokens, None);
        // A zero cached figure leaves the field absent rather than writing 0.
        Store::stamp_reported(&out, 4, 100, Some(0)).unwrap();
        assert_eq!(read(4).cached_tokens, None);
    }

    // A status write outside a changeset waits for the lock and is refused like a
    // commit when another writer landed first. Mirrors docs/compiler/graph.md#concurrency.
    #[test]
    fn a_status_write_takes_the_lock_and_is_refused_when_the_store_moved() {
        let out = own_dir("status-write");
        let mut a = Store {
            out: out.clone(),
            ..Default::default()
        };
        assert_eq!(a.apply(vec![create("ent:a", "A")], &session()).applied, 1);
        let held = FileLock::acquire(&out);
        let mut fresh = Store::load(&out);
        fresh.status.roots = vec!["main.md".into()];
        let writer = std::thread::spawn(move || fresh.write_status("roots"));
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(
            !writer.is_finished(),
            "a status write ran under a held lock"
        );
        drop(held);
        assert!(writer.join().unwrap().is_ok());
        assert_eq!(Store::load(&out).status.roots, vec!["main.md".to_string()]);
        let mut b = Store::load(&out);
        assert_eq!(b.apply(vec![create("ent:b", "B")], &session()).applied, 1);
        a.status.roots = vec!["stale.md".into()];
        let why = a.write_status("roots").unwrap_err();
        assert!(why.starts_with("store-moved:"), "{}", why);
        let disk = Store::load(&out);
        assert_eq!(disk.status.roots, vec!["main.md".to_string()]);
        assert_eq!(disk.status.generation, 2);
    }

    // A reader never accepts a store a commit is rewriting: it waits while the marker
    // stands under a held lock and reads the finished commit. A marker no writer holds
    // is a leftover and is read past at once. Mirrors docs/compiler/graph.md#concurrency.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_reader_waits_for_a_commit_in_progress() {
        let out = own_dir("reader-wait");
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        assert_eq!(s.apply(vec![create("ent:a", "A")], &session()).applied, 1);
        // A writer mid-commit: the lock held, the marker down, one node file rewritten,
        // the commit point not yet moved.
        let held = FileLock::acquire(&out);
        crate::vfs::write(out.join(COMMIT_MARKER), "1").unwrap();
        let mut next = Store::load(&out);
        next.graph
            .entities
            .insert("ent:b".into(), next.graph.entities["ent:a"].clone());
        next.status.generation = 2;
        next.save_nodes();
        let reader = {
            let out = out.clone();
            std::thread::spawn(move || Store::load(&out))
        };
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(
            !reader.is_finished(),
            "a reader accepted a half-committed store"
        );
        next.save_status_files();
        crate::vfs::remove_file(out.join(COMMIT_MARKER)).unwrap();
        drop(held);
        let read = reader.join().unwrap();
        assert!(read.load_errors.is_empty(), "{:?}", read.load_errors);
        assert_eq!(read.status.generation, 2);
        assert!(read.graph.entities.contains_key("ent:b"));
        // A marker left by a writer that stopped: no lock is held, nothing waits.
        crate::vfs::write(out.join(COMMIT_MARKER), "1").unwrap();
        let started = std::time::Instant::now();
        let read = Store::load(&out);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(read.load_errors.is_empty(), "{:?}", read.load_errors);
        // The next commit replaces the leftover and removes it.
        let mut w = Store::load(&out);
        assert_eq!(w.apply(vec![create("ent:c", "C")], &session()).applied, 1);
        assert!(!out.join(COMMIT_MARKER).vexists());
    }

    // A store that never holds still within the wait loads read-only with `store-busy`
    // instead of a mix of two commits. Mirrors docs/compiler/graph.md#concurrency.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_reader_that_never_reads_a_still_store_reports_store_busy() {
        let out = own_dir("reader-busy");
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        assert_eq!(s.apply(vec![create("ent:a", "A")], &session()).applied, 1);
        let held = FileLock::acquire(&out);
        crate::vfs::write(out.join(COMMIT_MARKER), "1").unwrap();
        let reader = {
            let out = out.clone();
            std::thread::spawn(move || {
                READ_WAIT_OVERRIDE.with(|o| o.set(Some(std::time::Duration::from_millis(200))));
                Store::load(&out)
            })
        };
        let mut busy = reader.join().unwrap();
        drop(held);
        assert!(
            busy.load_errors[0].starts_with("store-busy:"),
            "{:?}",
            busy.load_errors
        );
        let r = busy.apply(vec![create("ent:b", "B")], &session());
        assert_eq!(r.applied, 0);
        assert!(r.skipped[0].starts_with("store-busy:"), "{:?}", r.skipped);
        crate::vfs::remove_file(out.join(COMMIT_MARKER)).unwrap();
    }

    // Every committed file reaches the disk before its rename: no temporary file is
    // left beside it, and the directories the renames landed in are flushed.
    #[test]
    fn a_commit_leaves_no_temporary_files_and_no_marker() {
        let out = own_dir("durable");
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        assert_eq!(s.apply(vec![create("ent:a", "A")], &session()).applied, 1);
        let mut files = Vec::new();
        fn walk(d: &Path, files: &mut Vec<String>) {
            for e in crate::vfs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.vis_dir() {
                    walk(&p, files);
                } else {
                    files.push(e.file_name().to_string_lossy().to_string());
                }
            }
        }
        walk(&out, &mut files);
        assert!(
            !files
                .iter()
                .any(|f| f.ends_with(".tmp") || f == COMMIT_MARKER),
            "{:?}",
            files
        );
        assert!(
            TOUCHED_DIRS.with(|t| t.borrow().is_empty()),
            "every touched directory was flushed"
        );
    }

    // An in-memory store's marker lock: a stale marker is taken over at once, a nested
    // acquire never removes its holder's marker, and the holder's release does.
    // Mirrors docs/compiler/graph.md#concurrency.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_scoped_lock_takes_over_a_stale_marker_and_keeps_a_live_one() {
        let mut files = BTreeMap::new();
        files.insert(".jazyk/ds/.lock".to_string(), b"999".to_vec());
        let _scope = crate::vfs::Scope::enter(&files);
        let out = PathBuf::from("/project/.jazyk/ds");
        let marker = out.join(".lock");
        let started = std::time::Instant::now();
        let outer = FileLock::acquire(&out);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "a stale marker is not waited on"
        );
        assert!(marker.vexists());
        drop(FileLock::acquire(&out));
        assert!(
            marker.vexists(),
            "a nested release removed the holder's marker"
        );
        drop(outer);
        assert!(!marker.vexists());
        drop(FileLock::acquire(&out));
        assert!(!marker.vexists());
    }

    // A spend stamp rewrites a committed journal entry: it waits for the store lock
    // a writer holds and lands through the atomic write.
    // Mirrors docs/compiler/graph.md#concurrency.
    #[test]
    fn spend_stamps_wait_for_the_store_lock() {
        let out = own_dir("stamp-lock");
        crate::vfs::create_dir_all(out.join("journal")).unwrap();
        for g in [1, 2] {
            let entry = JournalEntry {
                build: format!("g{}", g),
                generation: g,
                kind: "session".to_string(),
                ..Default::default()
            };
            crate::vfs::write(
                out.join("journal").join(format!("g{}.yaml", g)),
                serde_norway::to_string(&entry).unwrap(),
            )
            .unwrap();
        }
        let read = |g: u64| -> JournalEntry {
            serde_norway::from_str(
                &crate::vfs::read_to_string(out.join("journal").join(format!("g{}.yaml", g)))
                    .unwrap(),
            )
            .unwrap()
        };
        let held = FileLock::acquire(&out);
        let (a, b) = (out.clone(), out.clone());
        let reported = std::thread::spawn(move || Store::stamp_reported(&a, 2, 500, None));
        let measured = std::thread::spawn(move || Store::stamp_tokens(&b, 0, 1, 700, 0));
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert_eq!(read(1).tokens, None, "a stamp wrote under a held lock");
        assert_eq!(read(2).tokens, None, "a stamp wrote under a held lock");
        drop(held);
        assert_eq!(reported.join().unwrap().unwrap().tokens, Some(500));
        measured.join().unwrap();
        assert_eq!(read(1).tokens, Some(700));
        assert_eq!(read(2).tokens, Some(500));
        let leftovers: Vec<String> = crate::vfs::read_dir(out.join("journal"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{:?}", leftovers);
    }

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

    // A save that dirties a section writes its edit records and, in the same
    // generation, the standing alignment block's record: every id stays unique.
    #[test]
    fn sync_docs_numbers_one_generation_without_duplicate_record_ids() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        let parse = |docs: &[(&str, &str)]| -> crate::reconcile::ParsedDocs {
            docs.iter()
                .map(|(d, t)| (d.to_string(), (hash_hex(t), crate::md::parse_sections(t))))
                .collect()
        };
        let t1 = "# T\n\n## Beta\nbeta body\n";
        let u1 = "# U\nunder body\n";
        s.sync_docs(&parse(&[("t.md", t1), ("u.md", u1)]));
        s.graph.requirements.insert(
            "req:t-1".into(),
            Requirement {
                statement: "The B betas.".into(),
                source: Some(mention("t.md", "/t/beta", "beta body")),
                ..Default::default()
            },
        );
        // Beta is edited in place: its anchor becomes a pending proposal.
        let t2 = "# T\n\n## Beta\nbeta CHANGED body\n";
        s.sync_docs(&parse(&[("t.md", t2), ("u.md", u1)]));
        assert!(s.status.has_change(CHANGE_ALIGNMENT_PENDING, "t.md"));
        // Another document's edit opens no proposal: its section records and the
        // standing alignment record share one generation.
        let u2 = "# U\nunder body, revised\n";
        s.sync_docs(&parse(&[("t.md", t2), ("u.md", u2)]));
        let generation = s.status.generation;
        let here: Vec<&ChangeRecord> = s
            .status
            .changes
            .iter()
            .filter(|c| c.generation == generation)
            .collect();
        assert!(here.iter().any(|c| c.kind == CHANGE_SECTION_DIRTY));
        assert!(here.iter().any(|c| c.kind == CHANGE_ALIGNMENT_PENDING));
        let ids: BTreeSet<&str> = s.status.changes.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids.len(), s.status.changes.len(), "{:?}", s.status.changes);
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
        let report = s.apply(vec![create("ent:fresh", "Fresh")], &Commit::store("decree"));
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
        let report = s.apply(vec![create("ent:other", "Other")], &Commit::store("decree"));
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
        // A glossary definition restating an inventory rule shares both entities
        // and, with their names out of the token pool, one other word ("triggers"
        // against "triggered"): two shared entities plus one token qualify the pair.
        s.graph.requirements.insert(
            "req:inv-3".into(),
            mk(
                "A Restock task is triggered when Stock falls below the Reorder point.",
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
        // Two shared entities and no word in common are what every statement of one
        // page shares: no pair (review-entity reads the entity's statements whole).
        s.graph.requirements.insert(
            "req:unrelated".into(),
            mk(
                "The Reorder point of a Restock task is set by hand.",
                &["ent:reorder-point", "ent:restock-task"],
            ),
        );
        let n = s.requirement_neighbors("req:glos-3");
        assert!(n.contains(&"req:inv-3".to_string()), "{:?}", n);
        assert!(!n.contains(&"req:other".to_string()), "{:?}", n);
        assert!(!n.contains(&"req:unrelated".to_string()), "{:?}", n);
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

    // Mirrors docs/compiler/reconciler.md#pairs: two transitions of one subject with
    // different ends are siblings of one machine and never pair; the same ends pair.
    #[test]
    fn sibling_transitions_of_one_machine_never_pair() {
        let mut s = Store::default();
        s.graph
            .entities
            .insert("ent:application".into(), entity("Application"));
        let mk = |st: &str, from: &str, to: &str| Requirement {
            statement: st.into(),
            entities: vec!["ent:application".into()],
            transition: Some(Transition {
                subject: "ent:application".into(),
                from: from.into(),
                to: to.into(),
                trigger: None,
                guard: None,
            }),
            source: Some(mention("hiring.md", "/moving", "q")),
            ..Default::default()
        };
        s.graph.requirements.insert(
            "req:h-1".into(),
            mk(
                "An Application moves from applied to screened when the recruiter reviews it.",
                "applied",
                "screened",
            ),
        );
        s.graph.requirements.insert(
            "req:h-2".into(),
            mk(
                "An Application moves from applied to rejected when the recruiter declines it.",
                "applied",
                "rejected",
            ),
        );
        s.graph.requirements.insert(
            "req:h-3".into(),
            mk(
                "The recruiter moves an Application from applied to screened after reviewing it.",
                "applied",
                "screened",
            ),
        );
        let n = s.requirement_neighbors("req:h-1");
        assert!(!n.contains(&"req:h-2".to_string()), "{:?}", n);
        assert!(n.contains(&"req:h-3".to_string()), "{:?}", n);
    }

    // The erp operation bullets: six statements of one entity sharing "supports" and
    // "operation" pair on nothing, and the two about a password still pair on it.
    // Mirrors docs/compiler/reconciler.md#pairs.
    #[test]
    fn entity_boilerplate_never_counts_as_a_shared_token() {
        let mut s = Store::default();
        s.graph
            .entities
            .insert("ent:um".into(), entity("User Management"));
        s.graph
            .entities
            .insert("ent:ua".into(), entity("User Account"));
        let mk = |statement: &str| Requirement {
            statement: statement.into(),
            entities: vec!["ent:um".into(), "ent:ua".into()],
            source: Some(mention("user.md", "/ops", "q")),
            ..Default::default()
        };
        for (id, st) in [
            ("req:u-1", "The user management system supports an `authenticateUser` operation that authenticates a user with a username and password."),
            ("req:u-2", "The user management system supports a `passwordChange` operation that changes the password of the caller's own account."),
            ("req:u-3", "The user management system supports a `createUser` operation that creates a new user account."),
            ("req:u-4", "The user management system supports an `updateUser` operation that updates an existing user account."),
            ("req:u-5", "The user management system supports a `deleteUser` operation that deletes a user account."),
            ("req:u-6", "The user management system supports a `passwordReset` operation that resets the password of a user account."),
        ] {
            s.graph.requirements.insert(id.into(), mk(st));
        }
        let bp = s.pair_boilerplate(
            &s.graph.requirements["req:u-3"],
            &s.graph.requirements["req:u-5"],
        );
        assert!(
            bp.contains("support") && bp.contains("operation"),
            "{:?}",
            bp
        );
        assert!(!bp.contains("password"), "{:?}", bp);
        let n = s.requirement_neighbors("req:u-3");
        assert!(n.is_empty(), "{:?}", n);
        let n = s.requirement_neighbors("req:u-2");
        assert_eq!(
            n,
            vec!["req:u-1".to_string(), "req:u-6".to_string()],
            "{:?}",
            n
        );
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
                "An Order ships in one Shipment; the carrier is noted.",
                &["ent:order", "ent:shipment"],
            ),
        );
        s.graph.requirements.insert(
            "req:o-3".into(),
            mk(
                "An Order is paid by one Payment; the receipt is kept.",
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
                    error: None,
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
        s.status.parked.push(
            Goal {
                id: "g:reconcile-section:t.md#/t/c".into(),
                kind: "reconcile-section".into(),
                target: format!("t.md#{}", refs[3]),
                ..Default::default()
            }
            .into(),
        );
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
            &align_item.commit(1, None),
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
        // A substantive quote change is a revision owed a pair judgment: the commit
        // records it, and the sweep behind it clears the record at once because
        // req:t-2 has no candidate pair (docs/compiler/reconciler.md#pairs). The
        // flagged anchor is recorded stale on its new section.
        assert!(report.changed_requirements.contains("req:t-2"));
        assert!(report
            .changes
            .iter()
            .any(|c| c.kind == CHANGE_REQ_REVISED && c.subject == "req:t-2"));
        assert!(!s.status.has_change(CHANGE_REQ_REVISED, "req:t-2"));
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

    // A heading renamed in place is an edit: its anchors follow the new reference
    // without a proposal, its coverage does not, the section is dirty, and the align
    // entry records the rename. Mirrors docs/compiler/alignment.md#what-applies-and-what-is-proposed.
    #[test]
    fn renamed_heading_carries_anchors_and_dirties_the_section() {
        let mut s = Store {
            out: own_dir("align-rename"),
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
        let v2 = format!("# T\nintro\n\n## Basket\n{}\n", CART);
        let mut parsed2 = BTreeMap::new();
        parsed2.insert(
            "t.md".to_string(),
            (hash_hex(&v2), crate::md::parse_sections(&v2)),
        );
        let d2 = s.sync_docs(&parsed2);
        assert_eq!(d2.len(), 1, "{:?}", d2);
        assert_eq!(d2[0].dirty_sections, vec!["/t/basket".to_string()]);
        assert!(d2[0].stale_anchors.is_empty(), "{:?}", d2[0].stale_anchors);
        assert_eq!(
            s.graph.requirements["req:t-1"]
                .source
                .as_ref()
                .unwrap()
                .section,
            "/t/basket"
        );
        assert!(!s.docs["t.md"].coverage.contains_key("/t/basket"));
        assert!(!s.docs["t.md"].coverage.contains_key("/t/cart"));
        assert!(s.status.alignment.is_empty(), "{:?}", s.status.alignment);
        // The edit entry dirties the renamed section only; the align entry records
        // the rename, no section-removed record for the old reference.
        assert_eq!(s.status.generation, gen_before + 2);
        let edit: JournalEntry = yaml_to(
            &s.out
                .join("journal")
                .join(format!("g{}.yaml", gen_before + 1)),
        )
        .unwrap();
        assert_eq!(edit.kind, "edit");
        assert_eq!(edit.dirtied, vec!["t.md#/t/basket".to_string()]);
        let entry: JournalEntry = yaml_to(
            &s.out
                .join("journal")
                .join(format!("g{}.yaml", gen_before + 2)),
        )
        .unwrap();
        assert_eq!(entry.kind, "align");
        assert_eq!(entry.mutations[0]["op"], "rename");
        assert_eq!(entry.mutations[0]["from"], "t.md#/t/cart");
        assert_eq!(entry.mutations[0]["to"], "t.md#/t/basket");
        assert!(!s
            .status
            .changes
            .iter()
            .any(|c| c.kind == CHANGE_SECTION_REMOVED));
    }

    // An entity mention whose quote stopped locating in a changed section is a stale
    // anchor too, not only a requirement source.
    // An edit that adds a line to a reconciled section: the record carries the line, the
    // loaded body marks it, and a covered claim that stages nothing for it bounces until
    // a staged quote or the coverage note accounts for it. The case that motivated the
    // rule: a layout lead-in added above three quoted paragraphs whose sentences were
    // already extracted. Mirrors docs/compiler/goals/reconcile-section.md#gate.
    #[test]
    fn an_added_line_rides_the_record_and_gates_the_covered_claim() {
        use crate::goals::GoalKind;
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        let parse = |text: &str| {
            let mut parsed = BTreeMap::new();
            parsed.insert(
                "t.md".to_string(),
                (hash_hex(text), crate::md::parse_sections(text)),
            );
            parsed
        };
        let v1 = "# T\nintro\n\n## Content\n\n> First. Second.\n\n> Third.\n";
        s.sync_docs(&parse(v1));
        let req = |quote: &str| Requirement {
            statement: format!("The page displays \"{}\"", quote),
            source: Some(SourceRef {
                doc: "t.md".into(),
                section: "/t/content".into(),
                quote: quote.into(),
            }),
            ..Default::default()
        };
        s.graph.requirements.insert("req:t-1".into(), req("First."));
        s.graph
            .requirements
            .insert("req:t-2".into(), req("Second."));
        s.graph.requirements.insert("req:t-3".into(), req("Third."));
        let v2 = "# T\nintro\n\n## Content\n\nWritten as separate paragraphs:\n\n> First. Second.\n\n> Third.\n";
        s.sync_docs(&parse(v2));
        let rec = s
            .status
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_SECTION_DIRTY && c.subject == "t.md#/t/content")
            .expect("a section-dirty record");
        assert_eq!(rec.detail["added"], 1, "{}", rec.detail);
        assert_eq!(rec.detail["removed"], 0, "{}", rec.detail);
        assert_eq!(
            rec.detail["addedLines"],
            serde_json::json!(["Written as separate paragraphs:"])
        );
        assert!(rec.detail["previous"].is_string(), "{}", rec.detail);

        // The loaded body marks the line from the record, though the store on disk
        // already holds the new body.
        let mut set = crate::context::LoadedSet::new(20_000);
        let text = set.load(&s, "t.md#/t/content", 1).unwrap();
        assert!(
            text.contains("+ Written as separate paragraphs:"),
            "{}",
            text
        );
        assert!(!text.contains("+ > First."), "{}", text);

        let gate = |staged: Vec<Op>| -> Vec<String> {
            crate::goals::ReconcileSection
                .gates(&s, &staged)
                .into_iter()
                .map(|v| v.rule.clone())
                .collect()
        };
        let cover = |note: &str| Op::SetCoverage {
            doc: "t.md".into(),
            section: "/t/content".into(),
            state: "covered".into(),
            note: Some(note.into()),
        };
        // The claim from the failed build: nothing staged, the old requirements cited.
        assert!(gate(vec![cover(
            "All sentences are already extracted. No new sentences."
        )])
        .contains(&"added-line-unaccounted".to_string()));
        // A requirement quoting the added line accounts for it.
        let staged = vec![
            Op::CreateRequirement {
                id: "req:t-4".into(),
                requirement: req("Written as separate paragraphs:"),
            },
            cover("req:t-4 carries the layout."),
        ];
        assert!(gate(staged).is_empty());
        // So does a note that repeats the line and says why it states nothing.
        assert!(gate(vec![cover(
            "\"Written as separate paragraphs:\" restates how the quotes already render."
        )])
        .is_empty());
    }

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

    // (b) and (d) of the pair degree cap: an accepted verdict marks the partner
    // judged on both sides' records at the commit, the slot it frees opens the next
    // waiting pair by score, a created record stands while its pair waits, and it
    // clears at the sweep when no candidate remains.
    // Mirrors docs/compiler/reconciler.md#pairs and graph.md#change-records.
    #[test]
    fn resolving_a_pair_marks_judged_and_opens_the_next_waiting_pair() {
        let mut s = crate::goals::tests::fanned_store();
        s.out = own_dir("paircap");
        let pair = |p: &str| format!("g:rejudge-pair:{}", crate::goals::pair_target("req:hub", p));
        let plan = crate::goals::pair_plan(&s);
        assert!(plan.waiting.contains_key(&pair("req:p08")));
        // An unrelated commit: the sweep keeps every record, the waiting pair's
        // record among them.
        s.apply(vec![], &Commit::store("checks"));
        assert!(s.status.has_change(CHANGE_REQ_CREATED, "req:p08"));
        assert!(s.status.has_change(CHANGE_REQ_CREATED, "req:p12"));
        // The session resolves the hub against req:p12 (score 4, opened first).
        let mut commit = Commit::session(vec![pair("req:p12")], 3, None);
        commit.resolved.push(Resolved {
            goal: pair("req:p12"),
            justification: "consistent: different obligations".into(),
            evidence: serde_json::json!({"verdict": "consistent"}),
        });
        let report = s.apply(vec![], &commit);
        assert_eq!(report.generation, 7);
        // req:p12's record listed the hub as its only candidate: judged, then swept.
        assert!(!s.status.has_change(CHANGE_REQ_CREATED, "req:p12"));
        // The hub has no record of its own; nothing to mark there. Every other
        // partner's record stands untouched, req:p08's among them.
        assert!(s.status.has_change(CHANGE_REQ_CREATED, "req:p08"));
        assert!(s
            .status
            .changes
            .iter()
            .filter(|c| c.kind == CHANGE_REQ_CREATED)
            .all(|c| c.judged().is_empty()));
        // The slot opens the next waiting pair by score, then target: req:p08.
        let plan = crate::goals::pair_plan(&s);
        assert!(
            plan.open.iter().any(|g| g.id == pair("req:p08")),
            "{:?}",
            plan.open.iter().map(|g| &g.id).collect::<Vec<_>>()
        );
        assert!(plan.open.iter().all(|g| g.id != pair("req:p12")));
        assert_eq!(plan.waiting.len(), 3);
        assert!(plan.waiting.contains_key(&pair("req:p09")));
        // The journal entry carries the resolution and the persisted status agrees.
        let entry: JournalEntry = yaml_to(&s.out.join("journal").join("g7.yaml")).unwrap();
        assert_eq!(entry.resolved_goals.len(), 1);
        assert!(!Store::load(&s.out)
            .status
            .has_change(CHANGE_REQ_CREATED, "req:p12"));
        // A record with two candidates keeps standing after one is judged: give the
        // hub a record, resolve it against req:p01, and the hub's record lists the
        // partner while the other candidates keep it alive.
        let mut batch = RecordBatch::new(8);
        batch.push(
            0,
            CHANGE_REQ_REVISED,
            "req:hub",
            "fields",
            serde_json::Value::Null,
        );
        for r in batch.records() {
            s.status.record_change(r.clone());
        }
        let mut commit = Commit::session(vec![pair("req:p01")], 3, None);
        commit.resolved.push(Resolved {
            goal: pair("req:p01"),
            justification: "consistent".into(),
            evidence: serde_json::json!({"verdict": "consistent"}),
        });
        s.apply(vec![], &commit);
        let hub = s
            .status
            .changes
            .iter()
            .find(|c| c.kind == CHANGE_REQ_REVISED && c.subject == "req:hub")
            .expect("the hub's record stands on its other candidates");
        assert_eq!(hub.judged(), vec!["req:p01".to_string()]);
        assert!(!s.status.has_change(CHANGE_REQ_CREATED, "req:p01"));
        let plan = crate::goals::pair_plan(&s);
        assert!(plan.open.iter().all(|g| g.id != pair("req:p01")));
        // A failed pair marks nothing judged.
        let mut commit = Commit::session(vec![pair("req:p02")], 3, None);
        commit.resolved.clear();
        s.apply(vec![], &commit);
        assert!(s
            .status
            .changes
            .iter()
            .find(|c| c.subject == "req:p02")
            .is_some_and(|c| c.judged().is_empty()));
        crate::vfs::remove_dir_all(&s.out).ok();
    }

    // The bounded journal readers answer as a full scan does: the newest-first stop
    // for a decree's prior and for a child's last move, the range reader for a
    // session's entries. Mirrors docs/compiler/graph.md#journal.
    #[test]
    fn bounded_journal_lookups_agree_with_a_full_scan() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        for (id, name, parent) in [
            ("ent:a", "A", Some("ent:p1")),
            ("ent:b", "B", Some("ent:p2")),
            ("ent:p1", "P1", None),
            ("ent:p2", "P2", None),
        ] {
            s.graph.entities.insert(
                id.into(),
                Entity {
                    parent: parent.map(String::from),
                    ..entity(name)
                },
            );
        }
        let decree = serde_json::json!({"decree": {"author": "me", "at": ""}});
        let entries: Vec<(u64, &str, Vec<serde_json::Value>)> = vec![
            (
                1,
                "session",
                vec![
                    serde_json::json!({"op": "create_entity", "id": "ent:dead", "entity": {"name": "Dead", "scope": "private"}}),
                    serde_json::json!({"op": "create_entity", "id": "ent:p1", "entity": {"name": "P1", "scope": "public"}}),
                ],
            ),
            (
                2,
                "decree",
                vec![
                    serde_json::json!({"op": "update_entity", "id": "ent:a", "parent": "ent:p1", "prior": {"parent": "ent:dead"}, "provenance": decree}),
                ],
            ),
            (
                3,
                "decree",
                vec![
                    serde_json::json!({"op": "update_requirement", "id": "req:x", "statement": "new", "prior": {"statement": "old1", "source": {"doc": "d.md", "section": "s", "quote": "old1"}}, "provenance": decree}),
                ],
            ),
            (
                5,
                "decree",
                vec![
                    serde_json::json!({"op": "update_requirement", "id": "req:x", "statement": "newer", "prior": {"statement": "old2", "source": {"doc": "d.md", "section": "s", "quote": "old2"}}, "provenance": decree}),
                ],
            ),
            (
                6,
                "session",
                vec![
                    serde_json::json!({"op": "update_requirement", "id": "req:x", "statement": "newest", "prior": {"statement": "old3"}}),
                ],
            ),
            (
                7,
                "gc",
                vec![
                    serde_json::json!({"op": "dissolve_entity", "id": "ent:grp", "parent": "ent:p2", "children": ["ent:b", "ent:c"]}),
                ],
            ),
            (
                8,
                "decree",
                vec![
                    serde_json::json!({"op": "update_entity", "id": "ent:a", "parent": "ent:p2", "prior": {"parent": "ent:p1"}, "provenance": decree}),
                ],
            ),
            (
                9,
                "session",
                vec![
                    serde_json::json!({"op": "update_entity", "id": "ent:b", "parent": "ent:p1", "prior": {"parent": "ent:p2"}}),
                    serde_json::json!({"op": "update_entity", "id": "ent:b", "parent": "ent:gone", "prior": {"parent": "ent:p1"}}),
                ],
            ),
            (
                11,
                "session",
                vec![
                    serde_json::json!({"op": "update_entity", "id": "ent:a", "parent": "ent:p1", "prior": {"parent": "ent:p2"}}),
                ],
            ),
        ];
        crate::vfs::create_dir_all(s.out.join("journal")).unwrap();
        for (g, kind, mutations) in &entries {
            let entry = JournalEntry {
                build: format!("g{}", g),
                generation: *g,
                kind: kind.to_string(),
                mutations: mutations.clone(),
                ..Default::default()
            };
            crate::vfs::write(s.journal_path(*g), serde_norway::to_string(&entry).unwrap())
                .unwrap();
        }
        // A torn entry is skipped, never fatal.
        crate::vfs::write(s.journal_path(10), "build: g10\nmutations: [unterminated\n").unwrap();

        let full = s.journal_entries();
        assert_eq!(full.len(), entries.len());
        let naive_prior = |id: &str, op: &str| -> Option<serde_json::Value> {
            full.iter().rev().find_map(|e| {
                e.mutations.iter().rev().find_map(|mv| {
                    (mv["op"] == op
                        && mv["id"] == id
                        && mv["prior"].is_object()
                        && mv["provenance"]["decree"].is_object())
                    .then(|| mv["prior"].clone())
                })
            })
        };
        for (id, op) in [
            ("req:x", "update_requirement"),
            ("ent:a", "update_entity"),
            ("ent:b", "update_entity"),
            ("ent:none", "update_entity"),
        ] {
            assert_eq!(s.decree_prior(id, op), naive_prior(id, op), "{} {}", id, op);
        }
        assert_eq!(
            s.decree_prior("req:x", "update_requirement").unwrap()["statement"],
            "old2"
        );
        assert_eq!(
            s.decree_prior("ent:a", "update_entity").unwrap()["parent"],
            "ent:p1"
        );
        assert_eq!(s.decree_prior("ent:b", "update_entity"), None);

        let moves = vec![
            (
                "ent:a".to_string(),
                Some("ent:p1".to_string()),
                Some("ent:p2".to_string()),
                1,
            ),
            (
                "ent:b".to_string(),
                Some("ent:gone".to_string()),
                Some("ent:dead".to_string()),
                2,
            ),
            ("ent:c".to_string(), None, Some("ent:p2".to_string()), 3),
            ("ent:never".to_string(), None, None, 4),
        ];
        let (last, names) = s.parent_history(&moves);
        let all_moves = parent_moves_in(&full);
        assert_eq!(all_moves, s.journaled_parent_moves());
        let mut naive_last: BTreeMap<String, ParentMove> = BTreeMap::new();
        for mv in &all_moves {
            if moves.iter().any(|(c, _, _, _)| *c == mv.child) {
                naive_last.insert(mv.child.clone(), mv.clone());
            }
        }
        assert_eq!(last, naive_last);
        assert_eq!(last["ent:a"].generation, 11);
        assert_eq!(last["ent:b"].to.as_deref(), Some("ent:gone"));
        assert_eq!(last["ent:c"].from.as_deref(), Some("ent:grp"));
        assert!(!last.contains_key("ent:never"));
        let all_names = created_names_in(&full);
        let mut dead: BTreeSet<String> = BTreeSet::new();
        for (_, from, to, _) in &moves {
            dead.extend(from.iter().chain(to.iter()).cloned());
        }
        for mv in last.values() {
            dead.extend(mv.from.iter().chain(mv.to.iter()).cloned());
        }
        let naive_names: BTreeMap<String, (String, String)> = all_names
            .into_iter()
            .filter(|(id, _)| dead.contains(id) && !s.graph.entities.contains_key(id))
            .collect();
        assert_eq!(names, naive_names);
        assert_eq!(
            names["ent:dead"],
            ("Dead".to_string(), "private".to_string())
        );
        assert!(
            !names.contains_key("ent:p1"),
            "a live parent needs no journal name"
        );

        let all = crate::goals::read_journal(&s.out);
        assert_eq!(all.len(), entries.len());
        for (from, to) in [(0, 20), (2, 8), (5, 6), (9, 9), (10, 11)] {
            let ranged = crate::goals::read_journal_range(&s.out, from, to);
            let naive: Vec<&JournalEntry> = all
                .iter()
                .filter(|e| e.generation > from && e.generation <= to)
                .collect();
            assert_eq!(
                ranged.iter().map(|e| e.generation).collect::<Vec<_>>(),
                naive.iter().map(|e| e.generation).collect::<Vec<_>>(),
                "({}, {}]",
                from,
                to
            );
        }
        crate::vfs::remove_dir_all(&s.out).ok();
    }

    // The sweep over a large store: the scorer's inputs are computed once per pass,
    // so a pass over thousands of created records costs one walk of the requirements
    // per record. Mirrors docs/compiler/reconciler.md#pairs.
    #[test]
    fn prune_records_scores_thousands_of_records_in_one_pass() {
        let mut s = Store {
            out: tmp(),
            ..Default::default()
        };
        let verbs = [
            "accepts",
            "rejects",
            "stores",
            "renders",
            "schedules",
            "archives",
        ];
        let nouns = ["order", "invoice", "ticket", "report", "session", "token"];
        for e in 0..300 {
            s.graph
                .entities
                .insert(format!("ent:e{}", e), entity(&format!("Entity {}", e)));
        }
        for r in 0..3000 {
            let a = r % 300;
            let b = (r * 7 + 1) % 300;
            s.graph.requirements.insert(
                format!("req:r{}", r),
                Requirement {
                    statement: format!(
                        "Entity {} {} the {} of entity {} within {} days.",
                        a,
                        verbs[r % verbs.len()],
                        nouns[(r / 3) % nouns.len()],
                        b,
                        r % 30
                    ),
                    entities: vec![format!("ent:e{}", a), format!("ent:e{}", b)],
                    ..Default::default()
                },
            );
            s.status.changes.push(ChangeRecord::new(
                1,
                r + 1,
                1,
                CHANGE_REQ_CREATED,
                &format!("req:r{}", r),
                "statement",
            ));
        }
        let before = s.status.changes.len();
        // One pass computes the scorer's inputs once for every record; the count is
        // the deterministic check. The wall clock only catches a pathological pass, so
        // its bound stays far above any machine under parallel test load.
        let scorings = PAIR_SCORINGS.with(|n| n.get());
        let started = std::time::Instant::now();
        s.prune_records();
        let took = started.elapsed();
        assert_eq!(
            PAIR_SCORINGS.with(|n| n.get()) - scorings,
            1,
            "prune_records over {} records scored more than once",
            before
        );
        assert!(
            took < std::time::Duration::from_secs(120),
            "prune_records over {} records took {:?}",
            before,
            took
        );
        // Every record stands or clears exactly as the per-record scorer says.
        let scoring = s.pair_scoring();
        for r in 0..10 {
            let rid = format!("req:r{}", r);
            let stands = s.status.changes.iter().any(|c| c.subject == rid);
            let rec = ChangeRecord::new(1, r + 1, 1, CHANGE_REQ_CREATED, &rid, "statement");
            let candidates = s.pair_review_candidates_with(&rec, &scoring);
            assert_eq!(stands, !candidates.is_empty(), "{}", rid);
            assert_eq!(candidates, s.pair_review_candidates(&rec), "{}", rid);
        }
        crate::vfs::remove_dir_all(&s.out).ok();
    }
}
