// The shared model of the walk: one card per entity and one page per diagram, each
// small, each listing one level in every direction. Docsgen renders it to markdown,
// the LSP hover renders it in short, the GUI serves it as JSON, so the three
// surfaces walk the same graph. Mirrors docs/consumers/docsgen.md#entity-cards and
// docs/consumers/docsgen.md#diagram-pages.
use crate::model::*;
use crate::store::Store;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Crumb {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Kin {
    pub id: String,
    pub name: String,
    #[serde(rename = "childCount")]
    pub child_count: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Relation {
    pub other: String,
    #[serde(rename = "type")]
    pub r#type: String,
    // `a` when the entity acts on the other, `b` when it is acted on, `both` when
    // contributions run each way.
    pub direction: String,
    pub count: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub stereotype: Option<String>,
    pub definition: String,
    // `quote`, `derived`, or `decree`.
    pub provenance: String,
    // From the scope root down to the entity itself, last.
    pub breadcrumb: Vec<Crumb>,
    // The structural level view of the parent's level: where the entity is used.
    pub context: Option<String>,
    // The entity's own level view when it has a level.
    pub inside: Option<String>,
    #[serde(rename = "insideFlows")]
    pub inside_flows: Vec<String>,
    pub relationships: Vec<Relation>,
    // The flow views at the parent's level the entity is drawn in.
    pub flows: Vec<String>,
    pub siblings: Vec<Kin>,
    pub children: Vec<Kin>,
    #[serde(rename = "requirementCount")]
    pub requirement_count: usize,
    pub proposal: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Drawn {
    pub id: String,
    pub name: String,
    pub stereotype: Option<String>,
    #[serde(rename = "levelView")]
    pub level_view: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Step {
    pub requirement: String,
    pub statement: String,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Around {
    #[serde(rename = "sameLevel")]
    pub same_level: Vec<String>,
    pub above: Option<String>,
    pub below: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct LevelOf {
    pub target: String,
    pub breadcrumb: Vec<Crumb>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ViewPage {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub level: Option<LevelOf>,
    pub drawn: Vec<Drawn>,
    pub steps: Vec<Step>,
    pub around: Around,
}

// What every card and page shares, computed once per store: which level each flow
// view belongs to. A walk over a whole project builds one of these, not one per page.
pub struct Walk {
    pub flow_levels: BTreeMap<String, String>,
    // Every structural level view with the level target it belongs to.
    pub level_views: BTreeMap<String, String>,
}

impl Walk {
    pub fn new(store: &Store) -> Walk {
        let mut targets: Vec<String> = store.graph.entities.keys().cloned().collect();
        let mut scopes: Vec<String> = store
            .graph
            .entities
            .values()
            .map(|e| scope_target(&e.scope))
            .collect();
        scopes.sort();
        scopes.dedup();
        targets.extend(scopes);
        let mut level_views = BTreeMap::new();
        for t in targets {
            if let Some(v) = crate::derive::level_view_id(store, &t) {
                level_views.insert(v, t);
            }
        }
        Walk {
            flow_levels: crate::derive::flow_view_levels(store),
            level_views,
        }
    }
}

fn scope_target(scope: &str) -> String {
    format!("{}{}", crate::board::SCOPE_TARGET_PREFIX, scope)
}

fn scope_name(scope: &str) -> String {
    let mut c = scope.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

// The chain from the scope root down to `id` itself, last. Redirects followed.
pub fn breadcrumb(store: &Store, id: &str) -> Vec<Crumb> {
    let mut chain: Vec<Crumb> = Vec::new();
    let mut cur = Some(store.resolve_id(id).to_string());
    let mut depth = 0;
    let mut scope = "public".to_string();
    while let Some(c) = cur {
        let Some(e) = store.graph.entities.get(&c) else {
            break;
        };
        scope = e.scope.clone();
        chain.push(Crumb {
            id: c.clone(),
            name: e.name.clone(),
        });
        cur = e.parent.as_ref().map(|p| store.resolve_id(p).to_string());
        depth += 1;
        if depth > 64 {
            break;
        }
    }
    chain.push(Crumb {
        id: scope_target(&scope),
        name: scope_name(&scope),
    });
    chain.reverse();
    chain
}

fn kin(store: &Store, id: &str) -> Kin {
    let e = &store.graph.entities[id];
    Kin {
        id: id.to_string(),
        name: e.name.clone(),
        child_count: crate::goals::level_members(store, id).len(),
    }
}

fn provenance_kind(e: &Entity) -> &'static str {
    match &e.provenance {
        Some(Provenance::Derived { .. }) => "derived",
        Some(Provenance::Decree { .. }) => "decree",
        _ => "quote",
    }
}

fn is_flow(kind: &str) -> bool {
    crate::derive::FLOW_KINDS.contains(&kind)
}

// The level target an entity's parent level has: the parent, or the scope root.
fn parent_target(store: &Store, e: &Entity) -> String {
    match &e.parent {
        Some(p) => store.resolve_id(p).to_string(),
        None => scope_target(&e.scope),
    }
}

pub fn entity_card(store: &Store, walk: &Walk, id: &str) -> Option<Card> {
    let id = store.resolve_id(id).to_string();
    let e = store.graph.entities.get(&id)?;
    let parent = parent_target(store, e);
    let context = crate::derive::level_view_id(store, &parent);
    let inside = crate::derive::level_view_id(store, &id);
    let mut inside_flows: Vec<String> = walk
        .flow_levels
        .iter()
        .filter(|(_, t)| **t == id)
        .map(|(v, _)| v.clone())
        .collect();
    inside_flows.sort();
    let mut flows: Vec<String> = walk
        .flow_levels
        .iter()
        .filter(|(_, t)| **t == parent)
        .filter(|(v, _)| crate::derive::drawn_entities_of(store, v).contains(&id))
        .map(|(v, _)| v.clone())
        .collect();
    flows.sort();
    // Lifted as the in-context diagram lifts its arrows: every relationship the
    // subtree carries, each end lifted to the parent level's member it sits under.
    let level_members = crate::derive::level_view_members(store, &parent);
    let mine = |x: &str| crate::derive::lift_into(store, std::slice::from_ref(&id), x).is_some();
    let lift = |x: &str| {
        crate::derive::lift_into(store, &level_members, x)
            .unwrap_or_else(|| store.resolve_id(x).to_string())
    };
    let mut grouped: BTreeMap<(String, String), (bool, bool, Vec<String>)> = BTreeMap::new();
    for r in store.graph.relationships.values() {
        for c in &r.contributions {
            if c.r#type == crate::derive::INSTANTIATION {
                continue;
            }
            let (a_mine, b_mine) = (mine(&c.a), mine(&c.b));
            if a_mine == b_mine {
                continue;
            }
            let other = lift(if a_mine { &c.b } else { &c.a });
            if other == id {
                continue;
            }
            let e = grouped
                .entry((other, c.r#type.clone()))
                .or_insert((false, false, Vec::new()));
            if a_mine {
                e.0 = true;
            } else {
                e.1 = true;
            }
            for rq in &c.requirements {
                if !e.2.contains(rq) {
                    e.2.push(rq.clone());
                }
            }
        }
    }
    let relationships: Vec<Relation> = grouped
        .into_iter()
        .map(|((other, t), (out, inn, reqs))| Relation {
            other,
            r#type: t,
            direction: match (out, inn) {
                (true, true) => "both",
                (true, false) => "a",
                _ => "b",
            }
            .to_string(),
            count: reqs.len(),
        })
        .collect();
    let siblings: Vec<Kin> = crate::goals::level_members(store, &parent)
        .iter()
        .filter(|s| **s != id)
        .map(|s| kin(store, s))
        .collect();
    let children: Vec<Kin> = crate::goals::level_members(store, &id)
        .iter()
        .map(|c| kin(store, c))
        .collect();
    let requirement_count = store
        .graph
        .requirements
        .values()
        .filter(|r| r.entities.iter().any(|x| store.resolve_id(x) == id))
        .count();
    let proposal = store
        .graph
        .diagnostics
        .iter()
        .find(|(_, d)| {
            d.rule == "ratification-pending"
                && d.lifecycle == "open"
                && d.subjects.iter().any(|s| store.resolve_id(s) == id)
        })
        .map(|(did, _)| did.clone());
    Some(Card {
        id: id.clone(),
        name: e.name.clone(),
        stereotype: e.stereotype.clone(),
        definition: e.definition.clone().unwrap_or_default(),
        provenance: provenance_kind(e).to_string(),
        breadcrumb: breadcrumb(store, &id),
        context,
        inside,
        inside_flows,
        relationships,
        flows,
        siblings,
        children,
        requirement_count,
        proposal,
    })
}

// The breadcrumb of a level target: the node's own chain, or the scope root alone.
fn level_breadcrumb(store: &Store, target: &str) -> Vec<Crumb> {
    match crate::board::scope_target(target) {
        Some(scope) => vec![Crumb {
            id: target.to_string(),
            name: scope_name(scope),
        }],
        None => breadcrumb(store, target),
    }
}

// The level a view belongs to: a level view's own target, a lifted flow view's
// level, none for a curated, object, or state view.
pub fn view_level(store: &Store, walk: &Walk, view_id: &str) -> Option<String> {
    let _ = store;
    walk.flow_levels
        .get(view_id)
        .or_else(|| walk.level_views.get(view_id))
        .cloned()
}

pub fn view_page(store: &Store, walk: &Walk, view_id: &str) -> Option<ViewPage> {
    let view_id = store.resolve_id(view_id).to_string();
    let v = store.graph.views.get(&view_id)?;
    let level_target = view_level(store, walk, &view_id);
    let level = level_target.as_ref().map(|t| LevelOf {
        target: t.clone(),
        breadcrumb: level_breadcrumb(store, t),
    });
    let drawn: Vec<Drawn> = crate::derive::drawn_entities_of(store, &view_id)
        .into_iter()
        .filter_map(|id| {
            let e = store.graph.entities.get(&id)?;
            Some(Drawn {
                level_view: crate::derive::level_view_id(store, &id),
                id,
                name: e.name.clone(),
                stereotype: e.stereotype.clone(),
            })
        })
        .collect();
    let steps: Vec<Step> = if is_flow(&v.kind) {
        crate::render::view_messages(store, &view_id, v)
            .into_iter()
            .map(|(rid, from, to)| Step {
                statement: store
                    .graph
                    .requirements
                    .get(&rid)
                    .map(|r| r.statement.clone())
                    .unwrap_or_default(),
                requirement: rid,
                from,
                to,
            })
            .collect()
    } else {
        Vec::new()
    };
    let same_level: Vec<String> = match &level_target {
        Some(t) => {
            let mut ids: Vec<String> = walk
                .flow_levels
                .iter()
                .filter(|(v2, lt)| *lt == t && **v2 != view_id)
                .map(|(v2, _)| v2.clone())
                .collect();
            if let Some(sv) = crate::derive::level_view_id(store, t) {
                if sv != view_id {
                    ids.push(sv);
                }
            }
            ids.sort();
            ids
        }
        None => Vec::new(),
    };
    let above = level_target.as_ref().and_then(|t| {
        let e = store.graph.entities.get(t)?;
        crate::derive::level_view_id(store, &parent_target(store, e))
    });
    let below: Vec<String> = drawn.iter().filter_map(|d| d.level_view.clone()).collect();
    Some(ViewPage {
        id: view_id,
        kind: v.kind.clone(),
        title: v.title.clone(),
        level,
        drawn,
        steps,
        around: Around {
            same_level,
            above,
            below,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive::tests::showcase_store;
    use crate::store::RecordBatch;

    fn shop() -> Store {
        let mut s = showcase_store();
        let mut batch = RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        s
    }

    // Mirrors docs/consumers/docsgen.md#entity-cards: one level in every direction.
    #[test]
    fn a_card_lists_one_level_in_every_direction() {
        let s = shop();
        let walk = Walk::new(&s);
        let c = entity_card(&s, &walk, "ent:order-service").unwrap();
        assert_eq!(c.name, "Order Service");
        assert_eq!(
            c.breadcrumb.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            vec!["scope:public", "ent:shop", "ent:order-service"]
        );
        assert_eq!(c.context.as_deref(), Some("view:component/shop"));
        assert_eq!(c.inside.as_deref(), Some("view:component/order-service"));
        assert!(c.children.iter().any(|k| k.id == "ent:order"), "{:?}", c.children);
        assert!(c.siblings.iter().any(|k| k.id == "ent:inventory-service"));
        assert!(
            c.relationships.iter().any(|r| r.other == "ent:stock-api" && r.r#type == "dependency"),
            "{:?}",
            c.relationships
        );
        assert!(c.flows.iter().any(|f| f == "view:usecase/shop-customer-shop"), "{:?}", c.flows);
        assert_eq!(c.provenance, "quote");
        // A node's card lifts its subtree's relationships to the parent level: the
        // shop, whose children carry every edge, is never `none`.
        let shop = entity_card(&s, &walk, "ent:shop").unwrap();
        assert!(
            shop.relationships.iter().any(|r| r.other == "ent:customer"),
            "{:?}",
            shop.relationships
        );
        // A leaf: no inside, no children, its context is its parent's level.
        let leaf = entity_card(&s, &walk, "ent:order-item").unwrap();
        assert_eq!(leaf.inside, None);
        assert!(leaf.children.is_empty());
        assert_eq!(leaf.context.as_deref(), Some("view:component/order-service"));
        assert_eq!(leaf.breadcrumb.last().unwrap().id, "ent:order-item");
        assert!(entity_card(&s, &walk, "ent:nobody").is_none());
    }

    // Mirrors docs/consumers/docsgen.md#diagram-pages: the level, the legend, the
    // steps as drawn, and the views around.
    #[test]
    fn a_view_page_carries_its_level_legend_steps_and_neighbors() {
        let s = shop();
        let walk = Walk::new(&s);
        let p = view_page(&s, &walk, "view:component/shop").unwrap();
        assert_eq!(p.level.as_ref().unwrap().target, "ent:shop");
        assert!(p.drawn.iter().any(|d| d.id == "ent:order-service"
            && d.level_view.as_deref() == Some("view:component/order-service")));
        assert!(p.steps.is_empty());
        assert!(p.around.below.contains(&"view:component/order-service".to_string()));
        assert!(p
            .around
            .same_level
            .contains(&"view:usecase/shop-customer-shop".to_string()));
        assert_eq!(p.around.above.as_deref(), Some("view:component/public"));
        let f = view_page(&s, &walk, "view:sequence/shop-customer-shop").unwrap();
        assert_eq!(f.level.as_ref().unwrap().target, "ent:shop");
        assert!(!f.steps.is_empty());
        assert_eq!(f.steps[0].requirement, "req:shop-1");
        assert!(f.around.same_level.contains(&"view:component/shop".to_string()));
    }
}
