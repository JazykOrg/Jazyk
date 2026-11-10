use super::text::oneline;
use crate::derive::{instance_types, query_matches};
use crate::ids::entity_slug;
use crate::limits;
use crate::model::*;
use crate::store::Store;
use std::collections::{BTreeMap, BTreeSet};

// ---- the scene: a view resolved against the snapshot ----

pub(super) struct Scene<'a> {
    pub(super) store: &'a Store,
    pub(super) id: &'a str,
    pub(super) view: &'a View,
    // Entity members, in member order, query matches joined, exclusions removed.
    pub(super) entities: Vec<String>,
    // Requirement members, in member order.
    pub(super) requirements: Vec<String>,
}

pub(super) fn scene<'a>(store: &'a Store, id: &'a str, view: &'a View) -> Scene<'a> {
    let instances = instance_types(store);
    let mut ids: Vec<String> = Vec::new();
    for m in &view.members {
        let r = store.resolve_id(m).to_string();
        if !ids.contains(&r) {
            ids.push(r);
        }
    }
    if let Some(q) = view.query.as_ref() {
        for m in query_matches(store, q, &view.excluded, &instances) {
            if !ids.contains(&m) {
                ids.push(m);
            }
        }
    }
    let excluded: BTreeSet<String> = view
        .excluded
        .iter()
        .map(|x| store.resolve_id(&x.id).to_string())
        .collect();
    ids.retain(|m| !excluded.contains(m));
    let entities = ids
        .iter()
        .filter(|m| store.graph.entities.contains_key(*m))
        .cloned()
        .collect();
    let requirements = ids
        .iter()
        .filter(|m| store.graph.requirements.contains_key(*m))
        .cloned()
        .collect();
    Scene {
        store,
        id,
        view,
        entities,
        requirements,
    }
}

impl Scene<'_> {
    pub(super) fn entity(&self, id: &str) -> &Entity {
        &self.store.graph.entities[id]
    }

    pub(super) fn name(&self, id: &str) -> String {
        oneline(&self.entity(id).name)
    }

    pub(super) fn labeled(&self, id: &str, stereotype: &str) -> bool {
        self.entity(id)
            .stereotype
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case(stereotype))
    }

    pub(super) fn requirement(&self, id: &str) -> &Requirement {
        &self.store.graph.requirements[id]
    }

    // A flow member's label: its statement followed by its id.
    pub(super) fn flow_label(&self, rid: &str) -> String {
        format!("{} ({})", oneline(&self.requirement(rid).statement), rid)
    }

    pub(super) fn machine(&self, entity: &str) -> Option<&StateMachine> {
        self.store
            .graph
            .state_machines
            .get(&format!("sm:{}", entity_slug(entity)))
    }
}

// ---- containment ----

// The entity and its ancestors, nearest first. Bounded against a cycle.
fn ancestry(store: &Store, id: &str) -> Vec<String> {
    let mut chain = vec![id.to_string()];
    let mut cur = id.to_string();
    while let Some(p) = store
        .graph
        .entities
        .get(&cur)
        .and_then(|e| e.parent.as_deref())
    {
        let p = store.resolve_id(p).to_string();
        if chain.contains(&p) || chain.len() > 64 {
            break;
        }
        chain.push(p.clone());
        cur = p;
    }
    chain
}

pub(super) fn is_below(store: &Store, ancestor: &str, id: &str) -> bool {
    ancestry(store, id).iter().skip(1).any(|x| x == ancestor)
}

// The shown node standing for an entity: the topmost collapsed member on its chain,
// else the nearest member on its chain, else none (not drawn).
fn representative(
    store: &Store,
    members: &BTreeSet<String>,
    collapse: &BTreeSet<String>,
    id: &str,
) -> Option<String> {
    let chain = ancestry(store, id);
    if let Some(c) = chain
        .iter()
        .rev()
        .find(|x| collapse.contains(*x) && members.contains(*x))
    {
        return Some(c.clone());
    }
    chain.into_iter().find(|x| members.contains(x))
}

// ---- lifting and collapse ----

// One drawn arrow: the strongest type among the concrete groups beneath it, the
// cardinality when one group stands alone, and the count of groups.
pub(super) struct Arrow {
    pub(super) a: String,
    pub(super) b: String,
    pub(super) rel_type: String,
    pub(super) cardinality: Option<String>,
    pub(super) count: usize,
}

pub(super) struct Structure {
    // The drawn entities, in member order.
    pub(super) shown: Vec<String>,
    pub(super) arrows: Vec<Arrow>,
}

fn pair_key(a: &str, b: &str) -> String {
    let (x, y) = if a <= b { (a, b) } else { (b, a) };
    format!("{}~{}", entity_slug(x), entity_slug(y))
}

