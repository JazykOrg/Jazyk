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

pub static REGISTRY: [&dyn GoalKind; 16] = [
    &PlaceAnchors,
    &ReconcileSection,
    &RejudgePair,
    &ReviewEntity,
    &Retrace,
    &ConformInstance,
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
        "bind" | "generate" | "verify" | "ratify" | "answer" => Some(3),
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

// A section with a body of its own: the heading line alone carries no content.
pub fn section_has_body(sec: &Section) -> bool {
    !sec.raw.lines().skip(1).all(|l| l.trim().is_empty())
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
    const STOP: [&str; 30] = [
        "the", "a", "an", "shall", "to", "of", "in", "on", "for", "is", "are", "be", "or", "and",
        "if", "with", "by", "it", "its", "when", "then", "that", "which", "this", "system", "not",
        "no", "only", "all", "each",
    ];
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
        .filter(|t| !t.is_empty() && !STOP.contains(t))
        .map(stem)
        .filter(|t| !name_toks.contains(t))
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

fn open_diags_naming<'a>(store: &'a Store, ids: &[&str]) -> Vec<(&'a String, &'a Diagnostic)> {
    store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| {
            d.lifecycle == "open" && d.subjects.iter().any(|s| ids.contains(&s.as_str()))
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

// Requirements sourced in a section, and the entities mentioned there.
fn anchored_in(store: &Store, doc: &str, section: &str) -> (Vec<String>, Vec<String>) {
    let reqs: Vec<String> = store
        .graph
        .requirements
        .iter()
        .filter(|(_, r)| r.anchored_at(doc, section))
        .map(|(id, _)| id.clone())
        .collect();
    let ents: Vec<String> = store
        .graph
        .entities
        .iter()
        .filter(|(_, e)| {
            e.mentions
                .iter()
                .any(|m| m.doc == doc && m.section == section)
        })
        .map(|(id, _)| id.clone())
        .collect();
    (reqs, ents)
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
    let mut out: Vec<(String, String, String)> = Vec::new();
    for (id, e) in &store.graph.entities {
        for m in &e.mentions {
            if m.doc == doc {
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
                out.push((id, ent.name.clone(), format!("{}#{}", s.doc, s.section)));
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
            if record
                .coverage
                .get(&sec)
                .is_some_and(|c| claimed_after(&c.claimed_by, rec.generation))
            {
                continue;
            }
            let w = work.entry(rec.subject.clone()).or_default();
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
            let w = work.entry(rec.subject.clone()).or_default();
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
                let full = format!("{}#{}", src.doc, src.section);
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
            let (reqs, ents) = anchored_in(store, &doc, &sec_ref);
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
                reqs.len()
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
            let subjects = linked_subjects(store, &doc);
            match subjects.len() {
                0 => {}
                1 => hints.push(format!(
                    "primarySubject: {} ({})",
                    subjects[0].0, subjects[0].1
                )),
                _ => hints.push(format!(
                    "candidateSubjects: {}",
                    subjects
                        .iter()
                        .map(|(id, name, _)| format!("{} ({})", id, name))
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            }
            let mentioned = store
                .graph
                .entities
                .values()
                .filter(|e| e.mentions.iter().any(|m| m.doc == doc))
                .count();
            hints.push(format!(
                "{} entities mentioned in the document, {} in the graph; search before creating",
                mentioned,
                store.graph.entities.len()
            ));
            let _ = ents;
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
                        let sourced = store
                            .graph
                            .requirements
                            .values()
                            .any(|r| r.anchored_at(doc, section))
                            || staged.iter().any(|o| match o {
                                Op::CreateRequirement { requirement, .. } => {
                                    requirement.anchored_at(doc, section)
                                }
                                Op::UpdateRequirement {
                                    source: Some(s), ..
                                } => s.doc == *doc && s.section == *section,
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
                } => {
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
        struct PairWork {
            change: Value,
            cause: Cause,
            hints: Vec<String>,
        }
        let mut pairs: BTreeMap<String, PairWork> = BTreeMap::new();
        for rec in records_of(
            store,
            &[store::CHANGE_REQ_CREATED, store::CHANGE_REQ_REVISED],
        ) {
            let rid = &rec.subject;
            let Some(req) = store.graph.requirements.get(rid) else {
                continue;
            };
            let judged = str_list(&rec.detail["judged"]);
            for nbr in store.pair_review_neighbors(rid) {
                if judged.contains(&nbr) {
                    continue;
                }
                let Some(other) = store.graph.requirements.get(&nbr) else {
                    continue;
                };
                let target = pair_target(rid, &nbr);
                let shared_ent = shared_entity(store, req, other);
                let tokens: Vec<String> = content_tokens(store, &req.statement, &req.entities)
                    .intersection(&content_tokens(store, &other.statement, &other.entities))
                    .cloned()
                    .collect();
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
                        "yes (delete the worse-sourced duplicate)"
                    } else {
                        "no (keep both and file duplicate-requirement)"
                    }
                ));
                let diags = open_diags_naming(store, &[rid, &nbr]);
                if diags.is_empty() {
                    hints.push("no open diagnostic on the pair".into());
                } else {
                    for (id, d) in diags {
                        hints.push(format!(
                            "open {} ({}, {}): {}",
                            id,
                            d.rule,
                            d.severity,
                            truncate(&d.message, 100)
                        ));
                    }
                }
                hints.push(format!("load {}; load {}", rid, nbr));
                hints.push("skill judgment; duplicate: delete_requirement or report_diagnostic duplicate-requirement; contradiction: report_diagnostic with a prompt; consistent: mark_goal_done alone".into());
                match pairs.get_mut(&target) {
                    Some(p)
                        if (p.cause.generation, p.cause.mutation)
                            <= (cause.generation, cause.mutation) => {}
                    _ => {
                        pairs.insert(
                            target,
                            PairWork {
                                change,
                                cause,
                                hints,
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
            if pairs.contains_key(&target) {
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
                },
            );
        }
        pairs
            .into_iter()
            .map(|(target, p)| build_goal(self, &target, true, p.change, Some(p.cause), p.hints))
            .collect()
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
            "report_diagnostic",
            "resolve_diagnostic",
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
        if r.entities.iter().any(|x| store.resolve_id(x) == id) {
            continue;
        }
        let text = blank_code(&r.statement);
        if names.iter().any(|n| word_bounded(&text, n)) {
            out.push((rid.clone(), r.statement.clone()));
            if out.len() >= 6 {
                break;
            }
        }
    }
    out
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
                    "multi-entity statements without edges: {}",
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
            let mut parentless: Vec<String> = Vec::new();
            for r in &reqs {
                let Some(q) = store.graph.requirements.get(r) else {
                    continue;
                };
                for edge in &q.edges {
                    if edge.rel_type.as_deref() != Some("composition") {
                        continue;
                    }
                    let Some(part) = store.graph.entities.get(&edge.b) else {
                        continue;
                    };
                    let line = match &part.parent {
                        None => format!("{} -> {}", edge.a, edge.b),
                        Some(p) if p != &edge.a => {
                            format!("{} -> {} (parent now {})", edge.a, edge.b, p)
                        }
                        _ => continue,
                    };
                    if !parentless.contains(&line) {
                        parentless.push(line);
                    }
                }
            }
            if !parentless.is_empty() {
                hints.push(format!(
                    "composition edges whose part has no parent or a parent disagreeing with the stated whole (set parent on the part with update_entity): {}",
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
            hints.push(format!(
                "type declares {}",
                if declared.is_empty() {
                    "no attributes".to_string()
                } else {
                    declared.join(", ")
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
            let links: Vec<String> = store
                .graph
                .relationships
                .values()
                .flat_map(|r| r.contributions.iter())
                .filter(|c| c.a == id && c.r#type != INSTANTIATION)
                .map(|c| format!("{} ({})", c.b, c.r#type))
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
            if let Op::UpdateEntity { id, .. } = op {
                if types.contains(id) {
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
                Some(build_goal(
                    self,
                    &rid,
                    true,
                    json!({"goal": "bind", "reason": reason, "entity": entity}),
                    Some(ledger_cause(store, &rid)),
                    hints,
                ))
            })
            .collect()
    }
    fn ready(&self, _goal: &Goal, board: &Board) -> Ready {
        tier3_ready(board)
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
        crate::gen::pending(store, gen)
            .into_iter()
            .filter_map(|p| {
                let id = p["entity"].as_str()?.to_string();
                let reason = p["reason"].as_str().unwrap_or("").to_string();
                let changed = str_list(&p["changed"]);
                let mut hints = vec![format!("reason: {}", reason)];
                if !changed.is_empty() {
                    hints.push(format!("changed: {}", changed.join(", ")));
                }
                let bound: Vec<String> = crate::gen::reqs_of_sorted(store, &id)
                    .iter()
                    .filter_map(|r| ledger.requirements.get(r).map(|row| row.test.name.clone()))
                    .collect();
                if !bound.is_empty() {
                    hints.push(format!("bound tests to make pass: {}", bound.join(", ")));
                }
                if let Some(b) = &ledger.build {
                    hints.push(format!("build: {} (cwd {})", b.run, b.cwd));
                }
                let n = crate::gen::reqs_of_sorted(store, &id).len();
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
            .filter(|p| p["status"] != "unimplemented" && p["reason"] != "not-generated")
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

// The lookalike score over every candidate pair: two entities of one scope, neither
// an ancestor of the other, no structural contribution between them, spanning at
// least two mention documents. Mirrors docs/compiler/goals/dedupe-candidates.md.
pub fn lookalike_scores(store: &Store) -> Vec<Lookalike> {
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
    let ids: Vec<&String> = store.graph.entities.keys().collect();
    let mut out = Vec::new();
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            let (a, b) = (ids[i], ids[j]);
            let (ea, eb) = (&store.graph.entities[a], &store.graph.entities[b]);
            if ea.scope != eb.scope
                || store.is_ancestor(a, b)
                || store.is_ancestor(b, a)
                || tied.contains(&(a.clone(), b.clone()))
            {
                continue;
            }
            let docs_a: BTreeSet<String> = ea.mentions.iter().map(|m| m.doc.clone()).collect();
            let docs_b: BTreeSet<String> = eb.mentions.iter().map(|m| m.doc.clone()).collect();
            if docs_a.union(&docs_b).count() < 2 {
                continue;
            }
            let (ta, tb) = (name_tokens(ea), name_tokens(eb));
            if ta.is_empty() || tb.is_empty() {
                continue;
            }
            let shared: Vec<String> = ta.intersection(&tb).cloned().collect();
            if shared.is_empty() {
                continue;
            }
            let tokens = 2.0 * shared.len() as f64 / (ta.len() + tb.len()) as f64;
            let union = docs_a.union(&docs_b).count();
            let documents = docs_a.intersection(&docs_b).count() as f64 / union as f64;
            let score = LOOKALIKE_TOKEN_WEIGHT * tokens + LOOKALIKE_DOCUMENT_WEIGHT * documents;
            if score + 1e-9 >= LOOKALIKE_THRESHOLD {
                out.push(Lookalike {
                    a: a.clone(),
                    b: b.clone(),
                    score: (score * 100.0).round() / 100.0,
                    tokens: (tokens * 100.0).round() / 100.0,
                    documents: (documents * 100.0).round() / 100.0,
                    shared,
                    docs_a,
                    docs_b,
                });
            }
        }
    }
    out
}

// Goal ids the journal records as resolved, across every entry.
pub fn journal_resolved_goals(out: &std::path::Path) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for entry in read_journal(out) {
        for r in &entry.resolved_goals {
            set.insert(r.goal.clone());
        }
    }
    set
}

// Every journal entry, by generation.
pub fn read_journal(out: &std::path::Path) -> Vec<JournalEntry> {
    let dir = out.join("journal");
    let mut entries: Vec<(u64, JournalEntry)> = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else {
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
        let Ok(text) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        let Ok(mut entry) = serde_norway::from_str::<JournalEntry>(&text) else {
            continue;
        };
        if entry.generation == 0 {
            entry.generation = n;
        }
        entries.push((n, entry));
    }
    entries.sort_by_key(|(n, _)| *n);
    entries.into_iter().map(|(_, e)| e).collect()
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
        let judged = journal_resolved_goals(&store.out);
        let mut out = Vec::new();
        for l in lookalike_scores(store) {
            let target = pair_target(&l.a, &l.b);
            let id = goal_id("dedupe-candidates", &target);
            if judged.contains(&id) {
                continue;
            }
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

fn descendants(store: &Store, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut frontier = vec![id.to_string()];
    while let Some(p) = frontier.pop() {
        for (cid, c) in &store.graph.entities {
            if c.parent.as_deref() == Some(p.as_str()) && !out.contains(cid) {
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
                        hints.push(format!("break after {} (section boundary)", m));
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
                    .map(|m| (descendants(store, m).len(), m.clone()))
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
// entity's statements, with the section quoting most of them.
pub fn cohesion_groups(store: &Store, id: &str) -> Vec<(String, usize, String)> {
    let reqs = store.requirements_referencing(id);
    let mut by_token: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for rid in &reqs {
        let r = &store.graph.requirements[rid];
        for t in content_tokens(store, &r.statement, &r.entities) {
            if t.len() >= 4 {
                by_token.entry(t).or_default().push(rid);
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
            (tok, rids.len(), section)
        })
        .collect();
    groups.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    groups.truncate(6);
    groups
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
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut by_entity: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(store, &[store::CHANGE_THRESHOLD_CROSSED]) {
            if rec.detail["goal"] == "abstract-entity"
                && store.graph.entities.contains_key(&rec.subject)
            {
                by_entity.entry(rec.subject.clone()).or_default().push(rec);
            }
        }
        let mut out = Vec::new();
        for (id, recs) in by_entity {
            let e = &store.graph.entities[&id];
            let crossed = crossings_for(&recs);
            let mandatory = crossed.iter().any(|c| c.count > c.hard);
            let cause = earliest_cause(&recs);
            let mut hints = vec![format!("load {}", id)];
            for c in &crossed {
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
            for (cid, _) in store
                .graph
                .entities
                .iter()
                .filter(|(_, c)| c.parent.as_deref() == Some(id.as_str()))
            {
                hints.push(format!(
                    "child {} ({} requirements)",
                    cid,
                    store.requirements_referencing(cid).len()
                ));
            }
            if let Some(m) = e.mentions.first() {
                let full = format!("{}#{}", m.doc, m.section);
                let too_large = store.graph.diagnostics.values().any(|d| {
                    d.lifecycle == "open"
                        && d.rule == "section-too-large"
                        && d.subjects.iter().any(|s| *s == full)
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
            hints
                .push("skill abstraction; upsert_entity, update_requirement, update_entity".into());
            out.push(build_goal(
                self,
                &id,
                mandatory,
                threshold_change(&crossed),
                cause,
                hints,
            ));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        gc_ready(goal, board)
    }
    fn pack(&self, store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            for (cid, _) in store
                .graph
                .entities
                .iter()
                .filter(|(_, c)| c.parent.as_deref() == Some(g.target.as_str()))
            {
                lines.push(format!("- {} stub", cid));
            }
            if let Some(m) = store
                .graph
                .entities
                .get(&g.target)
                .and_then(|e| e.mentions.first())
            {
                lines.push(format!("- {}#{} full", m.doc, m.section));
            }
            for (vid, v) in &store.graph.views {
                if v.members.contains(&g.target) {
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
            "upsert_view",
            "update_view",
            "report_diagnostic",
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = Vec::new();
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
                Op::DeleteEntity { id, .. } | Op::MergeEntities { absorb: id, .. } => {
                    if !staged_parents.contains(id) {
                        out.push(Violation::new(
                            "target-removed",
                            format!(
                                "{}: an abstraction adds structure; it never deletes or merges",
                                id
                            ),
                        ));
                    }
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
mod tests {
    use super::*;

    #[test]
    fn registry_covers_sixteen_kinds_with_pages_and_prompts() {
        assert_eq!(REGISTRY.len(), 16);
        let mut names: BTreeSet<&str> = BTreeSet::new();
        for k in REGISTRY.iter() {
            assert!(names.insert(k.kind()), "duplicate kind {}", k.kind());
            let page = format!(
                "{}/../docs/compiler/goals/{}.md",
                env!("CARGO_MANIFEST_DIR"),
                k.kind()
            );
            assert!(
                std::path::Path::new(&page).exists(),
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
                assert!(record_kinds(k.kind()).len() > 0 || k.kind() == "bind");
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
}
