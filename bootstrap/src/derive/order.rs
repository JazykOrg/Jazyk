use super::*;

// ---- document order ----

// Breadth-first document levels from the given root documents over the link graph in
// the stored sections; unreachable documents come after the reachable ones. With no
// roots, every document is its own level in path order. The board orders tier 1 with
// the same computation (board::doc_levels). Mirrors docs/compiler/reconciler.md#link-levels.
pub fn doc_levels_from(store: &Store, roots: &[String]) -> BTreeMap<String, usize> {
    doc_levels_in(&store.docs, roots)
}

// The same levels over a document map alone, for the store's own load and commit.
pub(crate) fn doc_levels_in(
    docs: &BTreeMap<String, crate::model::DocRecord>,
    roots: &[String],
) -> BTreeMap<String, usize> {
    let mut links: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (doc, rec) in docs {
        let mut targets: Vec<String> = Vec::new();
        for sec in rec.sections.values() {
            if sec.kind == "blob" {
                continue;
            }
            for l in md::doc_links(&sec.raw, doc) {
                if docs.contains_key(&l) && !targets.contains(&l) {
                    targets.push(l);
                }
            }
        }
        links.insert(doc.clone(), targets);
    }
    let roots: Vec<String> = roots
        .iter()
        .filter(|d| docs.contains_key(*d))
        .cloned()
        .collect();
    let mut level_of: BTreeMap<String, usize> = BTreeMap::new();
    if roots.is_empty() {
        for (i, d) in docs.keys().enumerate() {
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
    for d in docs.keys() {
        level_of.entry(d.clone()).or_insert(max + 1);
    }
    level_of
}

// Requirement ids in document order: document link level (from the roots the build
// stamped into the status), then path, then the source section's position, then id.
// Requirements with no quote source sort last, by id.
// Mirrors docs/compiler/model/view.md#default-views.
pub fn document_order(store: &Store) -> Vec<String> {
    document_order_with(store, &doc_levels_from(store, &store.status.roots))
}

// The document order over precomputed document levels.
fn document_order_with(store: &Store, levels: &BTreeMap<String, usize>) -> Vec<String> {
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

// ---- indices ----

// The indices one derivation reads, built once from the store at the start of a
// commit, a board derivation, or the checks, passed down, never stored: the document
// levels from the stamped roots, the requirement and entity document orders, parent to
// children, and entity to the requirements naming it. Each index holds what the scan it
// replaces computes. Mirrors docs/compiler/graph.md#derived-data.
pub struct Indices {
    pub doc_levels: BTreeMap<String, usize>,
    // Requirement ids in document order, and each id's position in it.
    pub document_order: Vec<String>,
    pub requirement_position: BTreeMap<String, usize>,
    pub entity_rank: BTreeMap<String, usize>,
    // Parent id to its direct children, by id. A parent no entity carries is a key
    // all the same, as the scan over `parent` would find it.
    pub children: BTreeMap<String, Vec<String>>,
    // Entity id (redirects followed) to the requirements naming it in `entities`, by
    // id: what `Store::requirements_referencing` answers.
    pub requirements_of: BTreeMap<String, Vec<String>>,
    // The levels with their members and flow views, built on first use.
    pub(super) levels: std::cell::OnceCell<Levels>,
}

impl Indices {
    pub fn build(store: &Store) -> Indices {
        let doc_levels = doc_levels_from(store, &store.status.roots);
        let document_order = document_order_with(store, &doc_levels);
        let requirement_position = document_order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect();
        let entity_rank = entity_document_rank_with(store, &doc_levels);
        let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (id, e) in &store.graph.entities {
            if let Some(p) = e.parent.as_ref() {
                children.entry(p.clone()).or_default().push(id.clone());
            }
        }
        let mut requirements_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (rid, r) in &store.graph.requirements {
            for e in &r.entities {
                let list = requirements_of
                    .entry(store.resolve_id(e).to_string())
                    .or_default();
                if list.last() != Some(rid) {
                    list.push(rid.clone());
                }
            }
        }
        Indices {
            doc_levels,
            document_order,
            requirement_position,
            entity_rank,
            children,
            requirements_of,
            levels: std::cell::OnceCell::new(),
        }
    }

    // The level index, built on first use from the store the indices were built from.
    pub(super) fn levels(&self, store: &Store) -> &Levels {
        self.levels.get_or_init(|| Levels::build(store, self))
    }

    // Whether any entity names `id` as its parent.
    pub fn has_children(&self, id: &str) -> bool {
        self.children.contains_key(id)
    }

    // The direct children of `id`, by id.
    pub fn children_of(&self, id: &str) -> &[String] {
        self.children.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    // The requirements naming `id` in `entities`, by id.
    pub fn requirements_referencing(&self, id: &str) -> &[String] {
        self.requirements_of
            .get(id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}
