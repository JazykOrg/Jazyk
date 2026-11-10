// The goal registry: one trait, one static registry compiled into the binary, one
// implementation per kind. A kind derives its goals from disk state (the graph, the
// documents, the ledger, the change records), states when a goal is ready, names the
// targets a batch loads first, its tool slice, its batch gate, and its contract
// paragraph. Mirrors docs/compiler/reconciler.md#the-registry and the pages under
// docs/compiler/goals/.
use crate::board::Board;
use crate::gen::GenSettings;
use crate::limits::{
    self, LOOKALIKE_DISTINCTIVE_HOLDERS, LOOKALIKE_DOCUMENT_WEIGHT, LOOKALIKE_THRESHOLD,
    LOOKALIKE_TOKEN_WEIGHT,
};
use crate::model::*;
use crate::store::{self, Op, Store};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

mod abstract_entity;
mod answer;
mod bind;
mod conform_instance;
mod curate_view;
mod declare_edges;
mod dedupe_candidates;
mod derive_criteria;
mod fan_out;
mod generate;
mod helpers;
mod place_anchors;
mod ratify;
mod reconcile_section;
mod rejudge_pair;
mod retrace;
mod review_entity;
mod split_view;
mod verify;

pub use abstract_entity::*;
pub use answer::*;
pub use bind::*;
pub use conform_instance::*;
pub use curate_view::*;
pub use declare_edges::*;
pub use dedupe_candidates::*;
pub use derive_criteria::*;
pub use fan_out::*;
pub use generate::*;
pub use helpers::*;
pub use place_anchors::*;
pub use ratify::*;
pub use reconcile_section::*;
pub use rejudge_pair::*;
pub use retrace::*;
pub use review_entity::*;
pub use split_view::*;
pub use verify::*;

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

// What one goal's gate reads: the store, the session's staged work, the generation
// settings the ledger gates need, the goal itself, and the batch it rides in (the
// other goals own some of the staged work). Mirrors docs/compiler/tools.md#goal-tools.
pub struct GateContext<'a> {
    pub store: &'a Store,
    pub staged: &'a [Op],
    pub gen: &'a GenSettings,
    pub goal: &'a crate::tools::GoalScope,
    pub batch: &'a [crate::tools::GoalScope],
}

impl GateContext<'_> {
    // Whether another goal of the batch owns work on this node: its target is the
    // node, or a pair holding it.
    fn owned_elsewhere(&self, node: &str) -> bool {
        let node = self.store.resolve_id(node);
        self.batch
            .iter()
            .filter(|g| g.goal != self.goal.goal)
            .any(|g| {
                let hit = |t: &str| self.store.resolve_id(t) == node;
                match pair_members(&g.target) {
                    Some((a, b)) => hit(a) || hit(b),
                    None => hit(&g.target),
                }
            })
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
    // One goal's own gate: what its target, its change, and the rest of its batch
    // decide, checked at `mark_goal_done` and again at `done` after the batch gate.
    // Mirrors docs/compiler/tools.md#goal-tools.
    fn goal_gate(&self, _cx: &GateContext) -> Option<Violation> {
        None
    }
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
        "reconcile-section" => vec!["extraction"],
        "declare-edges" => vec!["edges"],
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

#[cfg(test)]
pub(crate) mod tests;
