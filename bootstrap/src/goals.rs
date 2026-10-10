// The goal registry: one trait, one static registry compiled into the binary, one
// implementation per kind. A kind derives its goals from disk state (the graph, the
// documents, the ledger, the change records), states when a goal is ready, names the
// targets a batch loads first, its tool slice, its batch gate, and its contract
// paragraph. Mirrors docs/compiler/reconciler.md#the-registry and the pages under
// docs/compiler/goals/.
use crate::board::Board;
use crate::gen::GenSettings;
use crate::limits;
use crate::model::*;
use crate::store::{self, Op, Store};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Compile,
    Gc,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::Compile => "compile",
            Class::Gc => "gc",
        }
    }

    pub fn parse(s: &str) -> Class {
        if s == "gc" {
            Class::Gc
        } else {
            Class::Compile
        }
    }
}

// The readiness answer, the reason rendered as a sentence because every surface shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Ready {
    Ready,
    Blocked(String),
}

impl Ready {
    pub fn is_ready(&self) -> bool {
        matches!(self, Ready::Ready)
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Ready::Ready => None,
            Ready::Blocked(r) => Some(r),
        }
    }
}

// A batch gate violation: the rule and the repair.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    pub rule: String,
    pub message: String,
}

impl Violation {
    fn new(rule: &str, message: String) -> Violation {
        Violation {
            rule: rule.to_string(),
            message,
        }
    }
}

pub trait GoalKind: Sync + Send {
    fn kind(&self) -> &'static str;
    fn class(&self) -> Class;
    // What one target is: document, section, pair, entity, node, instance, requirement,
    // ledger row, fact, diagnostic, entity pair, view.
    fn unit(&self) -> &'static str;
    // This kind's goals from disk state. Deterministic, idempotent, cheap. The ledger
    // kinds compare the graph against the ledger, which needs the deliverable path.
    fn derive_goals(&self, store: &Store, gen: &GenSettings) -> Vec<Goal>;
    fn ready(&self, goal: &Goal, board: &Board) -> Ready;
    // The targets a batch loads first, one per line, `- <target> <full|stub>`.
    fn pack(&self, store: &Store, batch: &[Goal]) -> String;
    // The kind's write slice; the serving adds the read and goal tools.
    fn toolset(&self) -> &'static [&'static str];
    // The batch gate over the store plus what the session staged.
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation>;
    // The contract paragraph. Empty for the blocked-on-human kinds.
    fn prompt(&self) -> &'static str;
}

pub static REGISTRY: [&dyn GoalKind; 17] = [
    &PlaceAnchors,
    &ReconcileSection,
    &RejudgePair,
    &ReviewEntity,
    &Retrace,
    &ConformInstance,
    &DeriveCriteria,
    &Bind,
    &Generate,
    &Verify,
    &Ratify,
    &Answer,
    &DeclareEdges,
    &DedupeCandidates,
    &CurateView,
    &SplitView,
    &AbstractEntity,
];

pub fn kind(name: &str) -> Option<&'static dyn GoalKind> {
    REGISTRY.iter().copied().find(|k| k.kind() == name)
}

// The readiness tier of a compile kind. GC kinds sit outside the tiers.
pub fn tier(kind: &str) -> Option<u8> {
    match kind {
        "place-anchors" => Some(0),
        "reconcile-section" => Some(1),
        "rejudge-pair" | "review-entity" | "retrace" | "conform-instance" => Some(2),
        "derive-criteria" | "bind" | "generate" | "verify" | "ratify" | "answer" => Some(3),
        _ => None,
    }
}

// The kinds a human resolves: never batched, always `blocked {on: human}`.
pub fn blocked_on_human(kind: &str) -> bool {
    matches!(kind, "ratify" | "answer")
}

// The change record kinds a goal of this kind stands on; resolving the goal clears
// the records of these kinds on its target (both members of a pair).
pub fn record_kinds(kind: &str) -> &'static [&'static str] {
    match kind {
        "place-anchors" => &[store::CHANGE_ALIGNMENT_PENDING],
        "reconcile-section" => &[store::CHANGE_SECTION_DIRTY, store::CHANGE_ANCHOR_STALE],
        "rejudge-pair" => &[
            store::CHANGE_REQ_CREATED,
            store::CHANGE_REQ_REVISED,
            store::CHANGE_NODE_DELETED,
        ],
        "review-entity" => &[store::CHANGE_ENTITY, store::CHANGE_NODE_DELETED],
        "retrace" => &[store::CHANGE_VIEW_MEMBER_GONE, store::CHANGE_NODE_DELETED],
        "conform-instance" => &[store::CHANGE_INSTANCE],
        "bind" | "generate" | "verify" => &[CHANGE_LEDGER_STALE],
        "ratify" => &[store::CHANGE_PROVENANCE_PENDING],
        "answer" => &[store::CHANGE_PROMPT_UNANSWERED],
        "declare-edges" => &[store::CHANGE_EDGES_MISSING],
        "dedupe-candidates" => &[CHANGE_LOOKALIKE],
        "curate-view" => &[store::CHANGE_QUERY_MATCH, CHANGE_FLOW_UNPLACED],
        "split-view" | "abstract-entity" => &[store::CHANGE_THRESHOLD_CROSSED],
        _ => &[],
    }
}

pub const CHANGE_LEDGER_STALE: &str = "ledger-stale";
pub const CHANGE_LOOKALIKE: &str = "lookalike";
pub const CHANGE_FLOW_UNPLACED: &str = "flow-unplaced";

// The skills a batch of this kind activates from the first round. The view and
// retrace kinds follow the target's kind.
pub fn skills_for(kind: &str, store: &Store, target: &str) -> Vec<&'static str> {
    match kind {
        "reconcile-section" | "declare-edges" => vec!["extraction"],
        "rejudge-pair" | "review-entity" | "dedupe-candidates" => vec!["judgment"],
        "conform-instance" => vec!["conformance"],
        "derive-criteria" => vec!["criteria"],
        "abstract-entity" => vec!["abstraction"],
        "curate-view" | "split-view" => view_skill(store, target).into_iter().collect(),
        "retrace" => {
            if target.starts_with("view:") {
                view_skill(store, target).into_iter().collect()
            } else if is_instance(store, target) {
                vec!["conformance"]
            } else if is_derived(store, target) {
                vec!["abstraction"]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

pub const FLOW_KINDS: [&str; 6] = [
    "use-case",
    "activity",
    "sequence",
    "communication",
    "timing",
    "overview",
];

pub fn is_flow_kind(kind: &str) -> bool {
    FLOW_KINDS.contains(&kind)
}

fn view_skill(store: &Store, view: &str) -> Option<&'static str> {
    let v = store.graph.views.get(view)?;
    Some(if is_flow_kind(&v.kind) {
        "flow-views"
    } else {
        "structural-views"
    })
}

fn is_instance(store: &Store, id: &str) -> bool {
    crate::derive::instance_types(store).contains_key(id)
}

fn is_derived(store: &Store, id: &str) -> bool {
    let pending = |p: &Option<Provenance>| !matches!(p, None | Some(Provenance::Quote(_)));
    store
        .graph
        .entities
        .get(id)
        .map(|e| pending(&e.provenance))
        .or_else(|| {
            store
                .graph
                .requirements
                .get(id)
                .map(|r| r.source.is_none() && pending(&r.provenance))
        })
        .unwrap_or(false)
}

// The per-target work item the current serving claims for a goal of this kind, or
// None for a kind the serving has no task for yet.
pub fn legacy_task(kind: &str) -> Option<&'static str> {
    match kind {
        "place-anchors" => Some("align-doc"),
        "reconcile-section" => Some("reconcile-doc"),
        "rejudge-pair" => Some("review-requirement"),
        "review-entity" => Some("review-entity"),
        "bind" => Some("bind-requirement"),
        "generate" => Some("generate-entity"),
        _ => None,
    }
}

pub fn goal_id(kind: &str, target: &str) -> String {
    format!("g:{}:{}", kind, target)
}

// `g:<kind>:<target>` split at the kind. Targets carry colons of their own.
pub fn parse_goal_id(id: &str) -> Option<(&str, &str)> {
    let rest = id.strip_prefix("g:")?;
    let (kind, target) = rest.split_once(':')?;
    if kind.is_empty() || target.is_empty() {
        return None;
    }
    Some((kind, target))
}

pub fn pair_target(a: &str, b: &str) -> String {
    if a <= b {
        format!("{}~{}", a, b)
    } else {
        format!("{}~{}", b, a)
    }
}

pub fn pair_members(target: &str) -> Option<(&str, &str)> {
    target.split_once('~')
}

// ---- shared derivation helpers ----

fn records_of<'a>(store: &'a Store, kinds: &[&str]) -> Vec<&'a ChangeRecord> {
    store
        .status
        .changes
        .iter()
        .filter(|c| kinds.contains(&c.kind.as_str()))
        .collect()
}

fn earliest_cause(records: &[&ChangeRecord]) -> Option<Cause> {
    records
        .iter()
        .min_by_key(|c| (c.generation, c.mutation))
        .map(|c| c.cause())
}

fn build_goal(
    kind: &dyn GoalKind,
    target: &str,
    mandatory: bool,
    change: Value,
    cause: Option<Cause>,
    hints: Vec<String>,
) -> Goal {
    Goal {
        id: goal_id(kind.kind(), target),
        kind: kind.kind().to_string(),
        class: kind.class().name().to_string(),
        mandatory,
        target: target.to_string(),
        unit: kind.unit().to_string(),
        change,
        cause,
        state: GoalState::Open,
        hints,
    }
}

// A section that owes coverage of its own: one with a body beyond its heading line,
// and never a code block, whose parent's mark covers it (the parent keeps its whole
// body). A diagram section owes its own. Mirrors docs/compiler/compilation.md#coverage.
// Whether a section owes a coverage mark and a reconcile goal: it has a body of its
// own. A heading section's body is its text after the heading line, a diagram's the
// block inside its fence, a preamble's its whole text (it has no heading line, so a
// one-line preamble owes its mark). A code block follows its parent's mark.
// Mirrors docs/compiler/compilation.md#coverage.
// The section that owns a reference's goal and mark: a fenced block's holding section
// (walking up through nested blocks), any other section itself.
// Mirrors docs/compiler/parsing.md#section-tree.
pub fn owning_section(store: &Store, doc: &str, sec: &str) -> String {
    let mut cur = sec.to_string();
    let Some(rec) = store.docs.get(doc) else {
        return cur;
    };
    for _ in 0..16 {
        match rec.sections.get(&cur) {
            Some(s) if s.kind == "code-block" => match &s.parent {
                Some(p) => cur = p.clone(),
                None => break,
            },
            _ => break,
        }
    }
    cur
}

