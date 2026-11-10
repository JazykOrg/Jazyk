use super::*;

// ---- levels ----

// The stereotypes that make a level's view a component view.
// Mirrors docs/compiler/model/view.md#level-views.
const STRUCTURAL_STEREOTYPES: [&str; 5] = ["system", "component", "service", "interface", "actor"];

// One level: a node's direct children, or the parentless entities of a scope for the
// root form (`node` is None, `target` is `scope:<scope>`). Children are in document
// order. Mirrors docs/compiler/concepts/levels.md#levels.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Level {
    pub(super) target: String,
    pub(super) node: Option<String>,
    pub(super) scope: String,
    children: Vec<String>,
}

// Entities ranked in document order: the earliest position among an entity's mentions
// and the sources of the requirements naming it (document link level, then path, then
// section line), entities no document places last, ties by id.
// Mirrors docs/compiler/model/view.md#level-views.
pub(super) fn entity_document_rank_with(
    store: &Store,
    levels: &BTreeMap<String, usize>,
) -> BTreeMap<String, usize> {
    let position = |s: &SourceRef| -> (usize, String, usize) {
        let line = store
            .docs
            .get(&s.doc)
            .and_then(|d| d.sections.get(&s.section))
            .map(|sec| sec.lines[0])
            .unwrap_or(usize::MAX);
        let level = levels.get(&s.doc).copied().unwrap_or(usize::MAX);
        (level, s.doc.clone(), line)
    };
    let last = (usize::MAX, "~".to_string(), usize::MAX);
    let mut best: BTreeMap<String, (usize, String, usize)> = BTreeMap::new();
    for (id, e) in &store.graph.entities {
        let mut key = last.clone();
        for m in &e.mentions {
            key = key.min(position(m));
        }
        best.insert(id.clone(), key);
    }
    for r in store.graph.requirements.values() {
        let Some(src) = r.source.as_ref() else {
            continue;
        };
        let pos = position(src);
        for e in &r.entities {
            if let Some(key) = best.get_mut(store.resolve_id(e)) {
                if pos < *key {
                    *key = pos.clone();
                }
            }
        }
    }
    let mut keyed: Vec<((usize, String, usize), String)> =
        best.into_iter().map(|(id, key)| (key, id)).collect();
    keyed.sort();
    keyed
        .into_iter()
        .enumerate()
        .map(|(i, (_, id))| (id, i))
        .collect()
}

fn in_document_order(ids: &mut [String], rank: &BTreeMap<String, usize>) {
    ids.sort_by_key(|id| (rank.get(id).copied().unwrap_or(usize::MAX), id.clone()));
}

// Every level of the store with at least one child: the scope roots and every node
// that is a parent. Children in document order.
pub(super) fn levels(store: &Store, ix: &Indices) -> Vec<Level> {
    let rank = &ix.entity_rank;
    let mut by_parent: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut scopes: BTreeSet<&str> = BTreeSet::new();
    for (id, e) in &store.graph.entities {
        scopes.insert(&e.scope);
        let key = match &e.parent {
            Some(p) => p.clone(),
            None => scope_root_target(&e.scope),
        };
        by_parent.entry(key).or_default().push(id.clone());
    }
    let mut out = Vec::new();
    for scope in scopes {
        let target = scope_root_target(scope);
        let mut children = by_parent.remove(&target).unwrap_or_default();
        in_document_order(&mut children, rank);
        out.push(Level {
            target,
            node: None,
            scope: scope.to_string(),
            children,
        });
    }
    for (node, mut children) in by_parent {
        let Some(e) = store.graph.entities.get(&node) else {
            continue;
        };
        in_document_order(&mut children, rank);
        out.push(Level {
            target: node.clone(),
            node: Some(node),
            scope: e.scope.clone(),
            children,
        });
    }
    out
}

// Whether a level derives a view: two or more children, or one child that carries
// attributes, since the class box is the only place an attribute is drawn.
// Mirrors docs/compiler/model/view.md#level-views.
pub(super) fn level_holds(store: &Store, level: &Level) -> bool {
    level.children.len() >= 2
        || (level.children.len() == 1
            && store
                .graph
                .entities
                .get(&level.children[0])
                .is_some_and(|e| !e.attributes.is_empty()))
}

