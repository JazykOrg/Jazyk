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

// The cone of a target: every node reachable through stored references, upward
// (referents) and downward (referrers), never sideways; plus the sections that anchor
// those nodes and the documents holding them. Mirrors docs/compiler/reconciler.md#cones.
#[derive(Clone, Debug, Default)]
pub struct Cone {
    pub nodes: BTreeSet<String>,
    pub sections: BTreeSet<String>,
    pub docs: BTreeSet<String>,
}

// The scope root target form: `scope:<scope>` names the top level of a scope, its
// parentless entities. Mirrors docs/compiler/concepts/levels.md#the-scope-root.
pub const SCOPE_TARGET_PREFIX: &str = crate::store::SCOPE_ROOT_PREFIX;

// The scope a `scope:<scope>` target names; None for every other target.
pub fn scope_target(target: &str) -> Option<&str> {
    target
        .strip_prefix(SCOPE_TARGET_PREFIX)
        .filter(|s| !s.is_empty() && !s.contains(':') && !s.contains('#'))
}

// The top level of a scope: its parentless entities, by id.
pub fn scope_root(store: &Store, scope: &str) -> Vec<String> {
    store
        .graph
        .entities
        .iter()
        .filter(|(_, e)| e.parent.is_none() && e.scope == scope)
        .map(|(id, _)| id.clone())
        .collect()
}

// A target's level: the direct children of a node, or the scope root for the
// `scope:<scope>` form. Mirrors docs/compiler/concepts/levels.md#levels.
pub fn level_members(store: &Store, target: &str) -> Vec<String> {
    if let Some(scope) = scope_target(target) {
        return scope_root(store, scope);
    }
    store
        .graph
        .entities
        .iter()
        .filter(|(_, e)| e.parent.as_deref() == Some(target))
        .map(|(id, _)| id.clone())
        .collect()
}

impl Cone {
    pub fn holds_target(&self, target: &str) -> bool {
        if let Some((a, b)) = goals::pair_members(target) {
            return self.nodes.contains(a) || self.nodes.contains(b);
        }
        if target.contains('#') {
            return self.sections.contains(target);
        }
        if target.contains(':') {
            return self.nodes.contains(target);
        }
        self.docs.contains(target)
    }
}

fn parent_section(store: &Store, full: &str) -> Option<String> {
    let (doc, sec) = split_section_ref(full)?;
    let parent = store.docs.get(&doc)?.sections.get(&sec)?.parent.clone()?;
    Some(format!("{}#{}", doc, parent))
}

fn from_of(p: &Option<Provenance>) -> Vec<String> {
    match p {
        Some(Provenance::Derived { from, .. }) => from.clone(),
        _ => Vec::new(),
    }
}

// One hop upward: what the node references.
fn referents(store: &Store, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(e) = store.graph.entities.get(id) {
        out.extend(e.parent.iter().cloned());
        out.extend(
            e.mentions
                .iter()
                .map(|m| format!("{}#{}", m.doc, m.section)),
        );
        out.extend(from_of(&e.provenance));
        for a in &e.attributes {
            match &a.provenance {
                Provenance::Quote(s) => out.push(format!("{}#{}", s.doc, s.section)),
                Provenance::Derived { from, .. } => out.extend(from.iter().cloned()),
                Provenance::Decree { .. } => {}
            }
        }
    } else if let Some(r) = store.graph.requirements.get(id) {
        out.extend(r.entities.iter().map(|e| store.resolve_id(e).to_string()));
        for e in &r.edges {
            out.push(store.resolve_id(&e.a).to_string());
            out.push(store.resolve_id(&e.b).to_string());
        }
        if let Some(t) = &r.transition {
            out.push(store.resolve_id(&t.subject).to_string());
        }
        if let Some(s) = &r.source {
            out.push(format!("{}#{}", s.doc, s.section));
        }
        out.extend(from_of(&r.provenance));
    } else if let Some(v) = store.graph.views.get(id) {
        out.extend(v.members.iter().map(|m| store.resolve_id(m).to_string()));
        out.extend(v.collapse.iter().map(|m| store.resolve_id(m).to_string()));
        out.extend(
            v.excluded
                .iter()
                .map(|x| store.resolve_id(&x.id).to_string()),
        );
        out.extend(from_of(&v.provenance));
    } else if id.contains('#') {
        out.extend(parent_section(store, id));
        if let Some((doc, _)) = split_section_ref(id) {
            out.push(doc);
        }
    }
    out
}

// One hop downward: what references the node.
fn referrers(store: &Store, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let names = |from: &Option<Provenance>| from_of(from).iter().any(|f| f == id);
    if store.graph.entities.contains_key(id) {
        for (cid, c) in &store.graph.entities {
            if c.parent.as_deref() == Some(id)
                || names(&c.provenance)
                || c.attributes.iter().any(|a| match &a.provenance {
                    Provenance::Derived { from, .. } => from.iter().any(|f| f == id),
                    _ => false,
                })
            {
                out.push(cid.clone());
            }
        }
        for (rid, r) in &store.graph.requirements {
            let hit = r.entities.iter().any(|e| store.resolve_id(e) == id)
                || r.edges
                    .iter()
                    .any(|e| store.resolve_id(&e.a) == id || store.resolve_id(&e.b) == id)
                || r.transition
                    .as_ref()
                    .is_some_and(|t| store.resolve_id(&t.subject) == id)
                || names(&r.provenance);
            if hit {
                out.push(rid.clone());
            }
        }
    } else if store.graph.requirements.contains_key(id) {
        for (cid, c) in &store.graph.entities {
            if names(&c.provenance) {
                out.push(cid.clone());
            }
        }
        for (rid, r) in &store.graph.requirements {
            if names(&r.provenance) {
                out.push(rid.clone());
            }
        }
    } else if id.contains('#') {
        let Some((doc, sec)) = split_section_ref(id) else {
            return out;
        };
        for (rid, r) in &store.graph.requirements {
            if r.anchored_at(&doc, &sec) {
                out.push(rid.clone());
            }
        }
        for (eid, e) in &store.graph.entities {
            if e.mentions.iter().any(|m| m.doc == doc && m.section == sec) {
                out.push(eid.clone());
            }
        }
        if let Some(rec) = store.docs.get(&doc) {
            for (r, s) in &rec.sections {
                if s.parent.as_deref() == Some(sec.as_str()) {
                    out.push(format!("{}#{}", doc, r));
                }
            }
        }
        return out;
    } else if let Some(rec) = store.docs.get(id) {
        out.extend(rec.sections.keys().map(|r| format!("{}#{}", id, r)));
        return out;
    }
    if store.graph.entities.contains_key(id)
        || store.graph.requirements.contains_key(id)
        || store.graph.views.contains_key(id)
    {
        for (vid, v) in &store.graph.views {
            let listed = v
                .members
                .iter()
                .chain(v.collapse.iter())
                .any(|m| store.resolve_id(m) == id)
                || v.excluded.iter().any(|x| store.resolve_id(&x.id) == id)
                || names(&v.provenance);
            if listed {
                out.push(vid.clone());
            }
        }
    }
    out
}

pub fn cone(store: &Store, target: &str) -> Cone {
    let mut nodes: BTreeSet<String> = BTreeSet::new();
    // The scope root's cone is the downward walk from every parentless entity of the
    // scope: the whole scope, so the top level is regrouped only once everything in
    // it has settled. A parentless entity has nothing above it to walk up to.
    let scope = scope_target(target);
    let seeds: Vec<String> = match (scope, goals::pair_members(target)) {
        (Some(s), _) => scope_root(store, s),
        (None, Some((a, b))) => vec![a.to_string(), b.to_string()],
        (None, None) => vec![target.to_string()],
    };
    for seed in &seeds {
        nodes.insert(seed.clone());
        if scope.is_none() {
            let mut frontier = vec![seed.clone()];
            while let Some(id) = frontier.pop() {
                for r in referents(store, &id) {
                    if nodes.insert(r.clone()) {
                        frontier.push(r);
                    }
                }
            }
        }
        let mut frontier = vec![seed.clone()];
        while let Some(id) = frontier.pop() {
            for r in referrers(store, &id) {
                if nodes.insert(r.clone()) {
                    frontier.push(r);
                }
            }
        }
    }
    let mut sections: BTreeSet<String> = BTreeSet::new();
    let mut docs: BTreeSet<String> = BTreeSet::new();
    for n in &nodes {
        if n.contains('#') {
            sections.insert(n.clone());
        } else if !n.contains(':') {
            docs.insert(n.clone());
        }
        if let Some(e) = store.graph.entities.get(n) {
            sections.extend(
                e.mentions
                    .iter()
                    .map(|m| format!("{}#{}", m.doc, m.section)),
            );
        }
        if let Some(r) = store.graph.requirements.get(n) {
            if let Some(s) = &r.source {
                sections.insert(format!("{}#{}", s.doc, s.section));
            }
        }
    }
    for s in &sections {
        if let Some((doc, _)) = split_section_ref(s) {
            docs.insert(doc);
        }
    }
    Cone {
        nodes,
        sections,
        docs,
    }
}

fn target_exists(store: &Store, target: &str) -> bool {
    if let Some((a, b)) = goals::pair_members(target) {
        return target_exists(store, a) && target_exists(store, b);
    }
    // A scope root exists while the scope holds a parentless entity.
    if let Some(scope) = scope_target(target) {
        return !scope_root(store, scope).is_empty();
    }
    if let Some((doc, sec)) = split_section_ref(target) {
        return store
            .docs
            .get(&doc)
            .is_some_and(|r| r.sections.contains_key(&sec));
    }
    if target.contains(':') {
        return store.graph.entities.contains_key(target)
            || store.graph.requirements.contains_key(target)
            || store.graph.views.contains_key(target)
            || store.graph.diagnostics.contains_key(target);
    }
    store.docs.contains_key(target)
}

fn is_open(g: &Goal) -> bool {
    matches!(g.state, GoalState::Open | GoalState::Parked)
}

// The document a goal's target lives in, when it has one.
pub fn target_doc(target: &str) -> Option<String> {
    if let Some((doc, _)) = split_section_ref(target) {
        return Some(doc);
    }
    if !target.contains(':') && target.ends_with(".md") {
        return Some(target.to_string());
    }
    None
}

