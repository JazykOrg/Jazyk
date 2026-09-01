// Derived data, recomputed on every commit and never written by a tool: relationships
// (per-direction contributions), state machines, default views (with the flow
// clustering), query membership, and the limit counts that write threshold-crossed
// records. Mirrors docs/compiler/graph.md#derived-data.
use crate::limits;
use crate::md;
use crate::model::*;
use crate::store::{RecordBatch, Store, CHANGE_QUERY_MATCH, CHANGE_THRESHOLD_CROSSED};
use std::collections::{BTreeMap, BTreeSet};

// Everything derived, in dependency order: relationships feed the object and component
// rules, state machines feed the state rule, views feed the limit counts.
pub fn recompute(store: &mut Store, build: &str, batch: &mut RecordBatch) {
    recompute_relationships(store);
    recompute_state_machines(store);
    recompute_default_views(store, build, batch);
    record_threshold_crossings(store, batch);
}

// ---- relationships ----

// Group requirement edges by unordered entity pair, then by direction and type into
// contributions, each carrying the requirements behind it. An untyped edge contributes
// dependency. Mirrors docs/compiler/model/relationship.md#recompute.
pub fn recompute_relationships(store: &mut Store) {
    let mut rels: BTreeMap<String, Relationship> = BTreeMap::new();
    for (rid, r) in &store.graph.requirements {
        for e in &r.edges {
            let a = store.resolve_id(&e.a).to_string();
            let b = store.resolve_id(&e.b).to_string();
            if a == b
                || !store.graph.entities.contains_key(&a)
                || !store.graph.entities.contains_key(&b)
            {
                continue;
            }
            let (x, y) = if a <= b { (&a, &b) } else { (&b, &a) };
            let key = format!("rel:{}~{}", entity_slug(x), entity_slug(y));
            let t = e
                .rel_type
                .clone()
                .unwrap_or_else(|| DEFAULT_REL_TYPE.to_string());
            let rel = rels.entry(key).or_insert_with(|| Relationship {
                members: vec![x.clone(), y.clone()],
                contributions: Vec::new(),
            });
            match rel
                .contributions
                .iter_mut()
                .find(|c| c.a == a && c.b == b && c.r#type == t)
            {
                Some(c) => {
                    if !c.requirements.contains(rid) {
                        c.requirements.push(rid.clone());
                    }
                    // A group carries a cardinality only when every edge that states
                    // one agrees.
                    if let Some(card) = e.cardinality.as_ref() {
                        if c.cardinality.as_ref().is_some_and(|x| x != card) {
                            c.cardinality = None;
                        } else if c.cardinality.is_none() && c.requirements.len() == 1 {
                            c.cardinality = Some(card.clone());
                        }
                    }
                }
                None => rel.contributions.push(Contribution {
                    a: a.clone(),
                    b: b.clone(),
                    r#type: t,
                    cardinality: e.cardinality.clone(),
                    requirements: vec![rid.clone()],
                }),
            }
        }
    }
    for rel in rels.values_mut() {
        rel.contributions
            .sort_by(|p, q| (&p.a, &p.b, &p.r#type).cmp(&(&q.a, &q.b, &q.r#type)));
        for c in rel.contributions.iter_mut() {
            c.requirements.sort();
        }
    }
    store.graph.relationships = rels;
}

pub fn entity_slug(id: &str) -> &str {
    id.strip_prefix("ent:").unwrap_or(id)
}

// Instance -> type, from the instantiation groups.
pub fn instance_types(store: &Store) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for rel in store.graph.relationships.values() {
        for c in &rel.contributions {
            if c.r#type == INSTANTIATION {
                out.insert(c.a.clone(), c.b.clone());
            }
        }
    }
    out
}

// ---- document order ----

// Breadth-first document levels from the given root documents over the link graph in
// the stored sections; unreachable documents come after the reachable ones. With no
// roots, every document is its own level in path order. The board orders tier 1 with
// the same computation (board::doc_levels). Mirrors docs/compiler/reconciler.md#link-levels.
pub fn doc_levels_from(store: &Store, roots: &[String]) -> BTreeMap<String, usize> {
    let mut links: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (doc, rec) in &store.docs {
        let mut targets: Vec<String> = Vec::new();
        for sec in rec.sections.values() {
            for l in md::doc_links(&sec.raw, doc) {
                if store.docs.contains_key(&l) && !targets.contains(&l) {
                    targets.push(l);
                }
            }
        }
        links.insert(doc.clone(), targets);
    }
    let roots: Vec<String> = roots
        .iter()
        .filter(|d| store.docs.contains_key(*d))
        .cloned()
        .collect();
    let mut level_of: BTreeMap<String, usize> = BTreeMap::new();
    if roots.is_empty() {
        for (i, d) in store.docs.keys().enumerate() {
            level_of.insert(d.clone(), i);
        }
        return level_of;
    }
    let mut frontier = roots.clone();
    for r in &roots {
        level_of.insert(r.clone(), 0);
    }
    let mut depth = 0;
    while !frontier.is_empty() {
        depth += 1;
        let mut next = Vec::new();
        for doc in &frontier {
            for l in links.get(doc).map(|v| v.as_slice()).unwrap_or(&[]) {
                if !level_of.contains_key(l) {
                    level_of.insert(l.clone(), depth);
                    next.push(l.clone());
                }
            }
        }
        frontier = next;
    }
    let max = level_of.values().max().copied().unwrap_or(0);
    for d in store.docs.keys() {
        level_of.entry(d.clone()).or_insert(max + 1);
    }
    level_of
}

// Requirement ids in document order: document link level (from the roots the build
// stamped into the status), then path, then the source section's position, then id.
// Requirements with no quote source sort last, by id.
// Mirrors docs/compiler/model/view.md#default-views.
pub fn document_order(store: &Store) -> Vec<String> {
    let levels = doc_levels_from(store, &store.status.roots);
    let mut keyed: Vec<((usize, String, usize, String), String)> = store
        .graph
        .requirements
        .iter()
        .map(|(id, r)| {
            let key = match r.source.as_ref() {
                Some(s) => {
                    let line = store
                        .docs
                        .get(&s.doc)
                        .and_then(|d| d.sections.get(&s.section))
                        .map(|sec| sec.lines[0])
                        .unwrap_or(usize::MAX);
                    let level = levels.get(&s.doc).copied().unwrap_or(usize::MAX);
                    (level, s.doc.clone(), line, id.clone())
                }
                None => (usize::MAX, "~".to_string(), usize::MAX, id.clone()),
            };
            (key, id.clone())
        })
        .collect();
    keyed.sort();
    keyed.into_iter().map(|(_, id)| id).collect()
}

// ---- state machines ----

// State names compare after trimming, lowercasing, and collapsing whitespace.
pub fn normalize_state(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

// One machine per entity any transition names as subject: states in document order
// (first spelling kept), transitions ordered by requirement id, `initial` the one state
// no transition enters. Mirrors docs/compiler/model/state-machine.md#derivation.
pub fn recompute_state_machines(store: &mut Store) {
    let mut machines: BTreeMap<String, StateMachine> = BTreeMap::new();
    for rid in document_order(store) {
        let r = &store.graph.requirements[&rid];
        let Some(t) = r.transition.as_ref() else {
            continue;
        };
        let subject = store.resolve_id(&t.subject).to_string();
        if !store.graph.entities.contains_key(&subject) {
            continue;
        }
        let key = format!("sm:{}", entity_slug(&subject));
        let m = machines.entry(key).or_insert_with(|| StateMachine {
            subject: subject.clone(),
            ..Default::default()
        });
        let mut spelled = |name: &str| -> String {
            let norm = normalize_state(name);
            match m.states.iter().find(|s| normalize_state(s) == norm) {
                Some(s) => s.clone(),
                None => {
                    let s = name.split_whitespace().collect::<Vec<_>>().join(" ");
                    m.states.push(s.clone());
                    s
                }
            }
        };
        let from = spelled(&t.from);
        let to = spelled(&t.to);
        m.transitions.push(StateTransition {
            from,
            to,
            trigger: t.trigger.clone(),
            guard: t.guard.clone(),
            requirement: rid.clone(),
        });
    }
    for m in machines.values_mut() {
        m.transitions
            .sort_by(|a, b| a.requirement.cmp(&b.requirement));
        let entered: BTreeSet<String> = m
            .transitions
            .iter()
            .map(|t| normalize_state(&t.to))
            .collect();
        let candidates: Vec<&String> = m
            .states
            .iter()
            .filter(|s| !entered.contains(&normalize_state(s)))
            .collect();
        m.initial = if candidates.len() == 1 {
            Some(candidates[0].clone())
        } else {
            None
        };
    }
    store.graph.state_machines = machines;
}

// ---- default views ----

struct DefaultView {
    id: String,
    kind: &'static str,
    title: String,
    members: Vec<String>,
    query: Option<ViewQuery>,
    from: Vec<String>,
    rule: &'static str,
}

// Each word's first letter upper-cased: the scope's name as a title.
fn title_case(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || c == '-' || c == '_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// The document's title: its root section's title, or the file stem.
fn doc_title(store: &Store, doc: &str) -> String {
    store
        .docs
        .get(doc)
        .and_then(|d| d.sections.values().find(|s| s.kind == "root"))
        .map(|s| s.title.clone())
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| doc_stem(doc))
}

fn doc_stem(doc: &str) -> String {
    let stem = doc.rsplit('/').next().unwrap_or(doc);
    stem.strip_suffix(".md").unwrap_or(stem).to_string()
}

// Number of ancestors of an entity (a root has depth 0). Bounded against a cycle.
fn depth_of(store: &Store, id: &str) -> usize {
    let mut cur = id;
    let mut depth = 0;
    while let Some(p) = store
        .graph
        .entities
        .get(cur)
        .and_then(|e| e.parent.as_deref())
    {
        depth += 1;
        cur = p;
        if depth > 64 {
            break;
        }
    }
    depth
}

// Depth of `id` below `ancestor`: Some(1) for a child, None when not a descendant.
fn depth_below(store: &Store, ancestor: &str, id: &str) -> Option<usize> {
    let mut cur = id;
    let mut depth = 0;
    while let Some(p) = store
        .graph
        .entities
        .get(cur)
        .and_then(|e| e.parent.as_deref())
    {
        depth += 1;
        if p == ancestor {
            return Some(depth);
        }
        cur = p;
        if depth > 64 {
            break;
        }
    }
    None
}

// The entities a query matches: scope, stereotype, descendants of a parent bounded by
// depth, instances excluded, the view's exclusions removed. Ordered by id.
// Mirrors docs/compiler/model/view.md#fields.
pub fn query_matches(
    store: &Store,
    q: &ViewQuery,
    excluded: &[Exclusion],
    instances: &BTreeMap<String, String>,
) -> Vec<String> {
    let parent = q.parent.as_deref().map(|p| store.resolve_id(p).to_string());
    store
        .graph
        .entities
        .iter()
        .filter(|(id, _)| !instances.contains_key(*id))
        .filter(|(id, _)| !excluded.iter().any(|x| &x.id == *id))
        .filter(|(_, e)| q.scope.as_deref().map_or(true, |s| e.scope == s))
        .filter(|(_, e)| {
            q.stereotype.as_deref().map_or(true, |s| {
                e.stereotype
                    .as_deref()
                    .is_some_and(|x| x.eq_ignore_ascii_case(s))
            })
        })
        .filter(|(id, _)| match (&parent, q.depth) {
            (Some(p), Some(d)) => depth_below(store, p, id).is_some_and(|x| x <= d as usize),
            (Some(p), None) => depth_below(store, p, id).is_some(),
            (None, Some(d)) => depth_of(store, id) <= d as usize,
            (None, None) => true,
        })
        .map(|(id, _)| id.clone())
        .collect()
}

// The actor of a requirement: the first entity labeled actor among its entities, or
// its first entity. Mirrors docs/compiler/model/view.md#default-views.
fn actor_of(store: &Store, r: &Requirement) -> Option<String> {
    let resolved: Vec<String> = r
        .entities
        .iter()
        .map(|e| store.resolve_id(e).to_string())
        .filter(|e| store.graph.entities.contains_key(e))
        .collect();
    resolved
        .iter()
        .find(|e| {
            store.graph.entities[*e]
                .stereotype
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("actor"))
        })
        .or_else(|| resolved.first())
        .cloned()
}

// The rule instances that hold on the current graph.
fn default_view_rules(store: &Store) -> Vec<DefaultView> {
    let instances = instance_types(store);
    let mut out = Vec::new();

    // A class view per scope.
    let scopes: BTreeSet<&str> = store
        .graph
        .entities
        .iter()
        .filter(|(id, _)| !instances.contains_key(*id))
        .map(|(_, e)| e.scope.as_str())
        .collect();
    for scope in scopes {
        let query = ViewQuery {
            scope: Some(scope.to_string()),
            ..Default::default()
        };
        let members = query_matches(store, &query, &[], &instances);
        if members.is_empty() {
            continue;
        }
        out.push(DefaultView {
            id: format!("view:class/{}", md::slug(scope)),
            kind: "class",
            title: title_case(scope),
            from: members.clone(),
            members,
            query: Some(query),
            rule: "class per scope",
        });
    }

    // A component view per system: a containment root with at least one child.
    let children_of = |id: &str| -> Vec<String> {
        store
            .graph
            .entities
            .iter()
            .filter(|(_, e)| e.parent.as_deref() == Some(id))
            .map(|(cid, _)| cid.clone())
            .collect()
    };
    for (sid, system) in &store.graph.entities {
        if system.parent.is_some() {
            continue;
        }
        let children = children_of(sid);
        if children.is_empty() {
            continue;
        }
        let inside = |id: &str| id == sid || depth_below(store, sid, id).is_some();
        let mut realized: Vec<String> = Vec::new();
        for rel in store.graph.relationships.values() {
            for c in &rel.contributions {
                if c.r#type == "realization" && inside(&c.a) && !realized.contains(&c.b) {
                    realized.push(c.b.clone());
                }
            }
        }
        realized.sort();
        let mut dependents: Vec<String> = Vec::new();
        for rel in store.graph.relationships.values() {
            for c in &rel.contributions {
                if c.r#type == "dependency"
                    && realized.contains(&c.b)
                    && !inside(&c.a)
                    && !dependents.contains(&c.a)
                {
                    dependents.push(c.a.clone());
                }
            }
        }
        dependents.sort();
        let mut members = children;
        for id in realized.into_iter().chain(dependents) {
            if !members.contains(&id) {
                members.push(id);
            }
        }
        let mut from = vec![sid.clone()];
        from.extend(members.iter().cloned());
        out.push(DefaultView {
            id: format!("view:component/{}", entity_slug(sid)),
            kind: "component",
            title: system.name.clone(),
            members,
            query: None,
            from,
            rule: "component per system",
        });
    }

    // A use-case view and a sequence view per flow cluster.
    let mut clusters: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for rid in document_order(store) {
        let r = &store.graph.requirements[&rid];
        let flows = r
            .facets
            .iter()
            .any(|f| f.facet == "behavior" || f.facet == "failure-mode");
        let Some(doc) = r.source.as_ref().map(|s| s.doc.clone()) else {
            continue;
        };
        if !flows {
            continue;
        }
        let Some(actor) = actor_of(store, r) else {
            continue;
        };
        clusters.entry((actor, doc)).or_default().push(rid);
    }
    for ((actor, doc), members) in clusters {
        if members.len() < 2 {
            continue;
        }
        let slug = format!("{}-{}", entity_slug(&actor), md::slug(&doc_stem(&doc)));
        let title = format!(
            "{}: {}",
            store.graph.entities[&actor].name,
            doc_title(store, &doc)
        );
        let mut from = vec![actor.clone()];
        from.extend(members.iter().cloned());
        let with_edges: Vec<String> = members
            .iter()
            .filter(|m| !store.graph.requirements[*m].edges.is_empty())
            .cloned()
            .collect();
        out.push(DefaultView {
            id: format!("view:usecase/{}", slug),
            kind: "use-case",
            title: title.clone(),
            members,
            query: None,
            from: from.clone(),
            rule: "use-case per flow cluster",
        });
        if with_edges.len() >= 2 {
            let mut from = vec![actor.clone()];
            from.extend(with_edges.iter().cloned());
            out.push(DefaultView {
                id: format!("view:sequence/{}", slug),
                kind: "sequence",
                title,
                members: with_edges,
                query: None,
                from,
                rule: "sequence per flow cluster",
            });
        }
    }

    // A state view per derived machine.
    for m in store.graph.state_machines.values() {
        let Some(subject) = store.graph.entities.get(&m.subject) else {
            continue;
        };
        out.push(DefaultView {
            id: format!("view:state/{}", entity_slug(&m.subject)),
            kind: "state",
            title: subject.name.clone(),
            members: vec![m.subject.clone()],
            query: None,
            from: vec![m.subject.clone()],
            rule: "state per state machine",
        });
    }

    // An object view per type: an entity that is `b` of an instantiation group.
    let mut by_type: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (inst, ty) in &instances {
        by_type.entry(ty.clone()).or_default().push(inst.clone());
    }
    for (ty, mut insts) in by_type {
        let Some(t) = store.graph.entities.get(&ty) else {
            continue;
        };
        insts.sort();
        let mut from = vec![ty.clone()];
        from.extend(insts.iter().cloned());
        out.push(DefaultView {
            id: format!("view:object/{}", entity_slug(&ty)),
            kind: "object",
            title: t.name.clone(),
            members: insts,
            query: None,
            from,
            rule: "object per type",
        });
    }
    out
}

