use super::*;

// ---- grouping by component ----
// Where the graph carries containment, a component and its subtree generate as one
// group; the goal stays per entity, the group is derived at batch time, never
// stored. Mirrors docs/consumers/gen.md#grouping-by-component.

// A system: a containment root with at least one child (the match that derives the
// default component view), or an entity the documents stereotype as one.
pub fn is_system(store: &Store, id: &str) -> bool {
    let Some(e) = store.graph.entities.get(id) else {
        return false;
    };
    if e.stereotype
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("system"))
        .unwrap_or(false)
    {
        return true;
    }
    e.parent.is_none()
        && store
            .graph
            .entities
            .values()
            .any(|c| c.parent.as_deref() == Some(id))
}

// The group root an entity generates under: the direct child of a system, the
// «service»-like tier of the containment tree. A system generates alone, and so
// does a parentless entity without children.
pub fn group_root(store: &Store, id: &str) -> String {
    if is_system(store, id) {
        return id.to_string();
    }
    let mut cur = id.to_string();
    // Bounded walk: the store refuses parent cycles, the bound keeps this total.
    for _ in 0..64 {
        let Some(parent) = store
            .graph
            .entities
            .get(&cur)
            .and_then(|e| e.parent.clone())
        else {
            return cur;
        };
        if is_system(store, &parent) {
            return cur;
        }
        cur = parent;
    }
    cur
}

// The component groups of an ordered target list: one group per root, groups in
// first-member order, members keeping the leaf-first order they arrived in. A flat
// graph yields one group per entity.
// Mirrors docs/consumers/gen.md#grouping-by-component.
pub fn groups_in_order(store: &Store, ordered: &[String]) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for id in ordered {
        let root = group_root(store, id);
        match groups.iter_mut().find(|(r, _)| *r == root) {
            Some((_, members)) => members.push(id.clone()),
            None => groups.push((root, vec![id.clone()])),
        }
    }
    groups
}

// A group is its root plus every descendant through `parent`, in tree order.
pub fn group_members(store: &Store, root: &str) -> Vec<String> {
    let mut members = vec![root.to_string()];
    let mut i = 0;
    while i < members.len() {
        let cur = members[i].clone();
        for (cid, e) in &store.graph.entities {
            if e.parent.as_deref() == Some(cur.as_str()) && !members.iter().any(|m| m == cid) {
                members.push(cid.clone());
            }
        }
        i += 1;
    }
    members
}

// Leaf-first order over the per-direction contributions and the containment tree:
// every contribution's acted-on side first (the part, the dependency, the
// interface), and a child before its parent (the part before the whole). Ties by
// id; a cycle breaks at the entity with the fewest pending prerequisites.
// Mirrors docs/consumers/gen.md#order-from-relationships.
pub fn generation_order(store: &Store, targets: &[String]) -> Vec<String> {
    use std::collections::{BTreeMap, BTreeSet};
    let set: BTreeSet<&str> = targets.iter().map(|s| s.as_str()).collect();
    // Each target's pending prerequisites, and for each prerequisite the targets
    // waiting on it, once per contribution or child, so emitting one decrements the
    // count of every target it held back.
    let mut pending: BTreeMap<&str, usize> = set.iter().map(|id| (*id, 0)).collect();
    let mut waiting: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for rel in store.graph.relationships.values() {
        for c in &rel.contributions {
            if c.a != c.b && set.contains(c.a.as_str()) && set.contains(c.b.as_str()) {
                *pending.get_mut(c.a.as_str()).unwrap() += 1;
                waiting.entry(c.b.as_str()).or_default().push(c.a.as_str());
            }
        }
    }
    for (cid, e) in &store.graph.entities {
        let Some(parent) = e.parent.as_deref() else {
            continue;
        };
        if set.contains(parent) && set.contains(cid.as_str()) {
            *pending.get_mut(parent).unwrap() += 1;
            waiting.entry(cid.as_str()).or_default().push(parent);
        }
    }
    // The remaining targets by (pending, id, position): the first is the next one.
    let mut queue: BTreeSet<(usize, &str, usize)> = targets
        .iter()
        .enumerate()
        .map(|(i, id)| (pending[id.as_str()], id.as_str(), i))
        .collect();
    let mut emitted: BTreeSet<&str> = BTreeSet::new();
    let mut out = Vec::with_capacity(targets.len());
    while let Some((_, id, _)) = queue.pop_first() {
        out.push(id.to_string());
        if !emitted.insert(id) {
            continue;
        }
        for &held in waiting.get(id).map(Vec::as_slice).unwrap_or_default() {
            let n = pending[held];
            let positions: Vec<usize> = queue
                .range((n, held, 0)..=(n, held, usize::MAX))
                .map(|k| k.2)
                .collect();
            for i in positions {
                queue.remove(&(n, held, i));
                queue.insert((n - 1, held, i));
            }
            *pending.get_mut(held).unwrap() = n - 1;
        }
    }
    out
}