// The members minus the subtrees hidden by collapse, and every relationship among them,
// direct or lifted, one arrow per shown pair and direction.
// Mirrors docs/compiler/diagrams.md#lifting-and-collapse.
pub(super) fn structure(
    store: &Store,
    members: &[String],
    collapse: &BTreeSet<String>,
) -> Structure {
    let set: BTreeSet<String> = members.iter().cloned().collect();
    let rep = |id: &str| representative(store, &set, collapse, id);
    let shown: Vec<String> = members
        .iter()
        .filter(|m| rep(m).as_deref() == Some(m.as_str()))
        .cloned()
        .collect();
    let mut groups: BTreeMap<(String, String), Vec<&Contribution>> = BTreeMap::new();
    for rel in store.graph.relationships.values() {
        for c in &rel.contributions {
            if c.r#type == INSTANTIATION {
                continue;
            }
            let (Some(a), Some(b)) = (rep(&c.a), rep(&c.b)) else {
                continue;
            };
            if a == b {
                continue;
            }
            // An association draws without an arrowhead, so its two directions are one
            // line. Mirrors docs/compiler/diagrams.md#the-emitters.
            let key = if c.r#type == "association" && b < a {
                (b, a)
            } else {
                (a, b)
            };
            groups.entry(key).or_default().push(c);
        }
    }
    let mut arrows: Vec<Arrow> = groups
        .into_iter()
        .map(|((a, b), cs)| {
            let rel_type = cs
                .iter()
                .min_by_key(|c| rel_rank(&c.r#type))
                .map(|c| c.r#type.clone())
                .unwrap_or_else(|| DEFAULT_REL_TYPE.to_string());
            // A cardinality belongs to the pair the sentence counted: a lifted arrow
            // speaks for a hidden pair and drops it.
            // Mirrors docs/compiler/diagrams.md#lifting-and-collapse.
            // A lone association folded onto the sorted key keeps its own direction,
            // so the cardinality stays at its `b` end.
            let (a, b) = if cs.len() == 1 && cs[0].a == b && cs[0].b == a {
                (b, a)
            } else {
                (a, b)
            };
            let cardinality = if cs.len() == 1 && cs[0].a == a && cs[0].b == b {
                cs[0].cardinality.clone()
            } else {
                None
            };
            Arrow {
                a,
                b,
                rel_type,
                cardinality,
                count: cs.len(),
            }
        })
        .collect();
    arrows.sort_by(|x, y| {
        (pair_key(&x.a, &x.b), &x.a, &x.b).cmp(&(pair_key(&y.a, &y.b), &y.a, &y.b))
    });
    Structure { shown, arrows }
}

// ---- over-limit views ----

fn limit_pair(view: &View, limit: &str) -> (u64, u64) {
    limits::threshold(limit, view.limits.get(limit).map(|b| b.value))
        .unwrap_or((u64::MAX, u64::MAX))
}

fn structural_counts(kind: &str, st: &Structure) -> Vec<(&'static str, u64)> {
    let members = if kind == "object" {
        "instances-per-object-view"
    } else {
        "members-per-structural-view"
    };
    vec![
        (members, st.shown.len() as u64),
        ("edges-per-view", st.arrows.len() as u64),
    ]
}

pub(super) struct Frame {
    pub(super) st: Structure,
    pub(super) note: Option<String>,
}

// The structure of a view with its authored collapse, then auto-collapse of the largest
// subtrees while a count is past its hard threshold, until every count is within the
// soft one or nothing is left to collapse. Mirrors docs/compiler/diagrams.md#over-limit-views.
pub(super) fn frame(scene: &Scene) -> Frame {
    let store = scene.store;
    let view = scene.view;
    let kind = view.kind.as_str();
    let mut collapse: BTreeSet<String> = view
        .collapse
        .iter()
        .map(|c| store.resolve_id(c).to_string())
        .collect();
    let mut st = structure(store, &scene.entities, &collapse);
    let over_hard = |st: &Structure| {
        structural_counts(kind, st)
            .into_iter()
            .find(|(l, n)| *n > limit_pair(view, l).1)
    };
    let Some(first) = over_hard(&st) else {
        return Frame { st, note: None };
    };
    let mut auto = 0usize;
    loop {
        if structural_counts(kind, &st)
            .iter()
            .all(|(l, n)| *n <= limit_pair(view, l).0)
        {
            break;
        }
        let mut best: Option<(String, usize)> = None;
        for s in &st.shown {
            if collapse.contains(s) {
                continue;
            }
            let below = st
                .shown
                .iter()
                .filter(|o| *o != s && is_below(store, s, o))
                .count();
            if below > 0 && best.as_ref().is_none_or(|(_, n)| below > *n) {
                best = Some((s.clone(), below));
            }
        }
        let Some((node, _)) = best else {
            break;
        };
        collapse.insert(node);
        auto += 1;
        st = structure(store, &scene.entities, &collapse);
    }
    let note = if auto > 0 {
        format!("(collapsed: {} subtrees over limit)", auto)
    } else {
        let (limit, n) = first;
        let what = if limit == "edges-per-view" {
            "edges"
        } else {
            "members"
        };
        format!("(over limit: {} {})", n, what)
    };
    Frame {
        st,
        note: Some(note),
    }
}

// A flow view past its hard member or participant threshold renders every member,
// marked.
pub(super) fn flow_note(scene: &Scene, participants: Option<usize>) -> Option<String> {
    let n = scene.requirements.len() as u64;
    if n > limit_pair(scene.view, "members-per-flow-view").1 {
        return Some(format!("(over limit: {} members)", n));
    }
    if let Some(p) = participants {
        if p as u64 > limit_pair(scene.view, "participants-per-sequence-view").1 {
            return Some(format!("(over limit: {} participants)", p));
        }
    }
    None
}

pub(super) fn title_line(view: &View, note: Option<&str>) -> Option<String> {
    note.map(|n| format!("{} {}", oneline(&view.title), n))
}

// The relationships among a view's members as the rendering draws them: one arrow per
// shown pair and direction, lifted to the nearest shown ancestor and collapsed as the
// view (and its over-limit auto-collapse) says. (a, b, type, cardinality, count).
// Mirrors docs/compiler/tools.md#read-tools.
pub fn view_relationships(
    store: &Store,
    view_id: &str,
    view: &View,
) -> Vec<(String, String, String, Option<String>, usize)> {
    let sc = scene(store, view_id, view);
    frame(&sc)
        .st
        .arrows
        .into_iter()
        .map(|a| (a.a, a.b, a.rel_type, a.cardinality, a.count))
        .collect()
}
