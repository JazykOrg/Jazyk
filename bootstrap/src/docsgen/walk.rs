use super::levels::{level_href, level_link, level_targets};
use super::links::{
    caption, card_link, doc_path, embed, image, rel, requirement_link, view_page_link,
    view_page_path, view_pages_dir, view_parts, CARDS_DIR, FLOW_KINDS,
};
use crate::card::{Card, Crumb, Walk};
use crate::derive::instance_types;
use crate::ids::entity_slug;
use crate::model::View;
use crate::store::Store;
use std::collections::BTreeSet;

// Entity cards and diagram pages: the two nodes of the walk, one small page each,
// rendered from the shared model in card.rs. Mirrors docs/consumers/docsgen.md#entity-cards
// and docs/consumers/docsgen.md#diagram-pages.

// One crumb as a link from a page in `from`: the scope root to its level page, an
// entity to its card. The last crumb of a chain stays unlinked when `link_last` is
// false.
fn crumb_links(store: &Store, crumbs: &[Crumb], from: &str, link_last: bool) -> String {
    let n = crumbs.len();
    crumbs
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i + 1 == n && !link_last {
                return c.name.clone();
            }
            match crate::board::scope_target(&c.id) {
                // The scope root's page exists only when the root has a level view;
                // a one-member scope has none (docsgen.md#entity-cards).
                Some(_) if level_targets(store).iter().any(|t| t == &c.id) => {
                    format!("[{}]({})", c.name, level_href(&c.id, from))
                }
                Some(_) => c.name.clone(),
                None => card_link(store, &c.id, from),
            }
        })
        .collect::<Vec<_>>()
        .join(" › ")
}

// One line per flow cluster: the title, the use-case and sequence pages as links,
// and the step count. Mirrors docs/consumers/docsgen.md#entity-cards.
fn cluster_lines(store: &Store, view_ids: &[String], from: &str) -> Vec<String> {
    crate::card::flow_clusters(store, view_ids)
        .into_iter()
        .map(|c| {
            let kinds: Vec<String> = c
                .views
                .iter()
                .map(|(v, label)| match view_page_path(v) {
                    Some(path) => format!("[{}]({})", label, rel(from, &path)),
                    None => format!("`{}`", v),
                })
                .collect();
            format!("{} · {} ({} steps)", c.title, kinds.join(" · "), c.steps)
        })
        .collect()
}

// "[Name](<card>)" with the child count when the kin has a level.
fn kin_line(store: &Store, k: &crate::card::Kin, from: &str) -> String {
    let mut line = card_link(store, &k.id, from);
    if k.child_count >= 2 {
        line.push_str(&format!(" ({} children)", k.child_count));
    }
    line
}