// The level a target names, when it derives a view: a node id (redirects followed)
// or `scope:<scope>`. None for a leaf, a one-child node whose child carries no
// attributes, or an unknown id.
fn level_of_target<'a>(store: &Store, target: &str, ix: &'a Indices) -> Option<&'a Level> {
    let target = match crate::board::scope_target(target) {
        Some(_) => target.to_string(),
        None => store.resolve_id(target).to_string(),
    };
    let lv = ix.levels(store);
    let i = *lv.by_target.get(&target)?;
    lv.held.contains(&i).then(|| &lv.levels[i])
}

// The kind rule: component when the node, any child, or any descendant below a
// child carries a structural stereotype (a grouping of components is a component
// level), class otherwise. Mirrors docs/compiler/model/view.md#level-views.
pub(super) fn level_kind(store: &Store, ix: &Indices, level: &Level) -> &'static str {
    fn structural(store: &Store, id: &str) -> bool {
        store
            .graph
            .entities
            .get(id)
            .and_then(|e| e.stereotype.as_deref())
            .is_some_and(|s| {
                STRUCTURAL_STEREOTYPES
                    .iter()
                    .any(|x| s.eq_ignore_ascii_case(x))
            })
    }
    fn structural_below(store: &Store, ix: &Indices, id: &str, depth: usize) -> bool {
        if depth > 64 {
            return false;
        }
        ix.children_of(id)
            .iter()
            .any(|c| structural(store, c) || structural_below(store, ix, c, depth + 1))
    }
    let any = level.node.as_deref().is_some_and(|n| structural(store, n))
        || level
            .children
            .iter()
            .any(|c| structural(store, c) || structural_below(store, ix, c, 0));
    if any {
        "component"
    } else {
        "class"
    }
}

// The slug segment of a level view id: the node's slug, the scope's for the root form.
fn level_slug(level: &Level) -> String {
    match level.node.as_deref() {
        Some(node) => entity_slug(node).to_string(),
        None => md::slug(&level.scope),
    }
}

// The title of a level view: the node's name, the scope's name title-cased for the root.
pub(super) fn level_title(store: &Store, level: &Level) -> String {
    match level.node.as_deref() {
        Some(node) => store.graph.entities[node].name.clone(),
        None => title_case(&level.scope),
    }
}

// The nearest ancestor of `id` (itself included) among `members`: the renderer's
// lifting applied at derivation. None when no ancestor is a member.
// Mirrors docs/compiler/diagrams.md#lifting-and-collapse.
pub fn lift_into(store: &Store, members: &[String], id: &str) -> Option<String> {
    let mut cur = store.resolve_id(id).to_string();
    let mut depth = 0;
    loop {
        if members.contains(&cur) {
            return Some(cur);
        }
        let parent = store
            .graph
            .entities
            .get(&cur)
            .and_then(|e| e.parent.clone())?;
        cur = parent;
        depth += 1;
        if depth > 64 {
            return None;
        }
    }
}

// The ancestor chain `lift_into` walks for an id: the id (redirects followed), then
// each parent in turn, bounded as the walk is bounded. `lift_into` answers the first
// element among the members.
fn chain_of(store: &Store, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = store.resolve_id(id).to_string();
    let mut depth = 0;
    loop {
        out.push(cur.clone());
        let Some(parent) = store
            .graph
            .entities
            .get(&cur)
            .and_then(|e| e.parent.clone())
        else {
            break;
        };
        cur = parent;
        depth += 1;
        if depth > 64 {
            break;
        }
    }
    out
}

// The frame of a level: the node and its ancestors, never peers of the level.
fn frame_of(store: &Store, level: &Level) -> Vec<String> {
    let mut frame: Vec<String> = Vec::new();
    let mut cur = level.node.clone();
    while let Some(id) = cur {
        if frame.contains(&id) || frame.len() > 64 {
            break;
        }
        cur = store.graph.entities.get(&id).and_then(|e| e.parent.clone());
        frame.push(id);
    }
    frame
}

