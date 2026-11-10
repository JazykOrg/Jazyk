use super::documents::prov_short;
use super::links::{card_link, card_path, doc_path, embed, rel, DOCS_DIR, FLOW_KINDS, LEVELS_DIR};
use crate::card::level_name;
use crate::ids::entity_slug;
use crate::model::View;
use crate::store::Store;
use std::collections::BTreeSet;

// Level pages, one per node with a level view and one per scope root with one, nested
// as the containment tree is. Mirrors docs/consumers/docsgen.md#level-pages.

// The file of a level's page, relative to `<out>/docsgen/`: `levels/<node-slug>.md`
// for a node, `levels/scope-<scope>.md` for the scope root. Shared with the LSP
// hover, so its level link names the same file docsgen writes.
pub fn level_page(target: &str) -> String {
    match crate::board::scope_target(target) {
        Some(scope) => format!("levels/scope-{}.md", crate::md::slug(scope)),
        None => format!("levels/{}.md", entity_slug(target)),
    }
}

// Every target with a level page: the scope roots with a level view first, then every
// node with one, by id.
pub(super) fn level_targets(store: &Store) -> Vec<String> {
    let scopes: BTreeSet<&str> = store
        .graph
        .entities
        .values()
        .map(|e| e.scope.as_str())
        .collect();
    let mut out: Vec<String> = Vec::new();
    for scope in scopes {
        let target = crate::store::scope_root_target(scope);
        if crate::derive::level_view_id(store, &target).is_some() {
            out.push(target);
        }
    }
    for id in store.graph.entities.keys() {
        if crate::derive::level_view_id(store, id).is_some() {
            out.push(id.clone());
        }
    }
    out
}

// The href of a target's level page from a page in `from`: `./levels/<file>` from a
// requirements document or the index, the sibling `./<file>` from another level page.
pub(super) fn level_href(target: &str, from: &str) -> String {
    rel(from, &format!("{}/{}", DOCS_DIR, level_page(target)))
}

// "[Name](<page>)" for a target's level page, from a page in `from`. None when the
// target holds no level.
pub(super) fn level_link(store: &Store, target: &str, from: &str) -> Option<String> {
    crate::derive::level_view_id(store, target)?;
    Some(format!(
        "[{}]({})",
        level_name(store, target),
        level_href(target, from)
    ))
}

// The chain from the scope root down to the node: `scope:<scope>`, each ancestor,
// the node. Mirrors docs/consumers/docsgen.md#level-pages.
fn containment_chain(store: &Store, target: &str) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut cur = target.to_string();
    let mut scope = String::new();
    while crate::board::scope_target(&cur).is_none() {
        let Some(e) = store.graph.entities.get(&cur) else {
            break;
        };
        scope = e.scope.clone();
        chain.push(cur.clone());
        match e.parent.as_deref() {
            Some(p) => cur = store.resolve_id(p).to_string(),
            None => break,
        }
        if chain.len() > 64 {
            break;
        }
    }
    let root = match crate::board::scope_target(target) {
        Some(_) => target.to_string(),
        None => crate::store::scope_root_target(&scope),
    };
    if chain.last() != Some(&root) {
        chain.push(root);
    }
    chain.reverse();
    chain
}

// The breadcrumb: every ancestor linked to its level page (an ancestor holding one
// child has no level page and links to its card instead, so the chain stays
// walkable), the node itself last and unlinked. This is the link up.
fn breadcrumb(store: &Store, target: &str) -> String {
    let chain = containment_chain(store, target);
    let parts: Vec<String> = chain
        .iter()
        .map(|t| {
            if t == target {
                return level_name(store, t);
            }
            level_link(store, t, LEVELS_DIR).unwrap_or_else(|| card_link(store, t, LEVELS_DIR))
        })
        .collect();
    parts.join(" › ")
}

// The direct children of a level, in document order: the level view's members are
// the children first, then the outside entities the lifted edges bring in, and only
// the children are members of the page.
fn level_children(store: &Store, target: &str) -> Vec<String> {
    let direct: BTreeSet<String> = crate::board::level_members(store, target)
        .into_iter()
        .collect();
    crate::derive::level_view_members(store, target)
        .into_iter()
        .filter(|m| direct.contains(m))
        .collect()
}