// Breadth-first document levels from the roots over the link graph in the stored
// sections (derive::doc_levels_from does the walk). With no roots, every document is
// its own level in path order. Mirrors docs/compiler/reconciler.md#link-levels.
pub fn doc_levels(store: &Store, proj: &Project) -> BTreeMap<String, usize> {
    let roots: Vec<String> = store
        .docs
        .keys()
        .filter(|d| proj.is_root_file(d))
        .cloned()
        .collect();
    crate::derive::doc_levels_from(store, &roots)
}

// The characters a goal's initially loaded set costs, so a batch fills under the
// context budget. Mirrors docs/compiler/reconciler.md#batching.
pub fn estimate(store: &Store, g: &Goal) -> usize {
    match g.kind.as_str() {
        "reconcile-section" => {
            let body = split_section_ref(&g.target)
                .and_then(|(d, s)| {
                    store
                        .docs
                        .get(&d)
                        .and_then(|r| r.sections.get(&s).map(|x| x.raw.len()))
                })
                .unwrap_or(0);
            body + 1_200
        }
        "place-anchors" => {
            let proposals = store
                .status
                .alignment
                .iter()
                .find(|b| b.doc == g.target)
                .map(|b| b.proposals.len())
                .unwrap_or(0);
            800 + 700 * proposals
        }
        "rejudge-pair" => {
            // Both sides load full plus their neighbor stubs: the cost follows
            // the statements, never a flat guess (a 13-pair batch of long
            // statements does not fit where 13 flat guesses said it would).
            let side = |id: &str| {
                store
                    .graph
                    .requirements
                    .get(store.resolve_id(id))
                    .map(|r| 260 + r.statement.len() + 90 * r.entities.len())
                    .unwrap_or(400)
            };
            match g.target.split_once('~') {
                Some((a, b)) => 700 + side(a) + side(b),
                None => 1_400,
            }
        }
        "review-entity" => 1_500 + 140 * store.requirements_referencing(&g.target).len(),
        // The caps variant loads the node's requirements; the fan-out variant loads
        // the level as stubs (docs/compiler/goals/abstract-entity.md#fan-out-hints).
        "abstract-entity" => {
            1_500
                + 160 * store.requirements_referencing(&g.target).len()
                + 120 * level_members(store, &g.target).len()
        }
        "curate-view" | "split-view" | "retrace" => {
            let members = store
                .graph
                .views
                .get(&g.target)
                .map(|v| v.members.len())
                .unwrap_or(0);
            1_200 + 90 * members
        }
        "dedupe-candidates" => 2_400,
        _ => 1_500,
    }
}

#[derive(Clone, Debug)]
pub struct Board {
    pub generation: u64,
    pub goals: Vec<Goal>,
    pub readiness: BTreeMap<String, Ready>,
    pub gated: BTreeSet<String>,
    pub claimed: BTreeMap<String, String>,
    pub batches: Vec<Batch>,
    // Change records whose evidence lapsed at derivation: the reconciler clears them.
    pub lapsed: Vec<String>,
    // Failed entries whose subject changed again: the goal reopened.
    pub reopened: Vec<String>,
    // Parked entries whose target is gone or whose change moved on.
    pub dropped_parked: Vec<String>,
    pub verdict: Verdict,
    pub open_diags: BTreeMap<String, u64>,
    pub dangling_diags: bool,
    pub alignment_pending: BTreeSet<String>,
    tier_open: [usize; 4],
    doc_levels: BTreeMap<String, usize>,
    tier1_open_docs: BTreeSet<String>,
    bind_open_by_entity: BTreeMap<String, usize>,
    cone_blockers: BTreeMap<String, Vec<String>>,
    // The level each split-view target belongs to (a level view's own target, a
    // lifted flow view's level), the input of the yield-to-fan-out rule.
    // Mirrors docs/compiler/reconciler.md#gc-gating.
    view_levels: BTreeMap<String, String>,
    records: BTreeMap<String, Vec<String>>,
}

// The level a view belongs to: a lifted flow view's level, or the target whose
// structural level view it is (an entity, or `scope:<scope>` for a top level).
// None for a curated, object, or state view.
fn view_level_of(store: &Store, view_id: &str) -> Option<String> {
    if let Some(l) = crate::derive::flow_view_level(store, view_id) {
        return Some(l);
    }
    let mut targets: Vec<String> = store.graph.entities.keys().cloned().collect();
    let mut scopes: Vec<String> = store
        .graph
        .entities
        .values()
        .map(|e| format!("{}{}", SCOPE_TARGET_PREFIX, e.scope))
        .collect();
    scopes.sort();
    scopes.dedup();
    targets.extend(scopes);
    targets
        .into_iter()
        .find(|t| crate::derive::level_view_id(store, t).as_deref() == Some(view_id))
}

impl Board {
    // Derive the board from a store synced against the documents. Every process
    // computes the same board from the same disk state.
    pub fn derive(store: &Store, proj: &Project, control: &Control) -> Board {
        let gen = GenSettings::resolve(proj);
        let mut goals: Vec<Goal> = REGISTRY
            .iter()
            .flat_map(|k| k.derive_goals(store, &gen))
            .collect();
        goals.sort_by(|a, b| a.id.cmp(&b.id));
        goals.dedup_by(|a, b| a.id == b.id);

        // Parked and failed goals keep their identity (the change) and their state.
        let mut dropped_parked = Vec::new();
        let mut reopened = Vec::new();
        for p in &store.status.parked {
            match goals.iter_mut().find(|g| g.id == p.id) {
                Some(g) => {
                    if g.change == p.change || p.change.is_null() {
                        if matches!(g.state, GoalState::Open) {
                            g.state = GoalState::Parked;
                        }
                    } else {
                        dropped_parked.push(p.id.clone());
                    }
                }
                None => {
                    if target_exists(store, &p.target) && !goals::blocked_on_human(&p.kind) {
                        let mut g = p.clone();
                        g.state = GoalState::Parked;
                        goals.push(g);
                    } else {
                        dropped_parked.push(p.id.clone());
                    }
                }
            }
        }
        for f in &store.status.failed {
            match goals.iter_mut().find(|g| g.id == f.goal.id) {
                Some(g) => {
                    let same = g.change == f.goal.change
                        || g.cause.as_ref().map(|c| c.generation)
                            <= f.goal.cause.as_ref().map(|c| c.generation);
                    if same {
                        g.state = GoalState::Failed {
                            reason: f.reason.clone(),
                        };
                    } else {
                        reopened.push(f.goal.id.clone());
                    }
                }
                None => {
                    if target_exists(store, &f.goal.target) {
                        let mut g = f.goal.clone();
                        g.state = GoalState::Failed {
                            reason: f.reason.clone(),
                        };
                        goals.push(g);
                    }
                }
            }
        }
        goals.sort_by(|a, b| a.id.cmp(&b.id));

        // Manual-mode gating first: a gated goal is blocked on a release, not open,
        // so it neither schedules nor holds a cone.
        let mut gated: BTreeSet<String> = BTreeSet::new();
        if control.compile == "manual" {
            for g in goals.iter().filter(|g| is_open(g)) {
                if !matches!(g.kind.as_str(), "reconcile-section" | "place-anchors") {
                    continue;
                }
                let Some(doc) = target_doc(&g.target) else {
                    continue;
                };
                let current = store
                    .docs
                    .get(&doc)
                    .map(|r| r.content_hash.as_str())
                    .unwrap_or_default();
                if control.released.compile.get(&doc).map(String::as_str) != Some(current) {
                    gated.insert(g.id.clone());
                }
            }
        }
        if control.generate == "manual" && control.released.generate != store.status.generation {
            for g in goals.iter().filter(|g| is_open(g)) {
                if matches!(g.kind.as_str(), "bind" | "generate") {
                    gated.insert(g.id.clone());
                }
            }
        }
        for g in goals.iter_mut().filter(|g| gated.contains(&g.id)) {
            let stage = if matches!(g.kind.as_str(), "bind" | "generate") {
                "generate"
            } else {
                "compile"
            };
            g.state = GoalState::Blocked {
                on: format!("release: {}", stage),
            };
        }

        // Records the goals stand on, and the ones whose evidence lapsed.
        let mut records: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in &goals {
            let kinds = goals::record_kinds(&g.kind);
            let subjects: Vec<&str> = match goals::pair_members(&g.target) {
                Some((a, b)) => vec![a, b],
                None => vec![g.target.as_str()],
            };
            let ids: Vec<String> = store
                .status
                .changes
                .iter()
                .filter(|c| {
                    kinds.contains(&c.kind.as_str()) && subjects.contains(&c.subject.as_str())
                })
                .map(|c| c.id.clone())
                .collect();
            records.insert(g.id.clone(), ids);
        }
        let standing: BTreeSet<&String> = records.values().flatten().collect();
        let lapsed: Vec<String> = store
            .status
            .changes
            .iter()
            .filter(|c| {
                matches!(
                    c.kind.as_str(),
                    crate::store::CHANGE_SECTION_DIRTY | crate::store::CHANGE_ANCHOR_STALE
                ) && !standing.contains(&c.id)
            })
            .map(|c| c.id.clone())
            .collect();

        // Readiness inputs.
        let mut tier_open = [0usize; 4];
        let mut tier1_open_docs = BTreeSet::new();
        let mut bind_open_by_entity: BTreeMap<String, usize> = BTreeMap::new();
        for g in goals.iter().filter(|g| is_open(g)) {
            if let Some(t) = goals::tier(&g.kind) {
                tier_open[t as usize] += 1;
                if t == 1 {
                    if let Some(d) = target_doc(&g.target) {
                        tier1_open_docs.insert(d);
                    }
                }
            }
            if g.kind == "bind" {
                if let Some(e) = g.change["entity"].as_str() {
                    *bind_open_by_entity.entry(e.to_string()).or_insert(0) += 1;
                }
            }
        }
        let alignment_pending: BTreeSet<String> = store
            .status
            .changes
            .iter()
            .filter(|c| c.kind == crate::store::CHANGE_ALIGNMENT_PENDING)
            .map(|c| c.subject.clone())
            .collect();
        let mut cone_blockers: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in goals.iter().filter(|g| g.class == "gc") {
            let cone = cone(store, &g.target);
            let blockers: Vec<String> = goals
                .iter()
                .filter(|o| o.class == "compile" && is_open(o) && cone.holds_target(&o.target))
                .map(|o| o.id.clone())
                .collect();
            cone_blockers.insert(g.id.clone(), blockers);
        }
        let mut view_levels: BTreeMap<String, String> = BTreeMap::new();
        for g in goals.iter().filter(|g| g.kind == "split-view") {
            if let Some(l) = view_level_of(store, &g.target) {
                view_levels.insert(g.target.clone(), l);
            }
        }

        let mut board = Board {
            generation: store.status.generation,
            goals,
            readiness: BTreeMap::new(),
            gated: BTreeSet::new(),
            claimed: BTreeMap::new(),
            batches: Vec::new(),
            lapsed,
            reopened,
            dropped_parked,
            verdict: store.status.verdict.clone(),
            open_diags: store.open_diag_counts(),
            dangling_diags: store.has_dangling_diags(),
            alignment_pending,
            tier_open,
            doc_levels: doc_levels(store, proj),
            tier1_open_docs,
            bind_open_by_entity,
            cone_blockers,
            view_levels,
            records,
        };

        // Readiness per kind, then the control plane: a release gate and a lease both
        // render as blocked with the reason.
        let mut readiness: BTreeMap<String, Ready> = BTreeMap::new();
        for g in &board.goals {
            let r = match g.state {
                GoalState::Failed { ref reason } => Ready::Blocked(format!("failed: {}", reason)),
                GoalState::Blocked { ref on } => Ready::Blocked(format!("blocked on {}", on)),
                _ => goals::kind(&g.kind)
                    .map(|k| k.ready(g, &board))
                    .unwrap_or(Ready::Blocked("unknown kind".into())),
            };
            readiness.insert(g.id.clone(), r);
        }
        board.readiness = readiness;
        for id in &gated {
            let stage = if id.starts_with("g:bind:") || id.starts_with("g:generate:") {
                "generate"
            } else {
                "compile"
            };
            board.readiness.insert(
                id.clone(),
                Ready::Blocked(format!(
                    "awaiting release: `jazyk release {}` (or the GUI) approves it",
                    stage
                )),
            );
        }
        board.gated = gated;
        let leases = crate::control::task_leases(&store.out);
        if !leases.is_empty() {
            let mut claimed = BTreeMap::new();
            for g in &board.goals {
                let mut keys: Vec<String> = vec![g.id.clone(), g.target.clone()];
                if let Some((a, b)) = goals::pair_members(&g.target) {
                    keys.push(a.to_string());
                    keys.push(b.to_string());
                }
                if let Some(d) = target_doc(&g.target) {
                    keys.push(d);
                }
                let hit = keys
                    .iter()
                    .find_map(|k| leases.get(k))
                    .or_else(|| leases.values().find(|l| l.goals.iter().any(|x| x == &g.id)));
                if let Some(l) = hit {
                    claimed.insert(g.id.clone(), l.worker.clone());
                }
            }
            for (id, worker) in &claimed {
                board.readiness.insert(
                    id.clone(),
                    Ready::Blocked(format!(
                        "claimed by worker `{}` (the lease lapses {}s after its last heartbeat)",
                        worker,
                        crate::control::LEASE_TTL_SECS
                    )),
                );
            }
            board.claimed = claimed;
        }
        board.batches = board.form_batches(store, proj);
        board
    }