// One card. Mirrors docs/consumers/docsgen.md#entity-cards: the header, then one
// level in every direction (up, into context, down, sideways), then the long reads.
fn card_text(store: &Store, c: &Card) -> String {
    let from = CARDS_DIR;
    let mut s = format!("# {}\n\n", c.name);
    let mut head = format!("`{}`", c.id);
    if let Some(st) = &c.stereotype {
        head.push_str(&format!(" · «{}»", st));
    }
    if !c.definition.is_empty() {
        head.push_str(&format!(" · {}", c.definition));
    }
    s.push_str(&head);
    s.push_str("\n\n");
    if c.provenance != "quote" {
        s.push_str(&format!(
            "This entity is {}; see its [proposal]({}#proposals).\n\n",
            c.provenance,
            rel(from, &doc_path(&c.id))
        ));
    }

    s.push_str("## Sits in\n\n");
    s.push_str(&crumb_links(store, &c.breadcrumb, from, false));
    s.push_str("\n\n");

    s.push_str("## In context\n\n");
    match c
        .context
        .as_deref()
        .and_then(|v| store.graph.views.get(v).map(|x| (v, x)))
    {
        Some((vid, v)) => s.push_str(&embed(store, vid, v, from)),
        None => s.push_str("no level view: the level above holds this entity alone\n\n"),
    }

    s.push_str("## Inside\n\n");
    match c
        .inside
        .as_deref()
        .and_then(|v| store.graph.views.get(v).map(|x| (v, x)))
    {
        Some((vid, v)) => {
            s.push_str(&embed(store, vid, v, from));
            if !c.inside_flows.is_empty() {
                s.push_str("Flows of this level:\n\n");
                for line in cluster_lines(store, &c.inside_flows, from) {
                    s.push_str(&format!("- {}\n", line));
                }
                s.push('\n');
            }
        }
        None => s.push_str("a leaf\n\n"),
    }

    s.push_str("## Relationships\n\n");
    if c.relationships.is_empty() {
        s.push_str("none\n\n");
    } else {
        for r in &c.relationships {
            let arrow = match r.direction.as_str() {
                "a" => "→",
                "b" => "←",
                _ => "↔",
            };
            let n = if r.count == 1 {
                "1 requirement".to_string()
            } else {
                format!("{} requirements", r.count)
            };
            s.push_str(&format!(
                "- {} {} {} · {}\n",
                r.r#type,
                arrow,
                card_link(store, &r.other, from),
                n
            ));
        }
        s.push('\n');
    }

    s.push_str("## Flows\n\n");
    if c.flows.is_empty() {
        s.push_str("none\n\n");
    } else {
        for line in cluster_lines(store, &c.flows, from) {
            s.push_str(&format!("- {}\n", line));
        }
        s.push('\n');
    }

    s.push_str("## Siblings\n\n");
    if c.siblings.is_empty() {
        s.push_str("none\n\n");
    } else {
        for k in &c.siblings {
            s.push_str(&format!("- {}\n", kin_line(store, k, from)));
        }
        s.push('\n');
    }

    if !c.children.is_empty() {
        s.push_str("## Children\n\n");
        for k in &c.children {
            s.push_str(&format!("- {}\n", kin_line(store, k, from)));
        }
        s.push('\n');
    }

    s.push_str("## More\n\n");
    let n = if c.requirement_count == 1 {
        "1 requirement".to_string()
    } else {
        format!("{} requirements", c.requirement_count)
    };
    s.push_str(&format!(
        "- [Requirements document]({}) ({})\n",
        rel(from, &doc_path(&c.id)),
        n
    ));
    // The parent's level: the crumb before the entity itself.
    if let Some(parent) = c.breadcrumb.iter().rev().nth(1) {
        if let Some(link) = level_link(store, &parent.id, from) {
            s.push_str(&format!("- Level page of {}\n", link));
        }
    }
    if c.inside.is_some() {
        if let Some(link) = level_link(store, &c.id, from) {
            s.push_str(&format!("- Own level page: {}\n", link));
        }
    }
    if let Some(p) = &c.proposal {
        s.push_str(&format!(
            "- [Proposal]({}#proposals) `{}`\n",
            rel(from, &doc_path(&c.id)),
            p
        ));
    }
    s.push('\n');
    s
}

// Writes one card per entity and prunes the card of an entity that is gone. Returns
// the cards written.
pub(super) fn write_cards(store: &Store, walk: &Walk, dir: &std::path::Path) -> usize {
    let cards_dir = dir.join("entities");
    let live: BTreeSet<String> = store
        .graph
        .entities
        .keys()
        .map(|id| format!("{}.md", entity_slug(id)))
        .collect();
    if let Ok(rd) = crate::vfs::read_dir(&cards_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".md") && !live.contains(&name) {
                crate::vfs::remove_file(e.path()).ok();
            }
        }
    }
    if store.graph.entities.is_empty() {
        return 0;
    }
    crate::vfs::create_dir_all(&cards_dir).ok();
    let mut written = 0;
    for id in store.graph.entities.keys() {
        let Some(c) = crate::card::entity_card(store, walk, id) else {
            continue;
        };
        crate::vfs::write(
            cards_dir.join(format!("{}.md", entity_slug(id))),
            card_text(store, &c),
        )
        .ok();
        written += 1;
    }
    written
}

// What a view with no level belongs to: its machine's subject, its instances' type,
// or nothing (a curated view).
fn belongs_to(store: &Store, view: &View, from: &str) -> String {
    match view.kind.as_str() {
        "state" => match view.members.first() {
            Some(subject) => format!("the state machine of {}", card_link(store, subject, from)),
            None => "a state view with no subject".to_string(),
        },
        "object" => {
            let types = instance_types(store);
            match view
                .members
                .iter()
                .find_map(|m| types.get(store.resolve_id(m)))
            {
                Some(t) => format!("the object view of {}", card_link(store, t, from)),
                None => "an object view with no type".to_string(),
            }
        }
        _ => "a curated view; it belongs to no level".to_string(),
    }
}