// Every level of a derivation with its members and flow views, computed for all
// levels at once: a reference reaches the levels its ancestor chain passes through,
// so each contribution and each requirement is visited once per level it reaches
// instead of once per level. The level and flow view rules, the limit counts, and the
// level of every view read it. Built on first use, never stored.
// Mirrors docs/compiler/graph.md#derived-data.
pub(super) struct Levels {
    // Every level, in `levels` order; the held ones (the levels that derive a view)
    // by index; each target's index.
    pub(super) levels: Vec<Level>,
    pub(super) held: Vec<usize>,
    by_target: BTreeMap<String, usize>,
    // The members of every held level, by target.
    pub(super) members: BTreeMap<String, Vec<String>>,
    // Every level's flow views with the level each belongs to.
    pub(super) flow_views: Vec<(String, DefaultView)>,
}

impl Levels {
    pub(super) fn build(store: &Store, ix: &Indices) -> Levels {
        let levels = levels(store, ix);
        let by_target: BTreeMap<String, usize> = levels
            .iter()
            .enumerate()
            .map(|(i, l)| (l.target.clone(), i))
            .collect();
        let held: Vec<usize> = (0..levels.len())
            .filter(|i| level_holds(store, &levels[*i]))
            .collect();
        let is_held: BTreeSet<usize> = held.iter().copied().collect();
        // The ancestor chain of every entity, and the held levels it reaches: the
        // level each chain element is a child of.
        let mut chains: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut reach: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for id in store.graph.entities.keys() {
            let chain = chain_of(store, id);
            let mut reached: Vec<usize> = Vec::new();
            for c in &chain {
                let Some(e) = store.graph.entities.get(c) else {
                    continue;
                };
                let target = match &e.parent {
                    Some(p) => p.clone(),
                    None => scope_root_target(&e.scope),
                };
                if let Some(i) = by_target.get(&target) {
                    if is_held.contains(i) && !reached.contains(i) {
                        reached.push(*i);
                    }
                }
            }
            chains.insert(id.clone(), chain);
            reach.insert(id.clone(), reached);
        }
        let chain = |id: &str| -> Vec<String> {
            let cur = store.resolve_id(id);
            chains
                .get(cur)
                .cloned()
                .unwrap_or_else(|| vec![cur.to_string()])
        };
        let reached = |id: &str| -> &[usize] {
            reach
                .get(store.resolve_id(id))
                .map(Vec::as_slice)
                .unwrap_or(&[])
        };

        // The members of every held level: the direct children, then every outside
        // entity with a lifted edge into the level (one end lifts to a child, the other
        // lifts to nothing and is not the node), each group in document order. An
        // instantiation contribution does not count: instances live in object views,
        // so the reference graph's shop level lists the customer and not Ana's cart.
        // Mirrors docs/compiler/model/view.md#level-views.
        let frames: Vec<Vec<String>> = levels.iter().map(|l| frame_of(store, l)).collect();
        let mut outside: Vec<Vec<String>> = vec![Vec::new(); levels.len()];
        for rel in store.graph.relationships.values() {
            for c in &rel.contributions {
                if c.r#type == INSTANTIATION {
                    continue;
                }
                for (x, y) in [(&c.a, &c.b), (&c.b, &c.a)] {
                    let y = store.resolve_id(y).to_string();
                    if !store.graph.entities.contains_key(&y) {
                        continue;
                    }
                    let reach_y = reached(&y);
                    for i in reached(x) {
                        if reach_y.contains(i)
                            || frames[*i].contains(&y)
                            || outside[*i].contains(&y)
                        {
                            continue;
                        }
                        outside[*i].push(y.clone());
                    }
                }
            }
        }
        let mut members: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut member_sets: Vec<BTreeSet<String>> = vec![BTreeSet::new(); levels.len()];
        for i in &held {
            let mut out = std::mem::take(&mut outside[*i]);
            in_document_order(&mut out, &ix.entity_rank);
            let mut all = levels[*i].children.clone();
            all.extend(out);
            member_sets[*i] = all.iter().cloned().collect();
            members.insert(levels[*i].target.clone(), all);
        }

