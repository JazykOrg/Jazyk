use super::*;

// ---- limits ----

pub struct Crossing {
    pub subject: String,
    pub limit: &'static str,
    pub count: u64,
    pub soft: u64,
    pub hard: u64,
}

const STRUCTURAL_KINDS: [&str; 5] = ["class", "package", "component", "composite", "deployment"];
pub const FLOW_KINDS: [&str; 6] = [
    "use-case",
    "activity",
    "sequence",
    "communication",
    "timing",
    "overview",
];

// The live members of a view: ids that resolve to an existing node.
pub(super) fn live_members(store: &Store, v: &View) -> Vec<String> {
    v.members
        .iter()
        .map(|m| store.resolve_id(m).to_string())
        .filter(|m| {
            store.graph.entities.contains_key(m) || store.graph.requirements.contains_key(m)
        })
        .collect()
}

// Arrows a view renders: the ranked contribution groups among its members.
pub fn view_edge_count(store: &Store, members: &[String]) -> u64 {
    view_edge_count_with(&contributions_by_a(store), members)
}

// Every contribution by its `a` end, for the edge counts of many views at once.
fn contributions_by_a(store: &Store) -> BTreeMap<&str, Vec<&Contribution>> {
    let mut out: BTreeMap<&str, Vec<&Contribution>> = BTreeMap::new();
    for c in store
        .graph
        .relationships
        .values()
        .flat_map(|r| r.contributions.iter())
    {
        out.entry(c.a.as_str()).or_default().push(c);
    }
    out
}

fn view_edge_count_with(by_a: &BTreeMap<&str, Vec<&Contribution>>, members: &[String]) -> u64 {
    let set: BTreeSet<&str> = members.iter().map(String::as_str).collect();
    set.iter()
        .flat_map(|a| by_a.get(*a).into_iter().flatten())
        .filter(|c| c.r#type != INSTANTIATION && set.contains(c.b.as_str()))
        .count() as u64
}

// The participants of a flow: each member's message endpoints (its first dependency
// edge, or its first edge), or its first entity when it has no edge.
pub fn flow_participants(store: &Store, members: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for m in members {
        let Some(r) = store.graph.requirements.get(m) else {
            continue;
        };
        let edge = r
            .edges
            .iter()
            .find(|e| e.rel_type.as_deref() == Some("dependency"))
            .or_else(|| r.edges.first());
        match edge {
            Some(e) => {
                out.insert(store.resolve_id(&e.a).to_string());
                out.insert(store.resolve_id(&e.b).to_string());
            }
            None => {
                if let Some(e) = r.entities.first() {
                    out.insert(store.resolve_id(e).to_string());
                }
            }
        }
    }
    out
}

// Every count over its node's soft threshold. Mirrors docs/compiler/graph.md#limits.
pub fn threshold_crossings(store: &Store) -> Vec<Crossing> {
    threshold_crossings_with(store, &Indices::build(store))
}

fn threshold_crossings_with(store: &Store, ix: &Indices) -> Vec<Crossing> {
    let flow_levels = flow_view_level_index(store, ix);
    let by_a = contributions_by_a(store);
    let mut out = Vec::new();
    let mut check =
        |subject: &str, limit: &'static str, count: u64, bumps: &BTreeMap<String, LimitBump>| {
            let bump = bumps.get(limit).map(|b| b.value);
            if let Some((soft, hard)) = limits::threshold(limit, bump) {
                if count > soft {
                    out.push(Crossing {
                        subject: subject.to_string(),
                        limit,
                        count,
                        soft,
                        hard,
                    });
                }
            }
        };
    let mut children: BTreeMap<&str, u64> = BTreeMap::new();
    let mut reqs: BTreeMap<&str, u64> = BTreeMap::new();
    for e in store.graph.entities.values() {
        if let Some(p) = e.parent.as_deref() {
            *children.entry(p).or_insert(0) += 1;
        }
    }
    // A requirement counts toward the entity it leads (its first-listed entity, the
    // subject), never toward the ones it only references.
    // Mirrors docs/compiler/goals/abstract-entity.md#created-when.
    for r in store.graph.requirements.values() {
        if let Some(first) = r.entities.first() {
            *reqs.entry(store.resolve_id(first)).or_insert(0) += 1;
        }
    }
    let mut roots: BTreeMap<&str, u64> = BTreeMap::new();
    for e in store.graph.entities.values() {
        if e.parent.is_none() {
            *roots.entry(e.scope.as_str()).or_insert(0) += 1;
        }
    }
    for (id, e) in &store.graph.entities {
        check(
            id,
            "requirements-per-entity",
            reqs.get(id.as_str()).copied().unwrap_or(0),
            &e.limits,
        );
        check(
            id,
            limits::CHILDREN_PER_ENTITY,
            children.get(id.as_str()).copied().unwrap_or(0),
            &e.limits,
        );
    }
    // The scope root counts its parentless entities under the same row, its record on
    // `scope:<scope>`; a root has no node to carry a bump.
    // Mirrors docs/compiler/concepts/levels.md#the-scope-root.
    let no_bumps = BTreeMap::new();
    for (scope, count) in roots {
        check(
            &scope_root_target(scope),
            limits::CHILDREN_PER_ENTITY,
            count,
            &no_bumps,
        );
    }
    for m in store.graph.state_machines.values() {
        if let Some(subject) = store.graph.entities.get(&m.subject) {
            check(
                &m.subject,
                "states-per-state-machine",
                m.states.len() as u64,
                &subject.limits,
            );
        }
    }
    for (id, v) in &store.graph.views {
        let members = live_members(store, v);
        let kind = v.kind.as_str();
        if STRUCTURAL_KINDS.contains(&kind) {
            check(
                id,
                "members-per-structural-view",
                members.len() as u64,
                &v.limits,
            );
            check(
                id,
                "edges-per-view",
                view_edge_count_with(&by_a, &members),
                &v.limits,
            );
        }
        if kind == "object" {
            check(
                id,
                "instances-per-object-view",
                members.len() as u64,
                &v.limits,
            );
            check(
                id,
                "edges-per-view",
                view_edge_count_with(&by_a, &members),
                &v.limits,
            );
        }
        if FLOW_KINDS.contains(&kind) {
            let flow: Vec<String> = members
                .iter()
                .filter(|m| store.graph.requirements.contains_key(*m))
                .cloned()
                .collect();
            check(id, "members-per-flow-view", flow.len() as u64, &v.limits);
            if kind == "sequence" || kind == "communication" {
                check(
                    id,
                    "participants-per-sequence-view",
                    flow_view_participant_count_with(store, ix, &flow_levels, id, v) as u64,
                    &v.limits,
                );
            }
        }
    }
    out
}