    // Load the store, sync the section trees, read the control plane, derive.
    pub fn compute(proj: &Project, out: &Path) -> Board {
        let mut store = Store::load(out);
        let (parsed, _) = crate::reconcile::parse_all(proj);
        store.sync_docs(&parsed);
        let control = Control::load(proj, out);
        Board::derive(&store, proj, &control)
    }

    // ---- readiness queries the kinds ask ----

    pub fn tier_open(&self, tier: u8) -> usize {
        self.tier_open[tier as usize]
    }

    // The earlier level a document waits for, when one still has open tier 1 goals.
    pub fn level_waiting(&self, doc: &str) -> Option<usize> {
        let mine = *self.doc_levels.get(doc)?;
        self.tier1_open_docs
            .iter()
            .filter_map(|d| self.doc_levels.get(d))
            .filter(|l| **l < mine)
            .min()
            .copied()
    }

    pub fn bind_open_for_entity(&self, entity: &str) -> usize {
        self.bind_open_by_entity.get(entity).copied().unwrap_or(0)
    }

    pub fn open(&self, goal_id: &str) -> bool {
        self.goals.iter().any(|g| g.id == goal_id && is_open(g))
    }

    // The open compile goals in a GC goal's cone.
    pub fn cone_blockers(&self, goal_id: &str) -> Vec<String> {
        self.cone_blockers.get(goal_id).cloned().unwrap_or_default()
    }

    // The level a split-view target belongs to, when it is a level view or a lifted
    // flow view of one. Mirrors docs/compiler/reconciler.md#gc-gating.
    pub fn view_level(&self, view_id: &str) -> Option<&str> {
        self.view_levels.get(view_id).map(String::as_str)
    }

    pub fn goal(&self, id: &str) -> Option<&Goal> {
        self.goals.iter().find(|g| g.id == id)
    }

    // The change record ids a goal stands on.
    pub fn records_of(&self, goal_id: &str) -> Vec<String> {
        self.records.get(goal_id).cloned().unwrap_or_default()
    }

    pub fn is_ready(&self, goal_id: &str) -> bool {
        self.readiness.get(goal_id).is_some_and(|r| r.is_ready())
            && !self.gated.contains(goal_id)
            && !self.claimed.contains_key(goal_id)
    }

    pub fn ready_goals(&self) -> Vec<&Goal> {
        self.goals
            .iter()
            .filter(|g| is_open(g) && self.is_ready(&g.id))
            .collect()
    }

    // Open or parked goals, both classes.
    pub fn open_goals(&self) -> Vec<&Goal> {
        self.goals.iter().filter(|g| is_open(g)).collect()
    }

    // Mandatory goals still open or parked: what keeps a build from converging.
    pub fn open_mandatory(&self) -> usize {
        self.goals
            .iter()
            .filter(|g| is_open(g) && g.mandatory)
            .count()
    }

    // Open goals of the named kinds; the graph kinds are everything but the ledger's.
    pub fn open_of(&self, kinds: &[&str]) -> usize {
        self.goals
            .iter()
            .filter(|g| is_open(g) && kinds.contains(&g.kind.as_str()))
            .count()
    }

    pub fn ready_of(&self, kinds: &[&str]) -> usize {
        self.goals
            .iter()
            .filter(|g| is_open(g) && kinds.contains(&g.kind.as_str()) && self.is_ready(&g.id))
            .count()
    }

    pub fn gated_of(&self, kinds: &[&str]) -> usize {
        self.goals
            .iter()
            .filter(|g| kinds.contains(&g.kind.as_str()) && self.gated.contains(&g.id))
            .count()
    }