        // The flow rule instances of every held level: the behavior and failure-mode
        // requirements whose entities lift to at least one member, clustered by
        // document (the author's own cut of the subject into flows; the actors are
        // read off the members at render time, never baked into the key).
        // Mirrors docs/compiler/model/view.md#level-views.
        let mut clusters: Vec<BTreeMap<String, Vec<String>>> = vec![BTreeMap::new(); levels.len()];
        for rid in &ix.document_order {
            let r = &store.graph.requirements[rid];
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
            // Reaching means lifting to a child: an interactor is only ever the other
            // end of a flow, never what makes a flow the level's own.
            let mut reaching: Vec<usize> = Vec::new();
            for e in &r.entities {
                for i in reached(e) {
                    if !reaching.contains(i) {
                        reaching.push(*i);
                    }
                }
            }
            for i in reaching {
                // A flow whose every entity lifts to one child with a level of its own
                // is that child's, never a self-message here; a self-message on a leaf
                // stays.
                let lifts: Vec<Option<String>> = r
                    .entities
                    .iter()
                    .map(|e| chain(e).into_iter().find(|c| member_sets[i].contains(c)))
                    .collect();
                let lifted: BTreeSet<String> = lifts.iter().flatten().cloned().collect();
                if lifted.len() == 1 && lifts.iter().all(Option::is_some) {
                    let only = lifted.iter().next().unwrap();
                    if levels[i].children.contains(only) && ix.has_children(only) {
                        continue;
                    }
                }
                clusters[i]
                    .entry(doc.clone())
                    .or_default()
                    .push(rid.clone());
            }
        }
        let mut flow_views = Vec::new();
        for i in &held {
            let level = &levels[*i];
            let level_members = &members[&level.target];
            for w in flow_views_of_clusters(store, level, std::mem::take(&mut clusters[*i])) {
                if flow_view_all_groupings(store, level_members, &w) {
                    continue;
                }
                flow_views.push((level.target.clone(), w));
            }
        }
        Levels {
            levels,
            held,
            by_target,
            members,
            flow_views,
        }
    }
}

// Whether a flow view's participants all lift to groupings: boxes no document states
// messaging each other, so the view is dropped.
// Mirrors docs/compiler/model/view.md#level-views.
fn flow_view_all_groupings(store: &Store, members: &[String], w: &DefaultView) -> bool {
    let drawn: BTreeSet<String> = flow_participants(store, &w.members)
        .into_iter()
        .map(|p| lift_into(store, members, &p).unwrap_or(p))
        .collect();
    !drawn.is_empty() && drawn.iter().all(|p| store.is_grouping(p))
}

// The members of one level by the scan over every contribution: the reference the
// level index is checked against.
#[cfg(test)]
pub(super) fn level_members_of_scan(store: &Store, level: &Level, ix: &Indices) -> Vec<String> {
    let rank = &ix.entity_rank;
    let children = &level.children;
    // The frame: the node and its ancestors are never peers of the level.
    let mut frame: Vec<String> = Vec::new();
    let mut cur = level.node.clone();
    while let Some(id) = cur {
        if frame.contains(&id) || frame.len() > 64 {
            break;
        }
        cur = store.graph.entities.get(&id).and_then(|e| e.parent.clone());
        frame.push(id);
    }
    let mut outside: Vec<String> = Vec::new();
    for rel in store.graph.relationships.values() {
        for c in &rel.contributions {
            if c.r#type == INSTANTIATION {
                continue;
            }
            for (x, y) in [(&c.a, &c.b), (&c.b, &c.a)] {
                let y = store.resolve_id(y).to_string();
                if lift_into(store, children, x).is_some()
                    && lift_into(store, children, &y).is_none()
                    && !frame.contains(&y)
                    && store.graph.entities.contains_key(&y)
                    && !outside.contains(&y)
                {
                    outside.push(y);
                }
            }
        }
    }
    in_document_order(&mut outside, rank);
    let mut members = children.clone();
    members.extend(outside);
    members
}

