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
#[derive(Clone, Debug, Default, PartialEq)]
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

    // Every key `holds_target` answers true for: the nodes (a pair target through
    // either member), the sections, the documents.
    pub fn held_keys(&self) -> impl Iterator<Item = &str> {
        self.nodes
            .iter()
            .chain(self.sections.iter())
            .chain(self.docs.iter())
            .map(String::as_str)
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

// The referrer index of the cone walk, built once per derivation from the stored
// references and read for every node a downward walk visits: what one scan over the
// entities, requirements, views, and sections finds for a node, keyed by the node.
// Never stored. Mirrors docs/compiler/reconciler.md#cones and
// docs/compiler/graph.md#derived-data.
#[derive(Default)]
pub struct Refs {
    // Parent entity to its children.
    children: BTreeMap<String, Vec<String>>,
    // Upstream node to the entities whose derived provenance names it.
    entity_from: BTreeMap<String, Vec<String>>,
    // Upstream node to the entities with an attribute derived from it.
    attribute_from: BTreeMap<String, Vec<String>>,
    // Entity (redirects followed) to the requirements naming it in `entities`, an
    // edge end, or the transition subject.
    requirements_naming: BTreeMap<String, Vec<String>>,
    // Upstream node to the requirements whose derived provenance names it.
    requirement_from: BTreeMap<String, Vec<String>>,
    // Section reference to the requirements anchored in it.
    section_requirements: BTreeMap<String, Vec<String>>,
    // Section reference to the entities mentioned in it.
    section_entities: BTreeMap<String, Vec<String>>,
    // Section reference to its child sections.
    section_children: BTreeMap<String, Vec<String>>,
    // Node (redirects followed) to the views listing it in `members`, `collapse`,
    // or `excluded`.
    view_listing: BTreeMap<String, Vec<String>>,
    // Upstream node to the views whose derived provenance names it.
    view_from: BTreeMap<String, Vec<String>>,
}

fn push_ref(map: &mut BTreeMap<String, Vec<String>>, key: &str, id: &str) {
    map.entry(key.to_string()).or_default().push(id.to_string());
}

impl Refs {
    pub fn build(store: &Store) -> Refs {
        let mut refs = Refs::default();
        for (cid, c) in &store.graph.entities {
            if let Some(p) = c.parent.as_deref() {
                push_ref(&mut refs.children, p, cid);
            }
            for f in from_of(&c.provenance) {
                push_ref(&mut refs.entity_from, &f, cid);
            }
            for a in &c.attributes {
                if let Provenance::Derived { from, .. } = &a.provenance {
                    for f in from {
                        push_ref(&mut refs.attribute_from, f, cid);
                    }
                }
            }
            for m in &c.mentions {
                push_ref(
                    &mut refs.section_entities,
                    &format!("{}#{}", m.doc, m.section),
                    cid,
                );
            }
        }
        for (rid, r) in &store.graph.requirements {
            for e in &r.entities {
                push_ref(&mut refs.requirements_naming, store.resolve_id(e), rid);
            }
            for e in &r.edges {
                push_ref(&mut refs.requirements_naming, store.resolve_id(&e.a), rid);
                push_ref(&mut refs.requirements_naming, store.resolve_id(&e.b), rid);
            }
            if let Some(t) = &r.transition {
                push_ref(
                    &mut refs.requirements_naming,
                    store.resolve_id(&t.subject),
                    rid,
                );
            }
            for f in from_of(&r.provenance) {
                push_ref(&mut refs.requirement_from, &f, rid);
            }
            if let Some(s) = &r.source {
                push_ref(
                    &mut refs.section_requirements,
                    &format!("{}#{}", s.doc, s.section),
                    rid,
                );
            }
        }
        for (vid, v) in &store.graph.views {
            for m in v.members.iter().chain(v.collapse.iter()) {
                push_ref(&mut refs.view_listing, store.resolve_id(m), vid);
            }
            for x in &v.excluded {
                push_ref(&mut refs.view_listing, store.resolve_id(&x.id), vid);
            }
            for f in from_of(&v.provenance) {
                push_ref(&mut refs.view_from, &f, vid);
            }
        }
        for (doc, rec) in &store.docs {
            for (r, s) in &rec.sections {
                if let Some(p) = s.parent.as_deref() {
                    push_ref(
                        &mut refs.section_children,
                        &format!("{}#{}", doc, p),
                        &format!("{}#{}", doc, r),
                    );
                }
            }
        }
        refs
    }

    fn get<'a>(map: &'a BTreeMap<String, Vec<String>>, key: &str) -> &'a [String] {
        map.get(key).map(Vec::as_slice).unwrap_or(&[])
    }
}

// One hop downward: what references the node, read off the index.
fn referrers(store: &Store, refs: &Refs, id: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut take = |map: &BTreeMap<String, Vec<String>>| out.extend_from_slice(Refs::get(map, id));
    if store.graph.entities.contains_key(id) {
        take(&refs.children);
        take(&refs.entity_from);
        take(&refs.attribute_from);
        take(&refs.requirements_naming);
        take(&refs.requirement_from);
    } else if store.graph.requirements.contains_key(id) {
        take(&refs.entity_from);
        take(&refs.requirement_from);
    } else if id.contains('#') {
        take(&refs.section_requirements);
        take(&refs.section_entities);
        take(&refs.section_children);
        return out;
    } else if let Some(rec) = store.docs.get(id) {
        out.extend(rec.sections.keys().map(|r| format!("{}#{}", id, r)));
        return out;
    }
    if store.graph.entities.contains_key(id)
        || store.graph.requirements.contains_key(id)
        || store.graph.views.contains_key(id)
    {
        take(&refs.view_listing);
        take(&refs.view_from);
    }
    out
}

// One hop downward by one scan over the store: the reference the index is checked
// against.
#[cfg(test)]
fn referrers_scan(store: &Store, id: &str) -> Vec<String> {
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
    cone_with(store, &Refs::build(store), target)
}