    pub fn graph_kinds() -> Vec<&'static str> {
        REGISTRY
            .iter()
            .map(|k| k.kind())
            .filter(|k| !LEDGER_KINDS.contains(k))
            .collect()
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for g in &self.goals {
            match &g.state {
                GoalState::Open | GoalState::Parked => {
                    if g.mandatory {
                        c.open += 1;
                    } else {
                        c.optional += 1;
                    }
                    if matches!(g.state, GoalState::Parked) {
                        c.parked += 1;
                    }
                    *c.by_class.entry(g.class.clone()).or_insert(0) += 1;
                    *c.by_kind.entry(g.kind.clone()).or_insert(0) += 1;
                    if self.is_ready(&g.id) {
                        c.ready += 1;
                    }
                }
                GoalState::Blocked { .. } => c.blocked += 1,
                GoalState::Failed { .. } => {
                    if g.mandatory {
                        c.failed += 1;
                    } else {
                        c.optional += 1;
                    }
                }
            }
        }
        c.gated = self.gated.len();
        c.claimed = self.claimed.len();
        c
    }

    // The verdict the board implies; the checks decide the rest.
    pub fn verdict(&self) -> Verdict {
        let c = self.counts();
        Verdict {
            state: if c.open == 0 && c.failed == 0 {
                "converged".into()
            } else {
                "incomplete".into()
            },
            open: c.open as u64,
            failed: c.failed as u64,
            blocked: c.blocked as u64,
            optional: c.optional as u64,
        }
    }

    // `compile: N goals (k kind, ...), b blocked`. Mirrors docs/frontends/cli.md#jazyk-compile.
    pub fn summary_line(&self) -> String {
        let c = self.counts();
        let total: usize = c.by_kind.values().sum();
        let mut kinds: Vec<(&String, &usize)> = c.by_kind.iter().collect();
        kinds.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let per_kind: Vec<String> = kinds.iter().map(|(k, n)| format!("{} {}", n, k)).collect();
        let mut s = format!("compile: {} goals", total);
        if !per_kind.is_empty() {
            s.push_str(&format!(" ({})", per_kind.join(", ")));
        }
        if c.blocked > 0 {
            s.push_str(&format!(", {} blocked", c.blocked));
        }
        s
    }

    // ---- batching ----

    fn form_batches(&self, store: &Store, proj: &Project) -> Vec<Batch> {
        let ready: Vec<&Goal> = self.ready_goals();
        let executor = |g: &Goal| proj.executors.resolve(&g.kind, &g.class).map(String::from);
        // Group key: class, tier, executor. GC goals sit after the compile tiers,
        // mandatory before optional.
        let mut groups: BTreeMap<(u8, u8, Option<String>), Vec<&Goal>> = BTreeMap::new();
        for g in ready {
            let key = match Class::parse(&g.class) {
                Class::Compile => (0, goals::tier(&g.kind).unwrap_or(9), executor(g)),
                Class::Gc => (1, if g.mandatory { 0 } else { 1 }, executor(g)),
            };
            groups.entry(key).or_default().push(g);
        }
        let mut batches: Vec<Batch> = Vec::new();
        // A locality is where a batch starts, not where it must end: the document
        // and ledger localities are walls, the node, view, and level localities pack
        // on into the same batch until the budget is spent.
        // Mirrors docs/compiler/reconciler.md#batching.
        let walled = |id: &str| {
            self.goal(id).is_some_and(|g| {
                matches!(
                    g.kind.as_str(),
                    "place-anchors" | "reconcile-section" | "bind" | "generate" | "verify"
                )
            })
        };
        // The skills of the batch's goal kinds render into the same context budget
        // as the loaded set (sessions.md#budgets): the estimates fill what the skill
        // payloads of the goals actually in the batch leave, never the union over
        // the whole tier (a pair batch pays for judgment, not for the conformance
        // skill of an instance goal it does not hold).
        let skills_size = |names: &[&str]| -> usize {
            names
                .iter()
                .filter_map(|n| {
                    crate::session::SKILLS
                        .iter()
                        .find(|(k, _)| k == n)
                        .map(|(_, p)| p.len())
                })
                .sum()
        };
        let budget_with = |names: &[&str]| -> usize {
            limits::CONTEXT_BUDGET
                .saturating_sub(skills_size(names))
                .max(6_000)
        };
        for ((class, tier, exec), members) in groups {
            let localities = self.localities(store, &members);
            let mut current: Vec<String> = Vec::new();
            let mut skills: Vec<&str> = Vec::new();
            let mut label = String::new();
            let mut size = 0usize;
            let mut sections = 0usize;
            let flush = |current: &mut Vec<String>,
                         skills: &mut Vec<&str>,
                         label: &mut String,
                         batches: &mut Vec<Batch>| {
                skills.clear();
                if !current.is_empty() {
                    batches.push(Batch {
                        id: String::new(),
                        class: if class == 0 {
                            Class::Compile
                        } else {
                            Class::Gc
                        },
                        tier: if class == 0 { Some(tier) } else { None },
                        goals: std::mem::take(current),
                        executor: exec.clone(),
                        locality: std::mem::take(label),
                    });
                }
            };
            for (locality, mut ids) in localities {
                let wall = ids.iter().any(|id| walled(id));
                if wall {
                    flush(&mut current, &mut skills, &mut label, &mut batches);
                    size = 0;
                    sections = 0;
                }
                // Parked goals first, pairs before entity reviews, sections in
                // document order, deepest abstraction first.
                ids.sort_by_key(|id| {
                    let g = self.goal(id).unwrap();
                    let parked = !matches!(g.state, GoalState::Parked);
                    let order = match g.kind.as_str() {
                        "rejudge-pair" => 0,
                        "abstract-entity" => usize::MAX - entity_depth(store, &g.target),
                        "reconcile-section" => section_order(store, &g.target),
                        _ => 1,
                    };
                    (parked, order, id.clone())
                });
                for id in ids {
                    let g = self.goal(&id).unwrap();
                    let cost = estimate(store, g);
                    let is_section = g.kind == "reconcile-section";
                    let max_sections =
                        (limits::SESSION_ROUNDS / limits::ROUNDS_PER_SECTION) as usize;
                    let mut with_goal = skills.clone();
                    for s in goals::skills_for(&g.kind, store, &g.target) {
                        if !with_goal.contains(&s) {
                            with_goal.push(s);
                        }
                    }
                    let over = !current.is_empty()
                        && (size + cost > budget_with(&with_goal)
                            || (is_section && sections >= max_sections));
                    if over {
                        flush(&mut current, &mut skills, &mut label, &mut batches);
                        size = 0;
                        sections = 0;
                        with_goal = goals::skills_for(&g.kind, store, &g.target);
                    }
                    if current.is_empty() {
                        label = locality.clone();
                    }
                    skills = with_goal;
                    current.push(id);
                    size += cost;
                    if is_section {
                        sections += 1;
                    }
                }
                if wall {
                    flush(&mut current, &mut skills, &mut label, &mut batches);
                    size = 0;
                    sections = 0;
                }
            }
            flush(&mut current, &mut skills, &mut label, &mut batches);
        }
        // Compile tiers first, then GC with mandatory batches before optional ones;
        // within a tier, batches holding parked goals first, then document level and
        // path. Mirrors docs/compiler/reconciler.md#gc-gating.
        let rank = |b: &Batch| {
            let parked = b
                .goals
                .iter()
                .any(|id| matches!(self.goal(id).map(|g| &g.state), Some(GoalState::Parked)));
            let optional = b
                .goals
                .iter()
                .all(|id| self.goal(id).is_some_and(|g| !g.mandatory));
            let level = b
                .goals
                .first()
                .and_then(|id| self.goal(id))
                .and_then(|g| target_doc(&g.target))
                .and_then(|d| self.doc_levels.get(&d).copied())
                .unwrap_or(0);
            (
                matches!(b.class, Class::Gc),
                b.tier.unwrap_or(0),
                optional,
                !parked,
                level,
                b.locality.clone(),
            )
        };
        batches.sort_by_key(rank);
        for (i, b) in batches.iter_mut().enumerate() {
            b.id = format!("b{}-{}", self.generation, i + 1);
        }
        batches
    }

    // Locality groups within one class and tier: document, node neighborhood, view,
    // or ledger entity. Mirrors docs/compiler/reconciler.md#batching.
    fn localities(&self, store: &Store, members: &[&Goal]) -> Vec<(String, Vec<String>)> {
        // Union-find over the keys each goal touches.
        let mut parent: BTreeMap<String, String> = BTreeMap::new();
        fn find(parent: &mut BTreeMap<String, String>, k: &str) -> String {
            let p = parent.get(k).cloned().unwrap_or_else(|| k.to_string());
            if p == k {
                return p;
            }
            let root = find(parent, &p);
            parent.insert(k.to_string(), root.clone());
            root
        }
        fn union(parent: &mut BTreeMap<String, String>, a: &str, b: &str) {
            let (ra, rb) = (find(parent, a), find(parent, b));
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
        let mut keys_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in members {
            let keys = locality_keys(store, g);
            for k in &keys {
                parent.entry(k.clone()).or_insert_with(|| k.clone());
            }
            for w in keys.windows(2) {
                union(&mut parent, &w[0], &w[1]);
            }
            keys_of.insert(g.id.clone(), keys);
        }
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in members {
            let keys = &keys_of[&g.id];
            let root = keys
                .first()
                .map(|k| find(&mut parent, k))
                .unwrap_or_else(|| g.id.clone());
            groups.entry(root).or_default().push(g.id.clone());
        }
        let mut out: Vec<(String, Vec<String>)> = groups
            .into_iter()
            .map(|(root, ids)| {
                let label = keys_of[&ids[0]].first().cloned().unwrap_or(root);
                (label, ids)
            })
            .collect();
        out.sort();
        out
    }

    // ---- the answer the MCP serving and the GUI render ----

    pub fn goal_json(&self, g: &Goal) -> Value {
        let ready = self.is_ready(&g.id);
        let mut v = json!({
            "id": g.id,
            "kind": g.kind,
            "target": g.target,
            "unit": g.unit,
            "class": g.class,
            "mandatory": g.mandatory,
            "state": g.state,
            "ready": ready,
            "gated": self.gated.contains(&g.id),
            "hints": g.hints,
            "cause": g.cause,
            "change": g.change,
        });
        if let Some(r) = self.readiness.get(&g.id).and_then(|r| r.reason()) {
            v["blockedBy"] = json!(r);
        }
        if let Some(w) = self.claimed.get(&g.id) {
            v["claimedBy"] = json!(w);
        }
        if let Some(b) = self.batches.iter().find(|b| b.goals.contains(&g.id)) {
            v["batch"] = json!(b.id);
        }
        v
    }

    pub fn batch_json(&self, b: &Batch) -> Value {
        json!({
            "id": b.id,
            "class": b.class.name(),
            "tier": b.tier,
            "executor": b.executor,
            "locality": b.locality,
            "goals": b.goals.iter().filter_map(|id| self.goal(id)).map(|g| json!({
                "id": g.id, "kind": g.kind, "target": g.target, "mandatory": g.mandatory,
            })).collect::<Vec<_>>(),
        })
    }

    // The `goals` reply: every goal with readiness, the batches the scheduler would
    // form, the counts, and the verdict with the diagnostic counts when nothing is
    // open. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    pub fn answer(&self) -> Value {
        let counts = self.counts();
        let mut v = json!({
            "generation": self.generation,
            "goals": self.goals.iter().map(|g| self.goal_json(g)).collect::<Vec<_>>(),
            "batches": self.batches.iter().map(|b| self.batch_json(b)).collect::<Vec<_>>(),
            "counts": counts,
        });
        if self.open_goals().is_empty() {
            v["verdict"] = json!(self.verdict.to_string());
            v["note"] = json!("nothing to compile; the graph reflects the docs");
        } else if self.batches.is_empty() {
            v["note"] = json!("no goal is ready; blockedBy says why");
        } else {
            v["next"] = json!(format!(
                "begin_goals claims the next batch ({})",
                self.batches[0].id
            ));
        }
        if !self.open_diags.is_empty() {
            v["openDiagnostics"] = json!(self.open_diags);
        }
        v
    }

    // ---- explain ----

    // A goal: its change, cause, readiness, blockers, hints. A target: the goals a
    // change to it would open. Mirrors docs/frontends/cli.md#jazyk-explain.
    pub fn explain(&self, store: &Store, target: &str) -> Option<String> {
        if let Some(g) = self.goal(target) {
            let mut s = format!(
                "{}  {}, {}, {}, {}\n",
                g.id,
                g.kind,
                g.class,
                if g.mandatory { "mandatory" } else { "optional" },
                state_word(&g.state)
            );
            s.push_str(&format!("  change: {}\n", condense_change(&g.change)));
            if let Some(c) = &g.cause {
                s.push_str(&format!(
                    "  cause:  g{} mutation {} via {}\n",
                    c.generation, c.mutation, c.via
                ));
            }
            let tier = goals::tier(&g.kind)
                .map(|t| format!("tier {}", t))
                .unwrap_or_else(|| "gc (waits for its cone)".into());
            match self.readiness.get(&g.id) {
                Some(Ready::Ready) => s.push_str(&format!("  ready:  {}; ready now\n", tier)),
                Some(Ready::Blocked(r)) => {
                    s.push_str(&format!("  ready:  {}; waits: {}\n", tier, r))
                }
                None => {}
            }
            let blockers = self.cone_blockers(&g.id);
            if !blockers.is_empty() {
                s.push_str(&format!("  cone:   {}\n", blockers.join(", ")));
            }
            for r in self.records_of(&g.id) {
                s.push_str(&format!("  record: {}\n", r));
            }
            if !g.hints.is_empty() {
                s.push_str(&format!("  hints:  {}\n", g.hints.join("; ")));
            }
            return Some(s);
        }
        if !target_exists(store, target) {
            return None;
        }
        let mut s = format!("{}: a change here opens\n", target);
        for (kind, t, via) in opened_by_change(store, target) {
            s.push_str(&format!("  {:<18} {:<36} via {}\n", kind, t, via));
        }
        let recomputed = recomputed_by_change(store, target);
        if !recomputed.is_empty() {
            s.push_str(&format!(
                "  recomputed at commit: {}\n",
                recomputed.join(", ")
            ));
        }
        Some(s)
    }

    // The whole board, one line per goal: ready first, then waiting, blocked, parked,
    // failed.
    pub fn render(&self) -> String {
        let mut rows: Vec<(u8, String)> = self
            .goals
            .iter()
            .map(|g| {
                let rank = match (&g.state, self.is_ready(&g.id)) {
                    (GoalState::Open, true) => 0,
                    (GoalState::Parked, _) => 3,
                    (GoalState::Open, false) => 1,
                    (GoalState::Blocked { .. }, _) => 2,
                    (GoalState::Failed { .. }, _) => 4,
                };
                let readiness = match self.readiness.get(&g.id) {
                    Some(Ready::Ready) => "ready".to_string(),
                    Some(Ready::Blocked(r)) => r.clone(),
                    None => String::new(),
                };
                (
                    rank,
                    format!(
                        "{}  {}, {}, {}, {}",
                        g.id,
                        g.class,
                        if g.mandatory { "mandatory" } else { "optional" },
                        state_word(&g.state),
                        readiness
                    ),
                )
            })
            .collect();
        rows.sort();
        rows.into_iter()
            .map(|(_, r)| r)
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ---- the per-target work item the serving claims ----

    // The work items one batch runs through the serving, in order.
    pub fn work_items(&self, batch: &Batch) -> Vec<WorkItem> {
        let mut items: Vec<WorkItem> = Vec::new();
        for id in &batch.goals {
            let Some(g) = self.goal(id) else { continue };
            let Some(item) = self.work_item(g) else {
                continue;
            };
            match items
                .iter_mut()
                .find(|i| i.task == item.task && i.target == item.target)
            {
                Some(existing) => {
                    for s in item.dirty_sections {
                        if !existing.dirty_sections.contains(&s) {
                            existing.dirty_sections.push(s);
                        }
                    }
                    for a in item.stale_anchors {
                        if !existing.stale_anchors.contains(&a) {
                            existing.stale_anchors.push(a);
                        }
                    }
                }
                None => items.push(item),
            }
        }
        items
    }

    fn work_item(&self, g: &Goal) -> Option<WorkItem> {
        let task = goals::legacy_task(&g.kind)?;
        Some(match g.kind.as_str() {
            "reconcile-section" => {
                let (doc, sec) = split_section_ref(&g.target)?;
                WorkItem {
                    task: task.into(),
                    target: doc,
                    dirty_sections: vec![sec],
                    stale_anchors: g.change["staleAnchors"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    proposals: Vec::new(),
                }
            }
            "place-anchors" => WorkItem {
                task: task.into(),
                target: g.target.clone(),
                dirty_sections: Vec::new(),
                stale_anchors: Vec::new(),
                proposals: g.change["anchors"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            "rejudge-pair" => {
                let changed = g.change["revised"]
                    .as_str()
                    .or(g.change["created"].as_str())
                    .or(g.change["survivor"].as_str())
                    .map(String::from)
                    .or_else(|| goals::pair_members(&g.target).map(|(a, _)| a.to_string()))?;
                WorkItem::new(task, &changed)
            }
            _ => WorkItem::new(task, &g.target),
        })
    }

    // The work item for a target the serving names (a goal id, a document, a node),
    // or the first claimable batch's first item.
    pub fn find_item(&self, target: Option<&str>) -> Option<WorkItem> {
        match target {
            None => {
                let b = self.batches.first()?;
                self.work_items(b).into_iter().next()
            }
            Some(t) => {
                if let Some(g) = self.goal(t) {
                    if !self.is_ready(&g.id) {
                        return None;
                    }
                    let b = self.batches.iter().find(|b| b.goals.contains(&g.id))?;
                    return self.work_items(b).into_iter().find(|i| {
                        self.work_item(g)
                            .is_some_and(|w| w.task == i.task && w.target == i.target)
                    });
                }
                for b in &self.batches {
                    for item in self.work_items(b) {
                        if item.target == t || item.task == t {
                            return Some(item);
                        }
                    }
                }
                None
            }
        }
    }

    // Whether an open goal still maps to the serving's work item: the success test of
    // a session, read from the board instead of the agent's word.
    pub fn item_open(&self, item: &WorkItem) -> bool {
        self.goals
            .iter()
            .filter(|g| is_open(g))
            .any(|g| match self.work_item(g) {
                Some(w) => w.task == item.task && w.target == item.target,
                None => false,
            })
    }
}

fn state_word(s: &GoalState) -> String {
    match s {
        GoalState::Open => "open".into(),
        GoalState::Parked => "parked".into(),
        GoalState::Blocked { on } => format!("blocked ({})", on),
        GoalState::Failed { reason } => format!("failed ({})", reason),
    }
}

fn condense_change(v: &Value) -> String {
    let s = v.to_string();
    crate::llm::truncate(&s, 200)
}

fn entity_depth(store: &Store, id: &str) -> usize {
    let mut depth = 0;
    let mut cur = store.graph.entities.get(id).and_then(|e| e.parent.clone());
    while let Some(p) = cur {
        depth += 1;
        if depth > 32 {
            break;
        }
        cur = store.graph.entities.get(&p).and_then(|e| e.parent.clone());
    }
    depth
}

fn section_order(store: &Store, target: &str) -> usize {
    split_section_ref(target)
        .and_then(|(d, s)| {
            store
                .docs
                .get(&d)
                .and_then(|r| r.sections.get(&s).map(|x| x.lines[0]))
        })
        .unwrap_or(usize::MAX)
}

// The keys a goal's locality joins on: its document, its entities, its requirements,
// its view. The first key labels the locality. Ledger goals join through their
// entity's component group root, so the ready goals of one component subtree form
// one batch (docs/consumers/gen.md#grouping-by-component).
fn locality_keys(store: &Store, g: &Goal) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    fn entity_keys_into(store: &Store, keys: &mut Vec<String>, ids: &[String]) {
        for e in ids {
            keys.push(format!("ent {}", store.resolve_id(e)));
        }
    }
    let mut entity_keys = |ids: &[String]| entity_keys_into(store, &mut keys, ids);
    let mut own: Vec<String> = Vec::new();
    match g.kind.as_str() {
        "place-anchors" => own.push(format!("doc {}", g.target)),
        "reconcile-section" => {
            if let Some(d) = target_doc(&g.target) {
                own.push(format!("doc {}", d));
            }
        }
        "rejudge-pair" => {
            if let Some((a, b)) = goals::pair_members(&g.target) {
                let mut ents: Vec<String> = Vec::new();
                for r in [a, b] {
                    if let Some(req) = store.graph.requirements.get(r) {
                        ents.extend(req.entities.iter().cloned());
                    }
                }
                entity_keys(&ents);
                own.push(format!("req {}", a));
            }
        }
        "review-entity" | "abstract-entity" | "conform-instance" => {
            // Level locality: an abstraction joins through the level's members,
            // the node's direct children or the scope's parentless entities
            // (docs/compiler/reconciler.md#batching). The scope root is no entity
            // and never a key of its own.
            let mut ents: Vec<String> = if scope_target(&g.target).is_some() {
                Vec::new()
            } else {
                vec![g.target.clone()]
            };
            if g.kind == "abstract-entity" {
                ents.extend(level_members(store, &g.target));
            }
            if g.kind == "conform-instance" {
                if let Some(t) = g.change["type"].as_str() {
                    ents.push(t.to_string());
                }
            }
            if g.kind == "review-entity" {
                for rid in store.requirements_referencing(&g.target) {
                    if let Some(r) = store.graph.requirements.get(&rid) {
                        ents.extend(r.entities.iter().cloned());
                    }
                }
            }
            entity_keys(&ents);
        }
        "retrace" | "curate-view" | "split-view" => {
            own.push(format!("view {}", g.target));
            if let Some(v) = store.graph.views.get(&g.target) {
                let mut ents: Vec<String> = Vec::new();
                for m in &v.members {
                    if store.graph.entities.contains_key(m) {
                        ents.push(m.clone());
                    } else if let Some(r) = store.graph.requirements.get(m) {
                        ents.extend(r.entities.iter().cloned());
                    }
                }
                entity_keys(&ents);
            } else if let Some(r) = store.graph.requirements.get(&g.target) {
                entity_keys(&r.entities);
            } else {
                entity_keys(&[g.target.clone()]);
            }
        }
        "declare-edges" => {
            if let Some(r) = store.graph.requirements.get(&g.target) {
                entity_keys(&r.entities);
            }
        }
        "dedupe-candidates" => {
            if let Some((a, b)) = goals::pair_members(&g.target) {
                entity_keys(&[a.to_string(), b.to_string()]);
            }
        }
        "bind" | "verify" => {
            if let Some(e) = g.change["entity"].as_str() {
                let id = store.resolve_id(e).to_string();
                own.push(format!("ent {}", crate::gen::group_root(store, &id)));
            }
        }
        "generate" => own.push(format!("ent {}", crate::gen::group_root(store, &g.target))),
        _ => {}
    }
    drop(entity_keys);
    // The kind's own key labels the locality; the entity keys join it.
    own.extend(keys);
    if own.is_empty() {
        own.push(format!("goal {}", g.id));
    }
    own
}

// The goals a change to a target would open, walking the stored references from it.
// Mirrors docs/frontends/cli.md#jazyk-explain.
pub fn opened_by_change(store: &Store, target: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut push = |kind: &str, t: &str, via: &str| {
        let row = (kind.to_string(), t.to_string(), via.to_string());
        if !out.contains(&row) {
            out.push(row);
        }
    };
    let views_listing = |id: &str| -> Vec<String> {
        store
            .graph
            .views
            .iter()
            .filter(|(_, v)| {
                !v.default
                    && v.members
                        .iter()
                        .chain(v.collapse.iter())
                        .any(|m| store.resolve_id(m) == id)
            })
            .map(|(vid, _)| vid.clone())
            .collect()
    };
    if let Some((doc, sec)) = split_section_ref(target) {
        push("reconcile-section", target, "section-dirty");
        for (rid, r) in &store.graph.requirements {
            if r.anchored_at(&doc, &sec) {
                for n in store.pair_review_neighbors(rid) {
                    push("rejudge-pair", &goals::pair_target(rid, &n), "entities");
                }
                for e in &r.entities {
                    push("review-entity", store.resolve_id(e), "entities");
                }
                for v in views_listing(rid) {
                    push("retrace", &v, "members");
                }
            }
        }
        return out;
    }
    if store.docs.contains_key(target) {
        if let Some(rec) = store.docs.get(target) {
            for r in rec.sections.keys() {
                push(
                    "reconcile-section",
                    &format!("{}#{}", target, r),
                    "section-dirty",
                );
            }
        }
        return out;
    }
    if let Some(r) = store.graph.requirements.get(target) {
        for n in store.pair_review_neighbors(target) {
            push("rejudge-pair", &goals::pair_target(target, &n), "entities");
        }
        for e in &r.entities {
            push("review-entity", store.resolve_id(e), "entities");
        }
        for v in views_listing(target) {
            push("retrace", &v, "members");
        }
        push("bind", target, "ledger");
        return out;
    }
    if store.graph.entities.contains_key(target) {
        push("review-entity", target, "entity-changed");
        for (inst, ty) in crate::derive::instance_types(store) {
            if ty == target {
                push("conform-instance", &inst, "instantiation");
            }
        }
        for v in views_listing(target) {
            push("retrace", &v, "members");
        }
        for (cid, c) in &store.graph.entities {
            if c.parent.as_deref() == Some(target) {
                push("review-entity", cid, "parent");
            }
        }
        for (rid, r) in &store.graph.requirements {
            if r.entities.iter().any(|e| store.resolve_id(e) == target) {
                push("bind", rid, "ledger");
            }
        }
        push("generate", target, "ledger");
    }
    if store.graph.views.contains_key(target) {
        push("curate-view", target, "query");
    }
    out
}

// The bubbling preview: the goals a staged changeset will open at commit, derived
// over the snapshot plus the staged ops, one human line each. The tool serving
// appends these to a mutating reply. Mirrors docs/compiler/reconciler.md#bubbling.
pub fn staged_opens(store: &Store, staged: &[crate::store::Op]) -> Vec<String> {
    use crate::store::Op;
    let mut out: Vec<String> = Vec::new();
    let mut push = |kind: &str, target: &str, why: &str| {
        let line = format!("{} {} ({})", kind, target, why);
        if !out.contains(&line) && out.len() < 12 {
            out.push(line);
        }
    };
    let views_listing = |id: &str| -> Vec<String> {
        store
            .graph
            .views
            .iter()
            .filter(|(_, v)| {
                !v.default
                    && v.members
                        .iter()
                        .chain(v.collapse.iter())
                        .any(|m| store.resolve_id(m) == id)
            })
            .map(|(vid, _)| vid.clone())
            .collect()
    };
    let derived_from = |id: &str| -> Vec<String> {
        let names = |p: &Option<crate::model::Provenance>| match p {
            Some(crate::model::Provenance::Derived { from, .. }) => {
                from.iter().any(|f| store.resolve_id(f) == id)
            }
            _ => false,
        };
        let mut v: Vec<String> = store
            .graph
            .entities
            .iter()
            .filter(|(_, e)| names(&e.provenance))
            .map(|(eid, _)| eid.clone())
            .collect();
        v.extend(
            store
                .graph
                .requirements
                .iter()
                .filter(|(_, r)| r.source.is_none() && names(&r.provenance))
                .map(|(rid, _)| rid.clone()),
        );
        v
    };
    for op in staged {
        match op {
            Op::DeleteRequirement { id, .. } => {
                for v in views_listing(id) {
                    push("retrace", &v, "member gone");
                }
                for n in derived_from(id) {
                    push("retrace", &n, "justification gone");
                }
                if let Some(r) = store.graph.requirements.get(id) {
                    for e in &r.entities {
                        push("review-entity", store.resolve_id(e), "statement gone");
                    }
                }
            }
            Op::DeleteEntity { id, .. } => {
                for v in views_listing(id) {
                    push("retrace", &v, "member gone");
                }
                for n in derived_from(id) {
                    push("retrace", &n, "justification gone");
                }
            }
            Op::UpdateRequirement { id, statement, .. } if statement.is_some() => {
                for n in store.pair_review_neighbors(id) {
                    push("rejudge-pair", &goals::pair_target(id, &n), "revised");
                }
                if let Some(r) = store.graph.requirements.get(id) {
                    for e in &r.entities {
                        push("review-entity", store.resolve_id(e), "requirements changed");
                    }
                }
            }
            Op::MergeEntities { keep, .. } => {
                push("review-entity", store.resolve_id(keep), "merged");
            }
            Op::CreateRequirement { id, requirement } => {
                if store.graph.requirements.contains_key(id) {
                    for n in store.pair_review_neighbors(id) {
                        push("rejudge-pair", &goals::pair_target(id, &n), "revised");
                    }
                }
                for e in &requirement.entities {
                    push("review-entity", store.resolve_id(e), "requirements changed");
                }
            }
            _ => {}
        }
    }
    out
}

// The derived data a commit touching the target recomputes.
pub fn recomputed_by_change(store: &Store, target: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let ids: Vec<String> = match store.graph.requirements.get(target) {
        Some(r) => r
            .entities
            .iter()
            .map(|e| store.resolve_id(e).to_string())
            .collect(),
        None => vec![target.to_string()],
    };
    for id in &ids {
        for (rid, rel) in &store.graph.relationships {
            if rel.members.contains(id) && !out.contains(rid) {
                out.push(rid.clone());
            }
        }
        let sm = format!("sm:{}", crate::derive::entity_slug(id));
        if store.graph.state_machines.contains_key(&sm) && !out.contains(&sm) {
            out.push(sm);
        }
        for (vid, v) in &store.graph.views {
            if v.default
                && v.members
                    .iter()
                    .any(|m| store.resolve_id(m) == id || m == target)
                && !out.contains(vid)
            {
                out.push(vid.clone());
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::store::{self, RecordBatch};

    pub(crate) fn project_for(store: &Store) -> Project {
        Project {
            out: store.out.clone(),
            ..Default::default()
        }
    }

    pub(crate) fn record(
        store: &mut Store,
        generation: u64,
        kind: &str,
        subject: &str,
        via: &str,
        detail: Value,
    ) {
        let mut batch = RecordBatch::new(generation);
        batch.push(0, kind, subject, via, detail);
        for r in batch.records() {
            store.status.record_change(r.clone());
        }
    }

    // Compile released, generation manual: the graph goals run, the ledger goals wait.
    pub(crate) fn control_auto() -> Control {
        Control {
            compile: "auto".into(),
            generate: "manual".into(),
            worker: "any".into(),
            released: Default::default(),
        }
    }

    fn derive(store: &Store) -> Board {
        Board::derive(store, &project_for(store), &control_auto())
    }

    // The showcase with every section covered and a generation behind it: the store a
    // converged build leaves. The document carries every requirement's quote so no
    // anchor reads stale, and the derived data (relationships, machines, default
    // views) stands as a commit leaves it.
    pub(crate) fn settled_store() -> Store {
        let mut s = crate::derive::tests::showcase_store();
        let text = "# Shop\n\nThe shop.\nThe order service provides the checkout API.\nThe inventory service provides the stock API.\nA shopping cart holds one or more order items.\nThe shop shall confirm checkout within 2 seconds.\nThe shop is deployed in the EU region.\n\n## Checkout\n\nThe customer submits the shopping cart through the checkout API.\nWhen checkout succeeds, the order service reserves stock through the stock API.\n\n## Orders\n\nAn order carries a total and a currency.\nWhen payment succeeds, the order becomes paid.\nIf payment is declined, then the order is held for review.\n\n## Examples\n\nAna, a gold-tier customer, keeps 3 items in her cart, priced in EUR.\n";
        s.docs.insert(
            "shop.md".into(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
        let mut batch = RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        s.status.generation = 3;
        let docs: Vec<String> = s.docs.keys().cloned().collect();
        for d in docs {
            let rec = s.docs.get_mut(&d).unwrap();
            let refs: Vec<String> = rec.sections.keys().cloned().collect();
            for r in refs {
                rec.coverage.insert(
                    r,
                    Coverage {
                        state: "covered".into(),
                        note: None,
                        claimed_by: Some("g1".into()),
                    },
                );
            }
        }
        s.status.changes.clear();
        s
    }

    // The pair estimate follows the statements it will load, never a flat guess.
    #[test]
    fn rejudge_pair_estimate_follows_the_statements() {
        let mut s = Store::default();
        let mut req = |id: &str, statement: &str, n: usize| {
            s.graph.requirements.insert(
                id.into(),
                crate::model::Requirement {
                    statement: statement.into(),
                    entities: (0..n).map(|i| format!("ent:e{}", i)).collect(),
                    ..Default::default()
                },
            );
        };
        req("req:a", "Short.", 1);
        req("req:b", "Also short.", 1);
        req(
            "req:c",
            &"A long statement about approvals and thresholds. ".repeat(8),
            4,
        );
        req(
            "req:d",
            &"Another long statement naming several entities and bands. ".repeat(8),
            4,
        );
        let g = |target: &str| Goal {
            id: format!("g:rejudge-pair:{}", target),
            kind: "rejudge-pair".into(),
            class: "compile".into(),
            mandatory: true,
            target: target.into(),
            unit: "pair".into(),
            change: serde_json::json!({}),
            cause: None,
            state: GoalState::Open,
            hints: Vec::new(),
        };
        let short = estimate(&s, &g("req:a~req:b"));
        let long = estimate(&s, &g("req:c~req:d"));
        assert!(long > short, "{} vs {}", long, short);
        assert!(long > 1_400, "{}", long);
    }

    #[test]
    fn a_clean_store_derives_zero_goals() {
        let s = settled_store();
        let b = derive(&s);
        assert!(
            b.open_goals().is_empty(),
            "{:?}",
            b.open_goals().iter().map(|g| &g.id).collect::<Vec<_>>()
        );
        let v = b.verdict();
        assert!(v.converged());
        assert_eq!(v.open, 0);
        assert!(
            v.blocked > 0,
            "the ledger goals ride as blocked on the generate release"
        );
        assert!(
            b.summary_line().starts_with("compile: 0 goals, "),
            "{}",
            b.summary_line()
        );
        assert!(b.batches.is_empty());
    }

    // Ledger goals join through the component group root: the bind, generate, and
    // verify goals of one component subtree share one locality, so they batch into
    // one session. Mirrors docs/consumers/gen.md#grouping-by-component.
    #[test]
    fn ledger_goals_join_their_component_group() {
        let mut s = Store::default();
        for (id, parent) in [
            ("ent:sys", None),
            ("ent:svc", Some("ent:sys")),
            ("ent:svc-part", Some("ent:svc")),
        ] {
            s.graph.entities.insert(
                id.into(),
                Entity {
                    name: id.into(),
                    parent: parent.map(String::from),
                    ..Default::default()
                },
            );
        }
        let goal = |kind: &str, target: &str, change: Value| Goal {
            id: format!("g:{}:{}", kind, target),
            kind: kind.into(),
            class: "compile".into(),
            mandatory: true,
            target: target.into(),
            unit: "entity".into(),
            change,
            cause: None,
            state: GoalState::Open,
            hints: Vec::new(),
        };
        let gen_part = goal("generate", "ent:svc-part", json!({"goal": "generate"}));
        let bind_part = goal("bind", "req:shop-9", json!({"entity": "ent:svc-part"}));
        let verify_svc = goal("verify", "req:shop-9", json!({"entity": "ent:svc"}));
        assert_eq!(locality_keys(&s, &gen_part)[0], "ent ent:svc");
        assert_eq!(locality_keys(&s, &bind_part)[0], "ent ent:svc");
        assert_eq!(locality_keys(&s, &verify_svc)[0], "ent ent:svc");
        // The system generates alone.
        let gen_sys = goal("generate", "ent:sys", json!({"goal": "generate"}));
        assert_eq!(locality_keys(&s, &gen_sys)[0], "ent ent:sys");
    }

    #[test]
    fn goals_keep_their_identity_across_two_derivations_of_one_record() {
        let mut s = settled_store();
        record(
            &mut s,
            7,
            store::CHANGE_ENTITY,
            "ent:order",
            "entities",
            json!({"requirements": ["req:shop-5"]}),
        );
        let a = derive(&s);
        let b = derive(&s);
        let ga = a.goal("g:review-entity:ent:order").expect("derived");
        let gb = b.goal("g:review-entity:ent:order").expect("derived again");
        assert_eq!(ga.change, gb.change);
        assert_eq!(ga.cause, gb.cause);
        assert_eq!(ga.cause.as_ref().map(|c| c.generation), Some(7));
        assert_eq!(a.records_of(&ga.id), b.records_of(&gb.id));
        assert_eq!(a.records_of(&ga.id).len(), 1);
        // A parked entry with the same change keeps the goal parked and first in line.
        s.status.parked.push(ga.clone());
        let c = derive(&s);
        assert_eq!(c.goal(&ga.id).unwrap().state, GoalState::Parked);
        assert!(c.dropped_parked.is_empty());
        assert_eq!(c.counts().parked, 1);
        assert_eq!(c.counts().open, 1);
    }

    #[test]
    fn a_gc_goal_waits_for_the_compile_goal_in_its_cone() {
        let mut s = settled_store();
        // Order is over its requirement limit (bumped down by a record), and a section
        // anchoring one of its requirements is dirty.
        record(
            &mut s,
            9,
            store::CHANGE_THRESHOLD_CROSSED,
            "ent:order",
            "limits",
            json!({"limit": "requirements-per-entity", "count": 54, "soft": 50, "hard": 80, "level": "soft", "goal": "abstract-entity"}),
        );
        record(
            &mut s,
            10,
            store::CHANGE_SECTION_DIRTY,
            "shop.md#/shop/orders",
            "section",
            json!({"added": 1}),
        );
        let b = derive(&s);
        let gc = b
            .goal("g:abstract-entity:ent:order")
            .expect("gc goal derived");
        assert!(!gc.mandatory);
        assert_eq!(gc.class, "gc");
        let blockers = b.cone_blockers(&gc.id);
        assert!(
            blockers.contains(&"g:reconcile-section:shop.md#/shop/orders".to_string()),
            "{:?}",
            blockers
        );
        assert!(!b.is_ready(&gc.id));
        // Cover the section again at a later generation: the cone quiets and the GC
        // goal is ready.
        s.docs.get_mut("shop.md").unwrap().coverage.insert(
            "/shop/orders".into(),
            Coverage {
                state: "covered".into(),
                note: None,
                claimed_by: Some("g11".into()),
            },
        );
        let b = derive(&s);
        assert!(b.cone_blockers("g:abstract-entity:ent:order").is_empty());
        assert!(
            b.is_ready("g:abstract-entity:ent:order"),
            "{:?}",
            b.readiness.get("g:abstract-entity:ent:order")
        );
        assert!(
            b.lapsed.iter().any(|id| id.starts_with("c10-")),
            "the dirty record lapsed: {:?}",
            b.lapsed
        );
        assert_eq!(b.batches.len(), 1);
        assert_eq!(b.batches[0].id, "b3-1");
        assert_eq!(b.batches[0].class, Class::Gc);
    }

    // Mandatory GC goals run before optional ones within a burst: the batch holding
    // the goal past its hard limit ranks ahead of the batch holding the one over its
    // soft limit, whatever their localities sort like.
    // Mirrors docs/compiler/reconciler.md#gc-gating.
    #[test]
    fn mandatory_gc_batches_rank_before_optional_ones() {
        let mut s = settled_store();
        // Optional: the entity's locality sorts first by name.
        record(
            &mut s,
            9,
            store::CHANGE_THRESHOLD_CROSSED,
            "ent:order",
            "limits",
            json!({"limit": "requirements-per-entity", "count": 54, "soft": 50, "hard": 80, "level": "soft", "goal": "abstract-entity"}),
        );
        // Mandatory: a curated view past its hard limit.
        s.graph.views.insert(
            "view:class/zoo".into(),
            View {
                kind: "class".into(),
                title: "Zoo".into(),
                members: vec!["ent:order".into()],
                ..Default::default()
            },
        );
        record(
            &mut s,
            10,
            store::CHANGE_THRESHOLD_CROSSED,
            "view:class/zoo",
            "limits",
            json!({"limit": "members-per-structural-view", "count": 31, "soft": 20, "hard": 30, "level": "hard", "goal": "split-view"}),
        );
        let b = derive(&s);
        let optional = "g:abstract-entity:ent:order";
        let mandatory = "g:split-view:view:class/zoo";
        assert!(!b.goal(optional).unwrap().mandatory);
        assert!(b.goal(mandatory).unwrap().mandatory);
        assert!(b.is_ready(optional), "{:?}", b.readiness.get(optional));
        assert!(b.is_ready(mandatory), "{:?}", b.readiness.get(mandatory));
        let at = |goal: &str| {
            b.batches
                .iter()
                .position(|batch| batch.goals.iter().any(|g| g == goal))
                .expect("batched")
        };
        assert!(
            at(mandatory) < at(optional),
            "mandatory first: {:?}",
            b.batches.iter().map(|x| &x.goals).collect::<Vec<_>>()
        );
        assert!(b.batches.iter().all(|x| x.class == Class::Gc));
    }

    #[test]
    fn the_hard_threshold_escalates_to_mandatory() {
        let mut s = settled_store();
        record(
            &mut s,
            9,
            store::CHANGE_THRESHOLD_CROSSED,
            "view:class/public",
            "limits",
            json!({"limit": "members-per-structural-view", "count": 23, "soft": 20, "hard": 30, "level": "soft", "goal": "split-view"}),
        );
        s.graph.views.insert(
            "view:class/public".into(),
            View {
                kind: "class".into(),
                title: "Public".into(),
                ..Default::default()
            },
        );
        let b = derive(&s);
        let g = b.goal("g:split-view:view:class/public").unwrap();
        assert!(!g.mandatory);
        let v = b.verdict();
        assert!(v.converged());
        assert_eq!(v.optional, 1);
        record(
            &mut s,
            12,
            store::CHANGE_THRESHOLD_CROSSED,
            "view:class/public",
            "limits",
            json!({"limit": "members-per-structural-view", "count": 31, "soft": 20, "hard": 30, "level": "hard", "goal": "split-view"}),
        );
        let b = derive(&s);
        let g = b.goal("g:split-view:view:class/public").unwrap();
        assert!(g.mandatory, "{:?}", g.change);
        let v = b.verdict();
        assert_eq!(v.state, "incomplete");
        assert_eq!((v.open, v.failed, v.optional), (1, 0, 0));
    }

    // A split-view goal on a level's view yields to the fan-out goal on that level:
    // blocked naming it while the fan-out is open or parked, back under the cone rule
    // once it is gone; the fan-out never waits for the view. Mirrors
    // docs/compiler/reconciler.md#gc-gating.
    #[test]
    fn a_level_views_split_yields_to_the_levels_fan_out() {
        let mut s = settled_store();
        let scope = format!("{}public", SCOPE_TARGET_PREFIX);
        let lv = crate::derive::level_view_id(&s, &scope).expect("the top level has a view");
        if !s.graph.views.contains_key(&lv) {
            let kind = lv.trim_start_matches("view:").split('/').next().unwrap().to_string();
            s.graph.views.insert(
                lv.clone(),
                View {
                    kind,
                    title: "Public".into(),
                    ..Default::default()
                },
            );
        }
        record(
            &mut s,
            9,
            store::CHANGE_THRESHOLD_CROSSED,
            &lv,
            "limits",
            json!({"limit": "members-per-structural-view", "count": 23, "soft": 20, "hard": 30, "level": "soft", "goal": "split-view"}),
        );
        record(
            &mut s,
            9,
            store::CHANGE_THRESHOLD_CROSSED,
            &scope,
            "limits",
            json!({"limit": "children-per-entity", "count": 11, "soft": 9, "hard": 15, "level": "soft", "goal": "abstract-entity"}),
        );
        let b = derive(&s);
        let split = format!("g:split-view:{}", lv);
        let fan_out = format!("g:abstract-entity:{}", scope);
        assert!(b.goal(&fan_out).is_some(), "fan-out goal derived");
        assert_eq!(b.view_level(&lv), Some(scope.as_str()));
        assert!(!b.is_ready(&split));
        let reason = b.readiness[&split].reason().unwrap_or_default().to_string();
        assert!(
            reason.contains(&format!("fan-out first: {}", fan_out)),
            "{}",
            reason
        );
        let theirs = b.readiness[&fan_out].reason().unwrap_or_default().to_string();
        assert!(!theirs.contains("split-view"), "{}", theirs);
        // The fan-out resolved (its record cleared): the split falls back to the cone.
        s.status
            .clear_changes(&[store::CHANGE_THRESHOLD_CROSSED], &scope);
        let b = derive(&s);
        assert!(b.goal(&fan_out).is_none());
        let reason = b.readiness[&split].reason().unwrap_or_default().to_string();
        assert!(!reason.contains("fan-out first"), "{}", reason);
    }

    // Node localities pack on into one batch until the budget is spent: two pair
    // judgments over disjoint entity neighborhoods ride one session, since a
    // locality is where a batch starts, not where it must end.
    // Mirrors docs/compiler/reconciler.md#batching.
    #[test]
    fn pair_batches_fill_across_node_localities() {
        let mut s = Store::default();
        let text = "# T\n\nThe X serves the Y. The X also names the Y. The P serves the Q. The P also names the Q.\n";
        let mut coverage = BTreeMap::new();
        for r in ["/t"] {
            coverage.insert(
                r.to_string(),
                Coverage {
                    state: "covered".into(),
                    note: None,
                    claimed_by: Some("g1".into()),
                },
            );
        }
        s.docs.insert(
            "t.md".into(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage,
            },
        );
        let mention = |q: &str| crate::model::SourceRef {
            doc: "t.md".into(),
            section: "/t".into(),
            quote: q.into(),
        };
        for (id, name) in [("ent:x", "X"), ("ent:y", "Y"), ("ent:p", "P"), ("ent:q", "Q")] {
            s.graph.entities.insert(
                id.into(),
                Entity {
                    name: name.into(),
                    mentions: vec![mention(name)],
                    ..Default::default()
                },
            );
        }
        let req = |statement: &str, ents: &[&str], quote: &str| Requirement {
            statement: statement.into(),
            entities: ents.iter().map(|e| e.to_string()).collect(),
            source: Some(mention(quote)),
            ..Default::default()
        };
        s.graph.requirements.insert(
            "req:t-1".into(),
            req("The X serves the Y.", &["ent:x", "ent:y"], "The X serves the Y."),
        );
        s.graph.requirements.insert(
            "req:t-2".into(),
            req("The X also names the Y.", &["ent:x", "ent:y"], "The X also names the Y."),
        );
        s.graph.requirements.insert(
            "req:t-3".into(),
            req("The P serves the Q.", &["ent:p", "ent:q"], "The P serves the Q."),
        );
        s.graph.requirements.insert(
            "req:t-4".into(),
            req("The P also names the Q.", &["ent:p", "ent:q"], "The P also names the Q."),
        );
        s.status.generation = 5;
        for rid in ["req:t-1", "req:t-3"] {
            record(
                &mut s,
                5,
                store::CHANGE_REQ_REVISED,
                rid,
                "fields",
                json!({}),
            );
        }
        let b = derive(&s);
        let pairs: Vec<&Goal> = b
            .goals
            .iter()
            .filter(|g| g.kind == "rejudge-pair")
            .collect();
        assert_eq!(pairs.len(), 2, "{:?}", pairs.iter().map(|g| &g.id).collect::<Vec<_>>());
        for g in &pairs {
            assert!(b.is_ready(&g.id), "{:?}", b.readiness.get(&g.id));
        }
        // Two neighborhoods (x,y and p,q), one batch.
        let localities = b.localities(&s, &pairs);
        assert_eq!(localities.len(), 2, "{:?}", localities);
        assert_eq!(b.batches.len(), 1, "{:?}", b.batches);
        assert_eq!(b.batches[0].goals.len(), 2);
    }

    #[test]
    fn two_documents_never_share_a_batch() {
        let mut s = crate::derive::tests::showcase_store();
        s.status.generation = 3;
        let text = "# Other\n\nother body\n\n## Part\n\npart body\n";
        s.docs.insert(
            "other.md".into(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
        let b = derive(&s);
        let sections: Vec<&Goal> = b
            .goals
            .iter()
            .filter(|g| g.kind == "reconcile-section")
            .collect();
        assert!(sections.len() >= 5, "{}", sections.len());
        // No roots: documents are levels in path order, other.md before shop.md.
        assert!(b.is_ready("g:reconcile-section:other.md#/other"));
        assert!(!b.is_ready("g:reconcile-section:shop.md#/shop"));
        for batch in &b.batches {
            let docs: BTreeSet<String> = batch
                .goals
                .iter()
                .filter_map(|id| b.goal(id))
                .filter_map(|g| target_doc(&g.target))
                .collect();
            assert_eq!(docs.len(), 1, "{:?}", batch);
            assert!(batch.goals.len() <= 3, "{:?}", batch);
        }
        let items = b.work_items(&b.batches[0]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].task, "reconcile-doc");
        assert_eq!(items[0].target, "other.md");
        assert_eq!(items[0].dirty_sections.len(), 2);
        assert!(b.item_open(&items[0]));
        assert_eq!(b.find_item(None).map(|i| i.target), Some("other.md".into()));
    }

    #[test]
    fn cones_walk_up_and_down_never_sideways() {
        let s = crate::derive::tests::showcase_store();
        let c = cone(&s, "ent:order-item");
        assert!(
            c.nodes.contains("ent:order-service"),
            "parent chain is upward"
        );
        assert!(
            c.nodes.contains("req:shop-6"),
            "the requirement naming it is downward"
        );
        assert!(
            c.sections.contains("shop.md#/shop"),
            "its anchoring section"
        );
        assert!(
            !c.nodes.contains("ent:customer"),
            "a sibling reached sideways is out: {:?}",
            c.nodes
        );
        let top = cone(&s, "ent:shop");
        assert!(
            top.nodes.contains("ent:order"),
            "the system's cone holds its descendants"
        );
        assert!(top.nodes.contains("req:shop-1"));
    }

    // The cone of `scope:<scope>` is the downward walk from every parentless entity of
    // the scope: the whole scope, and nothing of another scope.
    // Mirrors docs/compiler/reconciler.md#cones.
    #[test]
    fn the_cone_of_a_scope_target_is_the_whole_scope() {
        let mut s = crate::derive::tests::showcase_store();
        s.graph.entities.insert(
            "ent:ledger".into(),
            Entity {
                name: "Ledger".into(),
                scope: "finance".into(),
                ..Default::default()
            },
        );
        assert_eq!(scope_target("scope:public"), Some("public"));
        assert_eq!(scope_target("ent:shop"), None);
        assert_eq!(scope_target("scope:"), None);
        let root = scope_root(&s, "public");
        assert!(root.contains(&"ent:shop".to_string()), "{:?}", root);
        assert!(root.contains(&"ent:customer".to_string()));
        assert!(
            !root.contains(&"ent:order".to_string()),
            "a child is no root"
        );
        assert!(!root.contains(&"ent:ledger".to_string()), "another scope");
        assert_eq!(level_members(&s, "scope:public"), root);
        assert_eq!(
            level_members(&s, "ent:shop"),
            vec![
                "ent:inventory-service".to_string(),
                "ent:order-service".into()
            ]
        );
        let c = cone(&s, "scope:public");
        for id in [
            "ent:shop",
            "ent:order-service",
            "ent:order",
            "ent:customer",
            "req:shop-1",
        ] {
            assert!(
                c.nodes.contains(id),
                "{} in the scope's cone: {:?}",
                id,
                c.nodes
            );
        }
        assert!(c.sections.contains("shop.md#/shop"));
        assert!(c.docs.contains("shop.md"));
        assert!(!c.nodes.contains("ent:ledger"), "another scope stays out");
        assert!(c.holds_target("req:shop-6"));
        assert!(c.holds_target("shop.md#/shop/orders"));
        assert!(cone(&s, "scope:finance").nodes.contains("ent:ledger"));
        assert!(cone(&s, "scope:nope").nodes.is_empty());
    }

    // A fan-out goal on the scope root is GC work: it waits while a compile goal is
    // open anywhere in the scope, and its locality is the level's members.
    // Mirrors docs/compiler/reconciler.md#fan-out and #gc-gating.
    #[test]
    fn a_fan_out_goal_on_the_root_waits_for_the_compile_goal_in_its_scope() {
        let mut s = settled_store();
        let goal = Goal {
            id: "g:abstract-entity:scope:public".into(),
            kind: "abstract-entity".into(),
            class: "gc".into(),
            mandatory: false,
            target: "scope:public".into(),
            unit: "entity".into(),
            change: json!({"fan_out": 4, "limit": {"soft": 9, "hard": 15}, "candidates": []}),
            cause: None,
            state: GoalState::Open,
            hints: vec!["load scope:public".into()],
        };
        s.status.parked.push(goal.clone());
        record(
            &mut s,
            10,
            store::CHANGE_SECTION_DIRTY,
            "shop.md#/shop/orders",
            "section",
            json!({"added": 1}),
        );
        let b = derive(&s);
        let g = b
            .goal(&goal.id)
            .expect("the parked root goal survives derivation");
        assert_eq!(g.state, GoalState::Parked);
        let blockers = b.cone_blockers(&goal.id);
        assert!(
            blockers.contains(&"g:reconcile-section:shop.md#/shop/orders".to_string()),
            "{:?}",
            blockers
        );
        assert!(!b.is_ready(&goal.id), "{:?}", b.readiness.get(&goal.id));
        // The level's members join the locality; the root itself is no key.
        let keys = locality_keys(&s, g);
        assert!(keys.contains(&"ent ent:shop".to_string()), "{:?}", keys);
        assert!(keys.contains(&"ent ent:customer".to_string()));
        assert!(!keys.iter().any(|k| k.contains("scope:")), "{:?}", keys);
        assert!(estimate(&s, g) > 1_500);
        // Cover the section again: the scope quiets and the root goal is ready.
        s.docs.get_mut("shop.md").unwrap().coverage.insert(
            "/shop/orders".into(),
            Coverage {
                state: "covered".into(),
                note: None,
                claimed_by: Some("g11".into()),
            },
        );
        let b = derive(&s);
        assert!(b.cone_blockers(&goal.id).is_empty());
        assert!(b.is_ready(&goal.id), "{:?}", b.readiness.get(&goal.id));
        assert_eq!(b.batches.len(), 1);
        assert_eq!(b.batches[0].class, Class::Gc);
        assert_eq!(b.batches[0].goals, vec![goal.id.clone()]);
        // A scope with no entities is no target: the parked entry drops.
        let mut empty = settled_store();
        let mut stray = goal.clone();
        stray.id = "g:abstract-entity:scope:nope".into();
        stray.target = "scope:nope".into();
        empty.status.parked.push(stray.clone());
        let b = derive(&empty);
        assert!(b.goal(&stray.id).is_none());
        assert!(b.dropped_parked.contains(&stray.id));
    }

    #[test]
    fn explain_names_the_goals_a_change_opens() {
        let s = settled_store();
        let b = derive(&s);
        let text = b.explain(&s, "ent:customer").expect("known target");
        assert!(text.contains("review-entity"), "{}", text);
        assert!(
            text.contains("conform-instance") && text.contains("ent:ana"),
            "{}",
            text
        );
        assert!(b.explain(&s, "ent:nope").is_none());
    }
}
