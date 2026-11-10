use super::*;

// ---- default views ----

#[derive(Clone, Debug, PartialEq)]
pub(super) struct DefaultView {
    pub(super) id: String,
    pub(super) kind: &'static str,
    pub(super) title: String,
    pub(super) members: Vec<String>,
    pub(super) query: Option<ViewQuery>,
    pub(super) from: Vec<String>,
    pub(super) rule: &'static str,
}

// Each word's first letter upper-cased: the scope's name as a title.
pub(super) fn title_case(s: &str) -> String {
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
pub(super) fn doc_title(store: &Store, doc: &str) -> String {
    store
        .docs
        .get(doc)
        .and_then(|d| d.sections.values().find(|s| s.kind == "root"))
        .map(|s| s.title.clone())
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| doc_stem(doc))
}

pub(super) fn doc_stem(doc: &str) -> String {
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
        .filter(|(_, e)| q.scope.as_deref().is_none_or(|s| e.scope == s))
        .filter(|(_, e)| {
            q.stereotype.as_deref().is_none_or(|s| {
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

// The entities a view draws, as the card model reads them.
pub fn drawn_entities_of(store: &Store, view_id: &str) -> Vec<String> {
    let Some(v) = store.graph.views.get(view_id) else {
        return Vec::new();
    };
    let ix = Indices::build(store);
    let flow_levels = flow_view_level_index(store, &ix);
    drawn_entities(store, &ix, &flow_levels, view_id, v)
}

// The entities a view draws: the entity members of a structural, object, or state
// view; the participants of a flow view (each member's message endpoints, lifted to the
// view's level when it has one), in first-appearance order.
pub(super) fn drawn_entities(
    store: &Store,
    ix: &Indices,
    flow_levels: &BTreeMap<String, String>,
    view_id: &str,
    v: &View,
) -> Vec<String> {
    let members = live_members(store, v);
    if !FLOW_KINDS.contains(&v.kind.as_str()) {
        return members
            .into_iter()
            .filter(|m| store.graph.entities.contains_key(m))
            .collect();
    }
    let level = flow_levels
        .get(view_id)
        .map(|t| level_view_members_with(store, ix, t));
    let mut out: Vec<String> = Vec::new();
    for m in &members {
        for p in flow_participants(store, std::slice::from_ref(m)) {
            // An endpoint with no ancestor among the members draws as itself, as
            // the emitter draws it. Mirrors docs/compiler/diagrams.md#level-views.
            let d = match level.as_deref() {
                Some(level) => lift_into(store, level, &p).unwrap_or(p),
                None => p,
            };
            if !out.contains(&d) {
                out.push(d);
            }
        }
    }
    out
}

// The member views one level down: for every entity the view draws that has a level
// view, `(member, view)` in member order. Computed at read time, never stored.
// Mirrors docs/compiler/model/view.md#fields.
pub fn children_of_view(store: &Store, view_id: &str) -> Vec<(String, String)> {
    let Some(v) = store.graph.views.get(view_id) else {
        return Vec::new();
    };
    let ix = Indices::build(store);
    let flow_levels = flow_view_level_index(store, &ix);
    drawn_entities(store, &ix, &flow_levels, view_id, v)
        .into_iter()
        .filter_map(|m| level_view_id_with(store, &ix, &m).map(|id| (m, id)))
        .collect()
}

// The rule instances that hold on the current graph.
pub(super) fn default_view_rules(store: &Store, ix: &Indices) -> Vec<DefaultView> {
    let instances = instance_types(store);
    let mut out = Vec::new();

    // One structural level view per level of two or more, the scope root included, and
    // the flow views of the requirements lifted into it.
    let lv = ix.levels(store);
    for level in lv.held.iter().map(|i| &lv.levels[*i]) {
        let members = lv.members[&level.target].clone();
        let mut from: Vec<String> = level.node.iter().cloned().collect();
        from.extend(members.iter().cloned());
        out.push(DefaultView {
            id: level_id(store, ix, level),
            kind: level_kind(store, ix, level),
            title: level_title(store, level),
            members: members.clone(),
            query: None,
            from,
            rule: if level.node.is_some() {
                "level view of a node"
            } else {
                "level view of the scope root"
            },
        });
    }
    out.extend(level_flow_views(store, ix).iter().map(|(_, w)| w.clone()));

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
    let ix = Indices::build(store);
    recompute_default_views_with(store, &ix, build, batch);
}

pub(super) fn recompute_default_views_with(
    store: &mut Store,
    ix: &Indices,
    build: &str,
    batch: &mut RecordBatch,
) {
    let wanted = default_view_rules(store, ix);
    let mut wanted_ids: BTreeSet<String> = BTreeSet::new();
    for w in wanted {
        wanted_ids.insert(w.id.clone());
        let provenance = Provenance::Derived {
            from: w.from,
            reasoning: format!("default view: {}", w.rule),
        };
        match store.graph.views.get_mut(&w.id) {
            Some(v) if v.default => {
                // The view's exclusions survive the rewrite and keep their nodes out.
                let members: Vec<String> = w
                    .members
                    .into_iter()
                    .filter(|m| !v.excluded.iter().any(|x| x.id == *m))
                    .collect();
                let changed = v.title != w.title
                    || v.members != members
                    || v.query != w.query
                    || v.provenance.as_ref() != Some(&provenance);
                if changed {
                    v.title = w.title;
                    v.members = members;
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