// The cone over a prebuilt referrer index: one index serves every GC goal of a
// derivation. Mirrors docs/compiler/reconciler.md#cones.
pub fn cone_with(store: &Store, refs: &Refs, target: &str) -> Cone {
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
    // Each walk keeps its own visited set: a node another seed or the other direction
    // reached still contributes its own direction's references here.
    let mut down: BTreeSet<String> = BTreeSet::new();
    for seed in &seeds {
        nodes.insert(seed.clone());
        if scope.is_none() {
            let mut up: BTreeSet<String> = BTreeSet::from([seed.clone()]);
            let mut frontier = vec![seed.clone()];
            while let Some(id) = frontier.pop() {
                for r in referents(store, &id) {
                    if up.insert(r.clone()) {
                        frontier.push(r);
                    }
                }
            }
            nodes.extend(up);
        }
        // The downward walks share one set: referrers reached from any seed are
        // walked down once, which is the same set either way.
        if down.insert(seed.clone()) {
            let mut frontier = vec![seed.clone()];
            while let Some(id) = frontier.pop() {
                for r in referrers(store, refs, &id) {
                    if down.insert(r.clone()) {
                        frontier.push(r);
                    }
                }
            }
        }
    }
    nodes.extend(down);
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
    if crate::md::is_document_target(target) {
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
    let mut levels = crate::derive::doc_levels_from(store, &roots);
    let last = levels.values().copied().max().unwrap_or(0) + 1;
    for (doc, rec) in &store.docs {
        if rec.sections.values().any(|s| s.kind == "blob") {
            levels.insert(doc.clone(), last);
        }
    }
    levels
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
        // The listed requirements load full and each yields two or three criteria.
        "derive-criteria" => {
            900 + 700
                * (g.change["none"].as_array().map(|a| a.len()).unwrap_or(0)
                    + g.change["revised"].as_array().map(|a| a.len()).unwrap_or(0))
        }
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

// Whether a failed goal's entry still covers the goal as re-derived: the same change,
// or no newer cause; for an abstract-entity goal over a limit, a count that has not
// grown by a fifth since the failure (growth alone is not a new subject).
// Mirrors docs/compiler/reconciler.md#parked-and-failed.
pub(crate) fn failure_stands(failed: &Goal, derived: &Goal) -> bool {
    if derived.change == failed.change {
        return true;
    }
    if derived.kind == "abstract-entity" {
        let within = |then: Option<u64>, now: Option<u64>| match (then, now) {
            (Some(then), Some(now)) => Some(now * 5 < then * 6),
            (None, None) => None,
            _ => Some(false),
        };
        let bound = |g: &Goal| {
            (
                g.change["limits"][0]["soft"].clone(),
                g.change["limits"][0]["hard"].clone(),
            )
        };
        let caps = within(
            failed.change["limits"][0]["count"].as_u64(),
            derived.change["limits"][0]["count"].as_u64(),
        )
        .map(|w| w && bound(failed) == bound(derived));
        // The fan-out variant: the coupling candidates move with every requirement
        // on the level, so only the child count and the limits in force (a bump)
        // reopen it. A level declared flat stays failed until it changes.
        let fan_out = within(
            failed.change["fan_out"].as_u64(),
            derived.change["fan_out"].as_u64(),
        )
        .map(|w| w && failed.change["limit"] == derived.change["limit"]);
        match (caps, fan_out) {
            (Some(true), None | Some(true)) | (None, Some(true)) => return true,
            _ => {}
        }
    }
    derived.cause.as_ref().map(|c| c.generation) <= failed.cause.as_ref().map(|c| c.generation)
}

// The state a stored parked entry resumes in. A goal parked by flip detection waits
// on the ruling and is never batched, whatever state the entry was written with: a
// parked goal resumes first, and a session cannot resolve one whose cause is the
// ruling owed (docs/compiler/reconciler.md#flip-detection).
fn parked_state(p: &Goal) -> GoalState {
    let Some(subject) = flip_subject(p) else {
        return GoalState::Parked;
    };
    GoalState::Blocked {
        on: format!("human: unstable-derivation ruling on {}", subject),
    }
}

// The node a flip-parked entry waits on a ruling for; None for any other park.
pub(crate) fn flip_subject(p: &Goal) -> Option<String> {
    if !p.cause.as_ref().is_some_and(|c| c.via == "flip-detection") {
        return None;
    }
    Some(
        p.change["reparent"]
            .as_str()
            .or_else(|| p.change["subject"].as_str())
            .unwrap_or(&p.target)
            .to_string(),
    )
}

// Whether the unstable-derivation ruling on a subject is still owed: an open
// diagnostic naming it carries no answer yet.
fn ruling_owed(store: &Store, subject: &str) -> bool {
    store.graph.diagnostics.values().any(|d| {
        d.lifecycle == "open"
            && d.rule == "unstable-derivation"
            && d.answer.is_none()
            && d.subjects.iter().any(|s| s == subject)
    })
}

// `b<generation>-<n>`, the id a batch lease is filed under.
fn is_batch_id(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('b') else {
        return false;
    };
    let Some((g, n)) = rest.split_once('-') else {
        return false;
    };
    !g.is_empty()
        && !n.is_empty()
        && g.bytes().all(|c| c.is_ascii_digit())
        && n.bytes().all(|c| c.is_ascii_digit())
}

// What a board depends on besides the files: the project's resolved settings.
#[cfg(target_arch = "wasm32")]
fn memo_key(proj: &Project, out: &Path) -> String {
    format!(
        "{:?}|{:?}|{:?}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}|{:?}|{:?}",
        proj.root,
        out,
        proj.set_id,
        proj.config_hash,
        proj.config_error,
        proj.docs_glob,
        proj.roots,
        proj.gen_deliverable,
        proj.gen_code,
        proj.workflow.compile,
        proj.workflow.generate,
        proj.workflow.worker,
        proj.explicit_files,
        proj.set_docs,
    )
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
}

impl Board {
    // Derive the board from a store synced against the documents. Every process
    // computes the same board from the same disk state.
    pub fn derive(store: &Store, proj: &Project, control: &Control) -> Board {
        let gen = GenSettings::resolve(proj);
        // The pair plan is computed once: its open goals stand in for the kind's
        // derivation, its waiting pairs ride on the board.
        let pairs = goals::pair_plan(store);
        let mut goals: Vec<Goal> = REGISTRY
            .iter()
            .filter(|k| k.kind() != "rejudge-pair")
            .flat_map(|k| k.derive_goals(store, &gen))
            .chain(pairs.open)
            .collect();
        goals.sort_by(|a, b| a.id.cmp(&b.id));
        goals.dedup_by(|a, b| a.id == b.id);

        // Parked and failed goals keep their identity (the change) and their state.
        let mut dropped_parked = Vec::new();
        let mut parked_reasons = BTreeMap::new();
        let mut reopened = Vec::new();
        let mut dropped_failed = Vec::new();
        for entry in &store.status.parked {
            let p = &entry.goal;
            let mut kept = true;
            // A pair parked by flip detection stays parked while its ruling is owed,
            // whatever change the goal derives with now (the parked change names the
            // flip, never the goal's own records); answering the ruling clears it.
            // Mirrors docs/compiler/reconciler.md#flip-detection.
            if let Some(subject) = flip_subject(p) {
                if !ruling_owed(store, &subject) {
                    dropped_parked.push(p.id.clone());
                    continue;
                }
                match goals.iter_mut().find(|g| g.id == p.id) {
                    Some(g) => g.state = parked_state(p),
                    None if target_exists(store, &p.target) => {
                        let mut g = p.clone();
                        g.state = parked_state(p);
                        goals.push(g);
                    }
                    None => {
                        dropped_parked.push(p.id.clone());
                        continue;
                    }
                }
                if let Some(r) = &entry.reason {
                    parked_reasons.insert(p.id.clone(), r.clone());
                }
                continue;
            }
            match goals.iter_mut().find(|g| g.id == p.id) {
                Some(g) => {
                    if g.change == p.change || p.change.is_null() {
                        if matches!(g.state, GoalState::Open) {
                            g.state = parked_state(p);
                        }
                    } else {
                        kept = false;
                    }
                }
                None => {
                    // A ledger goal is the ledger's to derive: a parked verify goal
                    // whose row reads verified has nothing left to resume (the slides
                    // example carried two such goals across every run).
                    let ledger_kind = matches!(p.kind.as_str(), "bind" | "generate" | "verify");
                    if target_exists(store, &p.target)
                        && !goals::blocked_on_human(&p.kind)
                        && !ledger_kind
                    {
                        let mut g = p.clone();
                        g.state = parked_state(p);
                        goals.push(g);
                    } else {
                        kept = false;
                    }
                }
            }
            if !kept {
                dropped_parked.push(p.id.clone());
            } else if let Some(r) = &entry.reason {
                parked_reasons.insert(p.id.clone(), r.clone());
            }
        }
        for f in &store.status.failed {
            match goals.iter_mut().find(|g| g.id == f.goal.id) {
                Some(g) => {
                    if failure_stands(&f.goal, g) {
                        g.state = GoalState::Failed {
                            reason: f.reason.clone(),
                        };
                    } else {
                        reopened.push(f.goal.id.clone());
                    }
                }
                None => {
                    // A ledger goal is the ledger's to derive, failed or parked: a
                    // failure about a row that reads current has nothing left to say.
                    // Mirrors docs/compiler/reconciler.md#parked-and-failed.
                    let ledger_kind = LEDGER_KINDS.contains(&f.goal.kind.as_str());
                    if target_exists(store, &f.goal.target) && !ledger_kind {
                        let mut g = f.goal.clone();
                        g.state = GoalState::Failed {
                            reason: f.reason.clone(),
                        };
                        goals.push(g);
                    } else if ledger_kind {
                        dropped_failed.push(f.goal.id.clone());
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

        // Records the goals stand on, and the ones whose evidence lapsed. The records
        // are indexed by subject once; a goal's list keeps the records' own order.
        let mut by_subject: BTreeMap<&str, Vec<(usize, &ChangeRecord)>> = BTreeMap::new();
        for (i, c) in store.status.changes.iter().enumerate() {
            by_subject
                .entry(c.subject.as_str())
                .or_default()
                .push((i, c));
        }
        let mut records: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in &goals {
            let kinds = goals::record_kinds(&g.kind);
            let subjects: Vec<&str> = match goals::pair_members(&g.target) {
                Some((a, b)) => vec![a, b],
                None => vec![g.target.as_str()],
            };
            let mut hits: Vec<(usize, &str)> = subjects
                .iter()
                .flat_map(|s| by_subject.get(s).into_iter().flatten())
                .filter(|(_, c)| kinds.contains(&c.kind.as_str()))
                .map(|(i, c)| (*i, c.id.as_str()))
                .collect();
            hits.sort_unstable();
            hits.dedup();
            records.insert(
                g.id.clone(),
                hits.into_iter().map(|(_, id)| id.to_string()).collect(),
            );
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
        // A document whose place-anchors goal failed stays undecided: its sections
        // wait behind the failure, and an undecided document blocks its own sections
        // and nothing else, so they hold no later tier or link level.
        // Mirrors docs/compiler/goals/place-anchors.md#readiness.
        let undecided_docs: BTreeSet<&str> = goals
            .iter()
            .filter(|g| g.kind == "place-anchors" && matches!(g.state, GoalState::Failed { .. }))
            .map(|g| g.target.as_str())
            .collect();
        for g in goals.iter().filter(|g| is_open(g)) {
            if g.kind == "reconcile-section"
                && target_doc(&g.target).is_some_and(|d| undecided_docs.contains(d.as_str()))
            {
                continue;
            }
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
        // The indices of this derivation: the referrer index every cone reads and the
        // level of every view. Mirrors docs/compiler/graph.md#derived-data.
        let refs = Refs::build(store);
        // The open compile goals by what a cone would hold them through: a node,
        // section, or document target directly, a pair target through either member.
        // Each cone then looks its blockers up instead of testing every goal, and
        // the list keeps the goals' order.
        let mut compile_by_key: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (i, o) in goals.iter().enumerate() {
            if o.class != "compile" || !is_open(o) {
                continue;
            }
            match goals::pair_members(&o.target) {
                Some((a, b)) => {
                    compile_by_key.entry(a).or_default().push(i);
                    compile_by_key.entry(b).or_default().push(i);
                }
                None => compile_by_key.entry(o.target.as_str()).or_default().push(i),
            }
        }
        let mut cone_blockers: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in goals.iter().filter(|g| g.class == "gc") {
            let cone = cone_with(store, &refs, &g.target);
            let mut hits: Vec<usize> = cone
                .held_keys()
                .flat_map(|k| compile_by_key.get(k).into_iter().flatten().copied())
                .collect();
            hits.sort_unstable();
            hits.dedup();
            let blockers: Vec<String> = hits.into_iter().map(|i| goals[i].id.clone()).collect();
            cone_blockers.insert(g.id.clone(), blockers);
        }
        let mut view_levels: BTreeMap<String, String> = BTreeMap::new();
        if goals.iter().any(|g| g.kind == "split-view") {
            let ix = crate::derive::Indices::build(store);
            let levels = crate::derive::view_levels(store, &ix);
            for g in goals.iter().filter(|g| g.kind == "split-view") {
                if let Some(l) = levels.get(&g.target) {
                    view_levels.insert(g.target.clone(), l.clone());
                }
            }
        }

        let by_id: BTreeMap<String, usize> = goals
            .iter()
            .enumerate()
            .map(|(i, g)| (g.id.clone(), i))
            .collect();
        let open_kinds: BTreeSet<(String, String)> = goals
            .iter()
            .filter(|g| is_open(g))
            .map(|g| (g.kind.clone(), g.target.clone()))
            .collect();
        let mut board = Board {
            generation: store.status.generation,
            goals,
            readiness: BTreeMap::new(),
            gated: BTreeSet::new(),
            claimed: BTreeMap::new(),
            held_batches: BTreeSet::new(),
            own: BTreeSet::new(),
            batches: Vec::new(),
            lapsed,
            reopened,
            dropped_parked,
            dropped_failed,
            parked_reasons,
            verdict: store.status.verdict.clone(),
            open_diags: store.open_diag_counts(),
            dangling_diags: store.has_dangling_diags(),
            alignment_pending,
            tier_open,
            doc_levels: doc_levels(store, proj),
            blob_inputs: store
                .docs
                .iter()
                .filter_map(|(doc, rec)| {
                    rec.sections
                        .values()
                        .find(|s| s.kind == "blob")
                        .map(|s| (doc.clone(), crate::input::referring_documents(s)))
                })
                .collect(),
            tier1_open_docs,
            bind_open_by_entity,
            cone_blockers,
            view_levels,
            records,
            by_id,
            open_kinds,
            waiting: pairs.waiting,
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
            board.held_batches = leases.keys().filter(|k| is_batch_id(k)).cloned().collect();
        }
        board.batches = board.form_batches(store, proj);
        board
    }

    // Load the store, align the documents in memory, read the control plane, derive.
    // A read-only derivation never commits: the `edit` and `align` entries are a
    // build's to write (a stray argument read as a document path once emptied a
    // project's section tree through this call).
    // Mirrors docs/compiler/reconciler.md#goal-derivation.
    pub fn compute(proj: &Project, out: &Path) -> Board {
        // The browser engine's tree changes only through its own writes, and a page
        // opening asks for the board several times over one tree: a board of an
        // unchanged tree and project is the last one's copy. A board holding leases is
        // never reused, since a lease lapses with time, not with a write.
        #[cfg(target_arch = "wasm32")]
        {
            thread_local! {
                static LAST: std::cell::RefCell<BTreeMap<String, (u64, Board)>> = const { std::cell::RefCell::new(BTreeMap::new()) };
            }
            let key = memo_key(proj, out);
            let now = crate::vfs::memory_generation();
            if let Some(board) = LAST.with(|l| {
                l.borrow()
                    .get(&key)
                    .filter(|(g, _)| *g == now)
                    .map(|(_, b)| b.clone())
            }) {
                return board;
            }
            let board = Self::compute_uncached(proj, out);
            let now = crate::vfs::memory_generation();
            LAST.with(|l| {
                let mut l = l.borrow_mut();
                l.retain(|_, (g, _)| *g == now);
                if board.claimed.is_empty() && board.held_batches.is_empty() {
                    l.insert(key, (now, board.clone()));
                }
            });
            board
        }
        #[cfg(not(target_arch = "wasm32"))]
        Self::compute_uncached(proj, out)
    }

    fn compute_uncached(proj: &Project, out: &Path) -> Board {
        let mut store = Store::load(out);
        let (parsed, _) = crate::reconcile::parse_all(proj);
        store.sync_docs_in_memory(&parsed);
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
        self.goal(goal_id).is_some_and(is_open)
    }

    // Whether an open goal of the kind stands on the target.
    pub fn open_kind_on(&self, kind: &str, target: &str) -> bool {
        self.open_kinds
            .contains(&(kind.to_string(), target.to_string()))
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
        // The index is built when the goals are final; a scan stands in for a board
        // whose goals moved under it.
        match self.by_id.get(id).and_then(|i| self.goals.get(*i)) {
            Some(g) if g.id == id => Some(g),
            _ => self.goals.iter().find(|g| g.id == id),
        }
    }

    // The change record ids a goal stands on.
    pub fn records_of(&self, goal_id: &str) -> Vec<String> {
        self.records.get(goal_id).cloned().unwrap_or_default()
    }

    // The pair waiting under the degree cap for a goal id or a pair target.
    pub fn waiting_pair(&self, id_or_target: &str) -> Option<&goals::WaitingPair> {
        let id = if id_or_target.starts_with("g:rejudge-pair:") {
            id_or_target.to_string()
        } else {
            goals::goal_id("rejudge-pair", id_or_target)
        };
        self.waiting.get(&id)
    }

    // One line for a waiting pair: that it waits, its score, the full side.
    fn explain_waiting(&self, store: &Store, w: &goals::WaitingPair) -> String {
        let mut s = format!(
            "g:rejudge-pair:{}  rejudge-pair, compile, mandatory, waiting under the cap\n",
            w.target
        );
        s.push_str(&format!(
            "  waits:  score {}; {} hold(s) {} open pairs ({})\n",
            w.score,
            w.full.join(" and "),
            crate::limits::PAIRS_PER_REQUIREMENT_CAP,
            crate::limits::PAIRS_PER_REQUIREMENT
        ));
        s.push_str(&format!(
            "  cause:  g{} mutation {} via {}\n",
            w.cause.generation, w.cause.mutation, w.cause.via
        ));
        if let Some((a, b)) = goals::pair_members(&w.target) {
            for c in store.status.changes.iter().filter(|c| {
                goals::record_kinds("rejudge-pair").contains(&c.kind.as_str())
                    && (c.subject == a || c.subject == b)
            }) {
                s.push_str(&format!("  record: {}\n", c.id));
            }
        }
        s
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
                    // ready counts mandatory goals like open does; an optional ready
                    // batch stays under optional (docs/frontends/mcp.md#compilation-over-mcp).
                    if g.mandatory && self.is_ready(&g.id) {
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
        if !self.waiting.is_empty() {
            s.push_str(&format!(
                ", {} pair(s) waiting under the cap",
                self.waiting.len()
            ));
        }
        s
    }

    // ---- batching ----

    // Hold back goals a build parked mid-run: they stay open on the board (the build
    // writes `parked` at its close) but take no batch, and the batches re-form
    // without them so their locality neighbors still run.
    // Mirrors docs/compiler/reconciler.md#parked-and-failed.
    pub fn withhold(&mut self, ids: &BTreeSet<String>, store: &Store, proj: &Project) {
        let held: Vec<String> = ids
            .iter()
            .filter(|id| self.batches.iter().any(|b| b.goals.contains(id)))
            .cloned()
            .collect();
        if held.is_empty() {
            return;
        }
        for id in held {
            self.readiness.insert(
                id,
                Ready::Blocked("parked in this build; the next build resumes it first".into()),
            );
        }
        self.batches = self.form_batches(store, proj);
    }

    fn form_batches(&self, store: &Store, proj: &Project) -> Vec<Batch> {
        let ready: Vec<&Goal> = self.ready_goals();
        // Generation runs leaf first over the relationships and the containment tree:
        // every ready generate target's position in that order.
        // Mirrors docs/compiler/goals/generate.md#readiness.
        let generate_targets: Vec<String> = ready
            .iter()
            .filter(|g| g.kind == "generate")
            .map(|g| g.target.clone())
            .collect();
        let topo: BTreeMap<String, usize> = crate::gen::generation_order(store, &generate_targets)
            .into_iter()
            .enumerate()
            .map(|(i, t)| (t, i))
            .collect();
        let generate_rank = |g: &Goal| {
            (g.kind == "generate")
                .then(|| topo.get(&g.target).copied())
                .flatten()
        };
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
            let mut criteria = 0usize;
            // The requirements the batch already loads: a pair pays only for the sides
            // the batch does not hold yet (docs/compiler/reconciler.md#batching).
            let mut charged: BTreeSet<String> = BTreeSet::new();
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
                    criteria = 0;
                    charged.clear();
                }
                // Parked goals first, pairs before entity reviews, sections in
                // document order, deepest abstraction first. Development starts
                // before standalone binding within a locality.
                ids.sort_by_key(|id| {
                    let g = self.goal(id).unwrap();
                    let parked = !matches!(g.state, GoalState::Parked);
                    let (kind, order) = match g.kind.as_str() {
                        "rejudge-pair" => (0, 0),
                        "generate" => (0, generate_rank(g).unwrap_or(usize::MAX)),
                        "bind" => (2, 0),
                        "abstract-entity" => (1, usize::MAX - entity_depth(store, &g.target)),
                        "reconcile-section" => (1, section_order(store, &g.target)),
                        _ => (1, 0),
                    };
                    (parked, kind, order, id.clone())
                });
                for id in ids {
                    let g = self.goal(&id).unwrap();
                    let sides: Vec<String> = match (g.kind.as_str(), g.target.split_once('~')) {
                        ("rejudge-pair", Some((a, b))) => vec![a.to_string(), b.to_string()],
                        _ => Vec::new(),
                    };
                    let mut cost = if sides.is_empty() {
                        estimate(store, g)
                    } else {
                        let side = |id: &str| {
                            store
                                .graph
                                .requirements
                                .get(store.resolve_id(id))
                                .map(|r| 260 + r.statement.len() + 90 * r.entities.len())
                                .unwrap_or(400)
                        };
                        700 + sides
                            .iter()
                            .filter(|x| !charged.contains(*x))
                            .map(|x| side(x))
                            .sum::<usize>()
                    };
                    let is_section = g.kind == "reconcile-section";
                    let is_criteria = g.kind == "derive-criteria";
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
                            || (is_section && sections >= max_sections)
                            || (is_criteria && criteria >= limits::CRITERIA_GOALS_PER_BATCH));
                    if over {
                        flush(&mut current, &mut skills, &mut label, &mut batches);
                        size = 0;
                        sections = 0;
                        criteria = 0;
                        charged.clear();
                        // A fresh batch holds none of the pair's sides yet.
                        cost = estimate(store, g);
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
                    if is_criteria {
                        criteria += 1;
                    }
                    charged.extend(sides);
                }
                if wall {
                    flush(&mut current, &mut skills, &mut label, &mut batches);
                    size = 0;
                    sections = 0;
                    criteria = 0;
                    charged.clear();
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
            // Generation batches follow the leaf-first order of their earliest goal.
            let generation = b
                .goals
                .iter()
                .filter_map(|id| self.goal(id))
                .filter_map(&generate_rank)
                .min();
            (
                matches!(b.class, Class::Gc),
                b.tier.unwrap_or(0),
                optional,
                !parked,
                level,
                generation,
                b.locality.clone(),
            )
        };
        // A batch is labeled by the key most of its goals share, never by the
        // union-find root of its locality group: forty-one pair batches over two
        // characters once all read as the notebook scene's.
        // Mirrors docs/compiler/reconciler.md#batching.
        for b in &mut batches {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for id in &b.goals {
                if let Some(g) = self.goal(id) {
                    for k in locality_keys(store, g) {
                        *counts.entry(k).or_insert(0) += 1;
                    }
                }
            }
            if let Some((k, _)) = counts.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))) {
                b.locality = k.clone();
            }
        }
        batches.sort_by_key(rank);
        // Numbering skips the ids live leases hold, so a batch formed while another
        // is being worked never takes the working batch's id (a session once read the
        // board's fresh `b24-1` as its own and reported the two as inconsistent).
        let mut n = 0;
        for b in batches.iter_mut() {
            loop {
                n += 1;
                let id = format!("b{}-{}", self.generation, n);
                if !self.held_batches.contains(&id) {
                    b.id = id;
                    break;
                }
            }
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

    // The goals leased to the reading serving are its own: the blocking sentence says
    // so instead of naming the serving's worker id as a stranger's claim.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    pub fn mark_own(&mut self, worker: &str) {
        let own: Vec<String> = self
            .claimed
            .iter()
            .filter(|(_, w)| w.as_str() == worker)
            .map(|(id, _)| id.clone())
            .collect();
        for id in own {
            self.readiness.insert(
                id.clone(),
                Ready::Blocked(
                    "yours: your open batch holds it; mark_goal_done or mark_goal_failed it, then done"
                        .to_string(),
                ),
            );
            self.own.insert(id);
        }
    }

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
        if let Some(r) = self.parked_reasons.get(&g.id) {
            v["parkedReason"] = json!(r);
        }
        if let Some(w) = self.claimed.get(&g.id) {
            v["claimedBy"] = json!(if self.own.contains(&g.id) {
                "you"
            } else {
                w.as_str()
            });
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

    // The compact `goals` reply: the counts, the batches, the ready goals outside a
    // batch, and the rest summarized by kind and blocking reason with their ids.
    // Mirrors docs/frontends/mcp.md#compilation-over-mcp.
    pub fn answer_compact(&self) -> Value {
        let mut full = self.answer();
        let batched: BTreeSet<&str> = self
            .batches
            .iter()
            .flat_map(|b| b.goals.iter().map(String::as_str))
            .collect();
        let mut ready_loose: Vec<Value> = Vec::new();
        let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
        let mut waiting: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in &self.goals {
            if matches!(g.state, GoalState::Failed { .. }) {
                continue;
            }
            if is_open(g) && self.is_ready(&g.id) {
                if !batched.contains(g.id.as_str()) {
                    ready_loose.push(json!({
                        "id": g.id, "kind": g.kind, "target": g.target, "mandatory": g.mandatory,
                    }));
                }
                continue;
            }
            // Open goals that are not ready and blocked goals (a release, a human
            // answer) both land here, so the counts and the summary agree.
            *by_kind.entry(g.kind.clone()).or_insert(0) += 1;
            let reason = self
                .readiness
                .get(&g.id)
                .and_then(|r| r.reason())
                .map(|r| r.to_string())
                .unwrap_or_else(|| match &g.state {
                    GoalState::Blocked { on } => format!("blocked on {}", on),
                    _ => "not ready".to_string(),
                });
            waiting.entry(reason).or_default().push(g.id.clone());
        }
        full["goals"] = json!(ready_loose);
        // Past the first few batches only the shape travels, and a reason lists a
        // handful of ids: a 41-batch board once answered with ninety thousand
        // characters. Mirrors docs/frontends/mcp.md#compilation-over-mcp.
        const COMPACT_BATCHES: usize = 6;
        const COMPACT_IDS: usize = 8;
        if let Some(arr) = full["batches"].as_array_mut() {
            if arr.len() > COMPACT_BATCHES {
                let rest: Vec<Value> = arr
                    .split_off(COMPACT_BATCHES)
                    .iter()
                    .map(|b| {
                        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
                        for g in b["goals"].as_array().into_iter().flatten() {
                            let k = g["kind"].as_str().unwrap_or("?").to_string();
                            *kinds.entry(k).or_insert(0) += 1;
                        }
                        json!({"id": b["id"], "locality": b["locality"], "goals": kinds})
                    })
                    .collect();
                full["moreBatches"] = json!(rest);
                full["batchesNote"] = json!(format!(
                    "the first {} batches in full; moreBatches lists the rest as counts by kind (begin_goals takes them in order)",
                    COMPACT_BATCHES
                ));
            }
        }
        let waiting: BTreeMap<String, Value> = waiting
            .into_iter()
            .map(|(reason, ids)| {
                let v = if ids.len() > COMPACT_IDS {
                    let mut shown: Vec<Value> =
                        ids[..COMPACT_IDS].iter().map(|i| json!(i)).collect();
                    shown.push(json!(format!("and {} more", ids.len() - COMPACT_IDS)));
                    json!(shown)
                } else {
                    json!(ids)
                };
                (reason, v)
            })
            .collect();
        full["notReady"] = json!({"byKind": by_kind, "byReason": waiting});
        full["countsNote"] = json!("open, ready, parked, and failed count mandatory goals; optional counts the rest; by_class and by_kind include both; blocked are the goals waiting on a release or a human answer, gated the subset a release opens");
        full["note"] = json!(match full.get("note").and_then(Value::as_str) {
            Some(n) => format!(
                "{}; goals {{full: true}} lists every goal with its hints",
                n
            ),
            None => "compact board; goals {full: true} lists every goal with its hints".to_string(),
        });
        full
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
        // No mandatory goal open: the verdict stands, and any batch still listed holds
        // optional goals only. Say so, since a reader once weighed "converged" against
        // a next line inviting a claim (docs/frontends/mcp.md#compilation-over-mcp).
        if self.open_mandatory() == 0 {
            v["verdict"] = json!(self.verdict.to_string());
            if let Some(b) = self.batches.first() {
                let optional: usize = self.batches.iter().map(|b| b.goals.len()).sum();
                v["note"] = json!(format!(
                    "nothing mandatory to compile; the graph reflects the docs. {} optional goal(s) in {} batch(es) stay claimable (begin_goals takes {}); the verdict does not wait for them",
                    optional,
                    self.batches.len(),
                    b.id
                ));
                v["next"] = json!(format!(
                    "optional: begin_goals claims {} or stop here",
                    b.id
                ));
            } else {
                v["note"] = json!("nothing to compile; the graph reflects the docs");
            }
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
            if let Some(r) = self.parked_reasons.get(&g.id) {
                s.push_str(&format!("  parked: {}\n", r));
            }
            for r in self.records_of(&g.id) {
                s.push_str(&format!("  record: {}\n", r));
            }
            if !g.hints.is_empty() {
                s.push_str(&format!("  hints:  {}\n", g.hints.join("; ")));
            }
            return Some(s);
        }
        if let Some(w) = self.waiting_pair(target) {
            return Some(self.explain_waiting(store, w));
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
                entity_keys(std::slice::from_ref(&g.target));
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

    // A failed abstract-entity goal stands while the count grows by less than a
    // fifth (docs/compiler/reconciler.md#parked-and-failed).
    #[test]
    fn failed_abstract_entity_stands_within_the_band() {
        let mk = |count: u64, generation: u64| Goal {
            id: "g:abstract-entity:ent:ines".into(),
            kind: "abstract-entity".into(),
            target: "ent:ines".into(),
            change: json!({"limits": [{"limit": "requirements-per-entity", "count": count, "soft": 50, "hard": 80, "level": "hard"}]}),
            cause: Some(crate::model::Cause {
                generation,
                mutation: 0,
                via: "limits".into(),
            }),
            ..Default::default()
        };
        let failed = mk(82, 60);
        assert!(failure_stands(&failed, &mk(82, 61)));
        assert!(failure_stands(&failed, &mk(95, 70)));
        assert!(!failure_stands(&failed, &mk(99, 70)));
        let other = Goal {
            kind: "review-entity".into(),
            ..mk(82, 60)
        };
        assert!(!failure_stands(
            &other,
            &Goal {
                kind: "review-entity".into(),
                ..mk(83, 61)
            }
        ));
    }

    // A declined fan-out goal stands while its candidates move: only the child count
    // growing by a fifth, or a bump of the limits, reopens it.
    // Mirrors docs/compiler/reconciler.md#parked-and-failed.
    #[test]
    fn failed_fan_out_stands_while_coupling_moves() {
        let mk = |count: u64, soft: u64, candidates: Value, generation: u64| Goal {
            id: "g:abstract-entity:ent:shop".into(),
            kind: "abstract-entity".into(),
            target: "ent:shop".into(),
            change: json!({"fan_out": count, "limit": {"soft": soft, "hard": 15}, "candidates": candidates}),
            cause: Some(crate::model::Cause {
                generation,
                mutation: 0,
                via: "limits".into(),
            }),
            ..Default::default()
        };
        let failed = mk(10, 9, json!([["ent:a", "ent:b"]]), 60);
        assert!(failure_stands(
            &failed,
            &mk(10, 9, json!([["ent:a", "ent:c"]]), 70)
        ));
        assert!(failure_stands(&failed, &mk(11, 9, json!([]), 71)));
        assert!(!failure_stands(&failed, &mk(12, 9, json!([]), 72)));
        assert!(!failure_stands(
            &failed,
            &mk(10, 8, json!([["ent:a", "ent:b"]]), 73)
        ));
    }
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
        s.status.parked.push(ParkedGoal::new(
            ga.clone(),
            "left open after its retry in session b7-1",
        ));
        let c = derive(&s);
        assert_eq!(
            c.parked_reasons.get(&ga.id).map(String::as_str),
            Some("left open after its retry in session b7-1")
        );
        assert!(c
            .explain(&s, &ga.id)
            .unwrap()
            .contains("parked: left open after its retry"));
        assert_eq!(
            c.goal_json(c.goal(&ga.id).unwrap())["parkedReason"],
            "left open after its retry in session b7-1"
        );
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
            let kind = lv
                .trim_start_matches("view:")
                .split('/')
                .next()
                .unwrap()
                .to_string();
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
        let theirs = b.readiness[&fan_out]
            .reason()
            .unwrap_or_default()
            .to_string();
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
        let text = "# T\n\nThe X serves the Y nightly. The X also names the Y nightly. The P serves the Q nightly. The P also names the Q nightly.\n";
        let mut coverage = BTreeMap::new();
        {
            let r = "/t";
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
        for (id, name) in [
            ("ent:x", "X"),
            ("ent:y", "Y"),
            ("ent:p", "P"),
            ("ent:q", "Q"),
        ] {
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
            req(
                "The X serves the Y nightly.",
                &["ent:x", "ent:y"],
                "The X serves the Y nightly.",
            ),
        );
        s.graph.requirements.insert(
            "req:t-2".into(),
            req(
                "The X also names the Y nightly.",
                &["ent:x", "ent:y"],
                "The X also names the Y nightly.",
            ),
        );
        s.graph.requirements.insert(
            "req:t-3".into(),
            req(
                "The P serves the Q nightly.",
                &["ent:p", "ent:q"],
                "The P serves the Q nightly.",
            ),
        );
        s.graph.requirements.insert(
            "req:t-4".into(),
            req(
                "The P also names the Q nightly.",
                &["ent:p", "ent:q"],
                "The P also names the Q nightly.",
            ),
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
        assert_eq!(
            pairs.len(),
            2,
            "{:?}",
            pairs.iter().map(|g| &g.id).collect::<Vec<_>>()
        );
        for g in &pairs {
            assert!(b.is_ready(&g.id), "{:?}", b.readiness.get(&g.id));
        }
        // Two neighborhoods (x,y and p,q), one batch.
        let localities = b.localities(&s, &pairs);
        assert_eq!(localities.len(), 2, "{:?}", localities);
        assert_eq!(b.batches.len(), 1, "{:?}", b.batches);
        assert_eq!(b.batches[0].goals.len(), 2);
    }

    // Criteria goals share batches, at most CRITERIA_GOALS_PER_BATCH to one, instead
    // of one session per entity. Mirrors docs/compiler/reconciler.md#batching.
    #[test]
    fn criteria_goals_pack_up_to_the_cap() {
        let mut s = Store::default();
        for i in 0..6 {
            let eid = format!("ent:e{}", i);
            s.graph.entities.insert(
                eid.clone(),
                Entity {
                    name: format!("E{}", i),
                    ..Default::default()
                },
            );
            s.graph.requirements.insert(
                format!("req:r-{}", i),
                Requirement {
                    statement: format!("The E{} answers within {} seconds.", i, i + 1),
                    entities: vec![eid],
                    ..Default::default()
                },
            );
        }
        let proj = Project {
            out: s.out.clone(),
            gen_deliverable: Some("product".into()),
            ..Default::default()
        };
        let b = Board::derive(&s, &proj, &control_auto());
        let criteria: Vec<&Goal> = b
            .goals
            .iter()
            .filter(|g| g.kind == "derive-criteria")
            .collect();
        assert_eq!(
            criteria.len(),
            6,
            "{:?}",
            b.goals.iter().map(|g| &g.id).collect::<Vec<_>>()
        );
        let per_batch: Vec<usize> = b
            .batches
            .iter()
            .map(|bt| {
                bt.goals
                    .iter()
                    .filter(|id| id.starts_with("g:derive-criteria:"))
                    .count()
            })
            .filter(|n| *n > 0)
            .collect();
        assert_eq!(per_batch.iter().sum::<usize>(), 6, "{:?}", b.batches);
        assert!(
            per_batch
                .iter()
                .all(|n| *n <= limits::CRITERIA_GOALS_PER_BATCH),
            "{:?}",
            per_batch
        );
        assert!(
            per_batch.iter().any(|n| *n > 1),
            "criteria goals never shared a batch: {:?}",
            per_batch
        );
    }

    // A pair batch charges each requirement once: one long statement paired with six
    // others loads once, so its six pairs ride one session where charging both sides
    // per pair would cut them in two. Mirrors docs/compiler/reconciler.md#batching.
    #[test]
    fn a_shared_side_is_charged_once_per_pair_batch() {
        let mut s = Store::default();
        let pad = " It holds under the nightly settlement rules for gold keys and silver locks."
            .repeat(6);
        s.graph.entities.insert(
            "ent:x".into(),
            Entity {
                name: "X".into(),
                ..Default::default()
            },
        );
        let req = |statement: String| Requirement {
            statement,
            entities: vec!["ent:x".into()],
            ..Default::default()
        };
        // Each pair shares two words no other statement carries, so no filter drops it.
        let words = [
            ("amber", "basil"),
            ("cedar", "dahlia"),
            ("elder", "fennel"),
            ("ginger", "hazel"),
            ("iris", "juniper"),
            ("kelp", "lotus"),
        ];
        let hub: Vec<String> = words.iter().map(|(a, b)| format!("{} {}", a, b)).collect();
        s.graph.requirements.insert(
            "req:t-0".into(),
            req(format!("The ledger weighs {}.{}", hub.join(", "), pad)),
        );
        for (i, (a, b)) in words.iter().enumerate() {
            s.graph.requirements.insert(
                format!("req:t-{}", i + 1),
                req(format!("The ledger weighs {} {} apart.{}", a, b, pad)),
            );
        }
        s.status.generation = 5;
        record(
            &mut s,
            5,
            store::CHANGE_REQ_REVISED,
            "req:t-0",
            "fields",
            json!({}),
        );
        let b = derive(&s);
        let pairs: Vec<&Goal> = b
            .goals
            .iter()
            .filter(|g| g.kind == "rejudge-pair")
            .collect();
        assert_eq!(
            pairs.len(),
            6,
            "{:?}",
            pairs.iter().map(|g| &g.id).collect::<Vec<_>>()
        );
        let with_pairs: Vec<usize> = b
            .batches
            .iter()
            .map(|bt| {
                bt.goals
                    .iter()
                    .filter(|id| id.starts_with("g:rejudge-pair:"))
                    .count()
            })
            .filter(|n| *n > 0)
            .collect();
        // Per pair both sides would be about 2.9k each, six pairs about 17k, past the
        // budget the judgment skill leaves; charged once, they fit one batch.
        assert_eq!(with_pairs, vec![6], "{:?}", with_pairs);
    }

    // Six entities owing criteria, each its own locality, pack on into one batch
    // until the cap and spill the rest into the next: four, then two.
    // Mirrors docs/compiler/reconciler.md#batching.
    #[test]
    fn six_criteria_goals_pack_as_four_and_two() {
        let mut s = Store::default();
        for i in 0..6 {
            let eid = format!("ent:e{}", i);
            s.graph.entities.insert(
                eid.clone(),
                Entity {
                    name: format!("E{}", i),
                    ..Default::default()
                },
            );
            s.graph.requirements.insert(
                format!("req:r-{}", i),
                Requirement {
                    statement: format!("The E{} answers within {} seconds.", i, i + 1),
                    entities: vec![eid],
                    ..Default::default()
                },
            );
        }
        let proj = Project {
            out: s.out.clone(),
            gen_deliverable: Some("product".into()),
            ..Default::default()
        };
        let b = Board::derive(&s, &proj, &control_auto());
        let per_batch: Vec<usize> = b
            .batches
            .iter()
            .map(|bt| {
                bt.goals
                    .iter()
                    .filter(|id| id.starts_with("g:derive-criteria:"))
                    .count()
            })
            .filter(|n| *n > 0)
            .collect();
        assert_eq!(limits::CRITERIA_GOALS_PER_BATCH, 4);
        assert_eq!(per_batch, vec![4, 2], "{:?}", b.batches);
        // A batch of criteria goals holds nothing else.
        for bt in b.batches.iter().filter(|bt| {
            bt.goals
                .iter()
                .any(|id| id.starts_with("g:derive-criteria:"))
        }) {
            assert!(
                bt.goals
                    .iter()
                    .all(|id| id.starts_with("g:derive-criteria:")),
                "{:?}",
                bt
            );
        }
    }

    // Generation batches follow the leaf-first order over the relationships: the part
    // a whole composes generates before the whole, whatever their ids.
    // Mirrors docs/compiler/goals/generate.md#readiness.
    #[test]
    fn generate_batches_run_leaf_first() {
        let mut s = Store::default();
        for (eid, rid, crit) in [
            ("ent:a-order", "req:r-a", "crit:a-1"),
            ("ent:z-line", "req:r-z", "crit:z-1"),
        ] {
            s.graph.entities.insert(
                eid.into(),
                Entity {
                    name: eid.into(),
                    ..Default::default()
                },
            );
            s.graph.requirements.insert(
                rid.into(),
                Requirement {
                    statement: format!("The {} answers.", eid),
                    entities: vec![eid.into()],
                    ..Default::default()
                },
            );
            s.graph.criteria.insert(
                crit.into(),
                Criterion {
                    requirement: rid.into(),
                    statement: format!("Asked once, the {} answers.", eid),
                    kind: "positive".into(),
                    ..Default::default()
                },
            );
        }
        s.graph.relationships.insert(
            "rel:a-order~z-line".into(),
            crate::model::Relationship {
                members: vec!["ent:a-order".into(), "ent:z-line".into()],
                contributions: vec![crate::model::Contribution {
                    a: "ent:a-order".into(),
                    b: "ent:z-line".into(),
                    r#type: "composition".into(),
                    cardinality: None,
                    requirements: vec!["req:r-a".into()],
                }],
            },
        );
        let proj = Project {
            out: s.out.clone(),
            gen_deliverable: Some("product".into()),
            ..Default::default()
        };
        let control = Control {
            generate: "auto".into(),
            ..control_auto()
        };
        let b = Board::derive(&s, &proj, &control);
        let pos = |id: &str| {
            b.batches
                .iter()
                .position(|bt| bt.goals.iter().any(|g| g == id))
                .unwrap_or_else(|| panic!("{} unbatched: {:?}", id, b.batches))
        };
        assert!(
            pos("g:generate:ent:z-line") < pos("g:generate:ent:a-order"),
            "{:?}",
            b.batches
        );
    }

    // A ledger locality is a wall: the batch flushes before and after it, and its
    // counters (size, sections, criteria, charged sides) start over. A pair never
    // meets a wall (pairs are tier 2, the walled kinds tiers 0, 1, and 3), so the
    // reset of charged sides has no pair to show it; what a wall does show is a
    // component's Generate and Bind goals together, without the criteria goals of
    // other localities. Those goals pack among themselves, where without the wall
    // they would share the ledger batch. Mirrors docs/compiler/reconciler.md#batching.
    #[test]
    fn a_walled_locality_never_shares_a_batch() {
        let mut s = Store::default();
        // One requirement already carrying a criterion owes development and a bind,
        // but no criteria. Development starts first within its component locality.
        s.graph.entities.insert(
            "ent:g".into(),
            Entity {
                name: "G".into(),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:r-g".into(),
            Requirement {
                statement: "The G answers within 9 seconds.".into(),
                entities: vec!["ent:g".into()],
                ..Default::default()
            },
        );
        s.graph.criteria.insert(
            "crit:g-1".into(),
            Criterion {
                requirement: "req:r-g".into(),
                statement: "A G asked once answers in 8 seconds.".into(),
                kind: "positive".into(),
                ..Default::default()
            },
        );
        for i in 0..6 {
            let eid = format!("ent:e{}", i);
            s.graph.entities.insert(
                eid.clone(),
                Entity {
                    name: format!("E{}", i),
                    ..Default::default()
                },
            );
            s.graph.requirements.insert(
                format!("req:r-{}", i),
                Requirement {
                    statement: format!("The E{} answers within {} seconds.", i, i + 1),
                    entities: vec![eid],
                    ..Default::default()
                },
            );
        }
        let proj = Project {
            out: s.out.clone(),
            gen_deliverable: Some("product".into()),
            ..Default::default()
        };
        let control = Control {
            generate: "auto".into(),
            ..control_auto()
        };
        let b = Board::derive(&s, &proj, &control);
        assert!(
            b.is_ready("g:generate:ent:g"),
            "{:?}",
            b.readiness.get("g:generate:ent:g")
        );
        assert!(
            b.is_ready("g:bind:req:r-g"),
            "{:?}",
            b.readiness.get("g:bind:req:r-g")
        );
        let bind = b
            .batches
            .iter()
            .find(|bt| bt.goals.iter().any(|id| id == "g:bind:req:r-g"))
            .unwrap_or_else(|| panic!("{:?}", b.batches));
        assert_eq!(
            bind.goals,
            vec!["g:generate:ent:g".to_string(), "g:bind:req:r-g".to_string()],
            "{:?}",
            bind
        );
        for batch in b.batches.iter().filter(|batch| {
            batch
                .goals
                .iter()
                .any(|id| id.starts_with("g:derive-criteria:"))
        }) {
            assert!(
                batch
                    .goals
                    .iter()
                    .all(|id| id.starts_with("g:derive-criteria:")),
                "{:?}",
                batch
            );
        }
        let per_batch: Vec<usize> = b
            .batches
            .iter()
            .map(|bt| {
                bt.goals
                    .iter()
                    .filter(|id| id.starts_with("g:derive-criteria:"))
                    .count()
            })
            .filter(|n| *n > 0)
            .collect();
        assert_eq!(per_batch, vec![4, 2], "{:?}", b.batches);
    }

    // A pair that overflows its batch opens the next one charged in full: the side
    // the spent batch held is loaded again by the fresh one. A hub paired with six
    // spokes, sized so two pairs fill a batch with the hub and four would without
    // it, goes three batches of two, never two and then four.
    // Mirrors docs/compiler/reconciler.md#batching.
    #[test]
    fn an_overflowing_pair_recharges_in_full() {
        let judgment = crate::session::SKILLS
            .iter()
            .find(|(k, _)| *k == "judgment")
            .map(|(_, p)| p.len())
            .unwrap();
        let budget = limits::CONTEXT_BUDGET.saturating_sub(judgment).max(6_000);
        // A side costs 260 + statement + 90 per entity; a pair 700 plus its uncharged
        // sides. A spoke side is 1800 (a pair step of 2500); the hub side sits so
        // that two steps fit beside it and three do not, while four fit without it.
        let spoke_len = 1800 - 350;
        let hub_len = (budget - 6250) - 350;
        assert!(4 * 2500 <= budget, "{}", budget);
        let pad = " It holds under the nightly settlement rules for gold keys and silver locks.";
        let fill = |base: String, len: usize| -> String {
            let mut s = base;
            while s.len() < len {
                s.push_str(pad);
            }
            s.truncate(len);
            s
        };
        let mut s = Store::default();
        s.graph.entities.insert(
            "ent:x".into(),
            Entity {
                name: "X".into(),
                ..Default::default()
            },
        );
        let req = |statement: String| Requirement {
            statement,
            entities: vec!["ent:x".into()],
            ..Default::default()
        };
        // Each pair shares two words no other statement carries, so no filter drops it.
        let words = [
            ("amber", "basil"),
            ("cedar", "dahlia"),
            ("elder", "fennel"),
            ("ginger", "hazel"),
            ("iris", "juniper"),
            ("kelp", "lotus"),
        ];
        let hub: Vec<String> = words.iter().map(|(a, b)| format!("{} {}", a, b)).collect();
        s.graph.requirements.insert(
            "req:t-0".into(),
            req(fill(
                format!("The ledger weighs {}.", hub.join(", ")),
                hub_len,
            )),
        );
        for (i, (a, b)) in words.iter().enumerate() {
            s.graph.requirements.insert(
                format!("req:t-{}", i + 1),
                req(fill(
                    format!("The ledger weighs {} {} apart.", a, b),
                    spoke_len,
                )),
            );
        }
        s.status.generation = 5;
        record(
            &mut s,
            5,
            store::CHANGE_REQ_REVISED,
            "req:t-0",
            "fields",
            json!({}),
        );
        let b = derive(&s);
        let pairs: Vec<&Goal> = b
            .goals
            .iter()
            .filter(|g| g.kind == "rejudge-pair")
            .collect();
        assert_eq!(
            pairs.len(),
            6,
            "{:?}",
            pairs.iter().map(|g| &g.id).collect::<Vec<_>>()
        );
        for g in &pairs {
            assert_eq!(estimate(&s, g), 700 + (budget - 6250) + 1800, "{}", g.id);
        }
        let with_pairs: Vec<usize> = b
            .batches
            .iter()
            .map(|bt| {
                bt.goals
                    .iter()
                    .filter(|id| id.starts_with("g:rejudge-pair:"))
                    .count()
            })
            .filter(|n| *n > 0)
            .collect();
        assert_eq!(with_pairs, vec![2, 2, 2], "{:?}", with_pairs);
    }

    // A read-only derivation never commits: Board::compute on a store whose documents
    // changed on disk leaves the journal, the section trees, and the generation
    // untouched while the board shows the dirty section's goal.
    // Mirrors docs/compiler/reconciler.md#goal-derivation.
    #[test]
    fn compute_derives_from_changed_documents_without_writing() {
        let root = std::env::temp_dir().join(format!(
            "jazyk-board-compute-{}-{}",
            crate::sys::process_id(),
            crate::verify::now_iso().replace([':', '.'], "-")
        ));
        let docs = root.join("docs");
        crate::vfs::create_dir_all(&docs).unwrap();
        crate::vfs::write(docs.join("a.md"), "# A\n\nThe first body.\n").unwrap();
        let out = root.join(".jazyk");
        let proj = Project {
            root: root.clone(),
            out: out.clone(),
            docs_glob: vec!["docs/**/*.md".into()],
            ..Default::default()
        };
        // A build's alignment writes the trees once.
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        let (parsed, _) = crate::reconcile::parse_all(&proj);
        assert!(parsed.contains_key("docs/a.md"), "{:?}", parsed.keys());
        s.sync_docs(&parsed);
        let generation = s.status.generation;
        let journal = |out: &Path| {
            crate::vfs::read_dir(out.join("journal"))
                .map(|d| d.count())
                .unwrap_or(0)
        };
        let entries = journal(&out);
        assert!(entries >= 1);
        // The document changes on disk; a read-only derivation sees it and writes nothing.
        crate::vfs::write(docs.join("a.md"), "# A\n\nThe second body.\n").unwrap();
        let b = Board::compute(&proj, &out);
        assert!(
            b.goals
                .iter()
                .any(|g| g.kind == "reconcile-section" && g.target == "docs/a.md#/a"),
            "{:?}",
            b.goals.iter().map(|g| &g.id).collect::<Vec<_>>()
        );
        assert_eq!(journal(&out), entries);
        let on_disk = Store::load(&out);
        assert_eq!(on_disk.status.generation, generation);
        assert!(on_disk.docs["docs/a.md"].sections["/a"]
            .raw
            .contains("The first body."));
        crate::vfs::remove_dir_all(&root).ok();
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
    }

    // Each direction keeps its own visited set: a node one seed reaches upward still
    // walks downward from the other seed. Mirrors docs/compiler/reconciler.md#cones.
    #[test]
    fn a_node_reached_upward_still_walks_downward_from_the_other_seed() {
        use crate::model::{Entity, Provenance, Requirement};
        let mut s = Store::default();
        s.graph.entities.insert(
            "ent:pricing".into(),
            Entity {
                name: "Pricing".into(),
                provenance: Some(Provenance::Derived {
                    from: vec!["req:b".into()],
                    reasoning: "split".into(),
                }),
                ..Default::default()
            },
        );
        s.graph.entities.insert(
            "ent:discount".into(),
            Entity {
                name: "Discount".into(),
                parent: Some("ent:pricing".into()),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:a".into(),
            Requirement {
                statement: "Pricing applies to every order.".into(),
                entities: vec!["ent:pricing".into()],
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:b".into(),
            Requirement {
                statement: "Orders are priced.".into(),
                ..Default::default()
            },
        );
        let c = cone(&s, "req:a~req:b");
        assert!(c.nodes.contains("ent:pricing"));
        assert!(
            c.nodes.contains("ent:discount"),
            "req:b's downward walk reaches the child: {:?}",
            c.nodes
        );
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
        s.status.parked.push(goal.clone().into());
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
        empty.status.parked.push(stray.clone().into());
        let b = derive(&empty);
        assert!(b.goal(&stray.id).is_none());
        assert!(b.dropped_parked.contains(&stray.id));
        // A parked ledger goal the ledger no longer derives (its row verified since)
        // has nothing to resume: it drops even though its target stands.
        let mut verified = settled_store();
        let req = verified
            .graph
            .requirements
            .keys()
            .next()
            .cloned()
            .expect("a requirement");
        let mut done = goal.clone();
        done.id = format!("g:verify:{}", req);
        done.kind = "verify".into();
        done.target = req;
        verified.status.parked.push(done.clone().into());
        let b = derive(&verified);
        assert!(b.goal(&done.id).is_none(), "{:?}", b.goal(&done.id));
        assert!(b.dropped_parked.contains(&done.id));
        // A failed ledger goal the ledger no longer derives drops the same way.
        verified.status.parked.clear();
        verified.status.failed.push(crate::model::FailedGoal {
            goal: done.clone(),
            reason: "the test cannot run".into(),
        });
        let b = derive(&verified);
        assert!(b.goal(&done.id).is_none(), "{:?}", b.goal(&done.id));
        assert!(
            b.dropped_failed.contains(&done.id),
            "{:?}",
            b.dropped_failed
        );
        // An entry parked by flip detection resumes blocked on the ruling, whatever
        // state it was written with, so no batch takes it.
        let mut ruled = settled_store();
        let ent = ruled
            .graph
            .entities
            .keys()
            .next()
            .cloned()
            .expect("an entity");
        let mut flip = goal.clone();
        flip.id = format!("g:conform-instance:{}", ent);
        flip.kind = "conform-instance".into();
        flip.target = ent.clone();
        flip.change = json!({"reparent": "ent:row-of-shops", "between": ["ent:a", "ent:b"]});
        flip.cause = Some(crate::model::Cause {
            generation: 95,
            mutation: 0,
            via: "flip-detection".into(),
        });
        ruled.status.parked.push(flip.clone().into());
        ruled.graph.diagnostics.insert(
            "diag:unstable-derivation-1".into(),
            unstable_ruling("ent:row-of-shops"),
        );
        let b = derive(&ruled);
        let g = b.goal(&flip.id).expect("the flip entry stands");
        assert!(
            matches!(&g.state, GoalState::Blocked { on } if on.contains("ent:row-of-shops")),
            "{:?}",
            g.state
        );
        assert!(!b.is_ready(&flip.id));
        assert!(b.batches.iter().all(|bt| !bt.goals.contains(&flip.id)));
        // Answering the ruling clears the entry.
        ruled
            .graph
            .diagnostics
            .get_mut("diag:unstable-derivation-1")
            .unwrap()
            .answer = Some(crate::model::DiagnosticAnswer {
            choice: Some(0),
            text: "keep".into(),
            status: "applied".into(),
            error: None,
        });
        let b = derive(&ruled);
        assert!(
            b.dropped_parked.contains(&flip.id),
            "{:?}",
            b.dropped_parked
        );
    }

    fn unstable_ruling(subject: &str) -> crate::model::Diagnostic {
        crate::model::Diagnostic {
            rule: "unstable-derivation".into(),
            severity: "warning".into(),
            subjects: vec![subject.into()],
            message: "flips".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        }
    }

    // A failed place-anchors goal leaves its document undecided: the document's
    // sections wait, and nothing else does. Mirrors
    // docs/compiler/goals/place-anchors.md#readiness.
    #[test]
    fn a_failed_place_anchors_goal_blocks_its_own_sections_only() {
        let mut s = settled_store();
        record(
            &mut s,
            10,
            store::CHANGE_ALIGNMENT_PENDING,
            "shop.md",
            "alignment",
            json!({"proposals": 1}),
        );
        record(
            &mut s,
            10,
            store::CHANGE_SECTION_DIRTY,
            "shop.md#/shop/orders",
            "section",
            json!({"added": 1}),
        );
        record(
            &mut s,
            10,
            store::CHANGE_ENTITY,
            "ent:customer",
            "entities",
            json!({"requirement": "req:shop-1"}),
        );
        let b = derive(&s);
        assert!(
            !b.is_ready("g:review-entity:ent:customer"),
            "tier 0 and 1 run first"
        );
        let anchors = Goal {
            id: "g:place-anchors:shop.md".into(),
            kind: "place-anchors".into(),
            class: "compile".into(),
            mandatory: true,
            target: "shop.md".into(),
            unit: "document".into(),
            change: json!({"proposals": 1}),
            ..Default::default()
        };
        s.status.failed.push(crate::model::FailedGoal {
            goal: anchors,
            reason: "no candidate fits".into(),
        });
        let b = derive(&s);
        assert!(matches!(
            b.goal("g:place-anchors:shop.md").map(|g| &g.state),
            Some(GoalState::Failed { .. })
        ));
        assert!(
            !b.is_ready("g:reconcile-section:shop.md#/shop/orders"),
            "{:?}",
            b.readiness.get("g:reconcile-section:shop.md#/shop/orders")
        );
        assert!(
            b.is_ready("g:review-entity:ent:customer"),
            "{:?}",
            b.readiness.get("g:review-entity:ent:customer")
        );
        assert!(!b.verdict().converged());
    }

    // A goal parked mid-build takes no batch, and its batch neighbors still run.
    // Mirrors docs/compiler/reconciler.md#parked-and-failed.
    #[test]
    fn withholding_a_parked_goal_keeps_its_batch_neighbors() {
        let mut s = settled_store();
        record(
            &mut s,
            10,
            store::CHANGE_ENTITY,
            "ent:customer",
            "entities",
            json!({"requirement": "req:shop-1"}),
        );
        record(
            &mut s,
            10,
            store::CHANGE_ENTITY,
            "ent:order",
            "entities",
            json!({"requirement": "req:shop-2"}),
        );
        let mut b = derive(&s);
        let (a, o) = (
            "g:review-entity:ent:customer".to_string(),
            "g:review-entity:ent:order".to_string(),
        );
        assert!(
            b.batches.iter().flat_map(|bt| &bt.goals).any(|g| *g == a),
            "{:?}",
            b.batches
        );
        assert!(
            b.batches.iter().flat_map(|bt| &bt.goals).any(|g| *g == o),
            "{:?}",
            b.batches
        );
        b.withhold(&BTreeSet::from([a.clone()]), &s, &project_for(&s));
        let batched: Vec<&String> = b.batches.iter().flat_map(|bt| &bt.goals).collect();
        assert!(!batched.contains(&&a), "{:?}", b.batches);
        assert!(batched.contains(&&o), "{:?}", b.batches);
        assert!(!b.is_ready(&a));
    }

    // A pending flip ruling holds its pair blocked on a human: the verdict converges
    // with the pair in the blocked count. Mirrors docs/compiler/compilation.md#convergence.
    #[test]
    fn a_pending_flip_ruling_rides_as_blocked_in_the_verdict() {
        let mut s = settled_store();
        let before = derive(&s).verdict();
        assert!(before.converged(), "{:?}", before);
        let flip = Goal {
            id: "g:review-entity:ent:customer".into(),
            kind: "review-entity".into(),
            class: "compile".into(),
            mandatory: true,
            target: "ent:customer".into(),
            unit: "entity".into(),
            change: json!({"unstable": "customer", "subject": "ent:customer"}),
            cause: Some(crate::model::Cause {
                generation: 2,
                mutation: 0,
                via: "flip-detection".into(),
            }),
            ..Default::default()
        };
        s.status.parked.push(flip.into());
        s.graph.diagnostics.insert(
            "diag:unstable-derivation-1".into(),
            unstable_ruling("ent:customer"),
        );
        let v = derive(&s).verdict();
        assert!(v.converged(), "{:?}", v);
        assert_eq!(v.blocked, before.blocked + 1, "{:?}", v);
    }

    // A goal parked by flip detection stays parked when it derives again: its parked
    // change names the flip, never the goal's own records, so a change comparison
    // would drop it at once. Mirrors docs/compiler/reconciler.md#flip-detection.
    #[test]
    fn a_flip_parked_goal_stays_parked_when_it_derives_again() {
        let mut s = settled_store();
        record(
            &mut s,
            10,
            store::CHANGE_ENTITY,
            "ent:customer",
            "entities",
            json!({"requirement": "req:shop-1"}),
        );
        let derived = derive(&s)
            .goal("g:review-entity:ent:customer")
            .cloned()
            .expect("the entity change derives a review");
        let mut flip = derived.clone();
        flip.change = json!({"unstable": "customer", "subject": "ent:customer"});
        flip.cause = Some(crate::model::Cause {
            generation: 40,
            mutation: 0,
            via: "flip-detection".into(),
        });
        s.status.parked.push(flip.clone().into());
        s.graph.diagnostics.insert(
            "diag:unstable-derivation-1".into(),
            unstable_ruling("ent:customer"),
        );
        let b = derive(&s);
        let g = b.goal(&flip.id).expect("the flip entry stands");
        assert_eq!(
            g.change, derived.change,
            "the goal keeps its derived change"
        );
        assert!(
            matches!(&g.state, GoalState::Blocked { on } if on.contains("ent:customer")),
            "{:?}",
            g.state
        );
        assert!(!b.dropped_parked.contains(&flip.id));
        assert!(b.batches.iter().all(|bt| !bt.goals.contains(&flip.id)));
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

    // The referrer index answers every node as the scan over the store does, so the
    // cone over the index is the cone over the scan.
    // Mirrors docs/compiler/graph.md#derived-data and docs/compiler/reconciler.md#cones.
    #[test]
    fn the_referrer_index_matches_the_scan() {
        let mut synthetic = crate::derive::tests::synthetic_store(300, 700);
        let mut batch = RecordBatch::new(1);
        crate::derive::recompute(&mut synthetic, "g1", &mut batch);
        for s in [settled_store(), synthetic] {
            let refs = Refs::build(&s);
            let mut ids: Vec<String> = s.graph.entities.keys().cloned().collect();
            ids.extend(s.graph.requirements.keys().cloned());
            ids.extend(s.graph.views.keys().cloned());
            for (doc, rec) in &s.docs {
                ids.push(doc.clone());
                ids.extend(rec.sections.keys().map(|r| format!("{}#{}", doc, r)));
            }
            ids.push("ent:nope".into());
            ids.push("nope.md#/nope".into());
            for id in &ids {
                let indexed: BTreeSet<String> = referrers(&s, &refs, id).into_iter().collect();
                let scanned: BTreeSet<String> = referrers_scan(&s, id).into_iter().collect();
                assert_eq!(indexed, scanned, "{}", id);
            }
            for id in &ids {
                assert_eq!(cone_with(&s, &refs, id), cone(&s, id), "{}", id);
            }
        }
    }

    // A store of two thousand entities and five thousand requirements recomputes its
    // derived data, derives its board, and runs the checks in bounded time: the
    // indices keep each linear in the graph where a scan per node would not be.
    // Ignored in the normal run (the ledger goals over five thousand requirements
    // alone take seconds in debug); `cargo test -- --ignored a_large_store` runs it.
    // Mirrors docs/compiler/graph.md#derived-data.
    // The indices a derivation builds (goal by id, open kinds, the records a goal
    // stands on, the compile goals in a GC goal's cone) answer what the scans over
    // the goals and the change records answered.
    #[test]
    fn derivation_indices_match_the_scans() {
        let mut synthetic = crate::derive::tests::synthetic_store(300, 700);
        let mut batch = RecordBatch::new(1);
        crate::derive::recompute(&mut synthetic, "g1", &mut batch);
        for r in batch.records() {
            synthetic.status.record_change(r.clone());
        }
        let mut settled = settled_store();
        record(
            &mut settled,
            10,
            store::CHANGE_SECTION_DIRTY,
            "shop.md#/shop/orders",
            "section",
            json!({"added": 1}),
        );
        record(
            &mut settled,
            10,
            store::CHANGE_ENTITY,
            "ent:order",
            "entities",
            json!({}),
        );
        let mut cones = 0;
        for s in [synthetic, settled] {
            let b = derive(&s);
            cones += b.goals.iter().filter(|g| g.class == "gc").count();
            let refs = Refs::build(&s);
            for g in &b.goals {
                assert!(std::ptr::eq(b.goal(&g.id).unwrap(), g));
                assert_eq!(
                    b.open_kind_on(&g.kind, &g.target),
                    b.open_goals()
                        .iter()
                        .any(|o| o.kind == g.kind && o.target == g.target)
                );
                let kinds = goals::record_kinds(&g.kind);
                let subjects: Vec<&str> = match goals::pair_members(&g.target) {
                    Some((x, y)) => vec![x, y],
                    None => vec![g.target.as_str()],
                };
                let scanned: Vec<String> = s
                    .status
                    .changes
                    .iter()
                    .filter(|c| {
                        kinds.contains(&c.kind.as_str()) && subjects.contains(&c.subject.as_str())
                    })
                    .map(|c| c.id.clone())
                    .collect();
                assert_eq!(b.records_of(&g.id), scanned, "{}", g.id);
                if g.class == "gc" {
                    let cone = cone_with(&s, &refs, &g.target);
                    let scanned: Vec<String> = b
                        .goals
                        .iter()
                        .filter(|o| {
                            o.class == "compile" && is_open(o) && cone.holds_target(&o.target)
                        })
                        .map(|o| o.id.clone())
                        .collect();
                    assert_eq!(b.cone_blockers(&g.id), scanned, "{}", g.id);
                }
            }
            assert!(b.goal("g:bind:req:nope").is_none());
            assert!(!b.open_kind_on("bind", "req:nope"));
        }
        assert!(cones > 0, "a GC goal walks a cone");
    }

    #[test]
    #[ignore]
    fn a_large_store_derives_and_checks_in_bounded_time() {
        let mut s = crate::derive::tests::synthetic_store(2000, 5000);
        let mut batch = RecordBatch::new(1);
        let started = std::time::Instant::now();
        crate::derive::recompute(&mut s, "g1", &mut batch);
        for r in batch.records() {
            s.status.record_change(r.clone());
        }
        let recomputed = started.elapsed();
        let b = derive(&s);
        let derived = started.elapsed() - recomputed;
        let f = crate::reconcile::checks(&s, &project_for(&s));
        let checked = started.elapsed() - recomputed - derived;
        assert!(
            b.goals.iter().any(|g| g.class == "gc"),
            "a GC goal walks a cone"
        );
        assert!(!f.is_empty());
        eprintln!(
            "large store: recompute {:?}, derive {:?} ({} goals, {} batches), checks {:?}",
            recomputed,
            derived,
            b.goals.len(),
            b.batches.len(),
            checked
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "recompute {:?}, derive {:?}, checks {:?}",
            recomputed,
            derived,
            checked
        );
    }
}
