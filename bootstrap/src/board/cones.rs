// Cones: the scope root target form, the upward and downward reference walks, and
// the referrer index. Mirrors docs/compiler/reconciler.md#cones.
use super::*;

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
    if let Some(ix) = crate::store::ScopeIndex::of(store) {
        return ix
            .scope_roots(store)
            .get(scope)
            .cloned()
            .unwrap_or_default();
    }
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
    if let Some(ix) = crate::store::ScopeIndex::of(store) {
        return ix.children(store).get(target).cloned().unwrap_or_default();
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
pub(super) fn referrers(store: &Store, refs: &Refs, id: &str) -> Vec<String> {
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
pub(super) fn referrers_scan(store: &Store, id: &str) -> Vec<String> {
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
