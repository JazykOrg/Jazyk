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

mod apply;
mod files;
mod gc;
mod journal;
mod lock;
mod minting;
mod neighbors;
mod prose;
mod read_scope;
mod status;
mod sync;

use self::apply::*;
pub use self::files::*;
pub use self::journal::*;
pub use self::lock::*;
pub use self::neighbors::*;
pub use self::read_scope::*;
pub use self::status::*;

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

#[cfg(test)]
mod concurrency_tests;

#[cfg(test)]
mod tests;