// Create the default views whose rule holds, rewrite title and members on the ones
// still marked default, remove the defaults whose rule stopped holding, leave curated
// views alone; then recompute query membership on every view with a query.
// Mirrors docs/compiler/model/view.md#default-views.
pub fn recompute_default_views(store: &mut Store, build: &str, batch: &mut RecordBatch) {
    let wanted = default_view_rules(store);
    let mut wanted_ids: BTreeSet<String> = BTreeSet::new();
    for w in wanted {
        wanted_ids.insert(w.id.clone());
        let provenance = Provenance::Derived {
            from: w.from,
            reasoning: format!("default view: {}", w.rule),
        };
        match store.graph.views.get_mut(&w.id) {
            Some(v) if v.default => {
                let changed = v.title != w.title
                    || v.members != w.members
                    || v.query != w.query
                    || v.provenance.as_ref() != Some(&provenance);
                if changed {
                    v.title = w.title;
                    v.members = w.members;
                    v.query = w.query;
                    v.provenance = Some(provenance);
                    v.updated = Some(build.to_string());
                }
            }
            Some(_) => {}
            None => {
                store.graph.views.insert(
                    w.id,
                    View {
                        kind: w.kind.to_string(),
                        title: w.title,
                        members: w.members,
                        query: w.query,
                        provenance: Some(provenance),
                        default: true,
                        created: Some(build.to_string()),
                        updated: Some(build.to_string()),
                        ..Default::default()
                    },
                );
            }
        }
    }
    store
        .graph
        .views
        .retain(|id, v| !v.default || wanted_ids.contains(id));

    // Query membership recomputes at every commit: silently on a default view, as a
    // query-match record on a curated one.
    let instances = instance_types(store);
    let queried: Vec<(String, Vec<String>)> = store
        .graph
        .views
        .iter()
        .filter_map(|(id, v)| {
            v.query
                .as_ref()
                .map(|q| (id.clone(), query_matches(store, q, &v.excluded, &instances)))
        })
        .collect();
    for (id, matches) in queried {
        let v = store.graph.views.get_mut(&id).unwrap();
        if v.default {
            if v.members != matches {
                v.members = matches;
                v.updated = Some(build.to_string());
            }
            continue;
        }
        let added: Vec<String> = matches
            .into_iter()
            .filter(|m| !v.members.contains(m))
            .collect();
        if !added.is_empty() {
            v.members.extend(added.iter().cloned());
            v.updated = Some(build.to_string());
            batch.push(
                0,
                CHANGE_QUERY_MATCH,
                &id,
                "query",
                serde_json::json!({ "added": added }),
            );
        }
    }
}

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
fn live_members(store: &Store, v: &View) -> Vec<String> {
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
    let set: BTreeSet<&str> = members.iter().map(String::as_str).collect();
    store
        .graph
        .relationships
        .values()
        .flat_map(|r| r.contributions.iter())
        .filter(|c| {
            c.r#type != INSTANTIATION && set.contains(c.a.as_str()) && set.contains(c.b.as_str())
        })
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
    for r in store.graph.requirements.values() {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for e in &r.entities {
            let id = store.resolve_id(e);
            if seen.insert(id) {
                *reqs.entry(id).or_insert(0) += 1;
            }
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
            "children-per-entity",
            children.get(id.as_str()).copied().unwrap_or(0),
            &e.limits,
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
                view_edge_count(store, &members),
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
                view_edge_count(store, &members),
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
                    flow_participants(store, &flow).len() as u64,
                    &v.limits,
                );
            }
        }
    }
    out
}

