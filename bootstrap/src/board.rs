// The goal board: derived from disk by any process (the documents, the graph, the
// ledger, the change records, the parked and failed lists), never stored. Cones,
// readiness tiers, link levels, GC gating, locality batching under the context
// budget, escalation, release gating, leases, and the answer the MCP serving and the
// GUI render. Mirrors docs/compiler/reconciler.md#goal-derivation, #readiness,
// #batching, and docs/frontends/mcp.md#compilation-over-mcp.
use crate::control::Control;
use crate::gen::GenSettings;
use crate::goals::{self, Class, Ready, REGISTRY};
use crate::limits;
use crate::model::*;
use crate::project::Project;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

mod answer;
mod batching;
mod cones;
mod derivation;
mod explain;
mod readiness;

pub use batching::estimate;
pub use cones::{
    cone, cone_with, level_members, scope_root, scope_target, Cone, Refs, SCOPE_TARGET_PREFIX,
};
pub use derivation::{doc_levels, target_doc};
pub use explain::{opened_by_change, recomputed_by_change, staged_opens};

use answer::{condense_change, state_word};
use derivation::{is_open, target_exists};

#[cfg(test)]
use batching::locality_keys;
#[cfg(test)]
use cones::{referrers, referrers_scan};
#[cfg(test)]
use derivation::failure_stands;

// The kinds the ledger derives; every other kind works the graph.
pub const LEDGER_KINDS: [&str; 3] = ["bind", "generate", "verify"];

// One session's worth of goals: one class, one tier, one executor, one locality.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub id: String,
    pub class: Class,
    pub tier: Option<u8>,
    pub goals: Vec<String>,
    pub executor: Option<String>,
    pub locality: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Counts {
    pub open: usize,
    pub parked: usize,
    pub failed: usize,
    pub blocked: usize,
    pub optional: usize,
    pub ready: usize,
    pub gated: usize,
    pub claimed: usize,
    pub by_class: BTreeMap<String, usize>,
    pub by_kind: BTreeMap<String, usize>,
}

#[derive(Clone, Debug)]
pub struct Board {
    pub generation: u64,
    pub goals: Vec<Goal>,
    pub readiness: BTreeMap<String, Ready>,
    pub gated: BTreeSet<String>,
    pub claimed: BTreeMap<String, String>,
    // Batch ids live leases hold: form_batches numbers past them.
    // Mirrors docs/compiler/reconciler.md#batching.
    pub held_batches: BTreeSet<String>,
    // Goals leased to the serving reading the board: rendered as `claimedBy: you`.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    pub own: BTreeSet<String>,
    pub batches: Vec<Batch>,
    // Change records whose evidence lapsed at derivation: the reconciler clears them.
    pub lapsed: Vec<String>,
    // Failed entries whose subject changed again: the goal reopened.
    pub reopened: Vec<String>,
    // Parked entries whose target is gone or whose change moved on.
    pub dropped_parked: Vec<String>,
    // Failed ledger entries the ledger no longer derives.
    pub dropped_failed: Vec<String>,
    // Why each parked goal parked, by goal id, from its stored entry.
    // Mirrors docs/compiler/reconciler.md#parked-and-failed.
    pub parked_reasons: BTreeMap<String, String>,
    pub verdict: Verdict,
    pub open_diags: BTreeMap<String, u64>,
    pub dangling_diags: bool,
    pub alignment_pending: BTreeSet<String>,
    tier_open: [usize; 4],
    doc_levels: BTreeMap<String, usize>,
    pub(crate) blob_inputs: BTreeMap<String, BTreeSet<String>>,
    tier1_open_docs: BTreeSet<String>,
    bind_open_by_entity: BTreeMap<String, usize>,
    cone_blockers: BTreeMap<String, Vec<String>>,
    // The level each split-view target belongs to (a level view's own target, a
    // lifted flow view's level), the input of the yield-to-fan-out rule.
    // Mirrors docs/compiler/reconciler.md#gc-gating.
    view_levels: BTreeMap<String, String>,
    records: BTreeMap<String, Vec<String>>,
    // Goal id to its position in `goals`, and the (kind, target) of every open goal:
    // the lookups batching and readiness make per goal, so a board of five thousand
    // ledger goals is not scanned once per goal. Built once the goals are final.
    by_id: BTreeMap<String, usize>,
    open_kinds: BTreeSet<(String, String)>,
    // The pairs the degree cap holds back, by goal id: they derive no goal until a
    // slot frees. Mirrors docs/compiler/reconciler.md#pairs.
    pub waiting: BTreeMap<String, goals::WaitingPair>,
    // The goals outside the selection of a selected build: never ready, held by
    // nothing. Mirrors docs/compiler/reconciler.md#selection.
    pub outside: BTreeSet<String>,
}

#[cfg(test)]
pub(crate) mod tests;
