// The view endpoints: the catalog with limits state, one view resolved for drawing
// (members in order, lifted arrows with the concrete edges beneath them, flow steps,
// the derived machine, the children one level down, and the rendered puml and svg),
// and the containment tree with each node's level view ids.
// Mirrors docs/frontends/gui.md#api and docs/compiler/model/view.md#membership.
use super::state::SharedState;
use crate::model::{rel_rank, View, INSTANTIATION};
use crate::store::{scope_root_target, Store};
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
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

// The direct children of every shown entity that are not shown yet, by parent:
// what one more level of detail draws. Mirrors docs/frontends/gui.md#explore.
fn next_level(store: &Store, shown: &BTreeSet<String>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = store
        .graph
        .entities
        .iter()
        .filter(|(id, _)| !shown.contains(*id))
        .filter_map(|(id, e)| {
            let parent = store.resolve_id(e.parent.as_deref()?).to_string();
            shown.contains(&parent).then(|| (id.clone(), parent))
        })
        .collect();
    out.sort();
    out
}

// One view resolved for drawing. This is what the map draws; the rendered picture
// rides beside it so the inspector can show the diagram. `detail` expands every
// shown entity that many levels down: its children join the shown set, marked with
// the level they came in at, and the arrows lift to the wider set the same way.
pub fn view_value(store: &Store, id: &str, detail: usize) -> Option<Value> {
    let id = store.resolve_id(id).to_string();
    let view = store.graph.views.get(&id)?;
    let members = effective_members(store, view);
    let entity_members: BTreeSet<String> = members
        .iter()
        .map(|m| store.resolve_id(m).to_string())
        .filter(|m| store.graph.entities.contains_key(m))
        .collect();
    let hidden = hidden_by_collapse(store, &entity_members, &view.collapse);
    let mut shown: BTreeSet<String> = entity_members.difference(&hidden).cloned().collect();
    let mut member_detail: Vec<Value> = members
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
    // Detail: each level adds the children of what is shown, nested inside their
    // parents; a level that adds nothing ends the expansion early.
    let mut applied = 0;
    for level in 1..=detail {
        let next = next_level(store, &shown);
        if next.is_empty() {
            break;
        }
        applied = level;
        for (cid, parent) in next {
            let e = &store.graph.entities[&cid];
            member_detail.push(json!({
                "id": cid, "node": "entity", "name": e.name,
                "stereotype": e.stereotype, "parent": parent,
                "hidden": false, "detail": level,
            }));
            shown.insert(cid);
        }
    }
    let deeper = !next_level(store, &shown).is_empty();
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
        "children": children_value(store, &id),
        "detail": applied,
        "deeper": deeper,
        "puml": puml,
        "svg": svg,
        "renderError": render_error,
    }))
}

// The level views reachable from a view's members: `{member, view}` for every drawn
// entity with a level view of its own, the same list `get_view` answers.
// Mirrors docs/compiler/concepts/levels.md#drill-down.
fn children_value(store: &Store, view_id: &str) -> Vec<Value> {
    crate::derive::children_of_view(store, view_id)
        .into_iter()
        .map(|(member, view)| json!({ "member": member, "view": view }))
        .collect()
}

#[derive(Deserialize)]
pub struct ViewQ {
    // Levels of detail beneath the members (docs/frontends/gui.md#explore).
    detail: Option<usize>,
}

pub async fn view(
    State(st): State<SharedState>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<ViewQ>,
) -> Response {
    let store = super::api::load_store(&st).await;
    match view_value(&store, &id, q.detail.unwrap_or(0)) {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no view {}", id)),
    }
}