// The members of a target's level view: the direct children plus the outside entities
// with a lifted edge into the level, in document order. Empty for a target that has
// no level view. Mirrors docs/compiler/model/view.md#level-views.
pub fn level_view_members(store: &Store, target: &str) -> Vec<String> {
    level_view_members_with(store, &Indices::build(store), target)
}

pub fn level_view_members_with(store: &Store, ix: &Indices, target: &str) -> Vec<String> {
    match level_of_target(store, target, ix) {
        Some(level) => ix.levels(store).members[&level.target].clone(),
        None => Vec::new(),
    }
}

// The structural level view a target has: `view:<kind>/<node-slug>` for a node with
// two or more children, `view:<kind>/<scope>` for a scope root with two or more
// parentless entities, the kind by the kind rule. None otherwise.
// Mirrors docs/compiler/model/view.md#level-views.
pub fn level_view_id(store: &Store, target: &str) -> Option<String> {
    level_view_id_with(store, &Indices::build(store), target)
}

pub fn level_view_id_with(store: &Store, ix: &Indices, target: &str) -> Option<String> {
    let level = level_of_target(store, target, ix)?;
    Some(level_id(store, ix, level))
}

// The id of a level's structural view: the kind by the kind rule, then the slug.
pub(super) fn level_id(store: &Store, ix: &Indices, level: &Level) -> String {
    format!(
        "view:{}/{}",
        level_kind(store, ix, level),
        level_slug(level)
    )
}

// The level every default level view and lifted flow view belongs to: a lifted flow
// view's level first, then the target whose structural level view the id is (an
// entity, or `scope:<scope>` for a top level; a node ahead of a scope root when two
// targets slug alike). A curated, object, or state view is absent.
// Mirrors docs/compiler/reconciler.md#gc-gating.
pub fn view_levels(store: &Store, ix: &Indices) -> BTreeMap<String, String> {
    let mut out = flow_view_level_index(store, ix);
    let lv = ix.levels(store);
    let held: Vec<&Level> = lv.held.iter().map(|i| &lv.levels[*i]).collect();
    for l in held.iter().filter(|l| l.node.is_some()) {
        out.entry(level_id(store, ix, l))
            .or_insert_with(|| l.target.clone());
    }
    for l in held.iter().filter(|l| l.node.is_none()) {
        out.entry(level_id(store, ix, l))
            .or_insert_with(|| l.target.clone());
    }
    out
}

// The flow rule instances of one level by the scan over every requirement: the
// reference the level index is checked against.
#[cfg(test)]
fn flow_view_rules_scan(
    store: &Store,
    ix: &Indices,
    level: &Level,
    members: &[String],
) -> Vec<DefaultView> {
    let mut clusters: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for rid in &ix.document_order {
        let r = &store.graph.requirements[rid];
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
        // Reaching means lifting to a child: an interactor is only ever the other
        // end of a flow, never what makes a flow the level's own.
        if !r
            .entities
            .iter()
            .any(|e| lift_into(store, &level.children, e).is_some())
        {
            continue;
        }
        // A flow whose every entity lifts to one child with a level of its own is
        // that child's, never a self-message here; a self-message on a leaf stays.
        let lifts: Vec<Option<String>> = r
            .entities
            .iter()
            .map(|e| lift_into(store, members, e))
            .collect();
        let lifted: BTreeSet<String> = lifts.iter().flatten().cloned().collect();
        if lifted.len() == 1 && lifts.iter().all(Option::is_some) {
            let only = lifted.iter().next().unwrap();
            if level.children.contains(only) && ix.has_children(only) {
                continue;
            }
        }
        clusters.entry(doc).or_default().push(rid.clone());
    }
    flow_views_of_clusters(store, level, clusters)
}