// One diagram page. Mirrors docs/consumers/docsgen.md#diagram-pages: the title, the
// level, the image, the legend, the steps, and the views around.
fn view_page_text(store: &Store, p: &crate::card::ViewPage) -> String {
    let Some((kind, _)) = view_parts(&p.id) else {
        return String::new();
    };
    let from = view_pages_dir(kind);
    let from = from.as_str();
    let view = &store.graph.views[&p.id];
    let count = if FLOW_KINDS.contains(&p.kind.as_str()) {
        format!("{} steps", view.members.len())
    } else {
        format!("{} members", view.members.len())
    };
    let mut s = format!("# {}\n\n`{}` · {} · {}\n\n", p.title, p.id, p.kind, count);

    s.push_str("## Level\n\n");
    match &p.level {
        Some(l) => {
            let mut line = crumb_links(store, &l.breadcrumb, from, true);
            if let Some(link) = level_link(store, &l.target, from) {
                // A node's chain ends in its card; the level page follows. The scope
                // root's chain is its level page already.
                if crate::board::scope_target(&l.target).is_none() {
                    line.push_str(&format!(" · level page: {}", link));
                }
            }
            s.push_str(&line);
        }
        None => s.push_str(&belongs_to(store, view, from)),
    }
    s.push_str("\n\n");

    s.push_str(&image(store, &p.id, view, from));
    s.push_str(&caption(store, &p.id, view, from));
    s.push_str("\n\n");

    s.push_str("## Drawn\n\n");
    if p.drawn.is_empty() {
        s.push_str("nothing\n\n");
    } else {
        for d in &p.drawn {
            let mut line = format!("- {}", card_link(store, &d.id, from));
            if let Some(st) = &d.stereotype {
                line.push_str(&format!(" · «{}»", st));
            }
            if let Some(lv) = &d.level_view {
                line.push_str(&format!(
                    " · [level below]({})",
                    rel(from, &view_page_path(lv).unwrap_or_default())
                ));
            }
            s.push_str(&line);
            s.push('\n');
        }
        s.push('\n');
    }

    if FLOW_KINDS.contains(&p.kind.as_str()) {
        s.push_str("## Steps\n\n");
        if p.steps.is_empty() {
            s.push_str("none\n\n");
        } else {
            for st in &p.steps {
                s.push_str(&format!(
                    "- {} {} · {} → {}\n",
                    requirement_link(store, &st.requirement, from),
                    st.statement,
                    card_link(store, &st.from, from),
                    card_link(store, &st.to, from)
                ));
            }
            s.push('\n');
        }
    }

    s.push_str("## Around\n\n");
    let list = |ids: &[String]| -> String {
        ids.iter()
            .map(|v| format!("{} `{}`", view_page_link(store, v, from), v))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut any = false;
    if !p.around.same_level.is_empty() {
        s.push_str(&format!("- Same level: {}\n", list(&p.around.same_level)));
        any = true;
    }
    if let Some(a) = &p.around.above {
        s.push_str(&format!("- Above: {}\n", list(std::slice::from_ref(a))));
        any = true;
    }
    if !p.around.below.is_empty() {
        s.push_str(&format!("- Below: {}\n", list(&p.around.below)));
        any = true;
    }
    if !any {
        s.push_str("none\n");
    }
    s.push('\n');
    s
}

// Writes one page per stored view under `diagrams/<kind>/` and prunes the page of a
// view that is gone. Returns the pages written.
pub(super) fn write_view_pages(store: &Store, walk: &Walk, dir: &std::path::Path) -> usize {
    let pages_dir = dir.join("diagrams");
    let live: BTreeSet<String> = store
        .graph
        .views
        .keys()
        .filter_map(|v| view_page_path(v))
        .map(|p| p.trim_start_matches("docsgen/diagrams/").to_string())
        .collect();
    if let Ok(kinds) = crate::vfs::read_dir(&pages_dir) {
        for k in kinds.flatten() {
            let kind = k.file_name().to_string_lossy().to_string();
            let Ok(rd) = crate::vfs::read_dir(k.path()) else {
                continue;
            };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".md") && !live.contains(&format!("{}/{}", kind, name)) {
                    crate::vfs::remove_file(e.path()).ok();
                }
            }
        }
    }
    let mut written = 0;
    for vid in store.graph.views.keys() {
        let Some(path) = view_page_path(vid) else {
            continue;
        };
        let Some(p) = crate::card::view_page(store, walk, vid) else {
            continue;
        };
        let file = store.out.join(&path);
        if let Some(parent) = file.parent() {
            crate::vfs::create_dir_all(parent).ok();
        }
        crate::vfs::write(file, view_page_text(store, &p)).ok();
        written += 1;
    }
    written
}