// The direct children of a level target, the level view's member order first (the
// children lead its members, in document order), the rest by id.
fn ordered_children(store: &Store, target: &str, ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = crate::derive::level_view_members(store, target)
        .into_iter()
        .filter(|m| ids.contains(m))
        .collect();
    for id in ids {
        if !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

// The containment tree the `parent` field makes: one root per scope (the scope root,
// addressed `scope:<scope>`), each node with its child count, its structural level
// view, the flow views derived for its level, and its grouping mark. Computed at read
// time from the shards, never stored. Mirrors docs/frontends/gui.md#graph.
pub fn tree_value(store: &Store) -> Value {
    // The flow views by the level they derive for.
    let mut flows: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, v) in &store.graph.views {
        if crate::goals::is_flow_kind(&v.kind) {
            if let Some(level) = crate::derive::flow_view_level(store, id) {
                flows.entry(level).or_default().push(id.clone());
            }
        }
    }
    // Children by parent; a parent that resolves to no entity leaves its children at
    // the scope root, so nothing disappears from the tree.
    let mut by_parent: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut scopes: BTreeSet<String> = BTreeSet::new();
    for (id, e) in &store.graph.entities {
        scopes.insert(e.scope.clone());
        let parent = e
            .parent
            .as_deref()
            .map(|p| store.resolve_id(p).to_string())
            .filter(|p| store.graph.entities.contains_key(p));
        let key = parent.unwrap_or_else(|| scope_root_target(&e.scope));
        by_parent.entry(key).or_default().push(id.clone());
    }
    fn node(
        store: &Store,
        by_parent: &BTreeMap<String, Vec<String>>,
        flows: &BTreeMap<String, Vec<String>>,
        id: &str,
        depth: usize,
    ) -> Value {
        let e = &store.graph.entities[id];
        let ids = by_parent.get(id).cloned().unwrap_or_default();
        // A containment cycle stops here rather than recursing forever.
        let children: Vec<Value> = if depth > 64 {
            Vec::new()
        } else {
            ordered_children(store, id, &ids)
                .iter()
                .map(|c| node(store, by_parent, flows, c, depth + 1))
                .collect()
        };
        json!({
            "id": id,
            "name": e.name,
            "stereotype": e.stereotype,
            "grouping": store.is_grouping(id),
            "count": ids.len(),
            "levelView": crate::derive::level_view_id(store, id),
            "views": flows.get(id).cloned().unwrap_or_default(),
            "children": children,
        })
    }
    let roots: Vec<Value> = scopes
        .iter()
        .map(|scope| {
            let target = scope_root_target(scope);
            let ids = by_parent.get(&target).cloned().unwrap_or_default();
            let children: Vec<Value> = ordered_children(store, &target, &ids)
                .iter()
                .map(|c| node(store, &by_parent, &flows, c, 0))
                .collect();
            json!({
                "scope": scope,
                "target": target,
                "count": ids.len(),
                "levelView": crate::derive::level_view_id(store, &target),
                "views": flows.get(&target).cloned().unwrap_or_default(),
                "children": children,
            })
        })
        .collect();
    json!({ "generation": store.status.generation, "roots": roots })
}

pub async fn tree(State(st): State<SharedState>) -> Json<Value> {
    let store = super::api::load_store(&st).await;
    Json(tree_value(&store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::RecordBatch;

    // The showcase graph with its derived data in place: the shop level view over the
    // two services, the order service's level view over its four children.
    fn store() -> Store {
        let mut s = crate::derive::tests::showcase_store();
        crate::derive::recompute(&mut s, "g1", &mut RecordBatch::new(1));
        s
    }

    // A view answers with the level views reachable from its members: the order
    // service has a level of its own, the inventory service (one child) does not.
    #[test]
    fn view_value_carries_children_with_level_views() {
        let s = store();
        let v = view_value(&s, "view:component/shop", 0).expect("the shop level view");
        assert_eq!(v["detail"], 0);
        assert_eq!(v["deeper"], true);
        let children = v["children"].as_array().expect("a children list");
        let pairs: Vec<(&str, &str)> = children
            .iter()
            .map(|c| (c["member"].as_str().unwrap(), c["view"].as_str().unwrap()))
            .collect();
        assert_eq!(
            pairs,
            vec![("ent:order-service", "view:component/order-service")]
        );
        let members: Vec<&str> = v["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert!(members.contains(&"ent:inventory-service"));
    }

    // One level of detail draws the members' children inside them and lifts the
    // arrows to the wider set: the dependency that lifted onto the inventory service
    // at the level's own detail now lands on the stock api itself. The expansion
    // stops where the tree ends, whatever detail was asked for.
    // Mirrors docs/frontends/gui.md#explore.
    #[test]
    fn view_value_detail_expands_every_grouping_one_level_and_relifts() {
        let s = store();
        let flat = view_value(&s, "view:component/shop", 0).unwrap();
        let lifted = flat["arrows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["a"] == "ent:order-service" && a["b"] == "ent:inventory-service")
            .expect("the dependency lifted onto the inventory service");
        assert_eq!(lifted["lifted"], true);
        let one = view_value(&s, "view:component/shop", 1).unwrap();
        assert_eq!(one["detail"], 1);
        assert_eq!(one["deeper"], false);
        let members = one["members"].as_array().unwrap();
        let stock = members
            .iter()
            .find(|m| m["id"] == "ent:stock-api")
            .expect("the stock api drawn one level down");
        assert_eq!(stock["parent"], "ent:inventory-service");
        assert_eq!(stock["detail"], 1);
        assert_eq!(stock["hidden"], false);
        assert!(members.iter().any(|m| m["id"] == "ent:order-item" && m["detail"] == 1));
        let arrows = one["arrows"].as_array().unwrap();
        let direct = arrows
            .iter()
            .find(|a| a["a"] == "ent:order-service" && a["b"] == "ent:stock-api")
            .expect("the dependency drawn on the stock api");
        assert_eq!(direct["lifted"], false);
        assert_eq!(direct["type"], "dependency");
        assert!(!arrows
            .iter()
            .any(|a| a["a"] == "ent:order-service" && a["b"] == "ent:inventory-service"));
        assert!(arrows
            .iter()
            .any(|a| a["a"] == "ent:shopping-cart" && a["b"] == "ent:order-item" && a["type"] == "composition"));
        // Asking for more than the tree holds applies what exists.
        let deep = view_value(&s, "view:component/shop", 5).unwrap();
        assert_eq!(deep["detail"], 1);
        assert_eq!(deep["members"].as_array().unwrap().len(), members.len());
    }

    // The tree: one root per scope addressed as `scope:<scope>` with the root's level
    // view, nodes nested by `parent` with their counts and level view ids, and a leaf
    // with no level view.
    #[test]
    fn tree_value_nests_scopes_nodes_and_level_view_ids() {
        let s = store();
        let t = tree_value(&s);
        let roots = t["roots"].as_array().expect("roots");
        let public = roots
            .iter()
            .find(|r| r["scope"] == "public")
            .expect("the public scope root");
        assert_eq!(public["target"], "scope:public");
        assert_eq!(public["levelView"], "view:component/public");
        assert_eq!(
            public["count"].as_u64().unwrap() as usize,
            public["children"].as_array().unwrap().len()
        );
        let shop = public["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == "ent:shop")
            .expect("the shop under the root");
        assert_eq!(shop["count"], 2);
        assert_eq!(shop["levelView"], "view:component/shop");
        assert_eq!(shop["grouping"], false);
        let kids = shop["children"].as_array().unwrap();
        let order = kids
            .iter()
            .find(|n| n["id"] == "ent:order-service")
            .expect("the order service under the shop");
        assert_eq!(order["levelView"], "view:component/order-service");
        assert_eq!(order["count"], 4);
        let inventory = kids
            .iter()
            .find(|n| n["id"] == "ent:inventory-service")
            .expect("the inventory service under the shop");
        assert_eq!(inventory["count"], 1);
        assert!(inventory["levelView"].is_null());
        let stock = &inventory["children"][0];
        assert_eq!(stock["id"], "ent:stock-api");
        assert_eq!(stock["count"], 0);
        assert!(stock["children"].as_array().unwrap().is_empty());
        assert!(stock["views"].as_array().unwrap().is_empty());
    }
}