// The flow views of one level's clusters: a cluster of two or more derives a use-case
// view and, when two or more of its members carry an edge, a sequence view. The root
// form keeps the unprefixed ids; a node prefixes its slug.
// Mirrors docs/compiler/model/view.md#level-views.
fn flow_views_of_clusters(
    store: &Store,
    level: &Level,
    clusters: BTreeMap<String, Vec<String>>,
) -> Vec<DefaultView> {
    let mut out = Vec::new();
    let prefix = match level.node.as_deref() {
        Some(node) => format!("{}-", entity_slug(node)),
        None => String::new(),
    };
    for (doc, cluster) in clusters {
        if cluster.len() < 2 {
            continue;
        }
        let slug = format!("{}{}", prefix, md::slug(&doc_stem(&doc)));
        // The document titles a root flow; a node level's flow carries the node too,
        // so the two levels' pictures of one document read apart.
        let title = match level.node.as_deref() {
            Some(node) => {
                let name = &store.graph.entities[node].name;
                let dt = doc_title(store, &doc);
                if dt.trim().eq_ignore_ascii_case(name.trim()) {
                    format!("Inside {}", name)
                } else {
                    format!("{}: {}", name, dt)
                }
            }
            None => doc_title(store, &doc),
        };
        let mut from: Vec<String> = level.node.iter().cloned().collect();
        let with_edges: Vec<String> = cluster
            .iter()
            .filter(|m| !store.graph.requirements[*m].edges.is_empty())
            .cloned()
            .collect();
        let mut uc_from = from.clone();
        uc_from.extend(cluster.iter().cloned());
        out.push(DefaultView {
            id: format!("view:usecase/{}", slug),
            kind: "use-case",
            title: title.clone(),
            members: cluster,
            query: None,
            from: uc_from,
            rule: "use-case per flow cluster of a level",
        });
        if with_edges.len() >= 2 {
            from.extend(with_edges.iter().cloned());
            out.push(DefaultView {
                id: format!("view:sequence/{}", slug),
                kind: "sequence",
                title,
                members: with_edges,
                query: None,
                from,
                rule: "sequence per flow cluster of a level",
            });
        }
    }
    out
}

// Every level's flow views with the level each belongs to. A view whose participants
// all lift to groupings is dropped: boxes no document states messaging each other.
// Mirrors docs/compiler/model/view.md#level-views.
pub(super) fn level_flow_views<'a>(store: &Store, ix: &'a Indices) -> &'a [(String, DefaultView)] {
    &ix.levels(store).flow_views
}

// Every level's flow views by the scans over every level: the reference the level
// index is checked against.
#[cfg(test)]
pub(super) fn level_flow_views_scan(store: &Store, ix: &Indices) -> Vec<(String, DefaultView)> {
    let mut out = Vec::new();
    for l in levels(store, ix)
        .into_iter()
        .filter(|l| level_holds(store, l))
    {
        let members = level_members_of_scan(store, &l, ix);
        for w in flow_view_rules_scan(store, ix, &l, &members) {
            if flow_view_all_groupings(store, &members, &w) {
                continue;
            }
            out.push((l.target.clone(), w));
        }
    }
    out
}

// The level a default flow view was derived for: the node id, or `scope:<scope>` for
// the root form. None for a flow view no level's rule names (a curated flow).
pub fn flow_view_level(store: &Store, view_id: &str) -> Option<String> {
    let ix = Indices::build(store);
    flow_view_level_index(store, &ix).remove(view_id)
}

// The level of every lifted flow view, the first rule instance naming an id winning,
// as `flow_view_level` reads it. Built once for a walk over every view.
pub(super) fn flow_view_level_index(store: &Store, ix: &Indices) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (t, w) in level_flow_views(store, ix) {
        out.entry(w.id.clone()).or_insert_with(|| t.clone());
    }
    out
}

// Every lifted flow view with the level it belongs to, computed once for a walk over
// the whole project. Mirrors docs/consumers/docsgen.md#entity-cards.
pub fn flow_view_levels(store: &Store) -> BTreeMap<String, String> {
    let ix = Indices::build(store);
    level_flow_views(store, &ix)
        .iter()
        .map(|(t, w)| (w.id.clone(), t.clone()))
        .collect()
}
