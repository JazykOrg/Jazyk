// The view endpoints: the catalog with limits state, and one view resolved for
// drawing (members in order, lifted arrows with the concrete edges beneath them,
// flow steps, the derived machine, and the rendered puml and svg).
// Mirrors docs/frontends/gui.md#api and docs/compiler/model/view.md#membership.
use super::state::SharedState;
use crate::model::{rel_rank, View, INSTANTIATION};
use crate::store::Store;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const STRUCTURAL: [&str; 6] = [
    "class",
    "object",
    "package",
    "component",
    "composite",
    "deployment",
];

// The effective members: the stored list plus what the query matches, exclusions
// removed, stored order first.
fn effective_members(store: &Store, view: &View) -> Vec<String> {
    let mut out: Vec<String> = view.members.clone();
    if let Some(q) = &view.query {
        let instances = crate::derive::instance_types(store);
        for id in crate::derive::query_matches(store, q, &view.excluded, &instances) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

// Where the view stands against each limit that applies to its kind.
fn limits_state(store: &Store, view: &View, members: &[String]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut push = |name: &str, count: u64| {
        let bump = view.limits.get(name).map(|b| b.value);
        if let Some((soft, hard)) = crate::limits::threshold(name, bump) {
            out.push(json!({
                "limit": name, "count": count, "soft": soft, "hard": hard,
                "over": count > soft, "overHard": count > hard,
            }));
        }
    };
    let n = members.len() as u64;
    if STRUCTURAL.contains(&view.kind.as_str()) {
        push("members-per-structural-view", n);
        push(
            "edges-per-view",
            crate::derive::view_edge_count(store, members),
        );
    } else {
        push("members-per-flow-view", n);
    }
    if view.kind == "sequence" {
        push(
            "participants-per-sequence-view",
            crate::derive::flow_participants(store, members).len() as u64,
        );
    }
    if view.kind == "object" {
        push("instances-per-object-view", n);
    }
    out
}

// Every view, default and curated, with counts and limits state.
pub fn views_value(store: &Store) -> Value {
    let views: Vec<Value> = store
        .graph
        .views
        .iter()
        .map(|(id, v)| {
            let members = effective_members(store, v);
            json!({
                "id": id,
                "kind": v.kind,
                "title": v.title,
                "default": v.default,
                "members": members.len(),
                "edges": crate::derive::view_edge_count(store, &members),
                "limits": limits_state(store, v, &members),
            })
        })
        .collect();
    json!({ "views": views })
}

pub async fn views(State(st): State<SharedState>) -> Json<Value> {
    let store = super::api::load_store(&st).await;
    Json(views_value(&store))
}

// The entity members hidden by the collapse set: strict descendants of a collapsed
// node. The collapsed node itself stays shown.
fn hidden_by_collapse(
    store: &Store,
    members: &BTreeSet<String>,
    collapse: &[String],
) -> BTreeSet<String> {
    let collapsed: BTreeSet<&str> = collapse.iter().map(String::as_str).collect();
    let mut hidden = BTreeSet::new();
    for m in members {
        let mut cur = store.graph.entities.get(m).and_then(|e| e.parent.clone());
        let mut hops = 0;
        while let Some(p) = cur {
            let p = store.resolve_id(&p).to_string();
            if collapsed.contains(p.as_str()) {
                hidden.insert(m.clone());
                break;
            }
            cur = store.graph.entities.get(&p).and_then(|e| e.parent.clone());
            hops += 1;
            if hops > 64 {
                break;
            }
        }
    }
    hidden
}

// The nearest shown ancestor of an entity: itself when shown, else up the parent
// chain. Mirrors docs/compiler/diagrams.md#lifting-and-collapse.
fn lift(store: &Store, shown: &BTreeSet<String>, id: &str) -> Option<String> {
    let mut cur = store.resolve_id(id).to_string();
    let mut hops = 0;
    loop {
        if shown.contains(&cur) {
            return Some(cur);
        }
        let parent = store
            .graph
            .entities
            .get(&cur)
            .and_then(|e| e.parent.clone())?;
        cur = store.resolve_id(&parent).to_string();
        hops += 1;
        if hops > 64 {
            return None;
        }
    }
}

// The arrows the view draws: one per shown pair, direction, and type, the concrete
// edges beneath each lifted arrow listed for the inspector.
fn view_arrows(store: &Store, shown: &BTreeSet<String>) -> Vec<Value> {
    struct Group {
        cardinality: Option<String>,
        requirements: BTreeSet<String>,
        concrete: Vec<Value>,
        lifted: bool,
        rel: String,
    }
    let mut groups: BTreeMap<(String, String, String), Group> = BTreeMap::new();
    for (rel_id, rel) in &store.graph.relationships {
        for c in &rel.contributions {
            let (Some(a), Some(b)) = (lift(store, shown, &c.a), lift(store, shown, &c.b)) else {
                continue;
            };
            if a == b {
                continue;
            }
            let lifted = a != store.resolve_id(&c.a) || b != store.resolve_id(&c.b);
            let g = groups
                .entry((a, b, c.r#type.clone()))
                .or_insert_with(|| Group {
                    cardinality: c.cardinality.clone(),
                    requirements: BTreeSet::new(),
                    concrete: Vec::new(),
                    lifted: false,
                    rel: rel_id.clone(),
                });
            g.lifted |= lifted;
            if !lifted {
                g.cardinality = g.cardinality.clone().or_else(|| c.cardinality.clone());
            }
            g.requirements.extend(c.requirements.iter().cloned());
            g.concrete.push(json!({
                "a": c.a, "b": c.b, "type": c.r#type,
                "cardinality": c.cardinality, "requirements": c.requirements,
            }));
        }
    }
    // Collapse groups that share a pair and direction to the strongest ranked type;
    // instantiation keeps its own arrow.
    let mut by_pair: BTreeMap<(String, String), Vec<((String, String, String), Group)>> =
        BTreeMap::new();
    for (key, g) in groups {
        by_pair
            .entry((key.0.clone(), key.1.clone()))
            .or_default()
            .push((key, g));
    }
    let mut arrows = Vec::new();
    for ((a, b), mut list) in by_pair {
        list.sort_by_key(|((_, _, t), _)| rel_rank(t));
        let ranked: Vec<&((String, String, String), Group)> = list
            .iter()
            .filter(|((_, _, t), _)| t != INSTANTIATION)
            .collect();
        let mut emit = |ty: &str, members: Vec<&((String, String, String), Group)>| {
            if members.is_empty() {
                return;
            }
            let mut requirements: BTreeSet<String> = BTreeSet::new();
            let mut concrete: Vec<Value> = Vec::new();
            let mut lifted = false;
            let mut cardinality: Option<String> = None;
            let rel = members[0].1.rel.clone();
            for (_, g) in &members {
                requirements.extend(g.requirements.iter().cloned());
                concrete.extend(g.concrete.iter().cloned());
                lifted |= g.lifted;
                cardinality = cardinality.or_else(|| g.cardinality.clone());
            }
            arrows.push(json!({
                "a": a, "b": b, "type": ty, "lifted": lifted,
                "count": concrete.len(), "cardinality": cardinality,
                "requirements": requirements, "concrete": concrete, "rel": rel,
            }));
        };
        if !ranked.is_empty() {
            let ty = ranked[0].0 .2.clone();
            emit(&ty, ranked);
        }
        let inst: Vec<&((String, String, String), Group)> = list
            .iter()
            .filter(|((_, _, t), _)| t == INSTANTIATION)
            .collect();
        emit(INSTANTIATION, inst);
    }
    arrows
}

// One view resolved for drawing. This is what the map draws; the rendered picture
// rides beside it so the inspector can show the diagram.
pub fn view_value(store: &Store, id: &str) -> Option<Value> {
    let id = store.resolve_id(id).to_string();
    let view = store.graph.views.get(&id)?;
    let members = effective_members(store, view);
    let entity_members: BTreeSet<String> = members
        .iter()
        .map(|m| store.resolve_id(m).to_string())
        .filter(|m| store.graph.entities.contains_key(m))
        .collect();
    let hidden = hidden_by_collapse(store, &entity_members, &view.collapse);
    let shown: BTreeSet<String> = entity_members.difference(&hidden).cloned().collect();
    let member_detail: Vec<Value> = members
        .iter()
        .map(|m| {
            let rid = store.resolve_id(m).to_string();
            if let Some(e) = store.graph.entities.get(&rid) {
                json!({
                    "id": rid, "node": "entity", "name": e.name,
                    "stereotype": e.stereotype, "parent": e.parent,
                    "hidden": hidden.contains(&rid),
                })
            } else if let Some(r) = store.graph.requirements.get(&rid) {
                json!({
                    "id": rid, "node": "requirement", "statement": r.statement,
                    "entities": r.entities, "transition": r.transition,
                })
            } else {
                json!({ "id": rid, "node": "gone" })
            }
        })
        .collect();
    // Flow kinds carry ordered steps with their participants.
    let steps: Vec<Value> = if crate::goals::is_flow_kind(&view.kind) {
        members
            .iter()
            .filter_map(|m| {
                let rid = store.resolve_id(m).to_string();
                store.graph.requirements.get(&rid).map(|r| {
                    let participants: Vec<Value> = r
                        .entities
                        .iter()
                        .map(|e| {
                            let eid = store.resolve_id(e).to_string();
                            let name = store
                                .graph
                                .entities
                                .get(&eid)
                                .map(|x| x.name.clone())
                                .unwrap_or_else(|| eid.clone());
                            json!({ "id": eid, "name": name })
                        })
                        .collect();
                    json!({
                        "requirement": rid, "statement": r.statement,
                        "participants": participants, "transition": r.transition,
                    })
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    // A state view carries the derived machine of its subject(s).
    let machines: Vec<Value> = if view.kind == "state" {
        store
            .graph
            .state_machines
            .iter()
            .filter(|(_, m)| entity_members.contains(&m.subject) || members.contains(&m.subject))
            .map(|(mid, m)| json!({ "id": mid, "machine": m }))
            .collect()
    } else {
        Vec::new()
    };
    let (puml, svg, render_error) = match crate::render::render_view(store, &id) {
        Ok((puml, svg)) => (Some(puml), Some(svg), None),
        Err(e) => (None, None, Some(e.0)),
    };
    Some(json!({
        "id": id,
        "kind": view.kind,
        "title": view.title,
        "default": view.default,
        "members": member_detail,
        "excluded": view.excluded,
        "collapse": view.collapse,
        "query": view.query,
        "provenance": view.provenance,
        "limits": limits_state(store, view, &members),
        "arrows": view_arrows(store, &shown),
        "steps": steps,
        "machines": machines,
        "puml": puml,
        "svg": svg,
        "renderError": render_error,
    }))
}

pub async fn view(State(st): State<SharedState>, UrlPath(id): UrlPath<String>) -> Response {
    let store = super::api::load_store(&st).await;
    match view_value(&store, &id) {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no view {}", id)),
    }
}