// The participants a sequence or communication view counts against its limit: for
// a level's flow view, what the diagram draws after lifting; for any other, the
// union of the members' initiators and receivers.
// Mirrors docs/compiler/goals/split-view.md#created-when.
pub fn flow_view_participant_count(store: &Store, view_id: &str, v: &View) -> usize {
    let ix = Indices::build(store);
    let flow_levels = flow_view_level_index(store, &ix);
    flow_view_participant_count_with(store, &ix, &flow_levels, view_id, v)
}

fn flow_view_participant_count_with(
    store: &Store,
    ix: &Indices,
    flow_levels: &BTreeMap<String, String>,
    view_id: &str,
    v: &View,
) -> usize {
    if flow_levels.contains_key(view_id) {
        return drawn_entities(store, ix, flow_levels, view_id, v).len();
    }
    let flow: Vec<String> = live_members(store, v)
        .into_iter()
        .filter(|m| store.graph.requirements.contains_key(m))
        .collect();
    flow_participants(store, &flow).len()
}

// Write a threshold-crossed record once per crossing: a record already present for
// the subject and limit is refreshed in place (count, level), never duplicated; one
// whose count fell back under the threshold clears.
pub fn record_threshold_crossings(store: &mut Store, batch: &mut RecordBatch) {
    let ix = Indices::build(store);
    record_threshold_crossings_with(store, &ix, batch);
}

pub(super) fn record_threshold_crossings_with(
    store: &mut Store,
    ix: &Indices,
    batch: &mut RecordBatch,
) {
    let crossings = threshold_crossings_with(store, ix);
    store.status.changes.retain(|c| {
        c.kind != CHANGE_THRESHOLD_CROSSED
            || crossings
                .iter()
                .any(|x| x.subject == c.subject && c.detail["limit"] == x.limit)
    });
    for x in crossings {
        let level = if x.count > x.hard { "hard" } else { "soft" };
        let detail = serde_json::json!({
            "limit": x.limit,
            "count": x.count,
            "soft": x.soft,
            "hard": x.hard,
            "level": level,
            "goal": limits::limit(x.limit).map(|l| l.goal).unwrap_or_default(),
        });
        match store.status.changes.iter_mut().find(|c| {
            c.kind == CHANGE_THRESHOLD_CROSSED
                && c.subject == x.subject
                && c.detail["limit"] == x.limit
        }) {
            Some(existing) => existing.detail = detail,
            None => {
                batch.push(0, CHANGE_THRESHOLD_CROSSED, &x.subject, "limits", detail);
            }
        }
    }
}