// The level's views in the order the page embeds them: the structural level view,
// then the flow views derived for the level (use case, then sequence), by id.
fn level_views(store: &Store, target: &str) -> Vec<(String, View)> {
    let mut out: Vec<(String, View)> = Vec::new();
    if let Some(vid) = crate::derive::level_view_id(store, target) {
        if let Some(v) = store.graph.views.get(&vid) {
            out.push((vid, v.clone()));
        }
    }
    let mut flows: Vec<(String, View)> = store
        .graph
        .views
        .iter()
        .filter(|(_, v)| FLOW_KINDS.contains(&v.kind.as_str()))
        .filter(|(vid, _)| crate::derive::flow_view_level(store, vid).as_deref() == Some(target))
        .map(|(vid, v)| (vid.clone(), v.clone()))
        .collect();
    let rank = |k: &str| {
        FLOW_KINDS
            .iter()
            .position(|f| *f == k)
            .unwrap_or(FLOW_KINDS.len())
    };
    flows.sort_by(|a, b| rank(&a.1.kind).cmp(&rank(&b.1.kind)).then(a.0.cmp(&b.0)));
    out.extend(flows);
    out
}

// One level page. Mirrors docs/consumers/docsgen.md#level-pages.
fn level_page_text(store: &Store, target: &str) -> String {
    let mut s = String::new();
    s.push_str(&breadcrumb(store, target));
    s.push_str("\n\n");
    s.push_str(&format!("# {}\n\n", level_name(store, target)));
    match crate::board::scope_target(target) {
        Some(scope) => {
            s.push_str(&format!("`{}` · scope `{}`\n\n", target, scope));
        }
        None => {
            let ent = &store.graph.entities[target];
            s.push_str(&format!("`{}`", target));
            if let Some(st) = &ent.stereotype {
                s.push_str(&format!(" · «{}»", st));
            }
            if let Some(d) = &ent.definition {
                s.push_str(&format!(" · {}", d));
            }
            s.push_str(&format!(
                " · [card]({}) · [entity page]({})\n\n",
                rel(LEVELS_DIR, &card_path(target)),
                rel(LEVELS_DIR, &doc_path(target))
            ));
            if let Some(p) = &ent.provenance {
                s.push_str(&format!(
                    "This entity is {} ({}); see its [proposals]({}#proposals).\n\n",
                    p.kind(),
                    prov_short(p),
                    rel(LEVELS_DIR, &doc_path(target))
                ));
            }
        }
    }

    let views = level_views(store, target);
    if !views.is_empty() {
        s.push_str("## Diagrams\n\n");
        for (vid, v) in &views {
            s.push_str(&embed(store, vid, v, LEVELS_DIR));
        }
    }

    // The members: the direct children in document order, each linked to its card
    // and, when it holds a level, to its level page with its child count. This is
    // the link down. An outside entity a lifted edge brings into the level view is
    // not a member.
    s.push_str("## Members\n\n");
    for c in level_children(store, target) {
        let Some(e) = store.graph.entities.get(&c) else {
            continue;
        };
        let mut line = format!("- {}", card_link(store, &c, LEVELS_DIR));
        if let Some(st) = &e.stereotype {
            line.push_str(&format!(" · «{}»", st));
        }
        if let Some(d) = &e.definition {
            line.push_str(&format!(" · {}", d));
        }
        if crate::derive::level_view_id(store, &c).is_some() {
            let n = crate::board::level_members(store, &c).len();
            line.push_str(&format!(
                " · [level]({}) ({} children)",
                level_href(&c, LEVELS_DIR),
                n
            ));
        }
        s.push_str(&line);
        s.push('\n');
    }
    s.push('\n');
    s
}

// Writes every level page and prunes the page of a node that lost its level (fewer
// than two children, or dissolved). Returns the pages written.
pub(super) fn write_level_pages(store: &Store, dir: &std::path::Path) -> usize {
    let levels_dir = dir.join("levels");
    let targets = level_targets(store);
    let live: BTreeSet<String> = targets
        .iter()
        .map(|t| level_page(t).trim_start_matches("levels/").to_string())
        .collect();
    if let Ok(rd) = crate::vfs::read_dir(&levels_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".md") && !live.contains(&name) {
                crate::vfs::remove_file(e.path()).ok();
            }
        }
    }
    if targets.is_empty() {
        return 0;
    }
    crate::vfs::create_dir_all(&levels_dir).ok();
    let mut written = 0;
    for t in &targets {
        crate::vfs::write(dir.join(level_page(t)), level_page_text(store, t)).ok();
        written += 1;
    }
    written
}