pub fn section_has_body(sec: &Section) -> bool {
    if sec.kind == "code-block" {
        return false;
    }
    let skip = if sec.kind == "preamble" { 0 } else { 1 };
    !sec.raw.lines().skip(skip).all(|l| l.trim().is_empty())
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn truncate(s: &str, n: usize) -> String {
    crate::llm::truncate(s, n)
}

fn claimed_after(claimed: &Option<String>, generation: u64) -> bool {
    claimed
        .as_deref()
        .and_then(|b| b.strip_prefix('g'))
        .and_then(|n| n.parse::<u64>().ok())
        .is_some_and(|n| n >= generation)
}

// Content tokens of a statement: normalized tokens minus stop words and the named
// entities' own name tokens, crudely stemmed. The pair scorer's vocabulary.
pub fn content_tokens(store: &Store, statement: &str, entities: &[String]) -> BTreeSet<String> {
    content_token_words(store, statement, entities)
        .into_iter()
        .map(|(stemmed, _)| stemmed)
        .collect()
}

// The content tokens of a statement in order, each stem paired with the word the
// statement wrote, for a hint that shows a word and never a stem.
pub fn content_token_words(
    store: &Store,
    statement: &str,
    entities: &[String],
) -> Vec<(String, String)> {
    let mut name_toks: BTreeSet<String> = BTreeSet::new();
    for e in entities {
        if let Some(ent) = store.graph.entities.get(store.resolve_id(e)) {
            for n in std::iter::once(&ent.name).chain(ent.aliases.iter()) {
                for t in store::normalize_statement(n).split(' ') {
                    name_toks.insert(stem(t));
                }
            }
        }
    }
    store::normalize_statement(statement)
        .split(' ')
        .filter(|t| store::is_content_token(t))
        .map(|t| (stem(t), t.to_string()))
        .filter(|(stemmed, _)| store::is_content_token(stemmed) && !name_toks.contains(stemmed))
        .collect()
}

fn stem(t: &str) -> String {
    for suffix in ["ing", "ed", "s"] {
        if t.len() > suffix.len() + 2 && t.ends_with(suffix) {
            return t[..t.len() - suffix.len()].to_string();
        }
    }
    t.to_string()
}

// Name tokens of an entity: name and aliases, normalized, stop words dropped, stemmed.
pub fn name_tokens(e: &Entity) -> BTreeSet<String> {
    const STOP: [&str; 5] = ["the", "a", "an", "of", "and"];
    std::iter::once(&e.name)
        .chain(e.aliases.iter())
        .flat_map(|n| {
            store::normalize_statement(n)
                .split(' ')
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .filter(|t| !t.is_empty() && !STOP.contains(&t.as_str()))
        .map(|t| stem(&t))
        .collect()
}

// The open judged diagnostics naming any of the ids. The build's own bookkeeping
// (incomplete-build, ratification-pending, unstable-*) is left out: a session never
// resolves it. Mirrors docs/compiler/goals/review-entity.md#hints.
fn open_diags_naming<'a>(store: &'a Store, ids: &[&str]) -> Vec<(&'a String, &'a Diagnostic)> {
    store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| {
            d.lifecycle == "open"
                && !crate::tools::harness_owned_rule(&d.rule)
                && d.subjects.iter().any(|s| ids.contains(&s.as_str()))
        })
        .collect()
}

fn node_label(store: &Store, id: &str) -> String {
    if let Some(e) = store.graph.entities.get(id) {
        return format!("{} ({})", id, e.name);
    }
    if let Some(r) = store.graph.requirements.get(id) {
        return format!("{} \"{}\"", id, truncate(&r.statement, 80));
    }
    if let Some(v) = store.graph.views.get(id) {
        return format!("{} ({})", id, v.title);
    }
    id.to_string()
}

// ---- place-anchors ----

pub struct PlaceAnchors;

impl GoalKind for PlaceAnchors {
    fn kind(&self) -> &'static str {
        "place-anchors"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "document"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut out = Vec::new();
        for rec in records_of(store, &[store::CHANGE_ALIGNMENT_PENDING]) {
            let Some(block) = store.status.alignment.iter().find(|b| b.doc == rec.subject) else {
                continue;
            };
            if block.proposals.is_empty() {
                continue;
            }
            let mut ops: BTreeMap<String, usize> = BTreeMap::new();
            for c in &block.changes {
                *ops.entry(c.op.clone()).or_insert(0) += 1;
            }
            let anchors: BTreeSet<String> =
                block.proposals.iter().map(|p| p.anchor.clone()).collect();
            let change = json!({
                "proposals": block.proposals.len(),
                "ops": ops,
                "anchors": anchors,
            });
            let ops_line: Vec<String> = ops.iter().map(|(k, n)| format!("{} {}", n, k)).collect();
            let mut hints = vec![format!(
                "{} proposal(s){}; aligned in g{}",
                block.proposals.len(),
                if ops_line.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", ops_line.join(", "))
                },
                rec.generation
            )];
            let mut loads: Vec<String> = Vec::new();
            for p in &block.proposals {
                let kind = if store.graph.requirements.contains_key(&p.anchor) {
                    "requirement"
                } else {
                    "mention"
                };
                let best = p.candidates.first();
                hints.push(format!(
                    "{} ({}): {} candidate(s), quote locates in the best: {}",
                    p.anchor,
                    kind,
                    p.candidates.len(),
                    match best {
                        Some(c) if c.quote_locates => "yes",
                        Some(_) => "no",
                        None => "no candidate (homeless)",
                    }
                ));
                if let Some(c) = best {
                    if !loads.contains(&c.section) && loads.len() < 6 {
                        loads.push(c.section.clone());
                    }
                }
            }
            hints.extend(loads.iter().map(|s| format!("load {}", s)));
            hints.push("place_anchor with reevaluate; orphan_anchor".into());
            out.push(build_goal(
                self,
                &rec.subject,
                true,
                change,
                Some(rec.cause()),
                hints,
            ));
        }
        out
    }
    fn ready(&self, _goal: &Goal, _board: &Board) -> Ready {
        Ready::Ready
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} section changes", g.target));
            lines.push(format!("- proposals {} full", g.target));
            if let Some(block) = store.status.alignment.iter().find(|b| b.doc == g.target) {
                let mut seen = BTreeSet::new();
                for p in &block.proposals {
                    for c in &p.candidates {
                        if seen.insert(c.section.clone()) {
                            lines.push(format!("- {} stub", c.section));
                        }
                    }
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &["place_anchor", "orphan_anchor"]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        // Every proposal of a document the changeset touches is decided.
        let decided: BTreeSet<(String, String)> = staged
            .iter()
            .filter_map(|o| match o {
                Op::PlaceAnchor { id, from, .. } | Op::OrphanAnchor { id, from } => {
                    Some((id.clone(), format!("{}#{}", from.doc, from.section)))
                }
                _ => None,
            })
            .collect();
        let docs: BTreeSet<&str> = staged
            .iter()
            .filter_map(|o| match o {
                Op::PlaceAnchor { from, .. } | Op::OrphanAnchor { from, .. } => {
                    Some(from.doc.as_str())
                }
                _ => None,
            })
            .collect();
        let mut out = Vec::new();
        for b in store
            .status
            .alignment
            .iter()
            .filter(|b| docs.contains(b.doc.as_str()))
        {
            let undecided: Vec<String> = b
                .proposals
                .iter()
                .filter(|p| !decided.contains(&(p.anchor.clone(), p.from.clone())))
                .map(|p| p.anchor.clone())
                .collect();
            if !undecided.is_empty() {
                out.push(Violation::new(
                    "undecided-proposal",
                    format!(
                        "{} still has undecided proposal(s): {}; place_anchor or orphan_anchor each",
                        b.doc,
                        undecided.join(", ")
                    ),
                ));
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/place-anchors.md")
    }
}

// ---- reconcile-section ----

pub struct ReconcileSection;

#[derive(Default)]
struct SectionWork {
    dirty: Option<Value>,
    stale: BTreeSet<String>,
    unprocessed: bool,
    causes: Vec<Cause>,
}

// How many requirements are sourced in each section (`doc#section`), one pass over
// the requirements.
fn requirements_per_section(store: &Store) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for r in store.graph.requirements.values() {
        if let Some(s) = &r.source {
            *out.entry(format!("{}#{}", s.doc, s.section)).or_insert(0) += 1;
        }
    }
    out
}

// The hint lines every section goal of one document shares: its subjects and the
// known entities its text names. Computed once per document, not per section (a
// store with two hundred dirty sections once lowercased its documents and searched
// two thousand names two hundred times).
struct DocHintLines {
    subjects: Option<String>,
    known: String,
}

// Every entity's name and aliases lowercased, the ones three characters or longer,
// by id: what "named in the document" searches the text for, prepared once per
// derivation rather than once per document per entity.
struct EntityNames(BTreeMap<String, (String, Vec<String>)>);

impl EntityNames {
    fn build(store: &Store) -> EntityNames {
        EntityNames(
            store
                .graph
                .entities
                .iter()
                .map(|(id, e)| {
                    let lc: Vec<String> = std::iter::once(&e.name)
                        .chain(e.aliases.iter())
                        .filter(|n| n.len() >= 3)
                        .map(|n| n.to_lowercase())
                        .collect();
                    (id.clone(), (e.name.clone(), lc))
                })
                .collect(),
        )
    }

    fn named_in(&self, id: &str, text: &str) -> bool {
        self.0
            .get(id)
            .is_some_and(|(_, lc)| lc.iter().any(|n| text.contains(n.as_str())))
    }
}

fn doc_hint_lines(
    store: &Store,
    doc: &str,
    linked: &[(String, String, String)],
    names: &EntityNames,
) -> DocHintLines {
    // The document's own text decides what it names: an entity linked from
    // another document counts as this one's subject only when this document
    // names it (or is titled with its name), and the known entities are the
    // ones whose name or alias occurs in the text, listed by id. A settings
    // document once read "primarySubject: Scene; 0 entities mentioned" because
    // a scene's setting linked to it and mentions were counted, not names.
    // Mirrors docs/compiler/goals/reconcile-section.md#what-the-model-sees.
    let text = store
        .docs
        .get(doc)
        .map(|d| {
            d.sections
                .values()
                .map(|s| s.raw.to_lowercase())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let root_title = store.docs.get(doc).and_then(|d| {
        d.sections
            .values()
            .find(|s| s.kind == "root")
            .map(|s| s.title.to_lowercase())
    });
    let subjects: Vec<&(String, String, String)> = linked
        .iter()
        .filter(|(id, name, _)| {
            root_title.as_deref() == Some(name.to_lowercase().as_str())
                || names.named_in(id, &text)
        })
        .collect();
    let subjects = match subjects.len() {
        0 if !linked.is_empty() => Some(format!(
            "linked from {} other document(s) (via {}); none of their entities is named here, so no primarySubject",
            linked.len(),
            linked
                .iter()
                .map(|(id, _, _)| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        0 => None,
        1 => Some(format!(
            "primarySubject: {} ({})",
            subjects[0].0, subjects[0].1
        )),
        _ => Some(format!(
            "candidateSubjects: {}",
            subjects
                .iter()
                .map(|(id, name, _)| format!("{} ({})", id, name))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    };
    let mut known: Vec<String> = names
        .0
        .iter()
        .filter(|(_, (_, lc))| lc.iter().any(|n| text.contains(n.as_str())))
        .map(|(id, (name, _))| format!("{} ({})", name, id))
        .collect();
    let known_count = known.len();
    known.truncate(8);
    if known_count > 8 {
        known.push(format!("and {} more", known_count - 8));
    }
    let known = if store.graph.entities.is_empty() {
        "the graph holds no entity yet: create what the section states, no search can find anything".to_string()
    } else if known_count == 0 {
        format!(
            "no known entity is named in the document; {} in the graph; search before creating",
            store.graph.entities.len()
        )
    } else {
        format!(
            "{} known entities named in the document: {}; {} in the graph; search before creating",
            known_count,
            known.join(", "),
            store.graph.entities.len()
        )
    };
    DocHintLines { subjects, known }
}

// Whether a stale anchor still owes work: its quote fails to locate or it is flagged
// for re-evaluation. A deleted anchor owes nothing.
fn anchor_stale(store: &Store, id: &str) -> bool {
    if store.status.reevaluate.iter().any(|x| x == id) {
        return true;
    }
    if let Some(r) = store.graph.requirements.get(id) {
        return r
            .source
            .as_ref()
            .map(|q| !store.quote_locates(&q.doc, &q.section, &q.quote))
            .unwrap_or(false);
    }
    if let Some(e) = store.graph.entities.get(id) {
        return e
            .mentions
            .iter()
            .any(|m| !store.quote_locates(&m.doc, &m.section, &m.quote));
    }
    false
}

// Entities another document introduced for this one: a mention or a requirement
// quote elsewhere whose link resolves to the document.
pub fn linked_subjects(store: &Store, doc: &str) -> Vec<(String, String, String)> {
    linked_subjects_all(store).remove(doc).unwrap_or_default()
}

// The linked subjects of every document in one pass: each mention's and each
// quote's links are parsed once and credited to every document they resolve to, in
// the order the per-document scan would list them, twelve per document. A per-section
// hint once parsed every link in the store again for every dirty section.
pub fn linked_subjects_all(store: &Store) -> BTreeMap<String, Vec<(String, String, String)>> {
    let mut out: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    // The sentence carrying the link must name the entity it introduces; a scene's
    // setting line linking to the settings page introduces no Scene.
    // Mirrors docs/compiler/goals/reconcile-section.md#incoming-links.
    let names = |e: &Entity, quote: &str| {
        let q = quote.to_lowercase();
        std::iter::once(&e.name)
            .chain(e.aliases.iter())
            .any(|n| n.len() >= 3 && q.contains(&n.to_lowercase()))
    };
    let listed = |out: &BTreeMap<String, Vec<(String, String, String)>>, doc: &str, id: &str| {
        out.get(doc)
            .is_some_and(|v| v.iter().any(|(x, _, _)| x == id))
    };
    for (id, e) in &store.graph.entities {
        for m in &e.mentions {
            if !names(e, &m.quote) {
                continue;
            }
            for l in crate::md::doc_links(&m.quote, &m.doc) {
                // One mention per entity per document: the first that links there.
                if l == m.doc || listed(&out, &l, id) {
                    continue;
                }
                out.entry(l).or_default().push((
                    id.clone(),
                    e.name.clone(),
                    format!("{}#{}", m.doc, m.section),
                ));
            }
        }
    }
    for r in store.graph.requirements.values() {
        let Some(src) = r.source.as_ref() else { continue };
        for l in crate::md::doc_links(&src.quote, &src.doc) {
            if l == src.doc {
                continue;
            }
            for e in &r.entities {
                let id = store.resolve_id(e).to_string();
                if listed(&out, &l, &id) {
                    continue;
                }
                if let Some(ent) = store.graph.entities.get(&id) {
                    if names(ent, &src.quote) {
                        out.entry(l.clone()).or_default().push((
                            id,
                            ent.name.clone(),
                            format!("{}#{}", src.doc, src.section),
                        ));
                    }
                }
            }
        }
    }
    for v in out.values_mut() {
        v.truncate(12);
    }
    out
}

// The linked subjects of one document by the plain scan: the reference the one-pass
// index is checked against.
#[cfg(test)]
pub(crate) fn linked_subjects_scan(store: &Store, doc: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    // The sentence carrying the link must name the entity it introduces; a scene's
    // setting line linking to the settings page introduces no Scene.
    // Mirrors docs/compiler/goals/reconcile-section.md#incoming-links.
    let names = |e: &Entity, quote: &str| {
        let q = quote.to_lowercase();
        std::iter::once(&e.name)
            .chain(e.aliases.iter())
            .any(|n| n.len() >= 3 && q.contains(&n.to_lowercase()))
    };
    for (id, e) in &store.graph.entities {
        for m in &e.mentions {
            if m.doc == doc || !names(e, &m.quote) {
                continue;
            }
            if crate::md::doc_links(&m.quote, &m.doc)
                .iter()
                .any(|l| l == doc)
            {
                out.push((
                    id.clone(),
                    e.name.clone(),
                    format!("{}#{}", m.doc, m.section),
                ));
                break;
            }
        }
    }
    for r in store.graph.requirements.values() {
        let Some(s) = r.source.as_ref() else { continue };
        if s.doc == doc
            || !crate::md::doc_links(&s.quote, &s.doc)
                .iter()
                .any(|l| l == doc)
        {
            continue;
        }
        for e in &r.entities {
            let id = store.resolve_id(e).to_string();
            if out.iter().any(|(x, _, _)| *x == id) {
                continue;
            }
            if let Some(ent) = store.graph.entities.get(&id) {
                if names(ent, &s.quote) {
                    out.push((id, ent.name.clone(), format!("{}#{}", s.doc, s.section)));
                }
            }
        }
    }
    out.truncate(12);
    out
}

impl GoalKind for ReconcileSection {
    fn kind(&self) -> &'static str {
        "reconcile-section"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "section"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut work: BTreeMap<String, SectionWork> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_SECTION_DIRTY]) {
            let Some((doc, sec)) = split_section_ref(&rec.subject) else {
                continue;
            };
            let Some(record) = store.docs.get(&doc) else {
                continue;
            };
            if !record.sections.contains_key(&sec) {
                continue;
            }
            // A fenced block's dirt is its holding section's: the block owns no goal.
            // Mirrors docs/compiler/parsing.md#section-tree.
            let sec = owning_section(store, &doc, &sec);
            // A bare heading has nothing to extract and nothing to mark.
            // Mirrors docs/compiler/goals/reconcile-section.md#created-when.
            if record.sections.get(&sec).is_some_and(|s| !section_has_body(s)) {
                continue;
            }
            if record
                .coverage
                .get(&sec)
                .is_some_and(|c| claimed_after(&c.claimed_by, rec.generation))
            {
                continue;
            }
            let w = work.entry(format!("{}#{}", doc, sec)).or_default();
            w.dirty = Some(rec.detail.clone());
            w.causes.push(rec.cause());
        }
        for rec in records_of(store, &[store::CHANGE_ANCHOR_STALE]) {
            let Some((doc, sec)) = split_section_ref(&rec.subject) else {
                continue;
            };
            if !store
                .docs
                .get(&doc)
                .is_some_and(|r| r.sections.contains_key(&sec))
            {
                continue;
            }
            let anchors: Vec<String> = str_list(&rec.detail["anchors"])
                .into_iter()
                .filter(|a| anchor_stale(store, a))
                .collect();
            if anchors.is_empty() {
                continue;
            }
            let sec = owning_section(store, &doc, &sec);
            let w = work.entry(format!("{}#{}", doc, sec)).or_default();
            w.stale.extend(anchors);
            w.causes.push(rec.cause());
        }
        // Anchors whose quote stopped locating, or flagged by a placement, ride on
        // their section whether or not a record names them.
        for doc in store.docs.keys() {
            let (stale, _) = store.stale_extras(doc);
            for rid in stale {
                let Some(src) = store
                    .graph
                    .requirements
                    .get(&rid)
                    .and_then(|r| r.source.as_ref())
                else {
                    continue;
                };
                if !store
                    .docs
                    .get(&src.doc)
                    .is_some_and(|r| r.sections.contains_key(&src.section))
                {
                    continue;
                }
                let sec = owning_section(store, &src.doc, &src.section);
                let full = format!("{}#{}", src.doc, sec);
                let w = work.entry(full).or_default();
                if w.stale.insert(rid) && w.causes.is_empty() {
                    w.causes.push(Cause {
                        generation: store.status.generation,
                        mutation: 0,
                        via: "quote".into(),
                    });
                }
            }
        }
        for (doc, record) in &store.docs {
            for (r, sec) in &record.sections {
                if section_has_body(sec) && !record.coverage.contains_key(r) {
                    let w = work.entry(format!("{}#{}", doc, r)).or_default();
                    w.unprocessed = true;
                    if w.causes.is_empty() {
                        w.causes.push(Cause {
                            generation: store.status.generation,
                            mutation: 0,
                            via: "section".into(),
                        });
                    }
                }
            }
        }
        let mut out = Vec::new();
        if work.is_empty() {
            return Vec::new();
        }
        let per_section = requirements_per_section(store);
        let linked = linked_subjects_all(store);
        let names = EntityNames::build(store);
        let mut doc_lines: BTreeMap<String, DocHintLines> = BTreeMap::new();
        for (full, w) in work {
            let Some((doc, sec_ref)) = split_section_ref(&full) else {
                continue;
            };
            let sec = &store.docs[&doc].sections[&sec_ref];
            let stale: Vec<String> = w.stale.iter().cloned().collect();
            let mut change = json!({});
            if let Some(d) = &w.dirty {
                change["dirty"] = d.clone();
            }
            if !stale.is_empty() {
                change["staleAnchors"] = json!(stale);
            }
            if w.unprocessed {
                change["unprocessed"] = json!(true);
            }
            let cause = w
                .causes
                .iter()
                .min_by_key(|c| (c.generation, c.mutation))
                .cloned();
            let sourced = per_section.get(&full).copied().unwrap_or(0);
            let mut hints = Vec::new();
            match (&w.dirty, w.unprocessed) {
                (Some(d), _) => {
                    let summary = ["added", "removed", "changed"]
                        .iter()
                        .filter_map(|k| d[k].as_u64().map(|n| format!("{} {}", n, k)))
                        .collect::<Vec<_>>();
                    hints.push(if summary.is_empty() {
                        format!(
                            "section dirty (edit g{})",
                            cause.as_ref().map(|c| c.generation).unwrap_or(0)
                        )
                    } else {
                        format!(
                            "{} (edit g{})",
                            summary.join(", "),
                            cause.as_ref().map(|c| c.generation).unwrap_or(0)
                        )
                    });
                }
                (None, true) => hints.push("unprocessed section".into()),
                _ => {}
            }
            hints.push(format!(
                "{} requirement(s) already sourced here; an unchanged statement is a no-op",
                sourced
            ));
            for a in &stale {
                let what = store
                    .graph
                    .requirements
                    .get(a)
                    .map(|r| {
                        format!(
                            "\"{}\" was quoted \"{}\"",
                            truncate(&r.statement, 80),
                            truncate(
                                r.source.as_ref().map(|s| s.quote.as_str()).unwrap_or(""),
                                60
                            )
                        )
                    })
                    .or_else(|| {
                        store
                            .graph
                            .entities
                            .get(a)
                            .map(|e| format!("entity {}: a mention's quote died", e.name))
                    })
                    .unwrap_or_default();
                hints.push(format!("stale {} {}", a, what));
            }
            // The document-level lines, once per document.
            // Mirrors docs/compiler/goals/reconcile-section.md#what-the-model-sees.
            let lines = doc_lines.entry(doc.clone()).or_insert_with(|| {
                doc_hint_lines(
                    store,
                    &doc,
                    linked.get(&doc).map(Vec::as_slice).unwrap_or(&[]),
                    &names,
                )
            });
            if let Some(l) = &lines.subjects {
                hints.push(l.clone());
            }
            hints.push(lines.known.clone());
            hints.push(format!("load {}", full));
            if sec.kind == "code" {
                hints.push(format!(
                    "code block: {} lines; coverage needs a requirement per behavioral step",
                    sec.raw.lines().count()
                ));
            }
            hints.push(
                "skill extraction; upsert_requirement, then set_coverage exactly once".into(),
            );
            out.push(build_goal(self, &full, true, change, cause, hints));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        if board.tier_open(0) > 0 {
            return Ready::Blocked("place-anchors goals decide the anchors first (tier 0)".into());
        }
        let doc = split_section_ref(&goal.target)
            .map(|(d, _)| d)
            .unwrap_or_default();
        if board.alignment_pending.contains(&doc) {
            return Ready::Blocked(format!(
                "alignment pending: g:place-anchors:{} decides the anchors first",
                doc
            ));
        }
        if let Some(level) = board.level_waiting(&doc) {
            return Ready::Blocked(format!("level {} documents reconcile first", level));
        }
        Ready::Ready
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        let mut docs: BTreeSet<String> = BTreeSet::new();
        for g in batch {
            lines.push(format!(
                "- {} full (section body with the diff marked)",
                g.target
            ));
            for a in str_list(&g.change["staleAnchors"]) {
                lines.push(format!("- {} full (stale anchor)", a));
            }
            if let Some((doc, _)) = split_section_ref(&g.target) {
                docs.insert(doc);
            }
        }
        for doc in docs {
            for (id, name, at) in linked_subjects(store, &doc) {
                lines.push(format!(
                    "- linked from {} introduced {} ({}) stub",
                    at, id, name
                ));
            }
            let mut mentioned: Vec<String> = store
                .graph
                .entities
                .iter()
                .filter(|(_, e)| e.mentions.iter().any(|m| m.doc == doc))
                .map(|(id, _)| id.clone())
                .collect();
            mentioned.truncate(40);
            for id in mentioned {
                lines.push(format!("- {} stub", id));
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "upsert_entity",
            "update_entity",
            "delete_entity",
            "upsert_requirement",
            "update_requirement",
            "delete_requirement",
            "upsert_criterion",
            "update_criterion",
            "delete_criterion",
            "set_coverage",
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = Vec::new();
        const REJECTED_NOTES: [&str; 3] = [
            "it states a fact, not a requirement",
            "it describes content or appearance, not behavior",
            "it is not a requirement on the system",
        ];
        for op in staged {
            match op {
                Op::SetCoverage {
                    doc,
                    section,
                    state,
                    note,
                } => {
                    if state == "covered" {
                        // A criterion sourced from the section covers it too
                        // (docs/compiler/goals/reconcile-section.md#gate).
                        let crit_at = |s: &Option<crate::model::SourceRef>| {
                            s.as_ref().is_some_and(|s| s.doc == *doc && s.section == *section)
                        };
                        let sourced = store
                            .graph
                            .requirements
                            .values()
                            .any(|r| r.anchored_at(doc, section))
                            || store.graph.criteria.values().any(|c| crit_at(&c.source))
                            || staged.iter().any(|o| match o {
                                Op::CreateRequirement { requirement, .. } => {
                                    requirement.anchored_at(doc, section)
                                }
                                Op::UpdateRequirement {
                                    source: Some(s), ..
                                } => s.doc == *doc && s.section == *section,
                                Op::CreateCriterion { criterion, .. } => crit_at(&criterion.source),
                                _ => false,
                            });
                        if !sourced {
                            out.push(Violation::new(
                                "uncovered-claim",
                                format!(
                                    "{}#{} is claimed covered but no requirement is sourced from it",
                                    doc, section
                                ),
                            ));
                        }
                    } else if state == "non-normative" {
                        let n = note.as_deref().unwrap_or("").trim().to_lowercase();
                        if n.is_empty() {
                            out.push(Violation::new(
                                "note-required",
                                format!("{}#{} is non-normative without a note", doc, section),
                            ));
                        } else if REJECTED_NOTES.iter().any(|r| n.contains(r)) {
                            out.push(Violation::new(
                                "rejected-reason",
                                format!(
                                    "{}#{}: \"{}\" is one of the rejected non-normative reasons",
                                    doc, section, n
                                ),
                            ));
                        }
                    }
                }
                Op::CreateRequirement { id, requirement } => {
                    if let Some(s) = requirement.source.as_ref() {
                        if !store.quote_locates(&s.doc, &s.section, &s.quote) {
                            out.push(Violation::new(
                                "quote-not-found",
                                format!(
                                    "{}: the quote does not locate in {}#{}",
                                    id, s.doc, s.section
                                ),
                            ));
                        }
                    }
                }
                Op::UpdateRequirement {
                    id,
                    source: Some(s),
                    ..
                }
                    if !store.quote_locates(&s.doc, &s.section, &s.quote) => {
                        out.push(Violation::new(
                            "quote-not-found",
                            format!(
                                "{}: the quote does not locate in {}#{}",
                                id, s.doc, s.section
                            ),
                        ));
                    }
                _ => {}
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/reconcile-section.md")
    }
}

// ---- rejudge-pair ----

pub struct RejudgePair;

fn shared_entity(store: &Store, a: &Requirement, b: &Requirement) -> Option<String> {
    let bs: BTreeSet<&str> = b.entities.iter().map(|e| store.resolve_id(e)).collect();
    a.entities
        .iter()
        .map(|e| store.resolve_id(e))
        .find(|e| bs.contains(e))
        .map(String::from)
}

// A pair the cap holds back: it derives no goal until a slot frees on the full side.
#[derive(Clone, Debug, PartialEq)]
pub struct WaitingPair {
    pub target: String,
    pub score: usize,
    // The side or sides holding `pairs-per-requirement` open pairs.
    pub full: Vec<String>,
    pub cause: Cause,
}

// The pair board: the open pair goals and the pairs waiting under the cap.
#[derive(Debug, Default)]
pub struct PairPlan {
    pub open: Vec<Goal>,
    pub waiting: BTreeMap<String, WaitingPair>,
}

impl PairPlan {
    // The waiting pair for a goal id or a pair target.
    pub fn waiting_for(&self, id_or_target: &str) -> Option<&WaitingPair> {
        let id = if id_or_target.starts_with("g:rejudge-pair:") {
            id_or_target.to_string()
        } else {
            goal_id("rejudge-pair", id_or_target)
        };
        self.waiting.get(&id)
    }
}

// Every candidate pair from the created, revised, and node-deleted records, placed
// in one deterministic order and opened while both sides have a slot: pinned pairs
// first (sticky, a dead diagnostic subject, parked, failed; a failed pair holds no
// slot), then by score, highest first, ties by target. The rest wait and derive no
// goal. Mirrors docs/compiler/reconciler.md#pairs.
pub fn pair_plan(store: &Store) -> PairPlan {
    struct PairWork {
        change: Value,
        cause: Cause,
        hints: Vec<String>,
        score: usize,
        pinned: bool,
    }
    let mut pairs: BTreeMap<String, PairWork> = BTreeMap::new();
    // The scorer's inputs once for the pass (docs/compiler/reconciler.md#pairs).
    let scoring = store.pair_scoring();
    for rec in records_of(
        store,
        &[store::CHANGE_REQ_CREATED, store::CHANGE_REQ_REVISED],
    ) {
        let rid = &rec.subject;
        let Some(req) = store.graph.requirements.get(rid) else {
            continue;
        };
        for (nbr, score) in store.pair_review_candidates_with(rec, &scoring) {
            let Some(other) = store.graph.requirements.get(&nbr) else {
                continue;
            };
            let target = pair_target(rid, &nbr);
            let shared_ent = shared_entity(store, req, other);
            // The words both statements share, minus the shared entities' boilerplate
            // the scorer dropped (docs/compiler/reconciler.md#pairs).
            let boilerplate = store.pair_boilerplate_with(req, other, &scoring);
            let other_stems = content_tokens(store, &other.statement, &other.entities);
            let mut tokens: Vec<String> = Vec::new();
            for (stemmed, word) in content_token_words(store, &req.statement, &req.entities) {
                if other_stems.contains(&stemmed)
                    && !boilerplate.contains(&store::content_stem(&word))
                    && !tokens.contains(&stemmed)
                {
                    tokens.push(stemmed);
                }
            }
            let sticky: Vec<String> = open_diags_naming(store, &[rid, &nbr])
                .into_iter()
                .filter(|(_, d)| {
                    (d.rule == "contradiction" || d.rule == "duplicate-requirement")
                        && d.subjects.iter().any(|s| s == rid)
                        && d.subjects.iter().any(|s| s == &nbr)
                })
                .map(|(id, _)| id.clone())
                .collect();
            let verb = if rec.kind == store::CHANGE_REQ_CREATED {
                "created"
            } else {
                "revised"
            };
            let change = json!({
                verb: rid,
                "fields": rec.detail["fields"],
                "shared": {"entity": shared_ent, "tokens": tokens},
                "sticky": sticky,
            });
            let cause = Cause {
                generation: rec.generation,
                mutation: rec.mutation,
                via: "entities".into(),
            };
            let same_doc = match (req.source.as_ref(), other.source.as_ref()) {
                (Some(a), Some(b)) => a.doc == b.doc,
                _ => false,
            };
            let mut hints = vec![format!(
                "{} {} in g{}{}{}",
                verb,
                rid,
                rec.generation,
                shared_ent
                    .as_ref()
                    .map(|e| format!("; shares {}", e))
                    .unwrap_or_default(),
                if tokens.is_empty() {
                    String::new()
                } else {
                    format!(" and tokens {}", tokens.join(", "))
                }
            )];
            if !sticky.is_empty() {
                hints.push(format!("sticky: {}", sticky.join(", ")));
            }
            hints.push(format!(
                "same document: {}",
                if same_doc {
                    "yes (a duplicate verdict would delete the worse-sourced side)"
                } else {
                    "no (a duplicate verdict would keep both and file duplicate-requirement)"
                }
            ));
            let diags = open_diags_naming(store, &[rid, &nbr]);
            if diags.is_empty() {
                hints.push("no open diagnostic on either side".into());
            } else {
                // A diagnostic naming both sides is on the pair; one naming one side
                // is context, and the hint says which.
                for (id, d) in diags {
                    let on_pair = d.subjects.iter().any(|s| s == rid)
                        && d.subjects.iter().any(|s| s == &nbr);
                    let where_ = if on_pair {
                        "on the pair".to_string()
                    } else if d.subjects.iter().any(|s| s == rid) {
                        format!("on {} only", rid)
                    } else {
                        format!("on {} only", nbr)
                    };
                    hints.push(format!(
                        "open {} {} ({}, {}): {}",
                        id,
                        where_,
                        d.rule,
                        d.severity,
                        truncate(&d.message, 100)
                    ));
                }
            }
            hints.push(format!("load {}; load {}", rid, nbr));
            if sticky.is_empty() {
                hints.push("skill judgment; duplicate: delete_requirement or report_diagnostic duplicate-requirement; contradiction: report_diagnostic with a prompt; consistent: mark_goal_done alone".into());
            } else {
                // The standing finding is the verdict's carrier: no second filing.
                // Mirrors docs/compiler/goals/rejudge-pair.md#tools.
                hints.push(format!(
                    "skill judgment; {} already stands on the pair: settle_pairs verdict contradiction (or duplicate) carries it and files nothing, resolved closes it when the conflict is gone, consistent is wrong while it stands",
                    sticky.join(", ")
                ));
            }
            let pinned = !sticky.is_empty();
            match pairs.get_mut(&target) {
                // The same pair from the other side: the earliest cause names the
                // goal, the score is the same from either side, a sticky tie pins.
                Some(p) => {
                    p.score = p.score.max(score);
                    p.pinned |= pinned;
                    if (cause.generation, cause.mutation) < (p.cause.generation, p.cause.mutation)
                    {
                        p.change = change;
                        p.cause = cause;
                        p.hints = hints;
                    }
                }
                None => {
                    pairs.insert(
                        target,
                        PairWork {
                            change,
                            cause,
                            hints,
                            score,
                            pinned,
                        },
                    );
                }
            }
        }
    }
    // A surviving subject of an open pair diagnostic whose partner died.
    for rec in records_of(store, &[store::CHANGE_NODE_DELETED]) {
        if rec.via != "subjects" || !rec.subject.starts_with("req:") {
            continue;
        }
        if !store.graph.requirements.contains_key(&rec.subject) {
            continue;
        }
        let deleted = str_list(&rec.detail["deleted"]);
        let Some(dead) = deleted.first() else {
            continue;
        };
        let did = rec.detail["diagnostic"].as_str().unwrap_or("").to_string();
        if !store
            .graph
            .diagnostics
            .get(&did)
            .is_some_and(|d| d.lifecycle == "open")
        {
            continue;
        }
        let target = pair_target(&rec.subject, dead);
        if let Some(p) = pairs.get_mut(&target) {
            p.pinned = true;
            continue;
        }
        let hints = vec![
            format!(
                "{} (deleted in g{}); {} names it",
                dead, rec.generation, did
            ),
            format!("load {}", rec.subject),
            "skill judgment; resolve_diagnostic when the finding died with the requirement, report_diagnostic to refile it".into(),
        ];
        pairs.insert(
            target,
            PairWork {
                change: json!({"deleted": deleted, "diagnostic": did, "survivor": rec.subject}),
                cause: rec.cause(),
                hints,
                score: 0,
                pinned: true,
            },
        );
    }
    // A parked or failed pair is pinned: it stays on the board with its state. A
    // failed pair holds no slot. Mirrors docs/compiler/reconciler.md#parked-and-failed.
    let parked: BTreeSet<&str> = store
        .status
        .parked
        .iter()
        .filter(|g| g.goal.kind == "rejudge-pair")
        .map(|g| g.goal.target.as_str())
        .collect();
    let failed: BTreeSet<&str> = store
        .status
        .failed
        .iter()
        .filter(|f| f.goal.kind == "rejudge-pair")
        .map(|f| f.goal.target.as_str())
        .collect();
    for (target, p) in pairs.iter_mut() {
        if parked.contains(target.as_str()) || failed.contains(target.as_str()) {
            p.pinned = true;
        }
    }

    // The order: pinned by target, then score descending, ties by target.
    let mut order: Vec<(&String, &PairWork)> = pairs.iter().collect();
    order.sort_by(|(ta, a), (tb, b)| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| {
                if a.pinned {
                    std::cmp::Ordering::Equal
                } else {
                    b.score.cmp(&a.score)
                }
            })
            .then_with(|| ta.cmp(tb))
    });
    let cap = limits::PAIRS_PER_REQUIREMENT_CAP;
    let mut degree: BTreeMap<&str, usize> = BTreeMap::new();
    let mut opened: Vec<&String> = Vec::new();
    let mut waiting: BTreeMap<String, WaitingPair> = BTreeMap::new();
    let mut waiting_by_side: BTreeMap<String, usize> = BTreeMap::new();
    for (target, p) in order {
        let Some((a, b)) = pair_members(target) else {
            continue;
        };
        let full: Vec<String> = [a, b]
            .iter()
            .filter(|s| degree.get(**s).copied().unwrap_or(0) >= cap)
            .map(|s| s.to_string())
            .collect();
        if p.pinned || full.is_empty() {
            opened.push(target);
            if !failed.contains(target.as_str()) {
                *degree.entry(a).or_insert(0) += 1;
                *degree.entry(b).or_insert(0) += 1;
            }
        } else {
            for s in [a, b] {
                *waiting_by_side.entry(s.to_string()).or_insert(0) += 1;
            }
            waiting.insert(
                goal_id("rejudge-pair", target),
                WaitingPair {
                    target: target.clone(),
                    score: p.score,
                    full,
                    cause: p.cause.clone(),
                },
            );
        }
    }
    let kind = &RejudgePair;
    let open = opened
        .into_iter()
        .map(|target| {
            let p = &pairs[target];
            let mut hints = p.hints.clone();
            if let Some((a, b)) = pair_members(target) {
                let side = |s: &str| match waiting_by_side.get(s).copied().unwrap_or(0) {
                    0 => format!("{}: none waiting", s),
                    n => format!("{}: {} pair(s) waiting", s, n),
                };
                hints.push(format!("score {}; {}, {}", p.score, side(a), side(b)));
            }
            build_goal(kind, target, true, p.change.clone(), Some(p.cause.clone()), hints)
        })
        .collect();
    PairPlan { open, waiting }
}

impl GoalKind for RejudgePair {
    fn kind(&self) -> &'static str {
        "rejudge-pair"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "pair"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        pair_plan(store).open
    }
    fn ready(&self, _goal: &Goal, board: &Board) -> Ready {
        tier2_ready(board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for g in batch {
            if let Some((a, b)) = pair_members(&g.target) {
                for id in [a, b] {
                    if seen.insert(id.to_string()) {
                        lines.push(format!("- {} full", id));
                    }
                }
            }
            if let Some(e) = g.change["shared"]["entity"].as_str() {
                if seen.insert(e.to_string()) {
                    lines.push(format!("- {} stub", e));
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "update_requirement",
            "delete_requirement",
            "update_entity",
            "report_diagnostic",
            "resolve_diagnostic",
            crate::tools::PAIR_SETTLE_TOOL,
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = Vec::new();
        for op in staged {
            if let Op::ReportDiagnostic { diagnostic, .. } = op {
                for s in &diagnostic.subjects {
                    let exists = store.graph.requirements.contains_key(s)
                        || store.graph.entities.contains_key(s)
                        || staged
                            .iter()
                            .any(|o| matches!(o, Op::CreateRequirement { id, .. } if id == s));
                    if !exists {
                        out.push(Violation::new(
                            "unknown-id",
                            format!("diagnostic subject {} does not exist", s),
                        ));
                    }
                }
            }
            if let Op::DeleteRequirement { id, reason } = op {
                if reason.trim().is_empty() {
                    out.push(Violation::new(
                        "reason-required",
                        format!("delete_requirement {} carries no reason", id),
                    ));
                }
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/rejudge-pair.md")
    }
}

fn tier2_ready(board: &Board) -> Ready {
    if board.tier_open(0) > 0 {
        return Ready::Blocked("place-anchors goals run first (tier 0)".into());
    }
    if board.tier_open(1) > 0 {
        return Ready::Blocked(format!(
            "{} reconcile-section goal(s) run first (tier 1)",
            board.tier_open(1)
        ));
    }
    Ready::Ready
}

fn tier3_ready(board: &Board) -> Ready {
    if let Ready::Blocked(r) = tier2_ready(board) {
        return Ready::Blocked(r);
    }
    if board.tier_open(2) > 0 {
        return Ready::Blocked(format!(
            "{} judgment goal(s) run first (tier 2)",
            board.tier_open(2)
        ));
    }
    Ready::Ready
}

// ---- review-entity ----

pub struct ReviewEntity;

// Lookalikes of an entity from the name index: name-similar (a token superset either
// way, or a shared stem) and related but separate (a name that extends this one).
pub fn lookalikes(store: &Store, id: &str) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let Some(e) = store.graph.entities.get(id) else {
        return (Vec::new(), Vec::new());
    };
    let mine = name_tokens(e);
    let mut similar = Vec::new();
    let mut related = Vec::new();
    for (oid, o) in &store.graph.entities {
        if oid == id || o.scope != e.scope {
            continue;
        }
        let theirs = name_tokens(o);
        if mine.is_empty() || theirs.is_empty() {
            continue;
        }
        let shared = mine.intersection(&theirs).count();
        if shared == 0 {
            continue;
        }
        let dice = 2.0 * shared as f64 / (mine.len() + theirs.len()) as f64;
        if dice >= 0.66 {
            similar.push((oid.clone(), o.name.clone()));
        } else if mine.is_subset(&theirs) || theirs.is_subset(&mine) {
            related.push((oid.clone(), o.name.clone()));
        }
    }
    similar.truncate(6);
    related.truncate(6);
    (similar, related)
}

// Statements whose prose names the entity or an alias (word-bounded, code spans
// blanked) without referencing it.
pub fn unreferenced_statements(store: &Store, id: &str) -> Vec<(String, String)> {
    let Some(e) = store.graph.entities.get(id) else {
        return Vec::new();
    };
    let names: Vec<String> = std::iter::once(&e.name)
        .chain(e.aliases.iter())
        .map(|n| n.to_lowercase())
        .filter(|n| n.len() >= 3)
        .collect();
    let blank_code = |s: &str| -> String {
        let mut out = String::new();
        let mut inside = false;
        for c in s.chars() {
            if c == '`' {
                inside = !inside;
                out.push(' ');
            } else if inside {
                out.push(' ');
            } else {
                out.push(c);
            }
        }
        out.to_lowercase()
    };
    let word_bounded = |hay: &str, needle: &str| -> bool {
        let mut start = 0;
        while let Some(pos) = hay[start..].find(needle) {
            let b = start + pos;
            let e = b + needle.len();
            let before_ok = b == 0 || !hay[..b].chars().last().unwrap().is_alphanumeric();
            let after_ok = e == hay.len() || !hay[e..].chars().next().unwrap().is_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
            start = e;
        }
        false
    };
    let mut out = Vec::new();
    for (rid, r) in &store.graph.requirements {
        // A word match a review already judged stays off the list
        // (docs/compiler/model/entity.md#fields).
        if e.not_about.contains(rid) || r.entities.iter().any(|x| store.resolve_id(x) == id) {
            continue;
        }
        // The quote is the document's own text; a statement is the session's wording,
        // and a paraphrase that added the name would otherwise raise its own hint
        // (docs/compiler/goals/review-entity.md#hints).
        // Both the quote (the document's text) and the statement (the session's) must
        // name the entity: a paraphrase that added the name fails the first, a sentence
        // split into several statements fails the second for the parts not about it.
        let quote_text = blank_code(
            r.source
                .as_ref()
                .map(|s| s.quote.as_str())
                .unwrap_or(&r.statement),
        );
        let statement_text = blank_code(&r.statement);
        if names.iter().any(|n| word_bounded(&quote_text, n) && word_bounded(&statement_text, n)) {
            out.push((rid.clone(), r.statement.clone()));
            if out.len() >= 6 {
                break;
            }
        }
    }
    out
}

// The composition edges on an entity's statements whose part has no parent, or a
// parent outside the stated whole: (whole, part, hint line), one per edge. The
// review hint lists them and the review gate holds one `parts` verdict per part.
// Mirrors docs/compiler/goals/review-entity.md#hints and #gate.
pub fn unparented_parts(store: &Store, id: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    for r in store.requirements_referencing(id) {
        let Some(q) = store.graph.requirements.get(&r) else {
            continue;
        };
        for edge in &q.edges {
            if edge.rel_type.as_deref() != Some("composition") {
                continue;
            }
            let Some(part) = store.graph.entities.get(&edge.b) else {
                continue;
            };
            // A parent under the stated whole satisfies the composition
            // transitively (a scene under a chapter under the manuscript).
            let line = match &part.parent {
                None => format!("{} -> {} (whole -> part)", edge.a, edge.b),
                Some(p) if p != &edge.a && !store.is_ancestor(&edge.a, &edge.b) => {
                    format!("{} -> {} (whole -> part; parent now {}, not under the whole)", edge.a, edge.b, p)
                }
                _ => continue,
            };
            if !out.iter().any(|(_, _, l)| l == &line) {
                out.push((edge.a.clone(), edge.b.clone(), line));
            }
        }
    }
    out
}

// The open diagnostics a review lists on an entity and its statements, harness-owned
// rules aside: the review gate holds one `diagnostics` verdict per id.
// Mirrors docs/compiler/goals/review-entity.md#gate.
pub fn review_diagnostics(store: &Store, id: &str) -> Vec<String> {
    let reqs = store.requirements_referencing(id);
    let mut subjects: Vec<&str> = reqs.iter().map(String::as_str).collect();
    subjects.push(id);
    open_diags_naming(store, &subjects)
        .into_iter()
        .map(|(did, _)| did.clone())
        .collect()
}

impl GoalKind for ReviewEntity {
    fn kind(&self) -> &'static str {
        "review-entity"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "entity"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut by_subject: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_ENTITY, store::CHANGE_NODE_DELETED]) {
            if !rec.subject.starts_with("ent:") || !store.graph.entities.contains_key(&rec.subject)
            {
                continue;
            }
            if rec.kind == store::CHANGE_NODE_DELETED && rec.via != "subjects" {
                continue;
            }
            by_subject.entry(rec.subject.clone()).or_default().push(rec);
        }
        let mut out = Vec::new();
        for (id, recs) in by_subject {
            let e = &store.graph.entities[&id];
            let vias: BTreeSet<String> = recs.iter().map(|r| r.via.clone()).collect();
            let mut requirements: Vec<String> = Vec::new();
            let mut deleted: Vec<String> = Vec::new();
            for r in &recs {
                requirements.extend(str_list(&r.detail["requirements"]));
                deleted.extend(str_list(&r.detail["deleted"]));
            }
            requirements.sort();
            requirements.dedup();
            let change = json!({
                "via": vias,
                "requirements": requirements,
                "deleted": deleted,
            });
            let cause = earliest_cause(&recs);
            let reqs = store.requirements_referencing(&id);
            let mut hints = vec![format!(
                "entity-changed via {} in g{}; {} requirement(s) on it",
                vias.iter().cloned().collect::<Vec<_>>().join(", "),
                cause.as_ref().map(|c| c.generation).unwrap_or(0),
                reqs.len()
            )];
            let (similar, related) = lookalikes(store, &id);
            if !similar.is_empty() {
                hints.push(format!(
                    "lookalikes (name-similar, a merge when one concept): {}",
                    similar
                        .iter()
                        .map(|(i, n)| format!("{} ({})", i, n))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !related.is_empty() {
                hints.push(format!(
                    "lookalikes (related, separate by default): {}",
                    related
                        .iter()
                        .map(|(i, n)| format!("{} ({})", i, n))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            let unref = unreferenced_statements(store, &id);
            if !unref.is_empty() {
                hints.push(format!(
                    "unreferenced (word matches, not judgments): {}",
                    unref
                        .iter()
                        .map(|(r, s)| format!("{} \"{}\"", r, truncate(s, 60)))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            let no_edges: Vec<&String> = reqs
                .iter()
                .filter(|r| {
                    store
                        .graph
                        .requirements
                        .get(*r)
                        .is_some_and(|q| q.entities.len() >= 2 && q.edges.is_empty())
                })
                .collect();
            if !no_edges.is_empty() {
                hints.push(format!(
                    "multi-entity statements without edges (each has its own declare-edges goal; leave the edges to it): {}",
                    no_edges
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            // A stated composition whose part has no parent, or a parent disagreeing
            // with the stated whole: the review's contract says to set parent on the
            // part, so name each such edge (both the missing and the mismatch case).
            let parentless: Vec<String> = unparented_parts(store, &id)
                .into_iter()
                .map(|(_, _, line)| line)
                .collect();
            if !parentless.is_empty() {
                hints.push(format!(
                    "composition edges whose part has no parent or a parent outside the stated whole (the edge reads whole -> part; set parent on the part with update_entity, or fix a reversed edge on the requirement, unless the part is a generic rule-carrier such as Department beside the named departments, which the judgment skill keeps parentless): {}",
                    parentless.join(", ")
                ));
            }
            let mut subjects: Vec<&str> = reqs.iter().map(String::as_str).collect();
            subjects.push(&id);
            let diags = open_diags_naming(store, &subjects);
            if !diags.is_empty() {
                hints.push(format!(
                    "{} open diagnostic(s): {}",
                    diags.len(),
                    diags
                        .iter()
                        .map(|(did, d)| {
                            let dead: Vec<&str> = d
                                .subjects
                                .iter()
                                .filter(|s| {
                                    !store.graph.requirements.contains_key(*s)
                                        && !store.graph.entities.contains_key(*s)
                                })
                                .map(String::as_str)
                                .collect();
                            format!(
                                "{} ({}{})",
                                did,
                                d.rule,
                                if dead.is_empty() {
                                    String::new()
                                } else {
                                    format!(", deleted: {}", dead.join(", "))
                                }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            // The level views around the entity, so a review never guesses a view
            // id. Mirrors docs/compiler/goals/review-entity.md#hints.
            {
                let mut views = Vec::new();
                if let Some(v) = crate::derive::level_view_id(store, &id) {
                    views.push(format!("{} (this entity)", v));
                }
                let level_target = match &e.parent {
                    Some(p) => store.resolve_id(p).to_string(),
                    None => format!("{}{}", crate::board::SCOPE_TARGET_PREFIX, e.scope),
                };
                if let Some(v) = crate::derive::level_view_id(store, &level_target) {
                    views.push(format!("{} (the level above)", v));
                }
                if !views.is_empty() {
                    hints.push(format!("level views: {}", views.join(", ")));
                }
            }
            if let Some((soft, _)) = limits::threshold(
                "requirements-per-entity",
                e.limits.get("requirements-per-entity").map(|b| b.value),
            ) {
                if reqs.len() as u64 > soft {
                    hints.push(format!(
                        "{} requirements > {} (requirements-per-entity); abstract-entity follows, information only",
                        reqs.len(),
                        soft
                    ));
                }
            }
            hints.push(format!("load {}", id));
            hints.push("skill judgment; update_entity, merge_entities, update_requirement, report_diagnostic, resolve_diagnostic".into());
            out.push(build_goal(self, &id, true, change, cause, hints));
        }
        out
    }
    fn ready(&self, _goal: &Goal, board: &Board) -> Ready {
        tier2_ready(board)
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            let (similar, related) = lookalikes(store, &g.target);
            for (id, _) in similar.iter().chain(related.iter()) {
                lines.push(format!("- {} stub", id));
            }
            for (rid, _) in unreferenced_statements(store, &g.target) {
                lines.push(format!("- {} stub", rid));
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "update_entity",
            "merge_entities",
            "update_requirement",
            "delete_requirement",
            "report_diagnostic",
            "resolve_diagnostic",
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = RejudgePair.gates(store, staged);
        for op in staged {
            if let Op::MergeEntities { keep, absorb, .. } = op {
                let scope = |id: &str| store.graph.entities.get(id).map(|e| e.scope.clone());
                if let (Some(a), Some(b)) = (scope(keep), scope(absorb)) {
                    if a != b {
                        out.push(Violation::new(
                            "scope-mismatch",
                            format!(
                                "merge_entities {} into {} crosses scopes ({} and {})",
                                absorb, keep, b, a
                            ),
                        ));
                    }
                }
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/review-entity.md")
    }
}

// ---- retrace ----

pub struct Retrace;

// The references on a node that name a dead or missing node.
pub fn dangling_references(store: &Store, id: &str) -> Vec<String> {
    let alive = |x: &str| {
        let r = store.resolve_id(x);
        store.graph.entities.contains_key(r) || store.graph.requirements.contains_key(r)
    };
    let mut out = Vec::new();
    if let Some(v) = store.graph.views.get(id) {
        for (i, m) in v.members.iter().enumerate() {
            if !alive(m) {
                out.push(format!("members[{}] {}", i, m));
            }
        }
        for m in &v.collapse {
            if !alive(m) {
                out.push(format!("collapse {}", m));
            }
        }
        for x in &v.excluded {
            if !alive(&x.id) {
                out.push(format!("excluded {}", x.id));
            }
        }
        if let Some(Provenance::Derived { from, .. }) = &v.provenance {
            for (i, f) in from.iter().enumerate() {
                if !alive(f) && !store.graph.views.contains_key(f) {
                    out.push(format!("from[{}] {}", i, f));
                }
            }
        }
    }
    if let Some(e) = store.graph.entities.get(id) {
        if let Some(p) = &e.parent {
            if !alive(p) {
                out.push(format!("parent {}", p));
            }
        }
        for a in &e.attributes {
            match &a.provenance {
                Provenance::Quote(s) => {
                    if !store.quote_locates(&s.doc, &s.section, &s.quote) {
                        out.push(format!(
                            "attributes.{}.provenance {}#{}",
                            a.name, s.doc, s.section
                        ));
                    }
                }
                Provenance::Derived { from, .. } => {
                    for f in from {
                        if !alive(f) {
                            out.push(format!("attributes.{}.from {}", a.name, f));
                        }
                    }
                }
                Provenance::Decree { .. } => {}
            }
        }
        if let Some(Provenance::Derived { from, .. }) = &e.provenance {
            for (i, f) in from.iter().enumerate() {
                if !alive(f) {
                    out.push(format!("from[{}] {}", i, f));
                }
            }
        }
    }
    if let Some(r) = store.graph.requirements.get(id) {
        for e in &r.entities {
            if !alive(e) {
                out.push(format!("entities {}", e));
            }
        }
        for edge in &r.edges {
            for end in [&edge.a, &edge.b] {
                if !alive(end) {
                    out.push(format!("edges {}", end));
                }
            }
        }
        if let Some(t) = &r.transition {
            if !alive(&t.subject) {
                out.push(format!("transition.subject {}", t.subject));
            }
        }
        if let Some(Provenance::Derived { from, .. }) = &r.provenance {
            for (i, f) in from.iter().enumerate() {
                if !alive(f) {
                    out.push(format!("from[{}] {}", i, f));
                }
            }
        }
    }
    out
}

impl GoalKind for Retrace {
    fn kind(&self) -> &'static str {
        "retrace"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "node"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut by_subject: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(
            store,
            &[store::CHANGE_VIEW_MEMBER_GONE, store::CHANGE_NODE_DELETED],
        ) {
            let exists = store.graph.views.contains_key(&rec.subject)
                || store.graph.entities.contains_key(&rec.subject)
                || store.graph.requirements.contains_key(&rec.subject);
            if !exists {
                continue;
            }
            if rec.kind == store::CHANGE_NODE_DELETED
                && !matches!(rec.via.as_str(), "from" | "edges" | "attributes")
            {
                continue;
            }
            by_subject.entry(rec.subject.clone()).or_default().push(rec);
        }
        let mut out = Vec::new();
        for (id, recs) in by_subject {
            let mut dead: Vec<String> = Vec::new();
            let mut vias: BTreeSet<String> = BTreeSet::new();
            for r in &recs {
                dead.extend(str_list(&r.detail["gone"]));
                dead.extend(str_list(&r.detail["deleted"]));
                vias.insert(r.via.clone());
            }
            dead.sort();
            dead.dedup();
            let cause = earliest_cause(&recs);
            let change = json!({"deleted": dead, "via": vias, "in": format!("g{}", cause.as_ref().map(|c| c.generation).unwrap_or(0))});
            let mut hints = Vec::new();
            for d in &dead {
                let fate = match store.graph.redirects.get(d) {
                    Some(t) if !t.is_empty() => format!("redirects to {}", t),
                    Some(_) => "tombstone".to_string(),
                    None => "gone".to_string(),
                };
                hints.push(format!(
                    "{} deleted in g{} ({}); jazyk ripple {} --back",
                    d,
                    cause.as_ref().map(|c| c.generation).unwrap_or(0),
                    fate,
                    d
                ));
            }
            let dangling = dangling_references(store, &id);
            if !dangling.is_empty() {
                hints.push(format!("dangling: {}", dangling.join(", ")));
            }
            if let Some(v) = store.graph.views.get(&id) {
                hints.push(format!(
                    "{} view, {} member(s) in order: {}",
                    v.kind,
                    v.members.len(),
                    v.members
                        .iter()
                        .map(|m| if dead.contains(m) {
                            format!("{} (deleted)", m)
                        } else {
                            m.clone()
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if let Some(ty) = crate::derive::instance_types(store).get(&id) {
                hints.push(format!("instance of {}", ty));
            }
            hints.push(format!("load {}", id));
            for s in skills_for("retrace", store, &id) {
                hints.push(format!("skill {}", s));
            }
            hints.push("update_view remove_members or add_members, update_entity, update_requirement, delete_view".into());
            out.push(build_goal(self, &id, true, change, cause, hints));
        }
        out
    }
    fn ready(&self, _goal: &Goal, board: &Board) -> Ready {
        tier2_ready(board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            for d in str_list(&g.change["deleted"]) {
                lines.push(format!("- {} tombstone", d));
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "upsert_view",
            "update_view",
            "delete_view",
            "upsert_entity",
            "update_entity",
            "delete_entity",
            "merge_entities",
            "upsert_requirement",
            "update_requirement",
            "delete_requirement",
            "report_diagnostic",
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = Vec::new();
        for op in staged {
            if let Op::CreateEntity { id, entity } = op {
                if store.graph.redirects.get(id).is_some_and(|t| t.is_empty())
                    || store.graph.redirects.iter().any(|(dead, t)| {
                        t.is_empty() && dead == &format!("ent:{}", crate::md::slug(&entity.name))
                    })
                {
                    out.push(Violation::new(
                        "recreated-node",
                        format!(
                            "{} re-creates a deleted node; point at a survivor instead",
                            id
                        ),
                    ));
                }
            }
            if let Op::DeleteView { id, reason } = op {
                if reason.trim().is_empty() {
                    out.push(Violation::new(
                        "reason-required",
                        format!("delete_view {} carries no reason", id),
                    ));
                }
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/retrace.md")
    }
}

// ---- conform-instance ----

pub struct ConformInstance;

// Attribute names a type declares, its generalizations included.
pub fn declared_attributes(store: &Store, ty: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut frontier = vec![ty.to_string()];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    while let Some(t) = frontier.pop() {
        if !seen.insert(t.clone()) {
            continue;
        }
        if let Some(e) = store.graph.entities.get(&t) {
            out.extend(e.attributes.iter().map(|a| a.name.to_lowercase()));
        }
        for rel in store.graph.relationships.values() {
            for c in &rel.contributions {
                if c.r#type == "generalization" && c.a == t {
                    frontier.push(c.b.clone());
                }
            }
        }
    }
    out
}

// The links an instance carries: its outgoing contributions other than the
// instantiation edge, (to, type), as the links hint lists them and the gate holds
// one verdict per `to`. Mirrors docs/compiler/goals/conform-instance.md#gate.
pub fn instance_links(store: &Store, id: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for c in store.graph.relationships.values().flat_map(|r| r.contributions.iter()) {
        if c.a == id && c.r#type != INSTANTIATION && !out.iter().any(|(b, t)| b == &c.b && t == &c.r#type) {
            out.push((c.b.clone(), c.r#type.clone()));
        }
    }
    out
}

impl GoalKind for ConformInstance {
    fn kind(&self) -> &'static str {
        "conform-instance"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "instance"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let types = crate::derive::instance_types(store);
        let mut by_subject: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_INSTANCE]) {
            if store.graph.entities.contains_key(&rec.subject) && types.contains_key(&rec.subject) {
                by_subject.entry(rec.subject.clone()).or_default().push(rec);
            }
        }
        let mut out = Vec::new();
        for (id, recs) in by_subject {
            let ty = types[&id].clone();
            let vias: BTreeSet<String> = recs.iter().map(|r| r.via.clone()).collect();
            let cause = earliest_cause(&recs);
            let change = json!({"type": ty, "via": vias});
            let inst = &store.graph.entities[&id];
            let mut hints = vec![format!(
                "type {} changed in g{} via {}",
                ty,
                cause.as_ref().map(|c| c.generation).unwrap_or(0),
                vias.iter().cloned().collect::<Vec<_>>().join(", ")
            )];
            let declared: Vec<String> = store
                .graph
                .entities
                .get(&ty)
                .map(|t| {
                    t.attributes
                        .iter()
                        .map(|a| {
                            format!("{}: {}", a.name, a.r#type.as_deref().unwrap_or("untyped"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            // Attributes a generalization of the type declares take a verdict too.
            let own: BTreeSet<String> = store
                .graph
                .entities
                .get(&ty)
                .map(|t| t.attributes.iter().map(|a| a.name.to_lowercase()).collect())
                .unwrap_or_default();
            let inherited: Vec<String> = declared_attributes(store, &ty)
                .into_iter()
                .filter(|n| !own.contains(n))
                .collect();
            hints.push(format!(
                "type declares {}{}",
                if declared.is_empty() {
                    "no attributes".to_string()
                } else {
                    declared.join(", ")
                },
                if inherited.is_empty() {
                    String::new()
                } else {
                    format!("; inherited {}", inherited.join(", "))
                }
            ));
            let values: Vec<String> = inst
                .attributes
                .iter()
                .map(|a| format!("{} = {}", a.name, a.value.as_deref().unwrap_or("?")))
                .collect();
            if !values.is_empty() {
                hints.push(format!("instance values {}", values.join(", ")));
            }
            let links: Vec<String> = instance_links(store, &id)
                .into_iter()
                .map(|(to, ty)| format!("{} ({})", to, ty))
                .collect();
            if !links.is_empty() {
                hints.push(format!("links {}", links.join(", ")));
            }
            let diags = open_diags_naming(store, &[&id]);
            if diags.is_empty() {
                hints.push("no open diagnostic".into());
            } else {
                hints.push(format!(
                    "open: {}",
                    diags
                        .iter()
                        .map(|(did, d)| format!("{} ({})", did, d.rule))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            hints.push(format!("load {}; load {}", id, ty));
            hints.push("skill conformance; update_entity on the instance, update_requirement on the example's requirement, report_diagnostic nonconformant-instance".into());
            out.push(build_goal(self, &id, true, change, cause, hints));
        }
        out
    }
    fn ready(&self, _goal: &Goal, board: &Board) -> Ready {
        tier2_ready(board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            if let Some(t) = g.change["type"].as_str() {
                lines.push(format!("- {} full", t));
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "update_entity",
            "update_requirement",
            "report_diagnostic",
            "resolve_diagnostic",
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        // The type is never edited from this goal.
        let types: BTreeSet<String> = crate::derive::instance_types(store).into_values().collect();
        let mut out = Vec::new();
        for op in staged {
            if let Op::UpdateEntity {
                id,
                name,
                definition,
                add_aliases,
                remove_aliases,
                add_mention,
                stereotype,
                parent,
                clear_parent,
                set_attributes,
                add_attributes,
                provenance,
                ..
            } = op
            {
                // A not_about-only update is a review annotation, not a type edit
                // (docs/compiler/goals/conform-instance.md#gate).
                let annotation_only = name.is_none()
                    && definition.is_none()
                    && add_aliases.is_empty()
                    && remove_aliases.is_empty()
                    && add_mention.is_none()
                    && stereotype.is_none()
                    && parent.is_none()
                    && !clear_parent
                    && set_attributes.is_none()
                    && add_attributes.is_empty()
                    && provenance.is_none();
                if types.contains(id) && !annotation_only {
                    out.push(Violation::new(
                        "type-edited",
                        format!("{} is a type; conform the instance, never the type", id),
                    ));
                }
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/conform-instance.md")
    }
}

// ---- criteria ----

// The requirements of one entity that still owe criteria: none recorded, or a
// derived one gone stale under a statement change. Keyed by the requirement's
// leading entity. Mirrors docs/compiler/goals/derive-criteria.md#created-when.
pub fn criteria_owed(store: &Store) -> BTreeMap<String, Vec<(String, &'static str)>> {
    let mut out: BTreeMap<String, Vec<(String, &'static str)>> = BTreeMap::new();
    for (rid, r) in &store.graph.requirements {
        let Some(first) = r.entities.first() else {
            continue;
        };
        let entity = store.resolve_id(first).to_string();
        if !store.graph.entities.contains_key(&entity) {
            continue;
        }
        let crits = store.criteria_of(rid);
        let reason = if crits.is_empty() {
            "none"
        } else if crits
            .iter()
            .any(|c| store.graph.criteria.get(c).is_some_and(|c| c.stale))
        {
            "revised"
        } else {
            continue;
        };
        out.entry(entity).or_default().push((rid.clone(), reason));
    }
    out
}

pub struct DeriveCriteria;

impl GoalKind for DeriveCriteria {
    fn kind(&self) -> &'static str {
        "derive-criteria"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "entity"
    }
    fn derive_goals(&self, store: &Store, gen: &GenSettings) -> Vec<Goal> {
        // No deliverable, no generation scope: stated criteria still enter through
        // extraction and the criterion-missing check reports the gaps.
        if !gen.configured {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (entity, reqs) in criteria_owed(store) {
            let none = reqs.iter().filter(|(_, why)| *why == "none").count();
            let revised = reqs.len() - none;
            let none_ids: Vec<&str> = reqs.iter().filter(|(_, w)| *w == "none").map(|(r, _)| r.as_str()).collect();
            let revised_ids: Vec<&str> = reqs.iter().filter(|(_, w)| *w == "revised").map(|(r, _)| r.as_str()).collect();
            let mut hints = vec![format!(
                "requirements: {} ({} without criteria, {} revised)",
                reqs.len(),
                none,
                revised
            )];
            for (rid, why) in &reqs {
                if let Some(r) = store.graph.requirements.get(rid) {
                    let crits = store.criteria_of(rid);
                    hints.push(format!(
                        "{} ({}): {}{}",
                        rid,
                        why,
                        truncate(&r.statement, 120),
                        if crits.is_empty() {
                            String::new()
                        } else {
                            format!("; criteria {}", crits.join(", "))
                        }
                    ));
                }
            }
            // Sections of the requirements' documents that read as examples or cases.
            let docs: BTreeSet<String> = reqs
                .iter()
                .filter_map(|(rid, _)| store.graph.requirements.get(rid))
                .filter_map(|r| r.source.as_ref().map(|s| s.doc.clone()))
                .collect();
            let mut nearby: Vec<String> = Vec::new();
            for doc in &docs {
                if let Some(rec) = store.docs.get(doc) {
                    for (r, sec) in &rec.sections {
                        let t = sec.title.to_lowercase();
                        if t.contains("example") || t.contains("test") || t.contains("case") {
                            nearby.push(format!("{}#{}", doc, r));
                        }
                    }
                }
            }
            if !nearby.is_empty() {
                nearby.sort();
                nearby.truncate(8);
                hints.push(format!("examples nearby: {}", nearby.join(", ")));
            }
            hints.push("skill criteria".into());
            hints.push("upsert_criterion per requirement; report_diagnostic invented-choice for a value the documents do not give".into());
            let cause = store
                .status
                .changes
                .iter()
                .filter(|c| reqs.iter().any(|(rid, _)| c.subject == *rid))
                .max_by_key(|c| (c.generation, c.mutation))
                .map(|c| c.cause());
            out.push(build_goal(
                self,
                &entity,
                true,
                json!({"none": none_ids, "revised": revised_ids}),
                cause,
                hints,
            ));
        }
        out
    }
    fn ready(&self, _goal: &Goal, board: &Board) -> Ready {
        tier3_ready(board)
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        // The source section of each listed requirement, once: the criterion's values
        // live there (docs/compiler/goals/derive-criteria.md#what-the-model-sees).
        let mut sections: Vec<String> = Vec::new();
        for g in batch {
            lines.push(format!("- {} stub", g.target));
            for key in ["none", "revised"] {
                for r in g.change[key].as_array().into_iter().flatten() {
                    if let Some(id) = r.as_str() {
                        lines.push(format!("- {} full", id));
                        if let Some(src) = store.graph.requirements.get(id).and_then(|q| q.source.as_ref()) {
                            let sec = format!("{}#{}", src.doc, src.section);
                            if !sections.contains(&sec) {
                                sections.push(sec);
                            }
                        }
                    }
                }
            }
        }
        for sec in sections {
            lines.push(format!("- {} full", sec));
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "upsert_criterion",
            "update_criterion",
            "delete_criterion",
            "report_diagnostic",
        ]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/derive-criteria.md")
    }
}

// ---- ledger kinds ----

// The persisted ledger-stale record on the subject, else the latest record that
// explains the disagreement, else the ledger comparison at the current generation.
fn ledger_cause(store: &Store, subject: &str) -> Cause {
    store
        .status
        .changes
        .iter()
        .filter(|c| c.kind == CHANGE_LEDGER_STALE && c.subject == subject)
        .max_by_key(|c| (c.generation, c.mutation))
        .map(|c| c.cause())
        .or_else(|| {
            store
                .status
                .changes
                .iter()
                .filter(|c| {
                    c.subject == subject
                        && matches!(
                            c.kind.as_str(),
                            store::CHANGE_REQ_CREATED
                                | store::CHANGE_REQ_REVISED
                                | store::CHANGE_ENTITY
                        )
                })
                .max_by_key(|c| (c.generation, c.mutation))
                .map(|c| c.cause())
        })
        .unwrap_or(Cause {
            generation: store.status.generation,
            mutation: 0,
            via: "ledger".into(),
        })
}

pub struct Bind;

impl GoalKind for Bind {
    fn kind(&self) -> &'static str {
        "bind"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "requirement"
    }
    fn derive_goals(&self, store: &Store, gen: &GenSettings) -> Vec<Goal> {
        let ledger = crate::gen::Ledger::load(&store.out);
        crate::bind::pending(store, gen)
            .into_iter()
            .filter_map(|p| {
                let rid = p["requirement"].as_str()?.to_string();
                let entity = p["entity"].as_str().unwrap_or("").to_string();
                let reason = p["reason"].as_str().unwrap_or("").to_string();
                let mut hints = vec![format!("reason: {}", reason)];
                if let Some(row) = ledger.requirements.get(&rid) {
                    hints.push(format!("previous test: {}", row.test.name));
                }
                if let Some(e) = ledger.entities.get(&crate::gen::slug_of(&entity)) {
                    if !e.files.is_empty() {
                        hints.push(format!("entity files: {}", e.files.join(", ")));
                    }
                }
                hints.push(format!(
                    "suggested test name: {}-{}",
                    crate::gen::req_slug(&rid),
                    &hash_hex(
                        &store
                            .graph
                            .requirements
                            .get(&rid)
                            .map(|r| r.statement.clone())
                            .unwrap_or_default()
                    )[..8]
                ));
                hints.push(format!(
                    "medium: {}",
                    ledger
                        .medium
                        .as_ref()
                        .map(|m| m.line())
                        .unwrap_or_else(|| "undecided; this session decides it".into())
                ));
                hints.push(format!("load {}", entity));
                hints.push("begin_binding, then record_binding".into());
                let criteria = store.criteria_of(&rid);
                if !criteria.is_empty() {
                    hints.push(format!("criteria: {}", criteria.join(", ")));
                }
                Some(build_goal(
                    self,
                    &rid,
                    true,
                    json!({"goal": "bind", "reason": reason, "entity": entity, "criteria": criteria.len()}),
                    Some(ledger_cause(store, &rid)),
                    hints,
                ))
            })
            .collect()
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        if let Ready::Blocked(r) = tier3_ready(board) {
            return Ready::Blocked(r);
        }
        // The evidence exists before a test encodes it
        // (docs/compiler/reconciler.md#tiers).
        let entity = goal.change["entity"].as_str().unwrap_or("");
        if board.open_kind_on("derive-criteria", entity) {
            return Ready::Blocked(format!(
                "criteria: derive-criteria on {} runs first",
                entity
            ));
        }
        if goal.change["criteria"].as_u64() == Some(0) {
            return Ready::Blocked(
                "criteria: the requirement carries none; derive-criteria writes them".into(),
            );
        }
        Ready::Ready
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        batch
            .iter()
            .flat_map(|g| {
                let mut v = vec![format!("- {} full", g.target)];
                if let Some(e) = g.change["entity"].as_str() {
                    v.push(format!("- {} stub", e));
                }
                v
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "binding_tasks",
            "begin_binding",
            "record_binding",
            "generation_tasks",
            "begin_generation",
            "record_generation",
            "run_tests",
        ]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/bind.md")
    }
}

pub struct Generate;

impl GoalKind for Generate {
    fn kind(&self) -> &'static str {
        "generate"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "entity"
    }
    fn derive_goals(&self, store: &Store, gen: &GenSettings) -> Vec<Goal> {
        let ledger = crate::gen::Ledger::load(&store.out);
        let pending = crate::gen::pending(store, gen);
        if pending.is_empty() {
            return Vec::new();
        }
        let by_entity = crate::gen::ReqsByEntity::build(store);
        pending
            .into_iter()
            .filter_map(|p| {
                let id = p["entity"].as_str()?.to_string();
                let reason = p["reason"].as_str().unwrap_or("").to_string();
                let changed = str_list(&p["changed"]);
                let mut hints = vec![format!("reason: {}", reason)];
                if !changed.is_empty() {
                    hints.push(format!("changed: {}", changed.join(", ")));
                }
                let rids = by_entity.of(&id);
                let bound: Vec<String> = rids
                    .iter()
                    .filter_map(|r| ledger.requirements.get(r).map(|row| row.test.name.clone()))
                    .collect();
                if !bound.is_empty() {
                    hints.push(format!("bound tests to make pass: {}", bound.join(", ")));
                }
                if let Some(b) = &ledger.build {
                    hints.push(format!("build: {} (cwd {})", b.run, b.cwd));
                }
                let n = rids.len();
                if n > 20 {
                    hints.push(format!("{} parts of 20 requirements", n.div_ceil(20)));
                }
                if let Some(p) = store.graph.entities.get(&id).and_then(|e| e.parent.clone()) {
                    hints.push(format!("load {}", p));
                }
                hints.push("begin_generation, then record_generation, then run_tests".into());
                Some(build_goal(
                    self,
                    &id,
                    true,
                    json!({"goal": "generate", "reason": reason, "changed": changed}),
                    Some(ledger_cause(store, &id)),
                    hints,
                ))
            })
            .collect()
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        if let Ready::Blocked(r) = tier3_ready(board) {
            return Ready::Blocked(r);
        }
        let owed = board.bind_open_for_entity(&goal.target);
        if owed > 0 {
            return Ready::Blocked(format!(
                "binding first: {} of the entity's requirements owe a bind",
                owed
            ));
        }
        Ready::Ready
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        batch
            .iter()
            .flat_map(|g| {
                let mut v = vec![format!("- {} full", g.target)];
                if let Some(e) = store.graph.entities.get(&g.target) {
                    if let Some(p) = &e.parent {
                        v.push(format!("- {} stub", p));
                    }
                }
                for (id, e) in &store.graph.entities {
                    if e.parent.as_deref() == Some(g.target.as_str()) {
                        v.push(format!("- {} stub", id));
                    }
                }
                v
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "generation_tasks",
            "begin_generation",
            "record_generation",
            "binding_tasks",
            "begin_binding",
            "record_binding",
            "run_tests",
        ]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/generate.md")
    }
}

pub struct Verify;

impl GoalKind for Verify {
    fn kind(&self) -> &'static str {
        "verify"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "ledger row"
    }
    fn derive_goals(&self, store: &Store, gen: &GenSettings) -> Vec<Goal> {
        let bind_reqs: BTreeSet<String> = crate::bind::pending(store, gen)
            .iter()
            .filter_map(|p| p["requirement"].as_str().map(String::from))
            .collect();
        crate::verify::pending(store, gen, Some("stale"), None)
            .into_iter()
            .filter(|p| p["status"] != "unimplemented" && p["status"] != "partial" && p["reason"] != "not-generated")
            .filter(|p| !bind_reqs.contains(p["requirement"].as_str().unwrap_or_default()))
            .filter_map(|p| {
                let rid = p["requirement"].as_str()?.to_string();
                let entity = p["entity"].as_str().unwrap_or("").to_string();
                let reason = p["reason"].as_str().unwrap_or("").to_string();
                let kind = p["test"]["kind"].as_str().unwrap_or("").to_string();
                let mut hints = vec![format!(
                    "reason: {}; previous verdict: {}",
                    reason,
                    p["lastVerdict"].as_str().unwrap_or("none")
                )];
                if kind == "programmatic" {
                    hints.push(format!(
                        "run: {} (cwd {})",
                        p["test"]["run"].as_str().unwrap_or(""),
                        p["test"]["cwd"].as_str().unwrap_or(".")
                    ));
                } else {
                    hints.push(format!(
                        "criteria: {}",
                        p["test"]["artifact"].as_str().unwrap_or("")
                    ));
                }
                hints.push("begin_verification, then record_verdict".into());
                Some(build_goal(
                    self,
                    &rid,
                    true,
                    json!({"goal": "verify", "reason": reason, "kind": kind, "entity": entity}),
                    Some(ledger_cause(store, &rid)),
                    hints,
                ))
            })
            .collect()
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        if let Ready::Blocked(r) = tier3_ready(board) {
            return Ready::Blocked(r);
        }
        let entity = goal.change["entity"].as_str().unwrap_or("");
        if board.bind_open_for_entity(entity) > 0 || board.open(&goal_id("bind", &goal.target)) {
            return Ready::Blocked(
                "binding first: the row's requirement or entity owes a bind".into(),
            );
        }
        if board.open(&goal_id("generate", entity)) {
            return Ready::Blocked(format!("generation first: {} owes a generate", entity));
        }
        Ready::Ready
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        batch
            .iter()
            .flat_map(|g| {
                let mut v = vec![format!("- {} full", g.target)];
                if let Some(e) = g.change["entity"].as_str() {
                    v.push(format!("- {} stub", e));
                }
                v
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "verification_tasks",
            "begin_verification",
            "run_tests",
            "record_verdict",
        ]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/verify.md")
    }
}

// ---- ratify and answer (blocked on a human) ----

pub struct Ratify;

impl GoalKind for Ratify {
    fn kind(&self) -> &'static str {
        "ratify"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "fact"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut out: BTreeMap<String, Goal> = BTreeMap::new();
        let proposal_for = |subject: &str| -> Option<(String, &Diagnostic)> {
            store
                .graph
                .diagnostics
                .iter()
                .find(|(_, d)| {
                    d.lifecycle == "open"
                        && d.rule == "ratification-pending"
                        && d.subjects.iter().any(|s| s == subject)
                })
                .map(|(id, d)| (id.clone(), d))
        };
        let provenance_of = |subject: &str| -> Option<Provenance> {
            store
                .graph
                .requirements
                .get(subject)
                .and_then(|r| r.provenance.clone())
                .or_else(|| {
                    store
                        .graph
                        .entities
                        .get(subject)
                        .and_then(|e| e.provenance.clone())
                })
                .or_else(|| {
                    store
                        .graph
                        .criteria
                        .get(subject)
                        .and_then(|c| c.provenance.clone())
                })
        };
        let hints_for =
            |subject: &str, prov: Option<&Provenance>, proposal: Option<&(String, &Diagnostic)>| {
                let mut hints = Vec::new();
                match prov {
                    Some(Provenance::Derived { from, reasoning }) => {
                        hints.push(format!(
                            "derived from {}: {}",
                            from.join(", "),
                            truncate(reasoning, 120)
                        ));
                    }
                    Some(Provenance::Decree { author, at, note }) => {
                        hints.push(format!(
                            "decree by {} at {}{}",
                            author,
                            at,
                            note.as_ref()
                                .map(|n| format!(": {}", n))
                                .unwrap_or_default()
                        ));
                    }
                    _ => {}
                }
                match proposal {
                    Some((did, d)) => {
                        if let Some(p) = &d.prompt {
                            hints.push(format!("proposal {}: {}", did, p.question));
                            for o in &p.options {
                                if let Some(e) = &o.edit {
                                    hints.push(format!(
                                        "edit {}#{}: {}",
                                        e.doc,
                                        e.section,
                                        truncate(&e.new_text, 120)
                                    ));
                                }
                            }
                        }
                        hints.push(format!("accept (Apply: the edit) or retract, on {}", did));
                    }
                    None => hints.push(format!("no proposal filed yet for {}", subject)),
                }
                hints
            };
        for rec in records_of(store, &[store::CHANGE_PROVENANCE_PENDING]) {
            let subject = &rec.subject;
            let Some(prov) = provenance_of(subject) else {
                continue;
            };
            if matches!(prov, Provenance::Quote(_)) {
                continue;
            }
            let proposal = proposal_for(subject);
            let change = json!({
                "provenance": prov.kind(),
                "from": match &prov { Provenance::Derived { from, .. } => json!(from), _ => Value::Null },
                "proposal": proposal.as_ref().map(|(id, _)| id.clone()),
            });
            let mut g = build_goal(
                self,
                subject,
                true,
                change,
                Some(rec.cause()),
                hints_for(subject, Some(&prov), proposal.as_ref()),
            );
            g.state = GoalState::Blocked {
                on: "human: ratification".into(),
            };
            out.insert(g.id.clone(), g);
        }
        // Attribute-level proposals carry no node-level record: the open diagnostic
        // alone derives the goal.
        for (did, d) in &store.graph.diagnostics {
            if d.lifecycle != "open" || d.rule != "ratification-pending" {
                continue;
            }
            let Some(subject) = d.subjects.first() else {
                continue;
            };
            let id = goal_id("ratify", subject);
            if out.contains_key(&id) {
                continue;
            }
            if !store.graph.entities.contains_key(subject)
                && !store.graph.requirements.contains_key(subject)
                && !store.graph.criteria.contains_key(subject)
            {
                continue;
            }
            let attribute = store.graph.entities.get(subject).and_then(|e| {
                e.attributes
                    .iter()
                    .find(|a| {
                        !matches!(a.provenance, Provenance::Quote(_)) && d.message.contains(&a.name)
                    })
                    .map(|a| a.name.clone())
            });
            let created = d
                .created
                .as_deref()
                .and_then(|b| b.strip_prefix('g'))
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(store.status.generation);
            let change =
                json!({"provenance": "attribute", "attribute": attribute, "proposal": did});
            let mut g = build_goal(
                self,
                subject,
                true,
                change,
                Some(Cause {
                    generation: created,
                    mutation: 0,
                    via: "provenance".into(),
                }),
                hints_for(subject, None, Some(&(did.clone(), d))),
            );
            g.state = GoalState::Blocked {
                on: "human: ratification".into(),
            };
            out.insert(g.id.clone(), g);
        }
        out.into_values().collect()
    }
    fn ready(&self, _goal: &Goal, _board: &Board) -> Ready {
        Ready::Blocked("awaiting ratification: accept the proposal or retract the fact".into())
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        batch
            .iter()
            .map(|g| format!("- {} full", g.target))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn prompt(&self) -> &'static str {
        ""
    }
}

pub struct Answer;

impl GoalKind for Answer {
    fn kind(&self) -> &'static str {
        "answer"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "diagnostic"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut out = Vec::new();
        for rec in records_of(store, &[store::CHANGE_PROMPT_UNANSWERED]) {
            let Some(d) = store.graph.diagnostics.get(&rec.subject) else {
                continue;
            };
            let Some(p) = d.prompt.as_ref() else { continue };
            if d.lifecycle != "open"
                || d.answer.is_some()
                || d.rule == "ratification-pending"
                || matches!(d.triage.as_deref(), Some("suppressed") | Some("wontfix"))
            {
                continue;
            }
            // An observation's prompt is advice, not a standing question: an
            // info-severity diagnostic (other than a decision, which exists to ask)
            // derives no answer goal. Mirrors docs/compiler/goals/answer.md#created-when.
            if d.severity == "info" && d.rule != "decision" {
                continue;
            }
            let change = json!({
                "rule": d.rule,
                "subjects": d.subjects,
                "options": p.options.len(),
                "freeform": p.freeform,
            });
            let mut hints = vec![format!("question: {}", p.question)];
            for (i, o) in p.options.iter().enumerate() {
                hints.push(match &o.edit {
                    Some(e) => format!(
                        "option {}: {} (edit {}#{})",
                        i + 1,
                        o.label,
                        e.doc,
                        e.section
                    ),
                    None => format!("option {}: {}", i + 1, o.label),
                });
            }
            if p.freeform {
                hints.push("a freeform reply is accepted".into());
            }
            for s in &d.subjects {
                hints.push(format!("subject {}", node_label(store, s)));
            }
            if let Some(r) = &d.reasoning {
                hints.push(format!("reasoning: {}", truncate(r, 160)));
            }
            hints.push(format!(
                "filed by {} in g{}; answer through the LSP code action, the GUI questions panel, or chat",
                rec.via, rec.generation
            ));
            let mut g = build_goal(self, &rec.subject, true, change, Some(rec.cause()), hints);
            g.state = GoalState::Blocked {
                on: "human: answer".into(),
            };
            out.push(g);
        }
        out
    }
    fn ready(&self, _goal: &Goal, _board: &Board) -> Ready {
        Ready::Blocked("awaiting a human answer".into())
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        batch
            .iter()
            .map(|g| format!("- {} full", g.target))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn prompt(&self) -> &'static str {
        ""
    }
}

// ---- GC: declare-edges ----

pub struct DeclareEdges;

fn gc_ready(goal: &Goal, board: &Board) -> Ready {
    match board.cone_blockers(&goal.id) {
        v if v.is_empty() => Ready::Ready,
        v => Ready::Blocked(format!(
            "compile goal(s) open in the cone: {}",
            v.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
        )),
    }
}

impl GoalKind for DeclareEdges {
    fn kind(&self) -> &'static str {
        "declare-edges"
    }
    fn class(&self) -> Class {
        Class::Gc
    }
    fn unit(&self) -> &'static str {
        "requirement"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut out = Vec::new();
        for rec in records_of(store, &[store::CHANGE_EDGES_MISSING]) {
            let Some(r) = store.graph.requirements.get(&rec.subject) else {
                continue;
            };
            if r.entities.len() < 2 || !r.edges.is_empty() {
                continue;
            }
            let ents: Vec<String> = r
                .entities
                .iter()
                .map(|e| store.resolve_id(e).to_string())
                .collect();
            let change = json!({"entities": ents, "edges": 0});
            let mut hints = vec![
                format!("load {}", rec.subject),
                format!("{} entities, no edges (g{})", ents.len(), rec.generation),
            ];
            for e in &ents {
                hints.push(format!("load {}", e));
            }
            for rel in store.graph.relationships.values() {
                if rel.members.len() == 2 && rel.members.iter().all(|m| ents.contains(m)) {
                    hints.push(format!(
                        "related {}~{}: {} ({})",
                        rel.members[0],
                        rel.members[1],
                        rel.strongest(),
                        rel.requirements().len()
                    ));
                }
            }
            hints.push("skill extraction; update_requirement passing only id and edges".into());
            out.push(build_goal(
                self,
                &rec.subject,
                false,
                change,
                Some(rec.cause()),
                hints,
            ));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        gc_ready(goal, board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        let mut seen = BTreeSet::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            for e in str_list(&g.change["entities"]) {
                if seen.insert(e.clone()) {
                    lines.push(format!("- {} stub", e));
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &["update_requirement"]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = Vec::new();
        for op in staged {
            if let Op::UpdateRequirement {
                id,
                statement,
                entities,
                edges,
                transition,
                facets,
                source,
                ..
            } = op
            {
                for (field, present) in [
                    ("statement", statement.is_some()),
                    ("entities", entities.is_some()),
                    ("transition", transition.is_some()),
                    ("facets", facets.is_some()),
                    ("section and quote", source.is_some()),
                ] {
                    if present {
                        out.push(Violation::new(
                            "field-not-this-goals",
                            format!(
                                "{}: {} is not this goal's to change; pass only id and edges",
                                id, field
                            ),
                        ));
                    }
                }
                if let Some(edges) = edges {
                    let listed: Vec<String> = store
                        .graph
                        .requirements
                        .get(id)
                        .map(|r| {
                            r.entities
                                .iter()
                                .map(|e| store.resolve_id(e).to_string())
                                .collect()
                        })
                        .unwrap_or_default();
                    for e in edges {
                        if e.rel_type.is_none() {
                            out.push(Violation::new(
                                "untyped-edge",
                                format!(
                                    "{}: edge {}~{} has no type; judging the type is the goal",
                                    id, e.a, e.b
                                ),
                            ));
                        }
                        if e.a == e.b {
                            out.push(Violation::new(
                                "bad-edge",
                                format!("{}: edge {}~{} ties an entity to itself", id, e.a, e.b),
                            ));
                        }
                        for end in [&e.a, &e.b] {
                            if !listed.contains(&store.resolve_id(end).to_string()) {
                                out.push(Violation::new(
                                    "bad-edge",
                                    format!(
                                        "{}: edge end {} is not among the requirement's entities",
                                        id, end
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/declare-edges.md")
    }
}

// ---- GC: dedupe-candidates ----

pub struct DedupeCandidates;

pub const LOOKALIKE_TOKEN_WEIGHT: f64 = 0.75;
pub const LOOKALIKE_DOCUMENT_WEIGHT: f64 = 0.25;
pub const LOOKALIKE_THRESHOLD: f64 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub struct Lookalike {
    pub a: String,
    pub b: String,
    pub score: f64,
    pub tokens: f64,
    pub documents: f64,
    pub shared: Vec<String>,
    pub docs_a: BTreeSet<String>,
    pub docs_b: BTreeSet<String>,
}

// The pairs the structural contributions tie: a composition, aggregation,
// generalization, or instantiation between two entities, both ways round.
fn tied_pairs(store: &Store) -> BTreeSet<(String, String)> {
    let mut tied: BTreeSet<(String, String)> = BTreeSet::new();
    for rel in store.graph.relationships.values() {
        for c in &rel.contributions {
            if matches!(
                c.r#type.as_str(),
                "composition" | "aggregation" | "generalization" | "instantiation"
            ) {
                tied.insert((c.a.clone(), c.b.clone()));
                tied.insert((c.b.clone(), c.a.clone()));
            }
        }
    }
    tied
}

// One candidate pair's score, None when the pair fails a candidate rule or the
// threshold. The pair rules and the formula of
// docs/compiler/goals/dedupe-candidates.md#the-lookalike-score.
#[allow(clippy::too_many_arguments)]
fn lookalike_of(
    store: &Store,
    tied: &BTreeSet<(String, String)>,
    a: &str,
    ta: &BTreeSet<String>,
    docs_a: &BTreeSet<String>,
    b: &str,
    tb: &BTreeSet<String>,
    docs_b: &BTreeSet<String>,
) -> Option<Lookalike> {
    let (ea, eb) = (&store.graph.entities[a], &store.graph.entities[b]);
    if ea.scope != eb.scope
        || store.is_ancestor(a, b)
        || store.is_ancestor(b, a)
        || tied.contains(&(a.to_string(), b.to_string()))
    {
        return None;
    }
    if docs_a.union(docs_b).count() < 2 {
        return None;
    }
    let (score, tokens, documents, shared) = score_pair(ta, docs_a, tb, docs_b)?;
    if score + 1e-9 < LOOKALIKE_THRESHOLD {
        return None;
    }
    Some(Lookalike {
        a: a.to_string(),
        b: b.to_string(),
        score,
        tokens,
        documents,
        shared,
        docs_a: docs_a.clone(),
        docs_b: docs_b.clone(),
    })
}

// The formula over two token sets and two mention-document sets: (score, tokens,
// documents, shared tokens), each figure rounded to two decimals; None when either name
// has no token or the names share none. The threshold is the caller's.
fn score_pair(
    ta: &BTreeSet<String>,
    docs_a: &BTreeSet<String>,
    tb: &BTreeSet<String>,
    docs_b: &BTreeSet<String>,
) -> Option<(f64, f64, f64, Vec<String>)> {
    if ta.is_empty() || tb.is_empty() {
        return None;
    }
    let shared: Vec<String> = ta.intersection(tb).cloned().collect();
    if shared.is_empty() {
        return None;
    }
    let tokens = 2.0 * shared.len() as f64 / (ta.len() + tb.len()) as f64;
    let union = docs_a.union(docs_b).count().max(1);
    let documents = docs_a.intersection(docs_b).count() as f64 / union as f64;
    let score = LOOKALIKE_TOKEN_WEIGHT * tokens + LOOKALIKE_DOCUMENT_WEIGHT * documents;
    let round = |x: f64| (x * 100.0).round() / 100.0;
    Some((round(score), round(tokens), round(documents), shared))
}

// The lookalike score over every candidate pair: two entities of one scope, neither
// an ancestor of the other, no structural contribution between them, spanning at
// least two mention documents. The name tokens and mention documents of every entity
// are computed once, and an inverted token index names the pairs that share a token:
// a pair sharing none scores nothing, so only those pairs are scored, in id order.
// Mirrors docs/compiler/goals/dedupe-candidates.md and
// docs/compiler/graph.md#derived-data.
pub fn lookalike_scores(store: &Store) -> Vec<Lookalike> {
    let tied = tied_pairs(store);
    let ids: Vec<&String> = store.graph.entities.keys().collect();
    let tokens: Vec<BTreeSet<String>> = ids
        .iter()
        .map(|id| name_tokens(&store.graph.entities[*id]))
        .collect();
    let docs: Vec<BTreeSet<String>> = ids
        .iter()
        .map(|id| {
            store.graph.entities[*id]
                .mentions
                .iter()
                .map(|m| m.doc.clone())
                .collect()
        })
        .collect();
    let mut by_token: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, ts) in tokens.iter().enumerate() {
        for t in ts {
            by_token.entry(t.as_str()).or_default().push(i);
        }
    }
    let mut pairs: BTreeSet<(usize, usize)> = BTreeSet::new();
    for holders in by_token.values() {
        for (k, i) in holders.iter().enumerate() {
            for j in &holders[k + 1..] {
                pairs.insert((*i, *j));
            }
        }
    }
    pairs
        .into_iter()
        .filter_map(|(i, j)| {
            lookalike_of(
                store, &tied, ids[i], &tokens[i], &docs[i], ids[j], &tokens[j], &docs[j],
            )
        })
        .collect()
}

// The lookalike scores by the plain pairwise scan over every entity pair: the
// reference the token index is checked against.
#[cfg(test)]
pub(crate) fn lookalike_scores_scan(store: &Store) -> Vec<Lookalike> {
    let tied = tied_pairs(store);
    let ids: Vec<&String> = store.graph.entities.keys().collect();
    let mut out = Vec::new();
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            let (a, b) = (ids[i], ids[j]);
            let (ea, eb) = (&store.graph.entities[a], &store.graph.entities[b]);
            let docs_a: BTreeSet<String> = ea.mentions.iter().map(|m| m.doc.clone()).collect();
            let docs_b: BTreeSet<String> = eb.mentions.iter().map(|m| m.doc.clone()).collect();
            let (ta, tb) = (name_tokens(ea), name_tokens(eb));
            out.extend(lookalike_of(store, &tied, a, &ta, &docs_a, b, &tb, &docs_b));
        }
    }
    out
}

// The formula's value for one pair without the candidate rules or the threshold: what a
// remembered verdict records and is compared against. 0 when the names share no token.
// Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
pub fn lookalike_pair_score(store: &Store, a: &str, b: &str) -> f64 {
    let (Some(ea), Some(eb)) = (store.graph.entities.get(a), store.graph.entities.get(b)) else {
        return 0.0;
    };
    let (ta, tb) = (name_tokens(ea), name_tokens(eb));
    let docs_a: BTreeSet<String> = ea.mentions.iter().map(|m| m.doc.clone()).collect();
    let docs_b: BTreeSet<String> = eb.mentions.iter().map(|m| m.doc.clone()).collect();
    score_pair(&ta, &docs_a, &tb, &docs_b)
        .map(|(score, _, _, _)| score)
        .unwrap_or(0.0)
}

// The scored pairs the board asks about: every candidate pair over the threshold whose
// verdict is not remembered (no `lookalikes` entry at the pair's score) and that no
// creation note told apart. The name index writes records for exactly these.
// Mirrors docs/compiler/goals/dedupe-candidates.md#created-when.
pub fn pending_lookalikes(store: &Store) -> Vec<Lookalike> {
    let told_apart = |x: &str, y: &str| {
        store
            .graph
            .entities
            .get(x)
            .is_some_and(|e| e.distinct_from.contains_key(y))
    };
    lookalike_scores(store)
        .into_iter()
        .filter(|l| {
            let remembered = store
                .status
                .lookalike(&pair_target(&l.a, &l.b))
                .is_some_and(|v| (v.score - l.score).abs() < 1e-6);
            !remembered && !told_apart(&l.a, &l.b) && !told_apart(&l.b, &l.a)
        })
        .collect()
}

// Every journal entry, by generation.
pub fn read_journal(out: &std::path::Path) -> Vec<JournalEntry> {
    let dir = out.join("journal");
    let mut entries: Vec<(u64, JournalEntry)> = Vec::new();
    let Ok(rd) = crate::vfs::read_dir(&dir) else {
        return Vec::new();
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(n) = name
            .strip_prefix('g')
            .and_then(|s| s.strip_suffix(".yaml"))
            .and_then(|s| s.parse::<u64>().ok())
        else {
            continue;
        };
        if let Some(entry) = read_journal_entry(&e.path(), n) {
            entries.push((n, entry));
        }
    }
    entries.sort_by_key(|(n, _)| *n);
    entries.into_iter().map(|(_, e)| e).collect()
}

// The journal entries of the generations in `(from, to]`, oldest first: the files
// named by those generations alone, so a build reading what one session wrote parses
// nothing older. Mirrors docs/compiler/graph.md#journal.
pub fn read_journal_range(out: &std::path::Path, from: u64, to: u64) -> Vec<JournalEntry> {
    let dir = out.join("journal");
    (from + 1..=to)
        .filter_map(|g| read_journal_entry(&dir.join(format!("g{}.yaml", g)), g))
        .collect()
}

// One journal entry by path; `generation` is the file's, stamped on an entry that
// carries none. A missing file is nothing; a file that does not parse is skipped with
// one warning per process naming it, so a torn entry is visible and never silent.
// Mirrors docs/compiler/graph.md#journal.
pub fn read_journal_entry(path: &std::path::Path, generation: u64) -> Option<JournalEntry> {
    let text = match crate::vfs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn_journal_once(path, &e.to_string());
            }
            return None;
        }
    };
    let mut entry = match serde_norway::from_str::<JournalEntry>(&text) {
        Ok(entry) => entry,
        Err(e) => {
            warn_journal_once(path, &e.to_string());
            return None;
        }
    };
    if entry.generation == 0 {
        entry.generation = generation;
    }
    Some(entry)
}

fn warn_journal_once(path: &std::path::Path, why: &str) {
    static WARNED: std::sync::Mutex<BTreeSet<std::path::PathBuf>> =
        std::sync::Mutex::new(BTreeSet::new());
    let fresh = WARNED
        .lock()
        .map(|mut set| set.insert(path.to_path_buf()))
        .unwrap_or(true);
    if fresh {
        eprintln!(
            "[jazyk] warning: journal entry {} skipped: {}",
            path.display(),
            why
        );
    }
}

impl GoalKind for DedupeCandidates {
    fn kind(&self) -> &'static str {
        "dedupe-candidates"
    }
    fn class(&self) -> Class {
        Class::Gc
    }
    fn unit(&self) -> &'static str {
        "entity pair"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut out = Vec::new();
        // A remembered verdict at the pair's score, or a note at creation that told
        // the two apart, is the standing verdict: the pair is not asked about again.
        // Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
        for l in pending_lookalikes(store) {
            let target = pair_target(&l.a, &l.b);
            let filed = store.graph.diagnostics.values().any(|d| {
                d.lifecycle == "open"
                    && d.rule == "duplicate-entity"
                    && d.subjects.iter().any(|s| s == &l.a)
                    && d.subjects.iter().any(|s| s == &l.b)
            });
            if filed {
                continue;
            }
            let record = store
                .status
                .changes
                .iter()
                .find(|c| c.kind == CHANGE_LOOKALIKE && c.subject == target);
            let cause = record.map(|c| c.cause()).unwrap_or(Cause {
                generation: store.status.generation,
                mutation: 0,
                via: "lookalike".into(),
            });
            let only_a: Vec<&String> = l.docs_a.difference(&l.docs_b).collect();
            let only_b: Vec<&String> = l.docs_b.difference(&l.docs_a).collect();
            let change = json!({
                "score": l.score, "tokens": l.tokens, "documents": l.documents,
                "shared": l.shared,
                "mentions": {&l.a: l.docs_a, &l.b: l.docs_b},
            });
            let mut hints = vec![
                format!("load {}; load {}", l.a, l.b),
                format!(
                    "score {} (tokens {}, documents {}; shared: {})",
                    l.score,
                    l.tokens,
                    l.documents,
                    l.shared.join(", ")
                ),
            ];
            if !only_a.is_empty() {
                hints.push(format!(
                    "{} mentions only {}",
                    only_a
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    l.a
                ));
            }
            if !only_b.is_empty() {
                hints.push(format!(
                    "{} mentions only {}",
                    only_b
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    l.b
                ));
            }
            let both: Vec<&String> = store
                .graph
                .requirements
                .iter()
                .filter(|(_, r)| {
                    r.entities.iter().any(|e| store.resolve_id(e) == l.a)
                        && r.entities.iter().any(|e| store.resolve_id(e) == l.b)
                })
                .map(|(id, _)| id)
                .collect();
            for r in both.iter().take(4) {
                hints.push(format!("load {} (names both)", r));
            }
            let (pa, pb) = (
                store.graph.entities[&l.a].parent.clone(),
                store.graph.entities[&l.b].parent.clone(),
            );
            if pa != pb {
                for p in [pa, pb].into_iter().flatten() {
                    hints.push(format!(
                        "load {} (a parent; two parents mean two entities)",
                        p
                    ));
                }
            }
            hints.push(
                "skill judgment; merge_entities or report_diagnostic duplicate-entity".into(),
            );
            out.push(build_goal(self, &target, false, change, Some(cause), hints));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        gc_ready(goal, board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        let mut seen = BTreeSet::new();
        for g in batch {
            if let Some((a, b)) = pair_members(&g.target) {
                for id in [a, b] {
                    if seen.insert(id.to_string()) {
                        lines.push(format!("- {} full", id));
                    }
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &["merge_entities", "update_entity", "report_diagnostic"]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = Vec::new();
        for op in staged {
            if let Op::MergeEntities {
                keep,
                absorb,
                reason,
            } = op
            {
                if reason.trim().is_empty() {
                    out.push(Violation::new(
                        "reason-required",
                        format!("merge_entities {} into {} carries no reason", absorb, keep),
                    ));
                }
                if store.is_ancestor(absorb, keep) {
                    out.push(Violation::new(
                        "parent-cycle",
                        format!(
                            "merging {} into {} would make the survivor its own ancestor",
                            absorb, keep
                        ),
                    ));
                }
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/dedupe-candidates.md")
    }
}

// ---- GC: curate-view ----

pub struct CurateView;

impl GoalKind for CurateView {
    fn kind(&self) -> &'static str {
        "curate-view"
    }
    fn class(&self) -> Class {
        Class::Gc
    }
    fn unit(&self) -> &'static str {
        "view"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut by_view: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_QUERY_MATCH, CHANGE_FLOW_UNPLACED]) {
            if store.graph.views.contains_key(&rec.subject) {
                by_view.entry(rec.subject.clone()).or_default().push(rec);
            }
        }
        let mut out = Vec::new();
        for (vid, recs) in by_view {
            let v = &store.graph.views[&vid];
            let mut matched: Vec<Value> = Vec::new();
            let mut hints = vec![format!("load {}", vid)];
            for r in &recs {
                if r.kind == store::CHANGE_QUERY_MATCH {
                    for id in str_list(&r.detail["added"]) {
                        matched.push(json!({"id": id, "why": "query-match"}));
                        hints.push(format!("load {} (query match)", id));
                    }
                } else {
                    let rid = r.detail["requirement"].as_str().unwrap_or("").to_string();
                    let facet = r.detail["facet"].as_str().unwrap_or("").to_string();
                    let rec_shared = str_list(&r.detail["shared"]);
                    let why = if facet == "failure-mode" {
                        "unrepresented failure mode"
                    } else {
                        "unplaced behavior"
                    };
                    matched.push(json!({"id": rid, "why": why, "facet": facet, "diagnostic": r.detail["diagnostic"]}));
                    hints.push(format!("load {} ({})", rid, why));
                    for alt in str_list(&r.detail["alternatives"]).iter().take(3) {
                        let shared = store
                            .graph
                            .views
                            .get(alt)
                            .map(|av| {
                                let ents: BTreeSet<String> = av
                                    .members
                                    .iter()
                                    .filter_map(|m| store.graph.requirements.get(m))
                                    .flat_map(|q| q.entities.iter().cloned())
                                    .collect();
                                rec_shared.iter().filter(|e| ents.contains(*e)).count()
                            })
                            .unwrap_or(0);
                        hints.push(format!("alternative {} (shares {} entities)", alt, shared));
                    }
                    if facet == "failure-mode" {
                        if let Some(req) = store.graph.requirements.get(&rid) {
                            if let Some(t) = req.transition.as_ref().and_then(|t| t.trigger.clone())
                            {
                                let after = v
                                    .members
                                    .iter()
                                    .find(|m| {
                                        store.graph.requirements.get(*m).is_some_and(|q| {
                                            q.transition.as_ref().and_then(|x| x.trigger.as_ref())
                                                == Some(&t)
                                        })
                                    })
                                    .cloned();
                                if let Some(a) = after {
                                    hints.push(format!("after {} ({})", a, t));
                                }
                            }
                        }
                    }
                }
            }
            let cause = earliest_cause(&recs);
            let change = json!({"matched": matched});
            for s in skills_for("curate-view", store, &vid) {
                hints.push(format!("skill {}", s));
            }
            hints.push("update_view add_members or exclude".into());
            out.push(build_goal(self, &vid, false, change, cause, hints));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        gc_ready(goal, board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            if let Some(m) = g.change["matched"].as_array() {
                for x in m {
                    if let Some(id) = x["id"].as_str() {
                        lines.push(format!("- {} full", id));
                    }
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &["upsert_view", "update_view", "delete_view"]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        view_gates(store, staged)
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/curate-view.md")
    }
}

fn view_gates(store: &Store, staged: &[Op]) -> Vec<Violation> {
    let mut out = Vec::new();
    const PLACEHOLDERS: [&str; 5] = ["none", "n/a", "na", "-", "null"];
    for op in staged {
        match op {
            Op::DeleteView { id, reason } => {
                if store.graph.views.get(id).is_some_and(|v| v.default) {
                    out.push(Violation::new(
                        "default-view",
                        format!("{} is a default view; exclude members or curate it instead of deleting", id),
                    ));
                }
                if reason.trim().is_empty() {
                    out.push(Violation::new(
                        "reason-required",
                        format!("delete_view {} carries no reason", id),
                    ));
                }
            }
            Op::UpdateView { id, exclude, .. } => {
                for x in exclude {
                    let n = x.note.trim().to_lowercase();
                    if n.is_empty() || PLACEHOLDERS.contains(&n.as_str()) {
                        out.push(Violation::new(
                            "note-required",
                            format!(
                                "{}: excluding {} needs a note naming the sentence or the rule",
                                id, x.id
                            ),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

// ---- GC: split-view and abstract-entity ----

pub struct SplitView;

struct Crossed {
    limit: String,
    count: u64,
    soft: u64,
    hard: u64,
}

fn crossings_for(recs: &[&ChangeRecord]) -> Vec<Crossed> {
    recs.iter()
        .map(|r| Crossed {
            limit: r.detail["limit"].as_str().unwrap_or("").to_string(),
            count: r.detail["count"].as_u64().unwrap_or(0),
            soft: r.detail["soft"].as_u64().unwrap_or(0),
            hard: r.detail["hard"].as_u64().unwrap_or(0),
        })
        .collect()
}

fn threshold_change(crossed: &[Crossed]) -> Value {
    json!({
        "limits": crossed.iter().map(|c| json!({
            "limit": c.limit, "count": c.count, "soft": c.soft, "hard": c.hard,
            "level": if c.count > c.hard { "hard" } else { "soft" },
        })).collect::<Vec<_>>()
    })
}

// Parent id to its direct children, by id: one scan for every descendant walk of a
// derivation. Mirrors docs/compiler/graph.md#derived-data.
fn children_index(store: &Store) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (cid, c) in &store.graph.entities {
        if let Some(p) = c.parent.as_ref() {
            out.entry(p.clone()).or_default().push(cid.clone());
        }
    }
    out
}

fn descendants(children: &BTreeMap<String, Vec<String>>, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut frontier = vec![id.to_string()];
    while let Some(p) = frontier.pop() {
        for cid in children.get(&p).into_iter().flatten() {
            if !out.contains(cid) {
                out.push(cid.clone());
                frontier.push(cid.clone());
            }
        }
    }
    out
}

impl GoalKind for SplitView {
    fn kind(&self) -> &'static str {
        "split-view"
    }
    fn class(&self) -> Class {
        Class::Gc
    }
    fn unit(&self) -> &'static str {
        "view"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut by_view: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_THRESHOLD_CROSSED]) {
            if rec.detail["goal"] == "split-view" && store.graph.views.contains_key(&rec.subject) {
                by_view.entry(rec.subject.clone()).or_default().push(rec);
            }
        }
        let mut out = Vec::new();
        let children = children_index(store);
        for (vid, recs) in by_view {
            let v = &store.graph.views[&vid];
            let crossed = crossings_for(&recs);
            let mandatory = crossed.iter().any(|c| c.count > c.hard);
            let cause = earliest_cause(&recs);
            let mut hints = vec![format!("load {}", vid)];
            for c in &crossed {
                hints.push(format!(
                    "{} > {} ({}, soft {}, hard {})",
                    c.count, c.soft, c.limit, c.soft, c.hard
                ));
            }
            if is_flow_kind(&v.kind) {
                let mut last_section: Option<String> = None;
                for m in &v.members {
                    let Some(r) = store.graph.requirements.get(m) else {
                        continue;
                    };
                    let sec = r
                        .source
                        .as_ref()
                        .map(|s| format!("{}#{}", s.doc, s.section));
                    if last_section.is_some() && sec != last_section {
                        hints.push(format!("break before {} (section boundary)", m));
                    }
                    last_section = sec;
                }
                if v.kind == "sequence" || v.kind == "communication" {
                    let parts = crate::derive::flow_participants(store, &v.members);
                    hints.push(format!(
                        "participants: {} ({})",
                        parts.len(),
                        parts.iter().cloned().collect::<Vec<_>>().join(", ")
                    ));
                }
            } else {
                let mut subtree: Vec<(usize, String)> = v
                    .members
                    .iter()
                    .filter(|m| !v.collapse.contains(m))
                    .map(|m| (descendants(&children, m).len(), m.clone()))
                    .filter(|(n, _)| *n > 0)
                    .collect();
                subtree.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                for (n, m) in subtree.iter().take(3) {
                    hints.push(format!("collapse {} ({} descendants hidden)", m, n));
                }
            }
            for (oid, o) in &store.graph.views {
                let linked = o
                    .query
                    .as_ref()
                    .and_then(|q| q.parent.as_ref())
                    .is_some_and(|p| v.collapse.contains(p))
                    || v.excluded.iter().any(|x| x.note.contains(oid.as_str()));
                if linked {
                    hints.push(format!("linked {}", oid));
                }
            }
            for s in skills_for("split-view", store, &vid) {
                hints.push(format!("skill {}", s));
            }
            hints.push("update_view collapse or exclude; upsert_view for sub-views".into());
            out.push(build_goal(
                self,
                &vid,
                mandatory,
                threshold_change(&crossed),
                cause,
                hints,
            ));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        // A level's picture yields to the level's fan-out: grouping is the structural
        // answer, splitting the picture the last resort. Mirrors
        // docs/compiler/goals/split-view.md and docs/compiler/reconciler.md#gc-gating.
        if let Some(level) = board.view_level(&goal.target) {
            let fan_out = format!("g:abstract-entity:{}", level);
            if board.open(&fan_out) {
                return Ready::Blocked(format!(
                    "fan-out first: {} groups this level, and the flow lifts to the groupings",
                    fan_out
                ));
            }
        }
        gc_ready(goal, board)
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            if let Some(v) = store.graph.views.get(&g.target) {
                for m in v.members.iter().take(40) {
                    lines.push(format!("- {} stub", m));
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &["upsert_view", "update_view", "delete_view"]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = view_gates(store, staged);
        // No member lost: a member removed from a view is collapsed, excluded with a
        // note, or a member of a view staged in the same changeset.
        let staged_members: BTreeSet<String> = staged
            .iter()
            .flat_map(|o| match o {
                Op::CreateView { view, .. } => view.members.clone(),
                Op::UpdateView {
                    members: Some(m), ..
                } => m.clone(),
                Op::UpdateView { add_members, .. } => add_members.clone(),
                _ => Vec::new(),
            })
            .collect();
        for op in staged {
            if let Op::UpdateView {
                id,
                members,
                remove_members,
                collapse,
                exclude,
                ..
            } = op
            {
                let Some(v) = store.graph.views.get(id) else {
                    continue;
                };
                let after: Vec<String> = match members {
                    Some(m) => m.clone(),
                    None => v
                        .members
                        .iter()
                        .filter(|m| !remove_members.contains(m))
                        .cloned()
                        .collect(),
                };
                let collapsed: Vec<String> = collapse.clone().unwrap_or_else(|| v.collapse.clone());
                for m in &v.members {
                    let kept = after.contains(m)
                        || staged_members.contains(m)
                        || exclude.iter().any(|x| &x.id == m)
                        || v.excluded.iter().any(|x| &x.id == m)
                        || collapsed.iter().any(|c| store.is_ancestor(c, m));
                    if !kept {
                        out.push(Violation::new(
                            "member-lost",
                            format!("{}: member {} is dropped without a sub-view, a collapse, or an exclusion note", id, m),
                        ));
                    }
                }
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/split-view.md")
    }
}

pub struct AbstractEntity;

// Candidate cohesion groups: content tokens recurring across at least three of the
// entity's statements, function words dropped, each shown as the word the statements
// use most (never a stem), with the section quoting most of them.
// Mirrors docs/compiler/goals/abstract-entity.md#hints.
pub fn cohesion_groups(store: &Store, id: &str) -> Vec<(String, usize, String)> {
    // Function words the pair scorer's stop list keeps (they carry weight between two
    // statements) that name no concept a group could be about.
    const FUNCTION: [&str; 72] = [
        "every", "from", "must", "may", "can", "will", "does", "did", "has", "have", "had",
        "into", "than", "more", "most", "one", "any", "some", "their", "they", "them",
        "after", "before", "about", "over", "under", "other", "same", "also", "via",
        "whenever", "while", "once", "per", "what", "who", "where", "how", "never", "always",
        "between", "within", "without", "through", "until", "since", "both", "either",
        "first", "last", "new", "such", "these", "those", "there", "here", "done", "make",
        "made", "use", "used", "like", "unless", "because", "would", "should", "could",
        "being", "been", "was", "were", "whose",
    ];
    let reqs = store.requirements_referencing(id);
    let mut by_token: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    let mut words: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for rid in &reqs {
        let r = &store.graph.requirements[rid];
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for (stemmed, word) in content_token_words(store, &r.statement, &r.entities) {
            if stemmed.len() < 4
                || FUNCTION.contains(&stemmed.as_str())
                || FUNCTION.contains(&word.as_str())
            {
                continue;
            }
            *words
                .entry(stemmed.clone())
                .or_default()
                .entry(word)
                .or_insert(0) += 1;
            if seen.insert(stemmed.clone()) {
                by_token.entry(stemmed).or_default().push(rid);
            }
        }
    }
    let mut groups: Vec<(String, usize, String)> = by_token
        .into_iter()
        .filter(|(_, rids)| rids.len() >= 3)
        .map(|(tok, rids)| {
            let mut sections: BTreeMap<String, usize> = BTreeMap::new();
            for rid in &rids {
                if let Some(s) = store.graph.requirements[*rid].source.as_ref() {
                    *sections.entry(s.section.clone()).or_insert(0) += 1;
                }
            }
            let section = sections
                .into_iter()
                .max_by_key(|(s, n)| (*n, std::cmp::Reverse(s.clone())))
                .map(|(s, _)| s)
                .unwrap_or_default();
            let word = words
                .get(&tok)
                .and_then(|w| {
                    w.iter()
                        .max_by_key(|(word, n)| (**n, std::cmp::Reverse((*word).clone())))
                        .map(|(word, _)| word.clone())
                })
                .unwrap_or(tok);
            (word, rids.len(), section)
        })
        .collect();
    groups.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    groups.truncate(6);
    groups
}

// The section that defines an entity, the ratification proposal's target: the root
// section of a document headed with the entity's name, else the section among its
// mentions quoting the most of its requirements, else its first mention (a front
// door's list item names the entity without defining it). Mirrors
// docs/compiler/goals/abstract-entity.md#hints.
pub fn defining_section(store: &Store, id: &str) -> Option<(String, String)> {
    let e = store.graph.entities.get(id)?;
    let mut docs: Vec<&String> = e.mentions.iter().map(|m| &m.doc).collect();
    docs.dedup();
    for doc in docs {
        let root = store.docs.get(doc).and_then(|d| {
            d.sections
                .iter()
                .find(|(_, s)| s.kind == "root")
                .map(|(path, s)| (path.clone(), s.title.clone()))
        });
        if let Some((path, title)) = root {
            if title.trim().eq_ignore_ascii_case(e.name.trim()) {
                return Some((doc.clone(), path));
            }
        }
    }
    let mut quoting: BTreeMap<(String, String), usize> = BTreeMap::new();
    for rid in store.requirements_referencing(id) {
        if let Some(s) = &store.graph.requirements[&rid].source {
            *quoting
                .entry((s.doc.clone(), s.section.clone()))
                .or_insert(0) += 1;
        }
    }
    // Among the mention sections, the one quoting most; ties keep mention order.
    e.mentions
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let key = (m.doc.clone(), m.section.clone());
            let n = quoting.get(&key).copied().unwrap_or(0);
            (n, std::cmp::Reverse(i), key)
        })
        .max()
        .map(|(_, _, key)| key)
}

// The caps gate for one goal at mark_goal_done: the requirement count on the target
// after the changeset is within the soft value in force (the record folds the
// entity's own bump in). A split that only de-escalates is refused here, so the
// session moves more detail or fails the goal with the reason. None when no
// requirement record stands on the target. Mirrors
// docs/compiler/goals/abstract-entity.md#gate.
pub fn caps_goal_gate(store: &Store, staged: &[Op], target: &str) -> Option<Violation> {
    let rec = store.status.changes.iter().find(|c| {
        c.kind == store::CHANGE_THRESHOLD_CROSSED
            && c.subject == target
            && c.detail["limit"] == "requirements-per-entity"
    })?;
    let soft = rec.detail["soft"].as_u64()?;
    let hard = rec.detail["hard"].as_u64().unwrap_or(soft);
    // The count is the statements the target leads (first-listed entity), as the
    // threshold counts them (docs/compiler/goals/abstract-entity.md#created-when).
    let names_target =
        |entities: &[String]| entities.first().is_some_and(|x| store.resolve_id(x) == target);
    let mut referencing: BTreeSet<String> = store
        .requirements_referencing(target)
        .into_iter()
        .filter(|id| {
            store
                .graph
                .requirements
                .get(id)
                .is_some_and(|r| names_target(&r.entities))
        })
        .collect();
    for op in staged {
        match op {
            Op::UpdateRequirement {
                id,
                entities: Some(e),
                ..
            } => {
                if names_target(e) {
                    referencing.insert(id.clone());
                } else {
                    referencing.remove(id);
                }
            }
            Op::CreateRequirement { id, requirement } => {
                if names_target(&requirement.entities) {
                    referencing.insert(id.clone());
                }
            }
            Op::DeleteRequirement { id, .. } => {
                referencing.remove(id);
            }
            _ => {}
        }
    }
    let count = referencing.len() as u64;
    if count <= soft {
        return None;
    }
    Some(Violation::new(
        "count-over-limit",
        format!(
            "{}: {} requirements after the changeset, over {} (requirements-per-entity, soft {}, hard {}); two honest exits, and the staged split commits at done either way: re-point more grouped statements to the sub-entities with update_requirement (id and entities only) when a documented noun holds them, or mark_goal_failed now with the reason that the remaining statements are about the whole, recommending a bump of the entity's own limit to the count",
            target, count, soft, soft, hard
        ),
    ))
}

// ---- the fan-out variant ----
// A level is a node's direct children; the top level of a scope is its parentless
// entities, addressed as `scope:<scope>`. Mirrors docs/compiler/concepts/levels.md.

// Ids per candidate before the rest folds into a count.
pub const FAN_OUT_CANDIDATE_IDS: usize = 12;

// The scope behind a `scope:<scope>` target. Mirrors
// docs/compiler/concepts/levels.md#the-scope-root.
pub fn scope_target(target: &str) -> Option<&str> {
    crate::board::scope_target(target)
}

// The level a node sits in: its parent, or its scope's root form when parentless.
pub fn level_of(store: &Store, id: &str) -> Option<String> {
    let e = store.graph.entities.get(store.resolve_id(id))?;
    Some(level_key(e.parent.as_deref(), &e.scope))
}

fn level_key(parent: Option<&str>, scope: &str) -> String {
    match parent.filter(|p| !p.is_empty()) {
        Some(p) => p.to_string(),
        None => store::scope_root_target(scope),
    }
}

// The members of a level, by id: the direct children of a node, or the parentless
// entities of a scope for the root form.
pub fn level_members(store: &Store, target: &str) -> Vec<String> {
    crate::board::level_members(store, target)
}

// The standing fan-out crossing on a level: the count and the thresholds in force
// (the node's bump folded in by the record writer).
#[derive(Clone, Debug, PartialEq)]
pub struct FanOut {
    pub count: u64,
    pub soft: u64,
    pub hard: u64,
}

impl FanOut {
    // Over hard the goal is mandatory. Mirrors docs/compiler/reconciler.md#escalation.
    pub fn mandatory(&self) -> bool {
        self.count > self.hard
    }

    // The count the level must reach: hard for a mandatory goal, soft otherwise.
    // Mirrors docs/compiler/goals/abstract-entity.md#the-fan-out-gate.
    pub fn limit_in_force(&self) -> u64 {
        if self.mandatory() {
            self.hard
        } else {
            self.soft
        }
    }
}

pub fn fan_out_crossing(store: &Store, target: &str) -> Option<FanOut> {
    store
        .status
        .changes
        .iter()
        .find(|c| {
            c.kind == store::CHANGE_THRESHOLD_CROSSED
                && c.subject == target
                && c.detail["limit"] == limits::CHILDREN_PER_ENTITY
        })
        .map(|c| FanOut {
            count: c.detail["count"].as_u64().unwrap_or(0),
            soft: c.detail["soft"].as_u64().unwrap_or(0),
            hard: c.detail["hard"].as_u64().unwrap_or(0),
        })
}

// One coupling partition: an ordered id list (at most FAN_OUT_CANDIDATE_IDS, the rest
// counted) with its internal weight.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub ids: Vec<String>,
    pub rest: usize,
    pub weight: u64,
}

// Every descendant of the level's members lifted to its member, the members to
// themselves.
fn lift_map(store: &Store, members: &[String]) -> BTreeMap<String, String> {
    let children = children_index(store);
    let mut lift = BTreeMap::new();
    for m in members {
        lift.insert(m.clone(), m.clone());
        for d in descendants(&children, m) {
            lift.insert(d, m.clone());
        }
    }
    lift
}

// weight(a, b) over the level's members: the requirements referencing both plus the
// derived relationships between them, descendants lifted to the member they sit under.
// Keys are ordered pairs (a < b). Mirrors docs/compiler/reconciler.md#fan-out.
pub fn coupling_weights(store: &Store, members: &[String]) -> BTreeMap<(String, String), u64> {
    let lift = lift_map(store, members);
    let mut weights: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut bump = |a: &str, b: &str| {
        if a != b {
            let key = if a < b {
                (a.to_string(), b.to_string())
            } else {
                (b.to_string(), a.to_string())
            };
            *weights.entry(key).or_insert(0) += 1;
        }
    };
    for r in store.graph.requirements.values() {
        let lifted: BTreeSet<&str> = r
            .entities
            .iter()
            .filter_map(|e| lift.get(store.resolve_id(e)))
            .map(String::as_str)
            .collect();
        let lifted: Vec<&str> = lifted.into_iter().collect();
        for (i, a) in lifted.iter().enumerate() {
            for b in &lifted[i + 1..] {
                bump(a, b);
            }
        }
    }
    for rel in store.graph.relationships.values() {
        if let [a, b] = rel.members.as_slice() {
            if let (Some(la), Some(lb)) =
                (lift.get(store.resolve_id(a)), lift.get(store.resolve_id(b)))
            {
                bump(la, lb);
            }
        }
    }
    weights
}

// Greedy agglomeration over the level's members: singletons merge by the highest
// total weight between two clusters, ties to the pair whose ids sort first, until the
// cluster count is at or under `soft` and every cluster has two or more members or is
// a singleton nothing touches. A zero-weight merge is a coupling artifact, so the
// agglomeration stops there. A singleton is no grouping and is not a candidate (the
// member lines show it); the rest go largest first, capped at `soft` candidates.
// Deterministic over the same graph. Mirrors docs/compiler/reconciler.md#fan-out.
pub fn coupling_candidates(store: &Store, target: &str, soft: u64) -> Vec<Candidate> {
    let members = level_members(store, target);
    let weights = coupling_weights(store, &members);
    let weight = |a: &str, b: &str| -> u64 {
        let key = if a < b {
            (a.to_string(), b.to_string())
        } else {
            (b.to_string(), a.to_string())
        };
        weights.get(&key).copied().unwrap_or(0)
    };
    let between = |x: &[String], y: &[String]| -> u64 {
        x.iter()
            .map(|a| y.iter().map(|b| weight(a, b)).sum::<u64>())
            .sum()
    };
    let internal = |c: &[String]| -> u64 {
        c.iter()
            .enumerate()
            .map(|(i, a)| c[i + 1..].iter().map(|b| weight(a, b)).sum::<u64>())
            .sum()
    };
    let mut clusters: Vec<Vec<String>> = members.iter().map(|m| vec![m.clone()]).collect();
    loop {
        let touched = |i: usize| {
            clusters
                .iter()
                .enumerate()
                .any(|(j, o)| j != i && between(&clusters[i], o) > 0)
        };
        let settled = clusters.len() as u64 <= soft
            && clusters
                .iter()
                .enumerate()
                .all(|(i, c)| c.len() >= 2 || !touched(i));
        if settled {
            break;
        }
        // A candidate is a cohesive subset the model can name: a merge never grows
        // a cluster past half the soft threshold, so a fully connected level yields
        // several candidates, never one tautological whole.
        let cap = (soft / 2).max(2) as usize;
        let mut best: Option<(u64, usize, usize)> = None;
        for i in 0..clusters.len() {
            for j in i + 1..clusters.len() {
                if clusters[i].len() + clusters[j].len() > cap {
                    continue;
                }
                let w = between(&clusters[i], &clusters[j]);
                if best.is_none_or(|(bw, _, _)| w > bw) {
                    best = Some((w, i, j));
                }
            }
        }
        match best {
            Some((w, i, j)) if w > 0 => {
                let other = clusters.remove(j);
                clusters[i].extend(other);
                clusters[i].sort();
                clusters.sort_by(|a, b| a[0].cmp(&b[0]));
            }
            _ => break,
        }
    }
    let mut scored: Vec<(Vec<String>, u64)> = clusters
        .into_iter()
        .filter(|c| c.len() >= 2)
        .map(|c| {
            let w = internal(&c);
            (c, w)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.len()
            .cmp(&a.0.len())
            .then(b.1.cmp(&a.1))
            .then(a.0[0].cmp(&b.0[0]))
    });
    scored.truncate(soft as usize);
    scored
        .into_iter()
        .map(|(mut ids, weight)| {
            let rest = ids.len().saturating_sub(FAN_OUT_CANDIDATE_IDS);
            ids.truncate(FAN_OUT_CANDIDATE_IDS);
            Candidate { ids, rest, weight }
        })
        .collect()
}

// The document a member is mentioned in most, its requirements' sources when it has
// no mention; ties to the name that sorts first. A naming hint for the model.
// Mirrors docs/compiler/concepts/levels.md#naming.
pub fn dominant_document(store: &Store, id: &str) -> Option<String> {
    let mut by_doc: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(e) = store.graph.entities.get(id) {
        for m in &e.mentions {
            *by_doc.entry(m.doc.clone()).or_insert(0) += 1;
        }
    }
    if by_doc.is_empty() {
        for rid in store.requirements_referencing(id) {
            if let Some(s) = &store.graph.requirements[&rid].source {
                *by_doc.entry(s.doc.clone()).or_insert(0) += 1;
            }
        }
    }
    by_doc
        .into_iter()
        .max_by_key(|(d, n)| (*n, std::cmp::Reverse(d.clone())))
        .map(|(d, _)| d)
}

// The fan-out hints for a level: the change, the candidates, each member with its
// stereotype and document, the existing groupings under the node.
// Mirrors docs/compiler/goals/abstract-entity.md#fan-out-hints.
fn fan_out_hints(store: &Store, target: &str, f: &FanOut, cands: &[Candidate]) -> Vec<String> {
    let mut hints = vec![format!(
        "fan-out {} > {} ({}, soft {}, hard {})",
        f.count,
        f.soft,
        limits::CHILDREN_PER_ENTITY,
        f.soft,
        f.hard
    )];
    for c in cands {
        let more = if c.rest > 0 {
            format!(" +{} more", c.rest)
        } else {
            String::new()
        };
        hints.push(format!(
            "candidate [{}]{} (weight {})",
            c.ids.join(", "),
            more,
            c.weight
        ));
            if crate::board::scope_target(target).is_some() {
                // The default scope's top level, not a scope anyone set
                // (docs/compiler/goals/abstract-entity.md#the-fan-out-variant).
                hints.push(format!(
                    "{} is the top level of the scope: its parentless entities, not a scope you set; a grouping under a documented name gives them a parent",
                    target
                ));
            }
    }
    let members = level_members(store, target);
    // The node's own document: the section that lists a member there is the heading
    // it sits under, the partition the model groups by first.
    // Mirrors docs/compiler/concepts/levels.md#naming.
    let node_doc = if store.graph.entities.contains_key(target) {
        dominant_document(store, target)
    } else {
        None
    };
    for m in &members {
        let e = &store.graph.entities[m];
        let mut line = format!("member {}", m);
        if let Some(s) = &e.stereotype {
            line.push_str(&format!(" «{}»", s));
        }
        let listing = node_doc.as_ref().and_then(|doc| {
            e.mentions
                .iter()
                .find(|mn| &mn.doc == doc)
                .map(|mn| format!("{}#{}", mn.doc, mn.section))
        });
        let dominant = dominant_document(store, m);
        match (listing, dominant) {
            (Some(l), Some(d)) if !l.starts_with(&format!("{}#", d)) => {
                line.push_str(&format!(" ({}; {})", l, d));
            }
            (Some(l), _) => line.push_str(&format!(" ({})", l)),
            (None, Some(d)) => line.push_str(&format!(" ({})", d)),
            (None, None) => {}
        }
        hints.push(line);
    }
    for m in &members {
        let n = level_members(store, m).len();
        if n > 0 {
            hints.push(format!("grouping {} ({} children)", m, n));
        }
    }
    // The namesake: a member whose name is the heading of the document that names
    // it and other members of the level. That entity is the level's node for the
    // document's entities. Mirrors docs/compiler/goals/abstract-entity.md#fan-out-hints.
    for m in &members {
        let Some(doc) = dominant_document(store, m) else {
            continue;
        };
        let Some(title) = store.docs.get(&doc).and_then(|d| {
            d.sections
                .values()
                .find(|s| s.kind == "root")
                .map(|s| s.title.clone())
        }) else {
            continue;
        };
        let name = &store.graph.entities[m].name;
        if !title.trim().eq_ignore_ascii_case(name.trim()) {
            continue;
        }
        let others = members
            .iter()
            .filter(|o| *o != m && dominant_document(store, o).as_deref() == Some(doc.as_str()))
            .count();
        if others > 0 {
            hints.push(format!(
                "namesake {} ({} is headed {}; {} other member(s) of the level from it)",
                m, doc, title, others
            ));
        }
    }
    hints
}

// A grouping staged in the changeset: a derived entity with no mentions, its members
// the entities whose parent becomes it in the same changeset.
struct StagedGrouping<'a> {
    id: &'a str,
    entity: &'a Entity,
    from: &'a [String],
    members: Vec<String>,
}

// The containment tree after the staged changeset: every surviving id with the parent
// and scope it holds at commit (a created grouping's members, a moved child, a
// dissolved grouping's children landing on its parent).
struct Projection<'a> {
    store: &'a Store,
    created: BTreeMap<&'a str, &'a Entity>,
    dissolved: BTreeMap<&'a str, Option<String>>,
    moved: BTreeMap<&'a str, &'a str>,
    all_ids: Vec<String>,
}

impl<'a> Projection<'a> {
    fn new(store: &'a Store, staged: &'a [Op]) -> Projection<'a> {
        let created: BTreeMap<&str, &Entity> = staged
            .iter()
            .filter_map(|o| match o {
                Op::CreateEntity { id, entity } => Some((id.as_str(), entity)),
                _ => None,
            })
            .collect();
        let deleted: BTreeSet<&str> = staged
            .iter()
            .filter_map(|o| match o {
                Op::DeleteEntity { id, .. }
                | Op::MergeEntities { absorb: id, .. }
                | Op::DissolveEntity { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let parent_before = |id: &str| -> Option<String> {
            store
                .graph
                .entities
                .get(store.resolve_id(id))
                .and_then(|e| e.parent.clone())
                .filter(|p| !p.is_empty())
        };
        // A dissolved grouping's children land on its parent.
        let dissolved: BTreeMap<&str, Option<String>> = staged
            .iter()
            .filter_map(|o| match o {
                Op::DissolveEntity { id, parent, .. } => Some((
                    id.as_str(),
                    parent
                        .clone()
                        .filter(|p| !p.is_empty())
                        .or_else(|| parent_before(id)),
                )),
                _ => None,
            })
            .collect();
        let moved: BTreeMap<&str, &str> = staged
            .iter()
            .filter_map(|o| match o {
                Op::UpdateEntity {
                    id,
                    parent: Some(p),
                    ..
                } => Some((id.as_str(), p.as_str())),
                _ => None,
            })
            .collect();
        let all_ids: Vec<String> = store
            .graph
            .entities
            .keys()
            .cloned()
            .chain(created.keys().map(|k| k.to_string()))
            .filter(|id| !deleted.contains(id.as_str()))
            .collect();
        Projection {
            store,
            created,
            dissolved,
            moved,
            all_ids,
        }
    }

    fn stored(&self, id: &str) -> Option<&'a Entity> {
        self.store.graph.entities.get(self.store.resolve_id(id))
    }

    fn parent_before(&self, id: &str) -> Option<String> {
        self.stored(id)
            .and_then(|e| e.parent.clone())
            .filter(|p| !p.is_empty())
    }

    fn scope_of(&self, id: &str) -> Option<String> {
        self.created
            .get(id)
            .map(|e| e.scope.clone())
            .or_else(|| self.stored(id).map(|e| e.scope.clone()))
    }

    fn parent_after(&self, id: &str) -> Option<String> {
        let p = match self.moved.get(id) {
            Some(p) => Some(p.to_string()).filter(|p| !p.is_empty()),
            None => self
                .created
                .get(id)
                .and_then(|e| e.parent.clone())
                .or_else(|| self.parent_before(id)),
        };
        match p.as_deref().and_then(|p| self.dissolved.get(p)) {
            Some(grand) => grand.clone(),
            None => p,
        }
    }

    // The level's member count after the changeset.
    fn level_count(&self, level: &str) -> u64 {
        self.all_ids
            .iter()
            .filter(|id| {
                let scope = self.scope_of(id).unwrap_or_default();
                level_key(self.parent_after(id).as_deref(), &scope) == level
            })
            .count() as u64
    }
}

// One level against its standing fan-out record: the count after the changeset is at
// or under the limit in force (hard when the goal is mandatory, soft otherwise). None
// when no record stands on the level.
fn fan_out_level_violation(proj: &Projection, level: &str) -> Option<Violation> {
    let f = fan_out_crossing(proj.store, level)?;
    let count = proj.level_count(level);
    if count <= f.limit_in_force() {
        return None;
    }
    Some(Violation::new(
        "fan-out-over-limit",
        format!(
            "{}: {} children after the changeset, over {} ({}, soft {}, hard {}); group more members, or fail the goal naming why the level is flat",
            level,
            count,
            f.limit_in_force(),
            limits::CHILDREN_PER_ENTITY,
            f.soft,
            f.hard
        ),
    ))
}

// The fan-out gate for one goal at mark_goal_done: the staged gates, then the goal's
// own level whether or not the changeset touched it, so a goal marked done without
// work still faces its count. Mirrors
// docs/compiler/goals/abstract-entity.md#the-fan-out-gate.
// The split-view claim: the target view, with this session's staged view updates
// applied, is within the soft value of every limit it crossed (or its bump). A split
// that leaves the view over the soft value commits at done but resolves nothing.
// Mirrors docs/compiler/goals/split-view.md#gate.
pub fn split_view_goal_gate(store: &Store, staged: &[Op], target: &str) -> Option<Violation> {
    let id = store.resolve_id(target).to_string();
    let mut v = store.graph.views.get(&id)?.clone();
    for op in staged {
        match op {
            Op::DeleteView { id: d, .. } if store.resolve_id(d) == id => return None,
            Op::UpdateView {
                id: u,
                members,
                add_members,
                remove_members,
                query,
                collapse,
                exclude,
                ..
            } if store.resolve_id(u) == id => {
                if let Some(ms) = members {
                    v.members = ms.clone();
                }
                for x in add_members {
                    if !v.members.contains(x) {
                        v.members.push(x.clone());
                    }
                }
                v.members.retain(|x| !remove_members.contains(x));
                if let Some(q) = query {
                    v.query = Some(q.clone());
                }
                if let Some(c) = collapse {
                    v.collapse = c.clone();
                }
                for x in exclude {
                    v.members.retain(|y| y != &x.id);
                }
            }
            _ => {}
        }
    }
    let mut after = store.clone();
    after.graph.views.insert(id.clone(), v);
    let c = crate::derive::threshold_crossings(&after)
        .into_iter()
        .find(|c| c.subject == id)?;
    Some(Violation::new(
        "over-limit",
        format!(
            "{} still has {} {} after the staged work, over the soft value {}; bring the view itself under it (for a flow view, keep one member per phase, the phase's first step, beside the overview), or fail the goal with the reason",
            id, c.count, c.limit, c.soft
        ),
    ))
}

pub fn fan_out_goal_gate(store: &Store, staged: &[Op], target: &str) -> Option<Violation> {
    if let Some(v) = fan_out_gates(store, staged).into_iter().next() {
        return Some(v);
    }
    fan_out_level_violation(&Projection::new(store, staged), target)
}

// The fan-out gate over the staged changeset. Every grouping created has two or more
// children, a definition, derived provenance naming exactly its members, and the
// members' shared current parent as its own (a grouping never crosses levels); no
// member changed scope; and every level the changeset touched that stands over its
// fan-out limit is brought to the limit in force (hard when the goal is mandatory,
// soft otherwise). A level the changeset leaves alone is a declined or failed goal's,
// never this gate's. Mirrors docs/compiler/goals/abstract-entity.md#the-fan-out-gate.
pub fn fan_out_gates(store: &Store, staged: &[Op]) -> Vec<Violation> {
    let mut out = Vec::new();
    let proj = Projection::new(store, staged);
    let created = &proj.created;

    // A grouping holds a level and carries no requirements of its own; a derived
    // entity a staged requirement points at is the caps variant's sub-entity.
    let carries_requirements = |id: &str| {
        staged.iter().any(|o| match o {
            Op::UpdateRequirement {
                entities: Some(e), ..
            } => e.iter().any(|x| x == id),
            Op::CreateRequirement { requirement, .. } => {
                requirement.entities.iter().any(|x| x == id)
            }
            _ => false,
        })
    };
    let groupings: Vec<StagedGrouping> = created
        .iter()
        .filter_map(|(id, entity)| match &entity.provenance {
            Some(Provenance::Derived { from, .. })
                if entity.mentions.is_empty() && !carries_requirements(id) =>
            {
                let members: Vec<String> = proj
                    .all_ids
                    .iter()
                    .filter(|m| m.as_str() != *id && proj.parent_after(m).as_deref() == Some(*id))
                    .cloned()
                    .collect();
                if members.is_empty() {
                    return None;
                }
                Some(StagedGrouping {
                    id,
                    entity,
                    from,
                    members,
                })
            }
            _ => None,
        })
        .collect();
    let mut touched: BTreeSet<String> = BTreeSet::new();
    for g in &groupings {
        touched.insert(level_key(g.entity.parent.as_deref(), &g.entity.scope));
        if g.members.len() < 2 {
            out.push(Violation::new(
                "grouping-too-small",
                format!(
                    "{}: a grouping holds two or more children ({} staged); below two there is nothing to judge",
                    g.id,
                    g.members.len()
                ),
            ));
        }
        if g.entity
            .definition
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            out.push(Violation::new(
                "definition-required",
                format!(
                    "{}: a grouping carries a one-sentence definition of its responsibility",
                    g.id
                ),
            ));
        }
        let named: BTreeSet<String> = g
            .from
            .iter()
            .map(|f| store.resolve_id(f).to_string())
            .collect();
        let members: BTreeSet<String> = g.members.iter().cloned().collect();
        if named != members {
            out.push(Violation::new(
                "provenance-mismatch",
                format!(
                    "{}: derived provenance names [{}] while its members are [{}]; from names exactly the members",
                    g.id,
                    named.iter().cloned().collect::<Vec<_>>().join(", "),
                    members.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
            ));
        }
        let own_parent = g.entity.parent.clone().filter(|p| !p.is_empty());
        for m in &g.members {
            if created.contains_key(m.as_str()) {
                continue;
            }
            let was = proj.parent_before(m);
            if was != own_parent {
                out.push(Violation::new(
                    "cross-level",
                    format!(
                        "{}: a grouping never crosses levels; {} sits under {} while the grouping takes {}",
                        g.id,
                        m,
                        was.as_deref().unwrap_or("the scope root"),
                        own_parent.as_deref().unwrap_or("the scope root")
                    ),
                ));
            }
            if proj.scope_of(m).as_deref() != Some(g.entity.scope.as_str()) {
                out.push(Violation::new(
                    "scope-mismatch",
                    format!(
                        "{}: a grouping takes its members' scope; {} is in {}",
                        g.id,
                        m,
                        proj.scope_of(m).unwrap_or_default()
                    ),
                ));
            }
        }
    }
    for (id, p) in &proj.moved {
        if let Some(before) = level_of(store, id) {
            touched.insert(before);
        }
        touched.insert(level_key(Some(p), &proj.scope_of(id).unwrap_or_default()));
    }
    for (id, grand) in &proj.dissolved {
        touched.insert(level_key(
            grand.as_deref(),
            &proj.scope_of(id).unwrap_or_default(),
        ));
    }
    for level in touched {
        out.extend(fan_out_level_violation(&proj, &level));
    }
    out
}

impl GoalKind for AbstractEntity {
    fn kind(&self) -> &'static str {
        "abstract-entity"
    }
    fn class(&self) -> Class {
        Class::Gc
    }
    fn unit(&self) -> &'static str {
        "entity"
    }
    // The caps variant on an entity's requirement and state counts, the fan-out
    // variant on a level's child count (a node, or `scope:<scope>` for the top level);
    // records for both on one node fold into one goal. Mirrors
    // docs/compiler/goals/abstract-entity.md#created-when and #the-fan-out-variant.
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut by_target: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_THRESHOLD_CROSSED]) {
            let mine = rec.detail["goal"] == "abstract-entity"
                || rec.detail["limit"] == limits::CHILDREN_PER_ENTITY;
            let subject_exists = store.graph.entities.contains_key(&rec.subject)
                || scope_target(&rec.subject).is_some();
            if mine && subject_exists {
                by_target.entry(rec.subject.clone()).or_default().push(rec);
            }
        }
        let mut out = Vec::new();
        for (id, recs) in by_target {
            let crossed = crossings_for(&recs);
            let mandatory = crossed.iter().any(|c| c.count > c.hard);
            let cause = earliest_cause(&recs);
            let (fan, caps): (Vec<&Crossed>, Vec<&Crossed>) = crossed
                .iter()
                .partition(|c| c.limit == limits::CHILDREN_PER_ENTITY);
            let caps: Vec<Crossed> = caps
                .into_iter()
                .map(|c| Crossed {
                    limit: c.limit.clone(),
                    count: c.count,
                    soft: c.soft,
                    hard: c.hard,
                })
                .collect();
            let mut hints = vec![format!("load {}", id)];
            let mut change = match fan.first() {
                Some(c) => {
                    let f = FanOut {
                        count: c.count,
                        soft: c.soft,
                        hard: c.hard,
                    };
                    let cands = coupling_candidates(store, &id, f.soft);
                    hints.extend(fan_out_hints(store, &id, &f, &cands));
                    json!({
                        "fan_out": f.count,
                        "limit": {"soft": f.soft, "hard": f.hard},
                        "candidates": cands.iter().map(|c| c.ids.clone()).collect::<Vec<_>>(),
                    })
                }
                None => Value::Null,
            };
            if !caps.is_empty() {
                let caps_change = threshold_change(&caps);
                match change.as_object_mut() {
                    Some(o) => {
                        o.insert("limits".into(), caps_change["limits"].clone());
                    }
                    None => change = caps_change,
                }
            }
            if let Some(e) = store.graph.entities.get(&id).filter(|_| !caps.is_empty()) {
                for c in &caps {
                    hints.push(format!(
                        "{} > {} ({}, soft {}, hard {})",
                        c.count, c.soft, c.limit, c.soft, c.hard
                    ));
                    if c.limit == "states-per-state-machine" {
                        hints.push(format!("load sm:{}", crate::derive::entity_slug(&id)));
                    }
                }
                for (tok, n, section) in cohesion_groups(store, &id) {
                    hints.push(format!("group {} ({} requirements, {})", tok, n, section));
                }
                for cid in level_members(store, &id) {
                    hints.push(format!(
                        "child {} ({} requirements)",
                        cid,
                        store.requirements_referencing(&cid).len()
                    ));
                }
                if let Some((doc, section)) = defining_section(store, &id) {
                    let full = format!("{}#{}", doc, section);
                    let too_large = store.graph.diagnostics.values().any(|d| {
                        d.lifecycle == "open"
                            && d.rule == "section-too-large"
                            && d.subjects.contains(&full)
                    });
                    hints.push(format!(
                        "load {}{}",
                        full,
                        if too_large {
                            " (section-too-large filed)"
                        } else {
                            ""
                        }
                    ));
                }
                let composed = store
                    .graph
                    .relationships
                    .values()
                    .flat_map(|r| r.contributions.iter())
                    .any(|c| c.r#type == "composition" && c.a == id);
                if e.parent.is_none() && !composed {
                    hints.push(
                        "root: no parent, no composition stated; a decision prompt is owed".into(),
                    );
                }
            }
            let tools = match (fan.is_empty(), caps.is_empty()) {
                (false, true) => "skill abstraction; group_entities, update_entity, dissolve_entity",
                (false, false) => "skill abstraction; group_entities, update_entity, dissolve_entity; upsert_entity, update_requirement",
                _ => "skill abstraction; upsert_entity, update_requirement, update_entity",
            };
            hints.push(tools.into());
            out.push(build_goal(self, &id, mandatory, change, cause, hints));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        gc_ready(goal, board)
    }
    // The node (or `scope:<scope>`, the top level as stubs) full and its children as
    // stubs. The caps variant adds the defining section; the fan-out variant the
    // level's views and the existing groupings' children. Mirrors
    // docs/compiler/goals/abstract-entity.md#what-the-model-sees and #the-fan-out-prompt.
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            let fan_out = g.change.get("fan_out").is_some();
            let caps = !fan_out || g.change.get("limits").is_some();
            lines.push(format!("- {} full", g.target));
            let members = level_members(store, &g.target);
            for cid in &members {
                lines.push(format!("- {} stub", cid));
            }
            if caps {
                if let Some((doc, section)) = defining_section(store, &g.target) {
                    lines.push(format!("- {}#{} full", doc, section));
                }
            }
            if fan_out {
                for m in &members {
                    for cid in level_members(store, m) {
                        lines.push(format!("- {} stub", cid));
                    }
                }
            }
            for (vid, v) in &store.graph.views {
                let level_view = v
                    .query
                    .as_ref()
                    .and_then(|q| q.parent.as_deref())
                    .is_some_and(|p| p == g.target)
                    || scope_target(&g.target).is_some_and(|s| {
                        vid == &format!("view:component/scope-{}", s)
                            || vid == &format!("view:class/scope-{}", s)
                    });
                if v.members.contains(&g.target) || level_view {
                    lines.push(format!("- {} stub", vid));
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "upsert_entity",
            "update_entity",
            "upsert_requirement",
            "update_requirement",
            "group_entities",
            "dissolve_entity",
            "upsert_view",
            "update_view",
            "report_diagnostic",
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = fan_out_gates(store, staged);
        let staged_parents: BTreeSet<String> = staged
            .iter()
            .filter_map(|o| match o {
                Op::CreateEntity { id, entity } if entity.parent.is_some() => Some(id.clone()),
                _ => None,
            })
            .collect();
        for op in staged {
            match op {
                Op::CreateEntity { id, entity } if entity.parent.is_some() => {
                    match &entity.provenance {
                        Some(Provenance::Derived { from, reasoning }) => {
                            if from.is_empty() || reasoning.trim().is_empty() {
                                out.push(Violation::new(
                                    "provenance-required",
                                    format!("{}: derived provenance needs from and reasoning", id),
                                ));
                            }
                        }
                        _ => out.push(Violation::new(
                            "provenance-required",
                            format!(
                                "{}: a sub-entity carries derived provenance (from, reasoning)",
                                id
                            ),
                        )),
                    }
                    if entity.definition.as_deref().unwrap_or("").trim().is_empty() {
                        out.push(Violation::new(
                            "definition-required",
                            format!(
                                "{}: the definition is the sentence the documents should gain",
                                id
                            ),
                        ));
                    }
                    let moved = staged.iter().any(|o| match o {
                        Op::UpdateRequirement {
                            entities: Some(e), ..
                        } => e.iter().any(|x| x == id),
                        Op::CreateRequirement { requirement, .. } => {
                            requirement.entities.iter().any(|x| x == id)
                        }
                        Op::UpdateEntity {
                            parent: Some(p), ..
                        } => p == id,
                        _ => false,
                    });
                    if !moved {
                        out.push(Violation::new(
                            "nothing-moved",
                            format!(
                                "{}: no requirement re-pointed and no child re-parented under it",
                                id
                            ),
                        ));
                    }
                    if let Some(p) = &entity.parent {
                        let scope_of =
                            |x: &str| store.graph.entities.get(x).map(|e| e.scope.clone());
                        if let Some(ps) = scope_of(p) {
                            if ps != entity.scope {
                                out.push(Violation::new(
                                    "scope-mismatch",
                                    format!(
                                        "{}: a split never crosses a scope ({} is in {})",
                                        id, p, ps
                                    ),
                                ));
                            }
                        }
                    }
                }
                Op::DeleteEntity { id, .. } | Op::MergeEntities { absorb: id, .. }
                    if !staged_parents.contains(id) => {
                        out.push(Violation::new(
                            "target-removed",
                            format!(
                                "{}: an abstraction adds structure; it never deletes or merges",
                                id
                            ),
                        ));
                    }
                _ => {}
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../docs/compiler/goals/prompts/abstract-entity.md")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::vfs::VPathExt;

    #[test]
    fn registry_covers_seventeen_kinds_with_pages_and_prompts() {
        assert_eq!(REGISTRY.len(), 17);
        let mut names: BTreeSet<&str> = BTreeSet::new();
        for k in REGISTRY.iter() {
            assert!(names.insert(k.kind()), "duplicate kind {}", k.kind());
            let page = format!(
                "{}/../docs/compiler/goals/{}.md",
                env!("CARGO_MANIFEST_DIR"),
                k.kind()
            );
            assert!(
                std::path::Path::new(&page).vexists(),
                "{} has no page",
                k.kind()
            );
            if blocked_on_human(k.kind()) {
                assert!(k.prompt().is_empty());
                assert!(k.toolset().is_empty());
            } else {
                assert!(
                    !k.prompt().trim().is_empty(),
                    "{} has no contract paragraph",
                    k.kind()
                );
                // bind and derive-criteria derive from state (the ledger, the criteria),
                // not from a record kind of their own.
                assert!(
                    !record_kinds(k.kind()).is_empty()
                        || matches!(k.kind(), "bind" | "derive-criteria")
                );
            }
            match k.class() {
                Class::Compile => assert!(tier(k.kind()).is_some(), "{} has no tier", k.kind()),
                Class::Gc => assert!(tier(k.kind()).is_none()),
            }
        }
        assert_eq!(
            parse_goal_id("g:retrace:view:usecase/holds"),
            Some(("retrace", "view:usecase/holds"))
        );
        assert_eq!(pair_target("req:b-1", "req:a-1"), "req:a-1~req:b-1");
    }

    #[test]
    fn lookalike_scores_pair_cross_document_name_variants_only() {
        let mut s = Store::default();
        let ent = |name: &str, docs: &[&str]| Entity {
            name: name.into(),
            mentions: docs
                .iter()
                .map(|d| SourceRef {
                    doc: d.to_string(),
                    section: "/x".into(),
                    quote: name.into(),
                })
                .collect(),
            ..Default::default()
        };
        s.graph
            .entities
            .insert("ent:backend".into(), ent("backend", &["api.md"]));
        s.graph.entities.insert(
            "ent:backend-system".into(),
            ent("backend system", &["deploy.md"]),
        );
        s.graph
            .entities
            .insert("ent:order".into(), ent("Order", &["api.md"]));
        s.graph.entities.insert(
            "ent:reorder-point".into(),
            ent("Reorder point", &["deploy.md"]),
        );
        s.graph
            .entities
            .insert("ent:lonely".into(), ent("backend host", &["api.md"]));
        let scores = lookalike_scores(&s);
        let pair = scores
            .iter()
            .find(|l| l.a == "ent:backend" && l.b == "ent:backend-system")
            .expect("backend and backend system are candidates");
        assert!((pair.tokens - 0.67).abs() < 0.01, "{}", pair.tokens);
        assert_eq!(pair.documents, 0.0);
        assert!((pair.score - 0.5).abs() < 0.01);
        assert!(!scores
            .iter()
            .any(|l| l.a == "ent:order" || l.b == "ent:reorder-point"));
        // Same document only: review-entity's to judge, never a candidate.
        assert!(!scores
            .iter()
            .any(|l| l.a == "ent:backend" && l.b == "ent:lonely"));
    }

    // ---- the fan-out variant ----

    fn gen_settings() -> GenSettings {
        GenSettings {
            configured: true,
            deliverable: std::path::PathBuf::from("/nonexistent"),
            worker: "agentic".into(),
            code: Vec::new(),
        }
    }

    fn child(name: &str, parent: Option<&str>, doc: &str) -> Entity {
        Entity {
            name: name.into(),
            parent: parent.map(String::from),
            stereotype: Some("component".into()),
            mentions: vec![SourceRef {
                doc: doc.into(),
                section: "/x".into(),
                quote: name.into(),
            }],
            ..Default::default()
        }
    }

    fn link(s: &mut Store, n: usize, a: &str, b: &str) {
        s.graph.requirements.insert(
            format!("req:x-{}", n),
            Requirement {
                statement: format!("{} calls {}.", a, b),
                entities: vec![a.into(), b.into()],
                source: Some(SourceRef {
                    doc: "api.md".into(),
                    section: "/x".into(),
                    quote: "calls".into(),
                }),
                ..Default::default()
            },
        );
    }

    // One record per subject and limit, as the limit counts write them at commit.
    fn threshold_record(s: &mut Store, subject: &str, limit: &str, count: u64) {
        let (soft, hard) = limits::threshold(limit, None).unwrap();
        s.status.changes.retain(|c| {
            !(c.kind == store::CHANGE_THRESHOLD_CROSSED
                && c.subject == subject
                && c.detail["limit"] == limit)
        });
        let n = s.status.changes.len();
        s.status.changes.push(
            ChangeRecord::new(7, n, 0, store::CHANGE_THRESHOLD_CROSSED, subject, "limits")
                .with_detail(json!({
                    "limit": limit, "count": count, "soft": soft, "hard": hard,
                    "level": if count > hard { "hard" } else { "soft" },
                    "goal": "abstract-entity",
                })),
        );
    }

    fn fan_out_record(s: &mut Store, subject: &str, count: u64) {
        threshold_record(s, subject, limits::CHILDREN_PER_ENTITY, count);
    }

    // ---- the caps variant ----

    fn statement(s: &mut Store, id: &str, text: &str, entities: &[&str], section: &str) {
        s.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: text.into(),
                entities: entities.iter().map(|e| e.to_string()).collect(),
                source: Some(SourceRef {
                    doc: "svc.md".into(),
                    section: section.into(),
                    quote: text.into(),
                }),
                ..Default::default()
            },
        );
    }

    // A service over a requirement record with soft 2 and hard 3, four statements on it.
    fn caps_store() -> Store {
        let mut s = Store::default();
        s.graph
            .entities
            .insert("ent:svc".into(), child("Service", None, "svc.md"));
        for i in 1..=4 {
            statement(
                &mut s,
                &format!("req:svc-{}", i),
                &format!("The Service persists record {} in the database.", i),
                &["ent:svc"],
                "/svc/data",
            );
        }
        s.status.changes.push(
            ChangeRecord::new(7, 0, 0, store::CHANGE_THRESHOLD_CROSSED, "ent:svc", "limits")
                .with_detail(json!({
                    "limit": "requirements-per-entity", "count": 4, "soft": 2, "hard": 3,
                    "level": "hard", "goal": "abstract-entity",
                })),
        );
        s
    }

    fn repoint(id: &str, to: &str) -> Op {
        Op::UpdateRequirement {
            id: id.into(),
            statement: None,
            entities: Some(vec![to.into()]),
            edges: None,
            transition: None,
            facets: None,
            source: None,
            provenance: None,
        }
    }

    // Mirrors docs/compiler/goals/abstract-entity.md#gate: the count after the
    // changeset is within soft, so a split that only de-escalates is refused.
    #[test]
    fn caps_gate_refuses_a_count_still_over_soft_and_accepts_one_within() {
        let s = caps_store();
        let v = caps_goal_gate(&s, &[], "ent:svc").expect("four over soft two");
        assert_eq!(v.rule, "count-over-limit");
        assert!(v.message.contains("4 requirements after the changeset, over 2"));
        let one = [repoint("req:svc-1", "ent:data")];
        let v = caps_goal_gate(&s, &one, "ent:svc").expect("three is still over two");
        assert!(v.message.contains("3 requirements"));
        let two = [
            repoint("req:svc-1", "ent:data"),
            repoint("req:svc-2", "ent:data"),
        ];
        assert!(caps_goal_gate(&s, &two, "ent:svc").is_none());
        // A record on another limit is not this gate's.
        let mut other = Store::default();
        other
            .graph
            .entities
            .insert("ent:svc".into(), child("Service", None, "svc.md"));
        fan_out_record(&mut other, "ent:svc", 12);
        assert!(caps_goal_gate(&other, &[], "ent:svc").is_none());
    }

    // Mirrors docs/compiler/goals/abstract-entity.md#hints: a group hint shows the
    // word the statements use, and function words are never groups.
    #[test]
    fn cohesion_groups_drop_function_words_and_show_the_written_word() {
        let mut s = Store::default();
        s.graph
            .entities
            .insert("ent:svc".into(), child("Service", None, "svc.md"));
        for (i, t) in [
            "Every Service handler persists a session from the request.",
            "Every Service handler persists an audit entry from the request.",
            "Every Service handler persists an invoice from the queue.",
        ]
        .iter()
        .enumerate()
        {
            statement(&mut s, &format!("req:svc-{}", i), t, &["ent:svc"], "/svc/data");
        }
        let groups = cohesion_groups(&s, "ent:svc");
        let words: Vec<&str> = groups.iter().map(|(w, _, _)| w.as_str()).collect();
        assert!(words.contains(&"persists"), "{:?}", words);
        assert!(words.contains(&"handler"), "{:?}", words);
        // "request" recurs in two statements only; "every" and "from" recur in three
        // and are function words; "persist" is the stem, never shown.
        assert!(
            !words
                .iter()
                .any(|w| *w == "request" || *w == "every" || *w == "from" || *w == "persist"),
            "{:?}",
            words
        );
        let persists = groups.iter().find(|(w, _, _)| w == "persists").unwrap();
        assert_eq!((persists.1, persists.2.as_str()), (3, "/svc/data"));
    }

    // Mirrors docs/compiler/goals/abstract-entity.md#hints: the defining section is
    // the root of the document headed with the entity's name, never the front door's
    // list item that mentions it first.
    #[test]
    fn defining_section_prefers_the_document_headed_with_the_name() {
        let mut s = Store::default();
        let mut e = child("API Server", None, "README.md");
        e.mentions.push(SourceRef {
            doc: "api-server.md".into(),
            section: "/api-server/routing".into(),
            quote: "API Server".into(),
        });
        s.graph.entities.insert("ent:api-server".into(), e);
        let root = |title: &str| Section {
            title: title.into(),
            kind: "root".into(),
            order: 0,
            parent: None,
            raw: String::new(),
            hash: String::new(),
            lines: [1, 1],
        };
        let mut readme = DocRecord::default();
        readme.sections.insert("/trellis".into(), root("Trellis"));
        s.docs.insert("README.md".into(), readme);
        let mut api = DocRecord::default();
        api.sections.insert("/api-server".into(), root("API Server"));
        s.docs.insert("api-server.md".into(), api);
        assert_eq!(
            defining_section(&s, "ent:api-server"),
            Some(("api-server.md".into(), "/api-server".into()))
        );
        // Without a namesake document, the mention section quoting most of its
        // requirements; with nothing quoted, the first mention.
        s.docs.clear();
        assert_eq!(
            defining_section(&s, "ent:api-server"),
            Some(("README.md".into(), "/x".into()))
        );
        let quoted = |s: &mut Store, id: &str, doc: &str, section: &str| {
            s.graph.requirements.insert(
                id.into(),
                Requirement {
                    statement: "The API Server routes.".into(),
                    entities: vec!["ent:api-server".into()],
                    source: Some(SourceRef {
                        doc: doc.into(),
                        section: section.into(),
                        quote: "routes".into(),
                    }),
                    ..Default::default()
                },
            );
        };
        quoted(&mut s, "req:a-1", "README.md", "/x");
        quoted(&mut s, "req:a-2", "api-server.md", "/api-server/routing");
        quoted(&mut s, "req:a-3", "api-server.md", "/api-server/routing");
        quoted(&mut s, "req:a-4", "other.md", "/elsewhere");
        assert_eq!(
            defining_section(&s, "ent:api-server"),
            Some(("api-server.md".into(), "/api-server/routing".into()))
        );
    }

    // A backend with twelve children in two coupled groups plus stragglers, and a
    // public scope root with ten parentless entities.
    fn levels_store() -> Store {
        let mut s = Store::default();
        s.graph
            .entities
            .insert("ent:backend".into(), child("Backend", None, "arch.md"));
        for (i, n) in ["cart", "pricing", "checkout", "discount"]
            .iter()
            .enumerate()
        {
            s.graph.entities.insert(
                format!("ent:{}", n),
                child(
                    n,
                    Some("ent:backend"),
                    if i % 2 == 0 { "orders.md" } else { "api.md" },
                ),
            );
        }
        for n in ["shipment", "carrier", "tracking"] {
            s.graph.entities.insert(
                format!("ent:{}", n),
                child(n, Some("ent:backend"), "shipping.md"),
            );
        }
        for n in ["audit", "cache", "queue", "search", "mailer"] {
            s.graph.entities.insert(
                format!("ent:{}", n),
                child(n, Some("ent:backend"), "infra.md"),
            );
        }
        // A leaf under cart: its requirement lifts to cart.
        s.graph.entities.insert(
            "ent:cart-item".into(),
            child("cart item", Some("ent:cart"), "orders.md"),
        );
        let mut n = 0;
        let mut l = |s: &mut Store, a: &str, b: &str| {
            n += 1;
            link(s, n, a, b);
        };
        l(&mut s, "ent:cart", "ent:pricing");
        l(&mut s, "ent:cart", "ent:pricing");
        l(&mut s, "ent:pricing", "ent:discount");
        l(&mut s, "ent:cart-item", "ent:checkout");
        l(&mut s, "ent:checkout", "ent:discount");
        l(&mut s, "ent:shipment", "ent:carrier");
        l(&mut s, "ent:carrier", "ent:tracking");
        l(&mut s, "ent:shipment", "ent:tracking");
        l(&mut s, "ent:queue", "ent:mailer");
        s.graph.relationships.insert(
            "rel:ent:cart~ent:pricing".into(),
            Relationship {
                members: vec!["ent:cart".into(), "ent:pricing".into()],
                contributions: Vec::new(),
            },
        );
        // Ten parentless entities beside the backend at the scope root.
        for i in 0..9 {
            s.graph.entities.insert(
                format!("ent:top-{}", i),
                child(&format!("top {}", i), None, "arch.md"),
            );
        }
        fan_out_record(&mut s, "ent:backend", 12);
        fan_out_record(&mut s, "scope:public", 10);
        s
    }

    // Mirrors docs/compiler/goals/abstract-entity.md#fan-out-hints: a member the
    // node's own document lists carries the listing section (the heading it sits
    // under) beside the document it is mentioned in most; a member the node's
    // document never names carries its document alone.
    #[test]
    fn fan_out_hints_carry_the_listing_section_of_the_nodes_document() {
        let mut s = levels_store();
        s.graph
            .entities
            .get_mut("ent:cart")
            .unwrap()
            .mentions
            .push(SourceRef {
                doc: "arch.md".into(),
                section: "/arch/data-model".into(),
                quote: "cart".into(),
            });
        let goals = AbstractEntity.derive_goals(&s, &gen_settings());
        let node = goals.iter().find(|g| g.target == "ent:backend").unwrap();
        assert!(
            node.hints
                .iter()
                .any(|h| h.starts_with("member ent:cart «component» (arch.md#/arch/data-model")),
            "{:?}",
            node.hints
        );
        assert!(
            node.hints
                .contains(&"member ent:pricing «component» (api.md)".to_string()),
            "{:?}",
            node.hints
        );
    }

    // Mirrors docs/compiler/goals/review-entity.md#hints: a review names the level
    // views around its entity (its own when it has children, its parent's level),
    // and leaves the harness-owned bookkeeping out of the open diagnostics.
    #[test]
    fn review_hints_name_the_level_views_and_skip_harness_owned_diagnostics() {
        let mut s = levels_store();
        let n = s.status.changes.len();
        s.status.changes.push(ChangeRecord::new(
            7,
            n,
            0,
            store::CHANGE_ENTITY,
            "ent:backend",
            "fields",
        ));
        s.graph.diagnostics.insert(
            "diag:incomplete-build-1".into(),
            crate::model::Diagnostic {
                rule: "incomplete-build".into(),
                severity: "warning".into(),
                subjects: vec!["ent:backend".into()],
                message: "parked".into(),
                reasoning: None,
                lifecycle: "open".into(),
                triage: None,
                prompt: None,
                answer: None,
                created: None,
                updated: None,
            },
        );
        // A criterion-missing finding is the build's too: a review can neither add a
        // criterion nor clear the check that refiles it
        // (docs/compiler/goals/review-entity.md#hints).
        s.graph.requirements.insert(
            "req:backend-1".into(),
            Requirement {
                statement: "The Backend serves the REST API.".into(),
                entities: vec!["ent:backend".into()],
                ..Default::default()
            },
        );
        s.graph.diagnostics.insert(
            "diag:criterion-missing-1".into(),
            crate::model::Diagnostic {
                rule: "criterion-missing".into(),
                severity: "warning".into(),
                subjects: vec![s
                    .requirements_referencing("ent:backend")
                    .first()
                    .cloned()
                    .expect("the fixture's backend carries a requirement")],
                message: "no acceptance criterion".into(),
                reasoning: None,
                lifecycle: "open".into(),
                triage: None,
                prompt: None,
                answer: None,
                created: None,
                updated: None,
            },
        );
        let rid = s.requirements_referencing("ent:backend")[0].clone();
        s.graph.diagnostics.insert(
            "diag:ambiguity-1".into(),
            crate::model::Diagnostic {
                rule: "ambiguity".into(),
                severity: "warning".into(),
                subjects: vec![rid],
                message: "two readings".into(),
                reasoning: None,
                lifecycle: "open".into(),
                triage: None,
                prompt: None,
                answer: None,
                created: None,
                updated: None,
            },
        );
        let goals = ReviewEntity.derive_goals(&s, &gen_settings());
        let g = goals.iter().find(|g| g.target == "ent:backend").unwrap();
        // The control: a finding a review can act on is listed on the same requirement.
        assert!(
            g.hints.iter().any(|h| h.contains("diag:ambiguity-1")),
            "{:?}",
            g.hints
        );
        assert!(
            !g.hints.iter().any(|h| h.contains("criterion-missing")),
            "{:?}",
            g.hints
        );
        let own = crate::derive::level_view_id(&s, "ent:backend").unwrap();
        let parent = crate::derive::level_view_id(&s, "scope:public").unwrap();
        assert!(
            g.hints.contains(&format!(
                "level views: {} (this entity), {} (the level above)",
                own, parent
            )),
            "{:?}",
            g.hints
        );
        assert!(
            !g.hints.iter().any(|h| h.contains("incomplete-build")),
            "{:?}",
            g.hints
        );
    }

    // The agent contract asks for independent calls in one reply under the native
    // codec. Mirrors docs/compiler/goals/prompts/agent-contract.md.
    #[test]
    fn the_contract_asks_for_independent_calls_in_one_reply() {
        let contract = include_str!("../../docs/compiler/goals/prompts/agent-contract.md");
        assert!(contract.contains("Independent calls go together in one reply"));
        let skill = include_str!("../../docs/compiler/skills/extraction.md");
        assert!(skill.contains("one `search` with `queries`"));
    }

    // The extraction skill refuses a default stereotype: a concept the section only
    // names and uses carries none, and `component`, `entity`, `class` are never
    // defaults (the f2 root ingest labeled every domain concept «component»).
    // Mirrors docs/compiler/skills/extraction.md.
    #[test]
    fn the_extraction_skill_refuses_default_stereotypes() {
        let skill = include_str!("../../docs/compiler/skills/extraction.md");
        assert!(skill.contains("carries no stereotype: leave the field absent"));
        assert!(skill.contains("`component`, `entity`, and `class` are never defaults"));
    }

    // The abstraction skill refuses a grouping named as the plural of one member or
    // with a word another level uses for stated children (the org root fan-out minted
    // "Departments" beside the company that holds the departments; the near-duplicate
    // gate folds no plurals). Mirrors docs/compiler/skills/abstraction.md.
    #[test]
    fn the_abstraction_skill_refuses_a_plural_of_a_member_as_a_grouping_name() {
        let skill = include_str!("../../docs/compiler/skills/abstraction.md");
        assert!(skill.contains("never named as the plural of one member"));
        assert!(skill.contains("the near-duplicate gate folds no plurals"));
    }

    // Mirrors docs/compiler/goals/abstract-entity.md#fan-out-hints: a member whose
    // name is the heading of the document naming it and other members is the
    // level's namesake node.
    #[test]
    fn fan_out_hints_name_the_namesake_of_a_document() {
        let mut s = levels_store();
        let text = "# Shipment\nA shipment moves.\n";
        s.docs.insert(
            "shipping.md".into(),
            crate::model::DocRecord {
                content_hash: crate::model::hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: Default::default(),
            },
        );
        let goals = AbstractEntity.derive_goals(&s, &gen_settings());
        let node = goals.iter().find(|g| g.target == "ent:backend").unwrap();
        assert!(
            node.hints.contains(
                &"namesake ent:shipment (shipping.md is headed Shipment; 2 other member(s) of the level from it)"
                    .to_string()
            ),
            "{:?}",
            node.hints
        );
        assert!(!node.hints.iter().any(|h| h.starts_with("namesake ent:cart")));
    }

    // Mirrors docs/compiler/goals/derive-criteria.md#created-when: one goal per leading
    // entity whose requirements owe criteria, only under a configured deliverable, and
    // bind waits on the criterion (reconciler.md#tiers).
    #[test]
    fn derive_criteria_goals_follow_the_leading_entity_and_the_deliverable() {
        let mut s = Store::default();
        s.graph.entities.insert("ent:sort".into(), Entity { name: "Sort".into(), ..Default::default() });
        s.graph.entities.insert("ent:line".into(), Entity { name: "Line".into(), ..Default::default() });
        for (id, st, ents) in [
            ("req:a-1", "The utility sorts its lines.", vec!["ent:sort", "ent:line"]),
            ("req:a-2", "A line is trimmed.", vec!["ent:line"]),
            ("req:a-3", "The utility reads STDIN.", vec!["ent:sort"]),
        ] {
            s.graph.requirements.insert(
                id.into(),
                Requirement {
                    statement: st.into(),
                    entities: ents.iter().map(|e| e.to_string()).collect(),
                    ..Default::default()
                },
            );
        }
        s.graph.criteria.insert(
            "crit:a-1".into(),
            crate::model::Criterion {
                requirement: "req:a-3".into(),
                statement: "Two lines on STDIN are both read.".into(),
                kind: "positive".into(),
                provenance: Some(Provenance::Derived { from: vec!["req:a-3".into()], reasoning: "the input".into() }),
                stale: true,
                ..Default::default()
            },
        );
        let mut gen = gen_settings();
        gen.configured = false;
        assert!(DeriveCriteria.derive_goals(&s, &gen).is_empty());
        gen.configured = true;
        let goals = DeriveCriteria.derive_goals(&s, &gen);
        let mut targets: Vec<&str> = goals.iter().map(|g| g.target.as_str()).collect();
        targets.sort();
        assert_eq!(targets, vec!["ent:line", "ent:sort"], "{:?}", targets);
        let sort = goals.iter().find(|g| g.target == "ent:sort").unwrap();
        assert_eq!(sort.change["none"], json!(["req:a-1"]));
        assert_eq!(sort.change["revised"], json!(["req:a-3"]));
        assert!(sort.hints.iter().any(|h| h == "skill criteria"));
        assert_eq!(skills_for("derive-criteria", &s, "ent:sort"), vec!["criteria"]);
        assert_eq!(tier("derive-criteria"), Some(3));
    }

    // Mirrors docs/compiler/model/entity.md#fields: a word match a review judged stays
    // off the unreferenced list.
    #[test]
    fn judged_word_matches_leave_the_unreferenced_list() {
        let mut s = Store::default();
        s.graph.entities.insert(
            "ent:customer".into(),
            Entity {
                name: "Customer".into(),
                not_about: vec!["req:a".into()],
                ..Default::default()
            },
        );
        for (id, st) in [("req:a", "An Order names the Customer who placed it."), ("req:b", "A Customer receives a receipt.")] {
            s.graph.requirements.insert(
                id.into(),
                Requirement {
                    statement: st.into(),
                    entities: vec!["ent:order".into()],
                    ..Default::default()
                },
            );
        }
        let ids: Vec<String> = unreferenced_statements(&s, "ent:customer").into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec!["req:b".to_string()]);
    }

    // Mirrors docs/compiler/goals/reconcile-section.md#created-when: a bare heading's
    // dirt derives no goal, the sections with a body derive theirs.
    #[test]
    fn a_dirty_bare_heading_derives_no_reconcile_goal() {
        let mut s = Store::default();
        let text = "# Backend\n\n## API Endpoints\n\n## Database\n\nThe backend uses PostgreSQL.\n";
        s.docs.insert(
            "backend.md".into(),
            crate::model::DocRecord {
                content_hash: crate::model::hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: Default::default(),
            },
        );
        let mut batch = store::RecordBatch::new(1);
        for sec in ["/backend", "/backend/api-endpoints", "/backend/database"] {
            batch.push(
                0,
                store::CHANGE_SECTION_DIRTY,
                &format!("backend.md#{}", sec),
                "edit",
                json!({"added": 1}),
            );
        }
        for r in batch.records() {
            s.status.record_change(r.clone());
        }
        s.status.generation = 1;
        let goals = ReconcileSection.derive_goals(&s, &gen_settings());
        let targets: Vec<&str> = goals.iter().map(|g| g.target.as_str()).collect();
        assert_eq!(targets, vec!["backend.md#/backend/database"], "{:?}", targets);
    }

    // Mirrors docs/compiler/compilation.md#coverage: a code block follows its parent's
    // coverage; a diagram owes its own.
    #[test]
    fn a_code_block_section_owes_no_coverage_of_its_own() {
        let text = "# Doc\n\nprose\n\n```rust\nfn main() {}\n```\n\n```plantuml\n@startuml\nA -> B\n@enduml\n```\n";
        let sections = crate::md::parse_sections(text);
        let block = sections.values().find(|s| s.kind == "code-block").expect("code-block");
        assert!(!section_has_body(block));
        let diagram = sections.values().find(|s| s.kind == "diagram").expect("diagram");
        assert!(section_has_body(diagram));
        // A preamble's whole text is its body: one line of prose before the first
        // heading owes its mark; a heading alone owes none.
        let sections = crate::md::parse_sections("The system shall respond.\n\n# Doc\n");
        assert_eq!(sections["/"].kind, "preamble");
        assert!(section_has_body(&sections["/"]));
        assert_eq!(sections["/doc"].kind, "root");
        assert!(!section_has_body(&sections["/doc"]));
    }

    fn grouping(id: &str, parent: Option<&str>, from: &[&str]) -> Op {
        Op::CreateEntity {
            id: id.into(),
            entity: Entity {
                name: id.trim_start_matches("ent:").into(),
                definition: Some("Holds the ordering area of the backend.".into()),
                parent: parent.map(String::from),
                provenance: Some(Provenance::Derived {
                    from: from.iter().map(|f| f.to_string()).collect(),
                    reasoning: "the documents treat these as one area".into(),
                }),
                ..Default::default()
            },
        }
    }

    fn reparent(id: &str, parent: &str) -> Op {
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

    fn rules(v: &[Violation]) -> Vec<&str> {
        v.iter().map(|x| x.rule.as_str()).collect()
    }

    #[test]
    fn fan_out_derivation_at_a_node_and_at_the_scope_root() {
        let s = levels_store();
        let goals = AbstractEntity.derive_goals(&s, &gen_settings());
        let ids: Vec<&str> = goals.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "g:abstract-entity:ent:backend",
                "g:abstract-entity:scope:public"
            ]
        );
        let node = &goals[0];
        assert!(!node.mandatory);
        assert_eq!(node.change["fan_out"], 12);
        assert_eq!(node.change["limit"], json!({"soft": 9, "hard": 15}));
        assert!(node.change.get("limits").is_none());
        let cands = node.change["candidates"].as_array().unwrap();
        assert_eq!(
            cands[0],
            json!(["ent:cart", "ent:checkout", "ent:discount", "ent:pricing"])
        );
        assert_eq!(
            cands[1],
            json!(["ent:carrier", "ent:shipment", "ent:tracking"])
        );
        assert_eq!(cands[2], json!(["ent:mailer", "ent:queue"]));
        assert!(node.hints.contains(&"load ent:backend".to_string()));
        assert!(node
            .hints
            .contains(&"fan-out 12 > 9 (children-per-entity, soft 9, hard 15)".to_string()));
        assert!(node.hints.contains(
            &"candidate [ent:cart, ent:checkout, ent:discount, ent:pricing] (weight 6)".to_string()
        ));
        assert!(node
            .hints
            .contains(&"member ent:cart «component» (orders.md)".to_string()));
        assert!(node
            .hints
            .contains(&"grouping ent:cart (1 children)".to_string()));
        assert!(!node.hints.iter().any(|h| h.starts_with("child ")));
        assert_eq!(
            node.hints.last().map(String::as_str),
            Some("skill abstraction; group_entities, update_entity, dissolve_entity")
        );
        let root = &goals[1];
        assert_eq!(root.target, "scope:public");
        assert_eq!(root.change["fan_out"], 10);
        assert_eq!(root.change["candidates"].as_array().unwrap().len(), 0);
        assert!(root.hints.contains(&"load scope:public".to_string()));
        assert!(root
            .hints
            .contains(&"member ent:backend «component» (arch.md)".to_string()));
        assert!(root
            .hints
            .contains(&"grouping ent:backend (12 children)".to_string()));
        let pack = AbstractEntity.pack(&s, &goals);
        assert!(pack.contains("- scope:public full\n"));
        assert!(pack.contains("- ent:backend full\n"));
        assert!(pack.contains("- ent:top-0 stub\n"));
        assert!(pack.contains("- ent:cart-item stub\n"));
        assert!(!pack.contains("arch.md#/x full"));

        // Over hard the goal is mandatory; a caps record on the same node folds in.
        let mut s = levels_store();
        fan_out_record(&mut s, "ent:backend", 16);
        threshold_record(&mut s, "ent:cart", "requirements-per-entity", 54);
        fan_out_record(&mut s, "ent:cart", 10);
        let goals = AbstractEntity.derive_goals(&s, &gen_settings());
        let backend = goals.iter().find(|g| g.target == "ent:backend").unwrap();
        assert!(backend.mandatory);
        let cart = goals.iter().find(|g| g.target == "ent:cart").unwrap();
        assert_eq!(cart.change["fan_out"], 10);
        assert_eq!(cart.change["limits"][0]["limit"], "requirements-per-entity");
        assert!(cart
            .hints
            .contains(&"54 > 50 (requirements-per-entity, soft 50, hard 80)".to_string()));
        assert!(cart
            .hints
            .contains(&"child ent:cart-item (1 requirements)".to_string()));
        assert!(AbstractEntity
            .pack(&s, &goals)
            .contains("- orders.md#/x full\n"));
    }

    #[test]
    fn coupling_hints_deterministic_and_bounded() {
        let s = levels_store();
        let a = coupling_candidates(&s, "ent:backend", 9);
        let b = coupling_candidates(&s, "ent:backend", 9);
        assert_eq!(a, b);
        assert_eq!(a[0].weight, 6);
        assert_eq!(a[1].weight, 3);
        assert_eq!(a[2].weight, 1);
        // The stragglers nothing touches are never merged at zero weight, and a
        // singleton is no candidate.
        assert_eq!(a.len(), 3);
        assert!(a.iter().all(|c| c.ids.len() >= 2 && c.rest == 0));
        // Descendants lift: the cart item's requirement counted for cart.
        let w = coupling_weights(&s, &level_members(&s, "ent:backend"));
        assert_eq!(w[&("ent:cart".to_string(), "ent:pricing".to_string())], 3);
        assert_eq!(w[&("ent:cart".to_string(), "ent:checkout".to_string())], 1);
        // Bounded: a level of thirty peers in one chain yields at most the soft
        // threshold of clusters and twelve ids per cluster with the rest counted.
        let mut s = Store::default();
        for i in 0..30 {
            s.graph.entities.insert(
                format!("ent:n{:02}", i),
                child(&format!("n {}", i), Some("ent:hub"), "hub.md"),
            );
        }
        s.graph
            .entities
            .insert("ent:hub".into(), child("Hub", None, "hub.md"));
        for i in 0..29 {
            link(
                &mut s,
                i,
                &format!("ent:n{:02}", i),
                &format!("ent:n{:02}", i + 1),
            );
        }
        let c = coupling_candidates(&s, "ent:hub", 9);
        assert!(c.len() <= 9);
        assert!(c.iter().all(|x| x.ids.len() <= FAN_OUT_CANDIDATE_IDS));
        // A candidate must itself fit under the limit: no cluster past the soft
        // threshold, so the chain yields several candidates, never one whole.
        assert!(c.iter().all(|x| x.ids.len() + x.rest <= 4), "{:?}", c);
        assert!(c.len() >= 4, "{:?}", c);
        let total: usize = c.iter().map(|x| x.ids.len() + x.rest).sum();
        assert!((28..=30).contains(&total), "{}", total);
        assert_eq!(c, coupling_candidates(&s, "ent:hub", 9));
        // Ties break by id: two equal-weight pairs merge the one whose ids sort first.
        let mut s = Store::default();
        for n in ["a", "b", "c", "d"] {
            s.graph
                .entities
                .insert(format!("ent:{}", n), child(n, Some("ent:hub"), "hub.md"));
        }
        s.graph
            .entities
            .insert("ent:hub".into(), child("Hub", None, "hub.md"));
        link(&mut s, 1, "ent:c", "ent:d");
        link(&mut s, 2, "ent:a", "ent:b");
        let c = coupling_candidates(&s, "ent:hub", 1);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].ids, vec!["ent:a", "ent:b"]);
    }

    #[test]
    fn fan_out_gate_accepts_a_valid_grouping() {
        let s = levels_store();
        let members = ["ent:cart", "ent:pricing", "ent:checkout", "ent:discount"];
        let mut staged = vec![grouping("ent:orders", Some("ent:backend"), &members)];
        staged.extend(members.iter().map(|m| reparent(m, "ent:orders")));
        // Twelve minus four plus one: nine, at the soft threshold.
        assert_eq!(fan_out_gates(&s, &staged), Vec::new());
        assert_eq!(AbstractEntity.gates(&s, &staged), Vec::new());
        // A top-level grouping at the scope root: ten minus two plus one.
        let staged = vec![
            grouping("ent:tops", None, &["ent:top-0", "ent:top-1"]),
            reparent("ent:top-0", "ent:tops"),
            reparent("ent:top-1", "ent:tops"),
        ];
        assert_eq!(fan_out_gates(&s, &staged), Vec::new());
        // A caps split with requirements re-pointed is not a grouping: one child
        // under it and a statement moved passes the fan-out gate untouched.
        let mut s = s;
        s.status.changes.clear();
        let staged = vec![
            grouping(
                "ent:cart-pricing",
                Some("ent:cart"),
                &["ent:cart", "req:x-1"],
            ),
            Op::UpdateRequirement {
                id: "req:x-1".into(),
                statement: None,
                entities: Some(vec!["ent:cart-pricing".into(), "ent:pricing".into()]),
                edges: None,
                transition: None,
                facets: None,
                source: None,
                provenance: None,
            },
        ];
        assert_eq!(fan_out_gates(&s, &staged), Vec::new());
    }

    #[test]
    fn fan_out_gate_rejects_a_one_member_grouping() {
        let s = levels_store();
        let staged = vec![
            grouping("ent:orders", Some("ent:backend"), &["ent:cart"]),
            reparent("ent:cart", "ent:orders"),
        ];
        let v = fan_out_gates(&s, &staged);
        assert!(rules(&v).contains(&"grouping-too-small"), "{:?}", v);
        assert!(v[0].message.contains("ent:orders"));
        // The level stays over the limit too: twelve minus one plus one.
        assert!(rules(&v).contains(&"fan-out-over-limit"), "{:?}", v);
        assert_eq!(
            rules(&AbstractEntity.gates(&s, &staged))[0],
            "grouping-too-small"
        );
        // Provenance naming other than the members, and a missing definition.
        let mut staged = vec![grouping(
            "ent:orders",
            Some("ent:backend"),
            &["ent:cart", "ent:pricing", "ent:backend"],
        )];
        staged.extend(
            ["ent:cart", "ent:pricing", "ent:checkout", "ent:discount"]
                .iter()
                .map(|m| reparent(m, "ent:orders")),
        );
        if let Op::CreateEntity { entity, .. } = &mut staged[0] {
            entity.definition = None;
        }
        let v = fan_out_gates(&s, &staged);
        assert!(rules(&v).contains(&"provenance-mismatch"), "{:?}", v);
        assert!(rules(&v).contains(&"definition-required"), "{:?}", v);
        // Grouping only two of twelve leaves the level over soft.
        let staged = vec![
            grouping(
                "ent:mail",
                Some("ent:backend"),
                &["ent:queue", "ent:mailer"],
            ),
            reparent("ent:queue", "ent:mail"),
            reparent("ent:mailer", "ent:mail"),
        ];
        let v = fan_out_gates(&s, &staged);
        assert_eq!(rules(&v), vec!["fan-out-over-limit"]);
        assert!(v[0]
            .message
            .starts_with("ent:backend: 11 children after the changeset, over 9"));
        // Over hard the limit in force is hard: sixteen to fifteen passes.
        let mut s = levels_store();
        fan_out_record(&mut s, "ent:backend", 16);
        assert_eq!(fan_out_gates(&s, &staged), Vec::new());
    }

    #[test]
    fn fan_out_gate_rejects_a_cross_level_grouping() {
        let s = levels_store();
        // A member from the scope root beside members of the backend's level.
        let members = ["ent:cart", "ent:pricing", "ent:top-0"];
        let mut staged = vec![grouping("ent:orders", Some("ent:backend"), &members)];
        staged.extend(members.iter().map(|m| reparent(m, "ent:orders")));
        let v = fan_out_gates(&s, &staged);
        assert_eq!(rules(&v)[0], "cross-level", "{:?}", v);
        assert_eq!(rules(&v).iter().filter(|r| **r == "cross-level").count(), 1);
        assert!(v[0]
            .message
            .contains("ent:top-0 sits under the scope root while the grouping takes ent:backend"));
        // The grouping's own parent is not the members' parent either.
        let members = ["ent:cart", "ent:pricing"];
        let mut staged = vec![grouping("ent:orders", None, &members)];
        staged.extend(members.iter().map(|m| reparent(m, "ent:orders")));
        let v = fan_out_gates(&s, &staged);
        assert_eq!(&rules(&v)[..2], &["cross-level", "cross-level"], "{:?}", v);
        // A member of another scope.
        let mut s = levels_store();
        s.graph.entities.get_mut("ent:pricing").unwrap().scope = "internal".into();
        let members = ["ent:cart", "ent:pricing"];
        let mut staged = vec![grouping("ent:orders", Some("ent:backend"), &members)];
        staged.extend(members.iter().map(|m| reparent(m, "ent:orders")));
        let v = fan_out_gates(&s, &staged);
        assert!(rules(&v).contains(&"scope-mismatch"), "{:?}", v);
        // A dissolve lands the children back on the level: the count check sees them.
        let mut s = levels_store();
        s.graph.entities.insert(
            "ent:mail".into(),
            Entity {
                name: "mail".into(),
                parent: Some("ent:backend".into()),
                provenance: Some(Provenance::Derived {
                    from: vec!["ent:queue".into(), "ent:mailer".into()],
                    reasoning: "one area".into(),
                }),
                ..Default::default()
            },
        );
        for m in ["ent:queue", "ent:mailer"] {
            s.graph.entities.get_mut(m).unwrap().parent = Some("ent:mail".into());
        }
        fan_out_record(&mut s, "ent:backend", 11);
        let staged = vec![Op::DissolveEntity {
            id: "ent:mail".into(),
            reason: "one member left".into(),
            parent: None,
            children: Vec::new(),
        }];
        let v = fan_out_gates(&s, &staged);
        assert_eq!(rules(&v), vec!["fan-out-over-limit"], "{:?}", v);
        assert!(v[0].message.starts_with("ent:backend: 12 children"));
    }

    // ---- the pair degree cap ----

    // One broad statement picked by twelve created requirements. Every partner
    // shares the Gateway entity and two content tokens with the hub (`reject` and
    // one word of its own from the hub's list) and only `reject` with the other
    // partners, so the hub is each partner's sole neighbor: twelve pairs on one
    // requirement, eleven at score 3 and `req:p12` at score 4.
    // Two words each, since the verb every partner would otherwise repeat is the
    // gateway's boilerplate and counts for nothing (reconciler.md#pairs).
    pub(crate) const HUB_WORDS: [&str; 12] = [
        "drops expired tokens",
        "logs refusals",
        "alerts operators",
        "counts failures",
        "throttles clients",
        "audits sessions",
        "blocks replays",
        "signs receipts",
        "rotates keys",
        "caches verdicts",
        "purges entries",
        "reports",
    ];

    pub(crate) fn fanned_store() -> Store {
        let mut s = Store::default();
        s.graph.entities.insert(
            "ent:gw".into(),
            Entity {
                name: "Gateway".into(),
                ..Default::default()
            },
        );
        let req = |statement: &str| Requirement {
            statement: statement.into(),
            entities: vec!["ent:gw".into()],
            ..Default::default()
        };
        s.graph.requirements.insert(
            "req:hub".into(),
            req("The gateway rejects expired tokens, logs each refusal, alerts operators, counts failures, throttles clients, audits sessions, blocks replays, signs receipts, rotates keys, caches verdicts, purges stale entries, and reports metrics."),
        );
        for (i, w) in HUB_WORDS.iter().enumerate() {
            let id = format!("req:p{:02}", i + 1);
            let statement = if i + 1 == 12 {
                "The gateway rejects reports and metrics.".to_string()
            } else {
                format!("The gateway {}.", w)
            };
            s.graph.requirements.insert(id.clone(), req(&statement));
            let mut batch = store::RecordBatch::new(5);
            batch.push(0, store::CHANGE_REQ_CREATED, &id, "section", Value::Null);
            for r in batch.records() {
                s.status.record_change(r.clone());
            }
        }
        s.status.generation = 5;
        s
    }

    fn pair_id(p: &str) -> String {
        goal_id("rejudge-pair", &pair_target("req:hub", p))
    }

    // (a) Twelve partners open eight and wait four: the best score first, then by
    // target; the hub's open degree is the cap; the hints carry the score and the
    // waiting count per side. Mirrors docs/compiler/reconciler.md#pairs.
    #[test]
    fn a_requirement_picked_by_twelve_opens_eight_and_waits_four() {
        let s = fanned_store();
        let plan = pair_plan(&s);
        let open: Vec<&str> = plan.open.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(open.len(), limits::PAIRS_PER_REQUIREMENT_CAP, "{:?}", open);
        assert_eq!(plan.waiting.len(), 4, "{:?}", plan.waiting.keys().collect::<Vec<_>>());
        // The score-4 pair opens ahead of the score-3 ones whatever its id says.
        assert!(open.contains(&pair_id("req:p12").as_str()), "{:?}", open);
        for p in ["req:p01", "req:p07"] {
            assert!(open.contains(&pair_id(p).as_str()), "{:?}", open);
        }
        for p in ["req:p08", "req:p09", "req:p10", "req:p11"] {
            let w = plan.waiting.get(&pair_id(p)).unwrap_or_else(|| panic!("{} waits", p));
            assert_eq!(w.score, 3);
            assert_eq!(w.full, vec!["req:hub".to_string()]);
            assert_eq!(w.cause.generation, 5);
        }
        let degree = plan
            .open
            .iter()
            .filter(|g| pair_members(&g.target).is_some_and(|(a, b)| a == "req:hub" || b == "req:hub"))
            .count();
        assert_eq!(degree, limits::PAIRS_PER_REQUIREMENT_CAP);
        let g = plan.open.iter().find(|g| g.id == pair_id("req:p01")).unwrap();
        assert!(
            g.hints
                .iter()
                .any(|h| h == "score 3; req:hub: 4 pair(s) waiting, req:p01: none waiting"),
            "{:?}",
            g.hints
        );
        let g12 = plan.open.iter().find(|g| g.id == pair_id("req:p12")).unwrap();
        assert!(g12.hints.iter().any(|h| h.starts_with("score 4;")), "{:?}", g12.hints);
        // The board lists the waiting pairs and explains one; none of them is a goal.
        let b = crate::board::Board::derive(
            &s,
            &crate::board::tests::project_for(&s),
            &crate::board::tests::control_auto(),
        );
        assert_eq!(b.waiting.len(), 4);
        assert!(b.goal(&pair_id("req:p08")).is_none());
        assert!(b.waiting_pair("req:hub~req:p08").is_some());
        let text = b.explain(&s, &pair_id("req:p08")).expect("explains a waiting pair");
        assert!(text.contains("waiting under the cap"), "{}", text);
        assert!(text.contains("req:hub hold(s) 8 open pairs"), "{}", text);
        assert!(b.summary_line().contains("4 pair(s) waiting under the cap"));
    }

    // (c) A pinned pair opens even when its side is saturated: an open contradiction
    // ties the hub to req:p11, which would otherwise wait, and the slot it takes
    // sends req:p07 to waiting instead. Mirrors docs/compiler/reconciler.md#pairs.
    #[test]
    fn a_pinned_pair_opens_on_a_saturated_side() {
        let mut s = fanned_store();
        s.graph.diagnostics.insert(
            "diag:contradiction-1".into(),
            Diagnostic {
                rule: "contradiction".into(),
                severity: "error".into(),
                subjects: vec!["req:hub".into(), "req:p11".into()],
                message: "cannot both hold".into(),
                reasoning: None,
                lifecycle: "open".into(),
                triage: None,
                prompt: None,
                answer: None,
                created: None,
                updated: None,
            },
        );
        let plan = pair_plan(&s);
        let open: Vec<&str> = plan.open.iter().map(|g| g.id.as_str()).collect();
        assert!(open.contains(&pair_id("req:p11").as_str()), "{:?}", open);
        assert!(open.contains(&pair_id("req:p12").as_str()), "{:?}", open);
        assert_eq!(open.len(), limits::PAIRS_PER_REQUIREMENT_CAP, "{:?}", open);
        assert!(plan.waiting.contains_key(&pair_id("req:p07")));
        assert!(!plan.waiting.contains_key(&pair_id("req:p11")));
        // A failed pair is pinned too and holds no slot: failing req:p08's pair
        // keeps it on the board without costing req:p07 its slot back.
        s.status.failed.push(FailedGoal {
            goal: plan
                .open
                .iter()
                .find(|g| g.id == pair_id("req:p01"))
                .cloned()
                .map(|mut g| {
                    g.id = pair_id("req:p08");
                    g.target = pair_target("req:hub", "req:p08");
                    g
                })
                .unwrap(),
            reason: "undecidable".into(),
        });
        let plan = pair_plan(&s);
        let open: Vec<&str> = plan.open.iter().map(|g| g.id.as_str()).collect();
        assert!(open.contains(&pair_id("req:p08").as_str()), "{:?}", open);
        assert_eq!(open.len(), limits::PAIRS_PER_REQUIREMENT_CAP + 1, "{:?}", open);
        assert!(plan.waiting.contains_key(&pair_id("req:p07")), "{:?}", plan.waiting.keys());
    }

    // (e) A judged partner derives no pair from the record that lists it; a new
    // record on the same requirement starts with an empty `judged`, so the pair
    // derives again with the new cause. Mirrors docs/compiler/graph.md#change-records.
    #[test]
    fn a_re_revised_requirement_starts_with_an_empty_judged_list() {
        let mut s = fanned_store();
        for c in s.status.changes.iter_mut().filter(|c| c.subject == "req:p01") {
            c.add_judged("req:hub");
            c.add_judged("req:hub");
            assert_eq!(c.judged(), vec!["req:hub".to_string()]);
        }
        let plan = pair_plan(&s);
        assert!(plan.open.iter().all(|g| g.id != pair_id("req:p01")));
        assert!(!plan.waiting.contains_key(&pair_id("req:p01")));
        // The slot freed opens the next pair by score and target.
        assert!(plan.open.iter().any(|g| g.id == pair_id("req:p08")));
        // Revised again: the new record supersedes nothing of the created kind but
        // carries an empty list, and the pair derives from it with the new cause.
        let mut batch = store::RecordBatch::new(6);
        batch.push(0, store::CHANGE_REQ_REVISED, "req:p01", "fields", Value::Null);
        for r in batch.records() {
            s.status.record_change(r.clone());
        }
        let revised = s
            .status
            .changes
            .iter()
            .find(|c| c.kind == store::CHANGE_REQ_REVISED && c.subject == "req:p01")
            .unwrap();
        assert!(revised.judged().is_empty());
        let plan = pair_plan(&s);
        let g = plan
            .open
            .iter()
            .find(|g| g.id == pair_id("req:p01"))
            .expect("the pair derives again");
        assert_eq!(g.cause.as_ref().map(|c| c.generation), Some(6));
        assert_eq!(g.change["revised"], "req:p01");
        // A second revision supersedes the first record; its list is empty again.
        for c in s.status.changes.iter_mut().filter(|c| c.subject == "req:p01") {
            c.add_judged("req:hub");
        }
        let mut batch = store::RecordBatch::new(7);
        batch.push(0, store::CHANGE_REQ_REVISED, "req:p01", "fields", Value::Null);
        for r in batch.records() {
            s.status.record_change(r.clone());
        }
        let revised: Vec<&ChangeRecord> = s
            .status
            .changes
            .iter()
            .filter(|c| c.kind == store::CHANGE_REQ_REVISED && c.subject == "req:p01")
            .collect();
        assert_eq!(revised.len(), 1);
        assert_eq!(revised[0].generation, 7);
        assert!(revised[0].judged().is_empty());
    }

    // ---- verdicts are remembered ----

    // A store on disk holding "backend" (api.md) and "backend system" (deploy.md): a
    // lookalike pair at the threshold.
    fn lookalike_store(label: &str) -> Store {
        let out = std::env::temp_dir().join(format!("jazyk-lookalikes-{}-{}", label, crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        crate::vfs::create_dir_all(&out).unwrap();
        let mut s = Store {
            out,
            ..Default::default()
        };
        // Each mention is backed by its document, so the sweep keeps the entities.
        for (doc, text) in [
            ("api.md", "# X\n\nThe backend serves requests. The backend system is deployed nightly.\n"),
            ("deploy.md", "# X\n\nThe backend system is deployed nightly.\n"),
        ] {
            s.docs.insert(
                doc.to_string(),
                DocRecord {
                    content_hash: hash_hex(text),
                    sections: crate::md::parse_sections(text),
                    coverage: BTreeMap::new(),
                },
            );
        }
        let ent = |name: &str, docs: &[&str]| Entity {
            name: name.into(),
            mentions: docs
                .iter()
                .map(|d| SourceRef {
                    doc: d.to_string(),
                    section: "/x".into(),
                    quote: name.into(),
                })
                .collect(),
            ..Default::default()
        };
        s.graph.entities.insert("ent:backend".into(), ent("backend", &["api.md"]));
        s.graph.entities.insert("ent:backend-system".into(), ent("backend system", &["deploy.md"]));
        s
    }

    fn session_resolving(goal: &str, evidence: Value) -> store::Commit {
        store::Commit {
            kind: "session".into(),
            batch: vec![goal.to_string()],
            resolved: vec![Resolved {
                goal: goal.to_string(),
                justification: "judged".into(),
                evidence,
            }],
            ..Default::default()
        }
    }

    const PAIR: &str = "ent:backend~ent:backend-system";
    const DEDUPE: &str = "g:dedupe-candidates:ent:backend~ent:backend-system";

    // The name index writes the pair's record at commit; the goal derives with that
    // record as its cause. Resolving it `separate` writes the entry at the pair's
    // score, clears the record, and the pair derives nothing more: a no-op rebuild
    // derives zero goals, from the store in memory and from disk.
    // Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
    #[test]
    fn a_separate_dedupe_verdict_is_remembered_and_the_pair_is_not_asked_again() {
        let mut s = lookalike_store("dedupe");
        s.apply(vec![], &store::Commit::store("edit"));
        let record = s.status.changes.iter().find(|c| c.kind == CHANGE_LOOKALIKE).expect("a lookalike record");
        assert_eq!((record.subject.as_str(), record.generation), (PAIR, 1));
        assert_eq!(record.detail["score"], 0.5);
        let goals = DedupeCandidates.derive_goals(&s, &gen_settings());
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].id, DEDUPE);
        assert_eq!(goals[0].cause.as_ref().map(|c| c.generation), Some(1));

        s.apply(vec![], &session_resolving(DEDUPE, json!("separate")));
        assert_eq!(
            s.status.lookalikes,
            vec![LookalikeVerdict {
                pair: PAIR.into(),
                verdict: "separate".into(),
                score: 0.5,
                generation: 2
            }]
        );
        assert!(!s.status.has_change(CHANGE_LOOKALIKE, PAIR));
        assert!(DedupeCandidates.derive_goals(&s, &gen_settings()).is_empty());
        // A later commit writes no record and derives nothing; the list persists.
        s.apply(vec![], &store::Commit::store("edit"));
        assert!(!s.status.has_change(CHANGE_LOOKALIKE, PAIR));
        let loaded = Store::load(&s.out);
        assert_eq!(loaded.status.lookalikes, s.status.lookalikes);
        assert!(DedupeCandidates.derive_goals(&loaded, &gen_settings()).is_empty());
        // The `{verdict}` shape counts too.
        let mut t = lookalike_store("dedupe-shape");
        t.apply(vec![], &session_resolving(DEDUPE, json!({"verdict": "separate"})));
        assert_eq!(t.status.lookalikes.len(), 1);
        crate::vfs::remove_dir_all(&s.out).ok();
        crate::vfs::remove_dir_all(&t.out).ok();
    }

    // A review-entity session that kept a lookalike apart in its evidence writes the
    // same entry, so dedupe-candidates never asks about the pair. A merged verdict
    // writes nothing: the merge itself takes the pair away.
    // Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
    #[test]
    fn a_separate_review_verdict_suppresses_the_dedupe_goal() {
        let mut s = lookalike_store("review");
        let evidence = json!({
            "definition": "current",
            "lookalikes": [
                {"id": "ent:backend-system", "verdict": "separate", "reason": "a deployment unit"},
            ],
        });
        s.apply(vec![], &session_resolving("g:review-entity:ent:backend", evidence));
        assert_eq!(s.status.lookalikes.len(), 1, "{:?}", s.status.lookalikes);
        assert_eq!((s.status.lookalikes[0].pair.as_str(), s.status.lookalikes[0].score), (PAIR, 0.5));
        assert!(!s.status.has_change(CHANGE_LOOKALIKE, PAIR));
        assert!(DedupeCandidates.derive_goals(&s, &gen_settings()).is_empty());
        let mut t = lookalike_store("review-merged");
        let evidence = json!({"definition": "current", "lookalikes": [{"id": "ent:backend-system", "verdict": "merged"}]});
        t.apply(vec![], &session_resolving("g:review-entity:ent:backend", evidence));
        assert!(t.status.lookalikes.is_empty());
        crate::vfs::remove_dir_all(&s.out).ok();
        crate::vfs::remove_dir_all(&t.out).ok();
    }

    // The score moves (a new mention document here): the entry drops at that commit,
    // the name index writes a fresh record, and the pair is judged again with the new
    // commit as its cause. Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
    #[test]
    fn a_moved_score_drops_the_entry_and_reopens_the_pair() {
        let mut s = lookalike_store("moved");
        s.apply(vec![], &session_resolving(DEDUPE, json!("separate")));
        assert_eq!(s.status.lookalikes.len(), 1);
        // The same evidence again: nothing moves.
        s.apply(vec![], &store::Commit::store("edit"));
        assert_eq!(s.status.lookalikes.len(), 1);
        assert!(DedupeCandidates.derive_goals(&s, &gen_settings()).is_empty());
        s.graph.entities.get_mut("ent:backend-system").unwrap().mentions.push(SourceRef {
            doc: "api.md".into(),
            section: "/x".into(),
            quote: "backend system".into(),
        });
        s.apply(vec![], &store::Commit::store("edit"));
        assert!(s.status.lookalikes.is_empty(), "{:?}", s.status.lookalikes);
        let record = s.status.changes.iter().find(|c| c.kind == CHANGE_LOOKALIKE).expect("a fresh record");
        assert_eq!((record.subject.as_str(), record.generation), (PAIR, 3));
        assert_eq!(record.detail["documents"], 0.5);
        let goals = DedupeCandidates.derive_goals(&s, &gen_settings());
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].change["score"], 0.63);
        assert_eq!(goals[0].cause.as_ref().map(|c| c.generation), Some(3));
        crate::vfs::remove_dir_all(&s.out).ok();
    }

    // A merge takes one id with it: the entry and any record drop at the same commit.
    // Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
    #[test]
    fn a_merge_drops_the_entry_and_the_record() {
        let absorb = || Op::MergeEntities {
            keep: "ent:backend".into(),
            absorb: "ent:backend-system".into(),
            reason: "one concept".into(),
        };
        let mut s = lookalike_store("merge");
        s.apply(vec![], &session_resolving(DEDUPE, json!("separate")));
        assert_eq!(s.status.lookalikes.len(), 1);
        let report = s.apply(vec![absorb()], &store::Commit::store("decree"));
        assert_eq!(report.applied, 1, "{:?}", report.skipped);
        assert!(s.status.lookalikes.is_empty(), "{:?}", s.status.lookalikes);
        assert!(!s.status.changes.iter().any(|c| c.kind == CHANGE_LOOKALIKE));
        assert!(DedupeCandidates.derive_goals(&s, &gen_settings()).is_empty());
        // A record with no verdict drops the same way.
        let mut t = lookalike_store("merge-record");
        t.apply(vec![], &store::Commit::store("edit"));
        assert!(t.status.has_change(CHANGE_LOOKALIKE, PAIR));
        t.apply(vec![absorb()], &store::Commit::store("decree"));
        assert!(!t.status.has_change(CHANGE_LOOKALIKE, PAIR));
        crate::vfs::remove_dir_all(&s.out).ok();
        crate::vfs::remove_dir_all(&t.out).ok();
    }

    // The token index names every pair the pairwise scan scores, in the scan's order,
    // with the scan's scores. Mirrors docs/compiler/graph.md#derived-data.
    #[test]
    fn the_lookalike_token_index_matches_the_pairwise_scan() {
        let mut synthetic = crate::derive::tests::synthetic_store(300, 700);
        let mut batch = store::RecordBatch::new(1);
        crate::derive::recompute(&mut synthetic, "g1", &mut batch);
        let mut s = Store::default();
        let ent = |name: &str, docs: &[&str]| Entity {
            name: name.into(),
            mentions: docs
                .iter()
                .map(|d| SourceRef {
                    doc: d.to_string(),
                    section: "/x".into(),
                    quote: name.into(),
                })
                .collect(),
            ..Default::default()
        };
        s.graph.entities.insert("ent:backend".into(), ent("backend", &["api.md"]));
        s.graph.entities.insert("ent:backend-system".into(), ent("backend system", &["deploy.md"]));
        s.graph.entities.insert("ent:backend-host".into(), ent("backend host", &["api.md", "deploy.md"]));
        s.graph.entities.insert("ent:order".into(), ent("Order", &["api.md"]));
        s.graph.entities.insert("ent:reorder-point".into(), ent("Reorder point", &["deploy.md"]));
        for s in [crate::board::tests::settled_store(), synthetic, s] {
            let indexed = lookalike_scores(&s);
            assert_eq!(indexed, lookalike_scores_scan(&s));
        }
    }

    // The one-pass linked subjects list what the per-document scan lists, for every
    // document, in its order: one entry per entity through its first linking mention,
    // the requirement quotes after, twelve at most.
    #[test]
    fn linked_subjects_match_the_per_document_scan() {
        let mut s = Store::default();
        let ent = |name: &str, mentions: &[(&str, &str)]| Entity {
            name: name.into(),
            mentions: mentions
                .iter()
                .map(|(d, q)| SourceRef {
                    doc: d.to_string(),
                    section: "/x".into(),
                    quote: q.to_string(),
                })
                .collect(),
            ..Default::default()
        };
        // Order links to orders.md twice (the first mention counts), Cart links to
        // both, Stock names nothing it links, Ledger is linked only through a quote.
        s.graph.entities.insert(
            "ent:order".into(),
            ent("Order", &[("a.md", "An [Order](orders.md) is placed."), ("b.md", "The [Order](orders.md) again.")]),
        );
        s.graph.entities.insert(
            "ent:cart".into(),
            ent("Cart", &[("a.md", "The [Cart](cart.md) feeds the [order flow](orders.md).")]),
        );
        s.graph.entities.insert(
            "ent:stock".into(),
            ent("Stock", &[("a.md", "See [inventory](stock.md).")]),
        );
        s.graph.entities.insert("ent:ledger".into(), ent("Ledger", &[("orders.md", "Ledger")]));
        s.graph.requirements.insert(
            "req:a-1".into(),
            Requirement {
                statement: "The ledger records every order.".into(),
                entities: vec!["ent:ledger".into(), "ent:order".into()],
                source: Some(SourceRef {
                    doc: "a.md".into(),
                    section: "/a".into(),
                    quote: "The Ledger records every [Order](orders.md).".into(),
                }),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:o-1".into(),
            Requirement {
                statement: "Stock is reserved.".into(),
                entities: vec!["ent:stock".into()],
                source: Some(SourceRef {
                    doc: "orders.md".into(),
                    section: "/o".into(),
                    quote: "[Stock](stock.md) is reserved.".into(),
                }),
                ..Default::default()
            },
        );
        for st in [s, crate::board::tests::settled_store(), crate::derive::tests::synthetic_store(120, 300)] {
            let all = linked_subjects_all(&st);
            let mut docs: BTreeSet<String> = st.docs.keys().cloned().collect();
            docs.extend(all.keys().cloned());
            docs.extend(["orders.md".to_string(), "cart.md".to_string(), "stock.md".to_string(), "nope.md".to_string()]);
            for d in docs {
                assert_eq!(linked_subjects(&st, &d), linked_subjects_scan(&st, &d), "{}", d);
            }
        }
        let st = Store::default();
        assert!(linked_subjects_all(&st).is_empty());
    }
}