// Write a threshold-crossed record once per crossing: a record already present for
// the subject and limit is refreshed in place (count, level), never duplicated; one
// whose count fell back under the threshold clears.
pub fn record_threshold_crossings(store: &mut Store, batch: &mut RecordBatch) {
    let crossings = threshold_crossings(store);
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

// The ledger comparison, persisted as ledger-stale change records when the build
// derives its board: one record per row the ledger and the graph disagree on, the
// detail refreshed in place, the record cleared when the row agrees again. Bind,
// generate, and verify goals stand on these records.
// Mirrors docs/compiler/graph.md#change-records.
pub fn record_ledger_stale(store: &mut Store, gs: &crate::gen::GenSettings) {
    let fresh = crate::gen::ledger_stale_records(store, gs);
    let mut changed = false;
    let before = store.status.changes.len();
    store.status.changes.retain(|c| {
        c.kind != crate::goals::CHANGE_LEDGER_STALE
            || fresh
                .iter()
                .any(|f| f.subject == c.subject && f.detail["goal"] == c.detail["goal"])
    });
    changed |= store.status.changes.len() != before;
    for f in fresh {
        let pos = store.status.changes.iter().position(|c| {
            c.kind == crate::goals::CHANGE_LEDGER_STALE
                && c.subject == f.subject
                && c.detail["goal"] == f.detail["goal"]
        });
        match pos {
            Some(i) => {
                if store.status.changes[i].detail != f.detail {
                    store.status.changes[i].detail = f.detail;
                    changed = true;
                }
            }
            None => {
                store.status.changes.push(f);
                changed = true;
            }
        }
    }
    if changed {
        store.save_status();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::store::Store;

    fn src(doc: &str, sec: &str, q: &str) -> SourceRef {
        SourceRef {
            doc: doc.into(),
            section: sec.into(),
            quote: q.into(),
        }
    }

    fn edge(a: &str, b: &str, t: &str, card: Option<&str>) -> ReqEdge {
        ReqEdge {
            a: a.into(),
            b: b.into(),
            rel_type: Some(t.into()),
            cardinality: card.map(String::from),
        }
    }

    fn attr(name: &str, ty: Option<&str>, value: Option<&str>) -> Attribute {
        Attribute {
            name: name.into(),
            r#type: ty.map(String::from),
            value: value.map(String::from),
            provenance: Provenance::Quote(src("shop.md", "/shop", "attributes")),
        }
    }

    fn behavior() -> Facet {
        Facet {
            facet: "behavior".into(),
            reasoning: "a step".into(),
            measure: None,
        }
    }

    // The showcase graph of plans/ir-graph.md#every-diagram-from-one-example-graph,
    // in an in-memory store with the one document its quotes cite.
    pub(crate) fn showcase_store() -> Store {
        let mut s = Store::default();
        let text = "# Shop\n\nThe shop.\n\n## Checkout\n\ncheckout\n\n## Orders\n\norders\n\n## Examples\n\nexamples\n";
        s.docs.insert(
            "shop.md".into(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
        let ent = |name: &str,
                   stereotype: Option<&str>,
                   parent: Option<&str>,
                   attributes: Vec<Attribute>| Entity {
            name: name.into(),
            stereotype: stereotype.map(String::from),
            parent: parent.map(String::from),
            attributes,
            ..Default::default()
        };
        let entities = [
            (
                "ent:shop",
                ent(
                    "Shop",
                    Some("system"),
                    None,
                    vec![attr("region", None, Some("EU"))],
                ),
            ),
            (
                "ent:order-service",
                ent("Order Service", Some("service"), Some("ent:shop"), vec![]),
            ),
            (
                "ent:inventory-service",
                ent(
                    "Inventory Service",
                    Some("service"),
                    Some("ent:shop"),
                    vec![],
                ),
            ),
            (
                "ent:checkout-api",
                ent(
                    "checkout API",
                    Some("interface"),
                    Some("ent:order-service"),
                    vec![],
                ),
            ),
            (
                "ent:stock-api",
                ent(
                    "stock API",
                    Some("interface"),
                    Some("ent:inventory-service"),
                    vec![],
                ),
            ),
            (
                "ent:customer",
                ent(
                    "Customer",
                    Some("actor"),
                    None,
                    vec![attr("tier", Some("string"), None)],
                ),
            ),
            (
                "ent:shopping-cart",
                ent(
                    "Shopping Cart",
                    None,
                    Some("ent:order-service"),
                    vec![attr("items", None, None), attr("currency", None, None)],
                ),
            ),
            (
                "ent:order",
                ent(
                    "Order",
                    None,
                    Some("ent:order-service"),
                    vec![attr("total", None, None), attr("currency", None, None)],
                ),
            ),
            (
                "ent:order-item",
                ent("Order Item", None, Some("ent:order-service"), vec![]),
            ),
            (
                "ent:ana",
                ent("Ana", None, None, vec![attr("tier", None, Some("gold"))]),
            ),
            (
                "ent:anas-cart",
                ent(
                    "Ana's cart",
                    None,
                    None,
                    vec![
                        attr("items", None, Some("3")),
                        attr("currency", None, Some("EUR")),
                    ],
                ),
            ),
        ];
        for (id, e) in entities {
            s.graph.entities.insert(id.into(), e);
        }
        let req = |statement: &str,
                   section: &str,
                   entities: &[&str],
                   edges: Vec<ReqEdge>,
                   transition: Option<Transition>,
                   facets: Vec<Facet>| Requirement {
            statement: statement.into(),
            entities: entities.iter().map(|e| e.to_string()).collect(),
            edges,
            transition,
            facets,
            source: Some(src("shop.md", section, statement)),
            ..Default::default()
        };
        let reqs = [
            ("req:shop-1", req("The customer submits the shopping cart through the checkout API.", "/shop/checkout",
                &["ent:customer", "ent:shopping-cart", "ent:checkout-api"],
                vec![edge("ent:customer", "ent:checkout-api", "dependency", None), edge("ent:customer", "ent:shopping-cart", "association", None)],
                None, vec![behavior()])),
            ("req:shop-2", req("The order service provides the checkout API.", "/shop",
                &["ent:order-service", "ent:checkout-api"],
                vec![edge("ent:order-service", "ent:checkout-api", "realization", None)], None, vec![])),
            ("req:shop-3", req("When checkout succeeds, the order service reserves stock through the stock API.", "/shop/checkout",
                &["ent:customer", "ent:order-service", "ent:stock-api"],
                vec![edge("ent:order-service", "ent:stock-api", "dependency", None)], None, vec![behavior()])),
            ("req:shop-4", req("The inventory service provides the stock API.", "/shop",
                &["ent:inventory-service", "ent:stock-api"],
                vec![edge("ent:inventory-service", "ent:stock-api", "realization", None)], None, vec![])),
            ("req:shop-5", req("An order carries a total and a currency.", "/shop/orders", &["ent:order"], vec![], None, vec![])),
            ("req:shop-6", req("A shopping cart holds one or more order items.", "/shop",
                &["ent:shopping-cart", "ent:order-item"],
                vec![edge("ent:shopping-cart", "ent:order-item", "composition", Some("1..*"))], None, vec![])),
            ("req:shop-7", req("When payment succeeds, the order becomes paid.", "/shop/orders", &["ent:customer", "ent:order"], vec![],
                Some(Transition { subject: "ent:order".into(), from: "placed".into(), to: "paid".into(), trigger: Some("payment succeeds".into()), guard: None }),
                vec![behavior()])),
            ("req:shop-8", req("If payment is declined, then the order is held for review.", "/shop/orders", &["ent:customer", "ent:order"], vec![],
                Some(Transition { subject: "ent:order".into(), from: "placed".into(), to: "held".into(), trigger: Some("payment declined".into()), guard: None }),
                vec![Facet { facet: "failure-mode".into(), reasoning: "the declined branch".into(), measure: None }])),
            ("req:shop-9", req("Ana, a gold-tier customer, keeps 3 items in her cart, priced in EUR.", "/shop/examples",
                &["ent:ana", "ent:customer", "ent:anas-cart", "ent:shopping-cart"],
                vec![edge("ent:ana", "ent:customer", "instantiation", None), edge("ent:anas-cart", "ent:shopping-cart", "instantiation", None), edge("ent:ana", "ent:anas-cart", "association", None)],
                None, vec![])),
            ("req:shop-10", req("The shop shall confirm checkout within 2 seconds.", "/shop", &["ent:shop"], vec![], None,
                vec![Facet { facet: "quality".into(), reasoning: "bounded".into(), measure: Some("2 seconds".into()) }])),
            ("req:shop-11", req("The shop is deployed in the EU region.", "/shop", &["ent:shop"], vec![], None, vec![])),
        ];
        for (id, r) in reqs {
            s.graph.requirements.insert(id.into(), r);
        }
        s
    }

    #[test]
    fn relationships_group_per_direction_and_type() {
        let mut s = Store::default();
        for id in ["ent:a", "ent:b"] {
            s.graph.entities.insert(
                id.into(),
                Entity {
                    name: id.into(),
                    ..Default::default()
                },
            );
        }
        let req = |edges: Vec<ReqEdge>| Requirement {
            statement: "x".into(),
            entities: vec!["ent:a".into(), "ent:b".into()],
            edges,
            source: Some(src("t.md", "/t", "x")),
            ..Default::default()
        };
        s.graph.requirements.insert(
            "req:t-1".into(),
            req(vec![ReqEdge {
                a: "ent:a".into(),
                b: "ent:b".into(),
                rel_type: None,
                cardinality: None,
            }]),
        );
        s.graph.requirements.insert(
            "req:t-2".into(),
            req(vec![edge("ent:b", "ent:a", "association", Some("1"))]),
        );
        s.graph.requirements.insert(
            "req:t-3".into(),
            req(vec![edge("ent:a", "ent:b", "dependency", None)]),
        );
        s.graph.requirements.insert(
            "req:t-4".into(),
            req(vec![edge("ent:b", "ent:a", "association", Some("*"))]),
        );
        recompute_relationships(&mut s);
        assert_eq!(s.graph.relationships.len(), 1);
        let rel = &s.graph.relationships["rel:a~b"];
        assert_eq!(rel.contributions.len(), 2);
        let ab = rel.contributions.iter().find(|c| c.a == "ent:a").unwrap();
        assert_eq!(ab.r#type, "dependency");
        assert_eq!(
            ab.requirements,
            vec!["req:t-1".to_string(), "req:t-3".to_string()]
        );
        let ba = rel.contributions.iter().find(|c| c.a == "ent:b").unwrap();
        assert_eq!(ba.r#type, "association");
        // Disagreeing cardinalities leave the group without one.
        assert_eq!(ba.cardinality, None);
        assert_eq!(rel.strongest(), "association");
    }

    #[test]
    fn state_machine_derives_initial_and_keeps_a_nondeterministic_pair() {
        let mut s = showcase_store();
        recompute_relationships(&mut s);
        recompute_state_machines(&mut s);
        let m = &s.graph.state_machines["sm:order"];
        assert_eq!(m.subject, "ent:order");
        assert_eq!(m.states, vec!["placed", "paid", "held"]);
        assert_eq!(m.initial.as_deref(), Some("placed"));
        assert_eq!(m.transitions.len(), 2);
        assert_eq!(m.transitions[0].requirement, "req:shop-7");
        assert_eq!(m.transitions[1].to, "held");
        // A second transition out of placed on the same trigger, spelled differently,
        // lands on the same machine as a nondeterministic pair for the checks.
        s.graph.requirements.insert(
            "req:shop-99".into(),
            Requirement {
                statement: "When payment succeeds twice, the order is refunded.".into(),
                entities: vec!["ent:order".into()],
                transition: Some(Transition {
                    subject: "ent:order".into(),
                    from: " Placed".into(),
                    to: "refunded".into(),
                    trigger: Some("payment succeeds".into()),
                    guard: None,
                }),
                source: Some(src("shop.md", "/shop/orders", "twice")),
                ..Default::default()
            },
        );
        recompute_state_machines(&mut s);
        let m = &s.graph.state_machines["sm:order"];
        assert_eq!(m.states, vec!["placed", "paid", "held", "refunded"]);
        let same_trigger: Vec<&StateTransition> = m
            .transitions
            .iter()
            .filter(|t| t.from == "placed" && t.trigger.as_deref() == Some("payment succeeds"))
            .collect();
        assert_eq!(same_trigger.len(), 2);
        // Two states nobody enters: no initial.
        s.graph
            .requirements
            .get_mut("req:shop-99")
            .unwrap()
            .transition = Some(Transition {
            subject: "ent:order".into(),
            from: "draft".into(),
            to: "paid".into(),
            trigger: None,
            guard: None,
        });
        recompute_state_machines(&mut s);
        assert_eq!(s.graph.state_machines["sm:order"].initial, None);
        // The last transition gone removes the machine.
        for id in ["req:shop-7", "req:shop-8", "req:shop-99"] {
            s.graph.requirements.remove(id);
        }
        recompute_state_machines(&mut s);
        assert!(s.graph.state_machines.is_empty());
    }

    #[test]
    fn document_order_takes_link_levels_ahead_of_path() {
        let mut s = Store::default();
        let a = "# A\n\nalpha\n";
        let z = "# Z\n\nsee [a](./a.md)\n";
        for (doc, text) in [("a.md", a), ("z.md", z)] {
            s.docs.insert(
                doc.into(),
                DocRecord {
                    content_hash: hash_hex(text),
                    sections: md::parse_sections(text),
                    coverage: BTreeMap::new(),
                },
            );
        }
        let req = |doc: &str, section: &str| Requirement {
            statement: format!("{} states a fact.", doc),
            source: Some(src(doc, section, "")),
            ..Default::default()
        };
        s.graph
            .requirements
            .insert("req:a-1".into(), req("a.md", "/a"));
        s.graph
            .requirements
            .insert("req:z-1".into(), req("z.md", "/z"));
        // z.md is the stamped root and links a.md: link level orders ahead of path.
        s.status.roots = vec!["z.md".into()];
        assert_eq!(
            document_order(&s),
            vec!["req:z-1".to_string(), "req:a-1".to_string()]
        );
        // No stamped roots: every document is its own level in path order.
        s.status.roots.clear();
        assert_eq!(
            document_order(&s),
            vec!["req:a-1".to_string(), "req:z-1".to_string()]
        );
    }

    #[test]
    fn default_views_derive_from_the_showcase_graph() {
        let mut s = showcase_store();
        let mut batch = RecordBatch::new(1);
        recompute(&mut s, "g1", &mut batch);
        let views = &s.graph.views;
        // Class per scope: every non-instance entity of the public scope.
        let class = &views["view:class/public"];
        assert_eq!(class.kind, "class");
        assert_eq!(class.title, "Public");
        assert!(class.default);
        assert_eq!(class.members.len(), 9, "{:?}", class.members);
        assert!(!class.members.contains(&"ent:ana".to_string()));
        assert_eq!(
            class.query,
            Some(ViewQuery {
                scope: Some("public".into()),
                ..Default::default()
            })
        );
        // Component per system: the children, the interfaces they realize, and the
        // outside dependents on those interfaces.
        let comp = &views["view:component/shop"];
        assert_eq!(comp.title, "Shop");
        assert_eq!(
            comp.members,
            vec![
                "ent:inventory-service",
                "ent:order-service",
                "ent:checkout-api",
                "ent:stock-api",
                "ent:customer"
            ]
        );
        // State per machine, object per type.
        let state = &views["view:state/order"];
        assert_eq!(state.members, vec!["ent:order"]);
        assert_eq!(state.title, "Order");
        assert_eq!(views["view:object/customer"].members, vec!["ent:ana"]);
        assert_eq!(
            views["view:object/shopping-cart"].members,
            vec!["ent:anas-cart"]
        );
        // Flow cluster: the customer's behavior in shop.md, in document order.
        let uc = &views["view:usecase/customer-shop"];
        assert_eq!(uc.kind, "use-case");
        assert_eq!(uc.title, "Customer: Shop");
        assert_eq!(
            uc.members,
            vec!["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"]
        );
        let seq = &views["view:sequence/customer-shop"];
        assert_eq!(seq.members, vec!["req:shop-1", "req:shop-3"]);
        assert!(matches!(
            uc.provenance.as_ref(),
            Some(Provenance::Derived { reasoning, .. }) if reasoning == "default view: use-case per flow cluster"
        ));
        assert!(batch.records().is_empty());

        // A curated view is left alone and a default whose rule stops holding goes.
        s.graph.views.get_mut("view:state/order").unwrap().default = false;
        s.graph.views.get_mut("view:state/order").unwrap().members = vec!["ent:shop".into()];
        s.graph.requirements.remove("req:shop-9");
        let mut batch = RecordBatch::new(2);
        recompute(&mut s, "g2", &mut batch);
        assert_eq!(s.graph.views["view:state/order"].members, vec!["ent:shop"]);
        assert!(!s.graph.views.contains_key("view:object/customer"));
        assert!(s.graph.views.contains_key("view:class/public"));
        // Excluded and collapse survive on a still-default view.
        let class = s.graph.views.get_mut("view:class/public").unwrap();
        class.excluded.push(Exclusion {
            id: "ent:shop".into(),
            note: "the boundary, not a class".into(),
        });
        class.collapse.push("ent:order-service".into());
        let mut batch = RecordBatch::new(3);
        recompute(&mut s, "g3", &mut batch);
        let class = &s.graph.views["view:class/public"];
        assert!(class.default);
        assert_eq!(class.excluded.len(), 1);
        assert_eq!(class.collapse, vec!["ent:order-service"]);
        assert!(!class.members.contains(&"ent:shop".to_string()));
        // A curated query view gets new matches as a query-match record.
        s.graph.views.insert(
            "view:class/services".into(),
            View {
                kind: "class".into(),
                title: "Services".into(),
                query: Some(ViewQuery {
                    stereotype: Some("service".into()),
                    ..Default::default()
                }),
                provenance: Some(Provenance::Decree {
                    author: "owner".into(),
                    at: "g3".into(),
                    note: None,
                }),
                ..Default::default()
            },
        );
        let mut batch = RecordBatch::new(4);
        recompute(&mut s, "g4", &mut batch);
        assert_eq!(
            s.graph.views["view:class/services"].members,
            vec!["ent:inventory-service", "ent:order-service"]
        );
        let records = batch.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, CHANGE_QUERY_MATCH);
        assert_eq!(records[0].subject, "view:class/services");
    }

    #[test]
    fn threshold_crossings_count_against_bumps() {
        let mut s = showcase_store();
        let mut batch = RecordBatch::new(1);
        recompute(&mut s, "g1", &mut batch);
        assert!(threshold_crossings(&s).is_empty());
        // Eleven children cross children-per-entity at ten.
        for i in 0..11 {
            s.graph.entities.insert(
                format!("ent:part-{}", i),
                Entity {
                    name: format!("Part {}", i),
                    parent: Some("ent:order".into()),
                    ..Default::default()
                },
            );
        }
        let crossings = threshold_crossings(&s);
        assert_eq!(crossings.len(), 1);
        assert_eq!(crossings[0].subject, "ent:order");
        assert_eq!(crossings[0].limit, "children-per-entity");
        assert_eq!(
            (crossings[0].count, crossings[0].soft, crossings[0].hard),
            (11, 10, 20)
        );
        let mut batch = RecordBatch::new(2);
        record_threshold_crossings(&mut s, &mut batch);
        assert_eq!(batch.records().len(), 1);
        assert_eq!(batch.records()[0].detail["level"], "soft");
        // A bump raises the threshold and the crossing lapses.
        s.graph
            .entities
            .get_mut("ent:order")
            .unwrap()
            .limits
            .insert("children-per-entity".into(), LimitBump { value: 15 });
        assert!(threshold_crossings(&s).is_empty());
    }
}
