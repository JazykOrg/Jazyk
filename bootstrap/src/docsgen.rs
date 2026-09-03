// Deterministic per-entity requirements documents, level pages, entity cards, and
// diagram pages: the reading surface between prose and graph. Rendered on every
// commit, no LLM. Renders the diagrams first so every image link resolves, embeds the
// views relevant to each entity, and cross-links the pages through the views they
// share. Mirrors docs/consumers/docsgen.md.
use crate::card::{Card, Crumb, Walk};
use crate::derive::{entity_slug, instance_types, view_edge_count};
use crate::gen::GenSettings;
use crate::model::{Diagnostic, Goal, Provenance, StateMachine, View};
use crate::store::Store;
use std::collections::{BTreeMap, BTreeSet};

fn slug(id: &str) -> String {
    id.strip_prefix("ent:").unwrap_or(id).to_string()
}

// The directories the pages live in, under `<out>`. Every link is a relative path
// from the page's directory to the target's path under `<out>`, so the out directory
// serves anywhere as-is. Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
const DOCS_DIR: &str = "docsgen";
const LEVELS_DIR: &str = "docsgen/levels";
const CARDS_DIR: &str = "docsgen/entities";

fn view_pages_dir(kind: &str) -> String {
    format!("docsgen/diagrams/{}", kind)
}

// The relative path from a page in `from_dir` to `to`, both under `<out>`: `./x.md`
// for a sibling, `../` per directory left otherwise.
fn rel(from_dir: &str, to: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let target: Vec<&str> = to.split('/').collect();
    let common = from
        .iter()
        .zip(target.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let ups = from.len() - common;
    let mut s = if ups == 0 {
        "./".to_string()
    } else {
        "../".repeat(ups)
    };
    s.push_str(&target[common..].join("/"));
    s
}

// The paths under `<out>` of the pages about an entity or a view.
fn doc_path(id: &str) -> String {
    format!("{}/{}.md", DOCS_DIR, slug(id))
}

fn card_path(id: &str) -> String {
    format!("{}/{}.md", CARDS_DIR, slug(id))
}

// `(kind, slug)` of a view id: `view:usecase/checkout` → `("usecase", "checkout")`.
fn view_parts(view_id: &str) -> Option<(&str, &str)> {
    view_id.strip_prefix("view:")?.split_once('/')
}

fn view_page_path(view_id: &str) -> Option<String> {
    let (kind, s) = view_parts(view_id)?;
    Some(format!("{}/{}.md", view_pages_dir(kind), s))
}

// "[Name](<requirements document>)" for a live entity, the bare id otherwise.
fn page_link(store: &Store, id: &str) -> String {
    doc_link(store, id, DOCS_DIR)
}

fn doc_link(store: &Store, id: &str, from: &str) -> String {
    let resolved = store.resolve_id(id);
    match store.graph.entities.get(resolved) {
        Some(e) => format!("[{}]({})", e.name, rel(from, &doc_path(resolved))),
        None => format!("`{}`", id),
    }
}

// "[Name](<card>)" for a live entity, the bare id otherwise. Every link to an entity
// from a card, a diagram page, a level page's members, or a caption is this one.
// Mirrors docs/consumers/docsgen.md#entity-cards.
fn card_link(store: &Store, id: &str, from: &str) -> String {
    let resolved = store.resolve_id(id);
    match store.graph.entities.get(resolved) {
        Some(e) => format!("[{}]({})", e.name, rel(from, &card_path(resolved))),
        None => format!("`{}`", id),
    }
}

// "[title](<diagram page>)" for a stored view, "[id](<diagram page>)" for one the
// store does not hold yet, the bare id when the id has no kind.
fn view_page_link(store: &Store, view_id: &str, from: &str) -> String {
    match view_page_path(view_id) {
        Some(p) => {
            let text = store
                .graph
                .views
                .get(view_id)
                .map(|v| v.title.clone())
                .unwrap_or_else(|| view_id.to_string());
            format!("[{}]({})", text, rel(from, &p))
        }
        None => format!("`{}`", view_id),
    }
}

// The relative paths a page links for a view's files: the `.svg` and the `.puml`
// under `<out>/diagrams/<kind>/`.
fn diagram_rel(view_id: &str) -> Option<(String, String)> {
    diagram_files(view_id, DOCS_DIR)
}

fn diagram_files(view_id: &str, from: &str) -> Option<(String, String)> {
    let (kind, s) = view_parts(view_id)?;
    Some((
        rel(from, &format!("diagrams/{}/{}.svg", kind, s)),
        rel(from, &format!("diagrams/{}/{}.puml", kind, s)),
    ))
}

fn svg_exists(store: &Store, view_id: &str) -> bool {
    let Some(rel) = view_id.strip_prefix("view:") else {
        return false;
    };
    let Some((kind, s)) = rel.split_once('/') else {
        return false;
    };
    store
        .out
        .join("diagrams")
        .join(kind)
        .join(format!("{}.svg", s))
        .exists()
}

const FLOW_KINDS: [&str; 4] = ["use-case", "activity", "sequence", "communication"];

// The arrows a view renders, counted over the members' relationships.
fn edge_count(store: &Store, view: &View) -> u64 {
    let members: Vec<String> = view
        .members
        .iter()
        .map(|m| store.resolve_id(m).to_string())
        .collect();
    view_edge_count(store, &members)
}

// The `split-view` goal a view carries when a count crossed its (possibly bumped)
// soft threshold: the member count for its kind, the edge count on structural and
// object views, the participant count on sequence and communication views. Mirrors
// docs/consumers/docsgen.md#diagrams-on-entity-pages.
fn over_limit_goal(store: &Store, view_id: &str, view: &View) -> Option<String> {
    let over = |name: &str, count: u64| -> bool {
        let bump = view.limits.get(name).map(|b| b.value);
        match crate::limits::threshold(name, bump) {
            Some((soft, _)) => count > soft,
            None => false,
        }
    };
    let members = view.members.len() as u64;
    let crossed = match view.kind.as_str() {
        "object" => {
            over("instances-per-object-view", members)
                || over("edges-per-view", edge_count(store, view))
        }
        k if FLOW_KINDS.contains(&k) => {
            over("members-per-flow-view", members)
                || ((k == "sequence" || k == "communication")
                    && over(
                        "participants-per-sequence-view",
                        participants(store, view).len() as u64,
                    ))
        }
        _ => {
            over("members-per-structural-view", members)
                || over("edges-per-view", edge_count(store, view))
        }
    };
    if crossed {
        return Some(format!("g:split-view:{}", view_id));
    }
    None
}

// The state machine whose subject is the first member of a state view.
fn machine_for<'a>(store: &'a Store, view: &View) -> Option<&'a StateMachine> {
    let subject = view.members.first()?;
    store
        .graph
        .state_machines
        .values()
        .find(|m| &m.subject == subject)
}

// The participant entities of a flow view, in order of first appearance across its
// member requirements.
fn participants(store: &Store, view: &View) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in &view.members {
        let Some(r) = store.graph.requirements.get(store.resolve_id(m)) else {
            continue;
        };
        for e in &r.entities {
            let resolved = store.resolve_id(e).to_string();
            if store.graph.entities.contains_key(&resolved) && !out.contains(&resolved) {
                out.push(resolved);
            }
        }
    }
    out
}

// The entities a view draws, as the caption lists them: the members of a structural
// or object view, the participants of a flow view lifted as the diagram draws them
// (the card model's reading, `drawn_entities_of`), the unlifted entities of a flow
// view the store does not hold (a synthesized one).
fn drawn(store: &Store, view_id: &str, view: &View) -> Vec<String> {
    if store.graph.views.contains_key(view_id) {
        return crate::derive::drawn_entities_of(store, view_id);
    }
    if FLOW_KINDS.contains(&view.kind.as_str()) {
        return participants(store, view);
    }
    view.members
        .iter()
        .map(|m| store.resolve_id(m).to_string())
        .filter(|m| store.graph.entities.contains_key(m))
        .collect()
}

// The caption line under an embedded rendering: the view id, its kind and count, the
// drawn entities as links to their cards, and the `.puml` source. The same line on
// every page that embeds the view; `from` is the page's directory under `<out>`, so
// the relative links resolve from wherever the page sits.
// Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
fn caption(store: &Store, view_id: &str, view: &View, from: &str) -> String {
    let (_, puml) = diagram_files(view_id, from).unwrap_or_default();
    let mut line = format!("`{}` (", view_id);
    let mut listed: Vec<String> = Vec::new();
    if view.kind == "state" {
        match machine_for(store, view) {
            Some(m) => line.push_str(&format!(
                "state, {} states: {}",
                m.states.len(),
                m.states.join(", ")
            )),
            None => line.push_str(&format!("state, {} members", view.members.len())),
        }
    } else if FLOW_KINDS.contains(&view.kind.as_str()) {
        line.push_str(&format!("{}, {} steps", view.kind, view.members.len()));
        listed = drawn(store, view_id, view);
    } else {
        line.push_str(&format!("{}, {} members", view.kind, view.members.len()));
        listed = drawn(store, view_id, view);
    }
    line.push(')');
    if !listed.is_empty() {
        let mut links: Vec<String> = listed
            .iter()
            .take(8)
            .map(|m| card_link(store, m, from))
            .collect();
        if listed.len() > 8 {
            links.push("...".to_string());
        }
        line.push_str(&format!(": {}", links.join(", ")));
    }
    line.push_str(&format!(" · [source]({})", puml));
    if let Some(goal) = over_limit_goal(store, view_id, view) {
        line.push_str(&format!(" · goal `{}`", goal));
    }
    line
}

// The image line of a rendering when its `.svg` exists; a view whose render failed
// keeps its caption and `.puml` link and no image; nothing is invented.
fn image(store: &Store, view_id: &str, view: &View, from: &str) -> String {
    match diagram_files(view_id, from) {
        Some((svg, _)) if svg_exists(store, view_id) => {
            format!("![{}]({})\n\n", view.title, svg)
        }
        _ => String::new(),
    }
}

// One embedded rendering on a page in `from`: the image, then the caption with the
// link to the view's diagram page.
fn embed(store: &Store, view_id: &str, view: &View, from: &str) -> String {
    if view_parts(view_id).is_none() {
        return String::new();
    }
    let mut s = image(store, view_id, view, from);
    s.push_str(&caption(store, view_id, view, from));
    s.push_str(&format!(" · [page]({})", rel(from, &view_page_path(view_id).unwrap_or_default())));
    s.push_str("\n\n");
    s
}

// Level pages, one per node with a level view and one per scope root with one, nested
// as the containment tree is. Mirrors docs/consumers/docsgen.md#level-pages.

// The file of a level's page, relative to `<out>/docsgen/`: `levels/<node-slug>.md`
// for a node, `levels/scope-<scope>.md` for the scope root.
fn level_page(target: &str) -> String {
    match crate::board::scope_target(target) {
        Some(scope) => format!("levels/scope-{}.md", crate::md::slug(scope)),
        None => format!("levels/{}.md", entity_slug(target)),
    }
}

// Every target with a level page: the scope roots with a level view first, then every
// node with one, by id.
fn level_targets(store: &Store) -> Vec<String> {
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

// The name a level page shows for its target: the node's name, or the scope's name.
fn level_name(store: &Store, target: &str) -> String {
    match crate::board::scope_target(target) {
        Some(scope) => {
            let mut cs = scope.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        }
        None => store
            .graph
            .entities
            .get(target)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| target.to_string()),
    }
}

// The href of a target's level page from a page in `from`: `./levels/<file>` from a
// requirements document or the index, the sibling `./<file>` from another level page.
fn level_href(target: &str, from: &str) -> String {
    rel(from, &format!("{}/{}", DOCS_DIR, level_page(target)))
}

// "[Name](<page>)" for a target's level page, from a page in `from`. None when the
// target holds no level.
fn level_link(store: &Store, target: &str, from: &str) -> Option<String> {
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
            s.push_str(&format!(
                " · [card]({}) · [entity page]({})\n\n",
                rel(LEVELS_DIR, &card_path(target)),
                rel(LEVELS_DIR, &doc_path(target))
            ));
            if let Some(d) = &ent.definition {
                s.push_str(d);
                s.push_str("\n\n");
            }
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
fn write_level_pages(store: &Store, dir: &std::path::Path) -> usize {
    let levels_dir = dir.join("levels");
    let targets = level_targets(store);
    let live: BTreeSet<String> = targets
        .iter()
        .map(|t| level_page(t).trim_start_matches("levels/").to_string())
        .collect();
    if let Ok(rd) = std::fs::read_dir(&levels_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".md") && !live.contains(&name) {
                std::fs::remove_file(e.path()).ok();
            }
        }
    }
    if targets.is_empty() {
        return 0;
    }
    std::fs::create_dir_all(&levels_dir).ok();
    let mut written = 0;
    for t in &targets {
        std::fs::write(dir.join(level_page(t)), level_page_text(store, t)).ok();
        written += 1;
    }
    written
}

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
                Some(_) => format!("[{}]({})", c.name, level_href(&c.id, from)),
                None => card_link(store, &c.id, from),
            }
        })
        .collect::<Vec<_>>()
        .join(" › ")
}

// The step count of a flow view: its member requirements.
fn step_count(store: &Store, view_id: &str) -> usize {
    store
        .graph
        .views
        .get(view_id)
        .map(|v| v.members.len())
        .unwrap_or(0)
}

// "[title](<diagram page>) `view id` (n steps)".
fn flow_line(store: &Store, view_id: &str, from: &str) -> String {
    format!(
        "{} `{}` ({} steps)",
        view_page_link(store, view_id, from),
        view_id,
        step_count(store, view_id)
    )
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
    match c.context.as_deref().and_then(|v| store.graph.views.get(v).map(|x| (v, x))) {
        Some((vid, v)) => s.push_str(&embed(store, vid, v, from)),
        None => s.push_str("no level view: the level above holds this entity alone\n\n"),
    }

    s.push_str("## Inside\n\n");
    match c.inside.as_deref().and_then(|v| store.graph.views.get(v).map(|x| (v, x))) {
        Some((vid, v)) => {
            s.push_str(&embed(store, vid, v, from));
            if !c.inside_flows.is_empty() {
                s.push_str("Flows of this level:\n\n");
                for f in &c.inside_flows {
                    s.push_str(&format!("- {}\n", flow_line(store, f, from)));
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
        for f in &c.flows {
            s.push_str(&format!("- {}\n", flow_line(store, f, from)));
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
fn write_cards(store: &Store, walk: &Walk, dir: &std::path::Path) -> usize {
    let cards_dir = dir.join("entities");
    let live: BTreeSet<String> = store
        .graph
        .entities
        .keys()
        .map(|id| format!("{}.md", slug(id)))
        .collect();
    if let Ok(rd) = std::fs::read_dir(&cards_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".md") && !live.contains(&name) {
                std::fs::remove_file(e.path()).ok();
            }
        }
    }
    if store.graph.entities.is_empty() {
        return 0;
    }
    std::fs::create_dir_all(&cards_dir).ok();
    let mut written = 0;
    for id in store.graph.entities.keys() {
        let Some(c) = crate::card::entity_card(store, walk, id) else {
            continue;
        };
        std::fs::write(cards_dir.join(format!("{}.md", slug(id))), card_text(store, &c)).ok();
        written += 1;
    }
    written
}

// The GitHub anchor of the heading "### `req:x`" in a requirements document: the
// backticks and the colon drop, the rest lowercases.
fn heading_anchor(rid: &str) -> String {
    rid.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase()
}

// "[`req:x`](<document>#anchor)": the requirement's block in the document of the
// first entity it names, the bare id when it names none.
fn requirement_link(store: &Store, rid: &str, from: &str) -> String {
    let doc = store
        .graph
        .requirements
        .get(rid)
        .and_then(|r| r.entities.iter().map(|e| store.resolve_id(e).to_string()).find(|e| store.graph.entities.contains_key(e)));
    match doc {
        Some(e) => format!(
            "[`{}`]({}#{})",
            rid,
            rel(from, &doc_path(&e)),
            heading_anchor(rid)
        ),
        None => format!("`{}`", rid),
    }
}

// What a view with no level belongs to: its machine's subject, its instances' type,
// or nothing (a curated view).
fn belongs_to(store: &Store, view: &View, from: &str) -> String {
    match view.kind.as_str() {
        "state" => match view.members.first() {
            Some(subject) => format!(
                "the state machine of {}",
                card_link(store, subject, from)
            ),
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
fn write_view_pages(store: &Store, walk: &Walk, dir: &std::path::Path) -> usize {
    let pages_dir = dir.join("diagrams");
    let live: BTreeSet<String> = store
        .graph
        .views
        .keys()
        .filter_map(|v| view_page_path(v))
        .map(|p| p.trim_start_matches("docsgen/diagrams/").to_string())
        .collect();
    if let Ok(kinds) = std::fs::read_dir(&pages_dir) {
        for k in kinds.flatten() {
            let kind = k.file_name().to_string_lossy().to_string();
            let Ok(rd) = std::fs::read_dir(k.path()) else {
                continue;
            };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".md") && !live.contains(&format!("{}/{}", kind, name)) {
                    std::fs::remove_file(e.path()).ok();
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
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(file, view_page_text(store, &p)).ok();
        written += 1;
    }
    written
}

// The synthesized state view of an entity's machine, when no stored view exists (the
// renderer synthesizes the same one). Mirrors render targets.
fn synthesized_state_view(store: &Store, id: &str) -> Option<View> {
    let m = store
        .graph
        .state_machines
        .values()
        .find(|m| m.subject == id)?;
    let name = store
        .graph
        .entities
        .get(&m.subject)
        .map(|e| e.name.clone())?;
    Some(View {
        kind: "state".to_string(),
        title: name,
        members: vec![m.subject.clone()],
        ..Default::default()
    })
}

// The views an entity page embeds, in the order the docs state: the level neighborhood
// (the parent's level view, or the scope root's, then the entity's own), the entity's
// own state view, every flow view naming it, the object view of its type, then every
// other curated view listing it.
// Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
fn relevant_views(
    store: &Store,
    id: &str,
    instances: &BTreeMap<String, String>,
) -> Vec<(String, View)> {
    let ent = &store.graph.entities[id];
    let mut out: Vec<(String, View)> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut push = |vid: String, view: View, out: &mut Vec<(String, View)>| {
        if seen.insert(vid.clone()) {
            out.push((vid, view));
        }
    };
    let stored = |vid: &str| store.graph.views.get(vid).cloned();

    // The level neighborhood: the parent's level view (the scope root's for a
    // parentless entity), then the entity's own level view when it holds a level.
    // Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
    let above = match ent.parent.as_deref() {
        Some(p) => store.resolve_id(p).to_string(),
        None => crate::store::scope_root_target(&ent.scope),
    };
    for target in [above, id.to_string()] {
        if let Some(vid) = crate::derive::level_view_id(store, &target) {
            if let Some(v) = stored(&vid) {
                push(vid, v, &mut out);
            }
        }
    }
    let state_id = format!("view:state/{}", entity_slug(id));
    match stored(&state_id) {
        Some(v) => push(state_id, v, &mut out),
        None => {
            if let Some(v) = synthesized_state_view(store, id) {
                push(state_id, v, &mut out);
            }
        }
    }
    for (vid, v) in &store.graph.views {
        if !FLOW_KINDS.contains(&v.kind.as_str()) {
            continue;
        }
        let names_it = v.members.iter().any(|m| {
            store
                .graph
                .requirements
                .get(store.resolve_id(m))
                .is_some_and(|r| r.entities.iter().any(|e| store.resolve_id(e) == id))
        });
        if names_it {
            push(vid.clone(), v.clone(), &mut out);
        }
    }
    let ty = instances
        .get(id)
        .cloned()
        .or_else(|| instances.values().any(|t| t == id).then(|| id.to_string()));
    if let Some(t) = ty {
        let ov = format!("view:object/{}", entity_slug(&t));
        if let Some(v) = stored(&ov) {
            push(ov, v, &mut out);
        }
    }
    for (vid, v) in &store.graph.views {
        if v.default {
            continue;
        }
        let listed = v
            .members
            .iter()
            .chain(v.collapse.iter())
            .any(|m| store.resolve_id(m) == id);
        if listed {
            push(vid.clone(), v.clone(), &mut out);
        }
    }
    out
}

fn prov_short(p: &Provenance) -> String {
    match p {
        Provenance::Quote(s) => format!("`{}#{}`", s.doc, s.section),
        Provenance::Derived { from, reasoning } => {
            format!("derived from {} ({})", from.join(", "), reasoning)
        }
        Provenance::Decree { author, at, note } => format!(
            "decreed by {} at {}{}",
            author,
            at,
            note.as_ref()
                .map(|n| format!(" ({})", n))
                .unwrap_or_default()
        ),
    }
}

// Whether an open diagnostic proposes a sentence for this entity, one of its
// attributes, or one of its requirements.
fn proposes_for(store: &Store, d: &Diagnostic, id: &str, rids: &[String]) -> bool {
    if d.lifecycle != "open" {
        return false;
    }
    if d.rule != "ratification-pending" && d.rule != "invented-choice" {
        return false;
    }
    d.subjects.iter().any(|sj| {
        let resolved = store.resolve_id(sj);
        resolved == id || rids.iter().any(|r| r == resolved)
    })
}

// One proposal, rendered as its prompt: the sentence, the target, the upstream nodes
// with their reasoning (or the decree's author and note), and the options.
// Mirrors docs/consumers/docsgen.md#ratification-proposals.
fn render_proposal(did: &str, d: &Diagnostic) -> String {
    let mut s = format!("### `{}`\n\n{}\n\n", did, d.message);
    if let Some(rsn) = &d.reasoning {
        s.push_str(&format!("{}\n\n", rsn));
    }
    let Some(p) = &d.prompt else {
        return s;
    };
    s.push_str(&format!("{}\n\n", p.question));
    for o in &p.options {
        match &o.edit {
            Some(e) => {
                s.push_str(&format!("- Apply: {}\n\n  > {}\n\n", o.label, e.new_text));
                s.push_str(&format!("  Target: `{}#{}`\n", e.doc, e.section));
            }
            None => s.push_str(&format!("- Answer: {}\n", o.label)),
        }
    }
    s.push('\n');
    s
}

// The goals the store still holds (parked and failed persist in status.yaml) whose
// target is the entity, one of its requirements, or a view listing the entity.
fn goal_lines(store: &Store, id: &str, rids: &[String]) -> Vec<String> {
    let matches = |g: &Goal| {
        let t = g.target.as_str();
        t == id
            || rids.iter().any(|r| r == t)
            || store.graph.views.get(t).is_some_and(|v| {
                v.members
                    .iter()
                    .chain(v.collapse.iter())
                    .any(|m| store.resolve_id(m) == id)
            })
    };
    let cause = |g: &Goal| {
        g.cause
            .as_ref()
            .map(|c| format!(" · cause c{}-{} via {}", c.generation, c.mutation, c.via))
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for g in &store.status.parked {
        if matches(g) {
            out.push(format!("- `{}` ({}) parked{}", g.id, g.kind, cause(g)));
        }
    }
    for f in &store.status.failed {
        if matches(&f.goal) {
            out.push(format!(
                "- `{}` ({}) failed: {}{}",
                f.goal.id,
                f.goal.kind,
                f.reason,
                cause(&f.goal)
            ));
        }
    }
    out
}

pub fn write_all(store: &Store, gs: &GenSettings) -> usize {
    // Render the diagrams first, so every image link below resolves even when this
    // run is `jazyk docsgen` alone. Unchanged views are skipped by content.
    crate::render::render_all(store, &store.out);
    let vmap = crate::verify::status_map(store, gs);
    let instances = instance_types(store);
    let dir = store.out.join("docsgen");
    std::fs::create_dir_all(&dir).ok();
    // Stale documents for absent entities are removed, so links never mislead.
    let live: BTreeSet<String> = store.graph.entities.keys().map(|id| slug(id)).collect();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(stem) = name.strip_suffix(".md") {
                if stem != "index" && !live.contains(stem) {
                    std::fs::remove_file(e.path()).ok();
                }
            }
        }
    }
    let mut written = 0;
    for (id, ent) in &store.graph.entities {
        let mut s = String::new();
        s.push_str(&format!("# {}\n\n", ent.name));
        s.push_str(&format!("`{}`", id));
        if let Some(st) = &ent.stereotype {
            s.push_str(&format!(" · «{}»", st));
        }
        if ent.scope != "public" {
            s.push_str(&format!(" · scope `{}`", ent.scope));
        }
        if !ent.aliases.is_empty() {
            s.push_str(&format!(" · also known as: {}", ent.aliases.join(", ")));
        }
        // The one link out of the documents' web: the card, where the walk starts.
        s.push_str(&format!(" · [card]({})", rel(DOCS_DIR, &card_path(id))));
        s.push_str("\n\n");
        if let Some(d) = &ent.definition {
            s.push_str(d);
            s.push_str("\n\n");
        }
        let children: Vec<&String> = store
            .graph
            .entities
            .iter()
            .filter(|(_, e)| {
                e.parent
                    .as_deref()
                    .is_some_and(|p| store.resolve_id(p) == id.as_str())
            })
            .map(|(cid, _)| cid)
            .collect();
        if ent.parent.is_some() || !children.is_empty() {
            let mut line = String::new();
            if let Some(p) = &ent.parent {
                line.push_str(&format!("Parent: {}", page_link(store, p)));
            }
            if !children.is_empty() {
                if !line.is_empty() {
                    line.push_str(" · ");
                }
                let links: Vec<String> = children.iter().map(|c| page_link(store, c)).collect();
                line.push_str(&format!("Children: {}", links.join(", ")));
            }
            s.push_str(&line);
            s.push_str("\n\n");
        }
        if !ent.attributes.is_empty() {
            s.push_str("Attributes:\n\n");
            for a in &ent.attributes {
                let mut line = format!("- `{}`", a.name);
                if let Some(t) = &a.r#type {
                    line.push_str(&format!(": {}", t));
                }
                if let Some(v) = &a.value {
                    line.push_str(&format!(" = {}", v));
                }
                line.push_str(&format!(" · {}", prov_short(&a.provenance)));
                s.push_str(&line);
                s.push('\n');
            }
            s.push('\n');
        }
        if let Some(p) = &ent.provenance {
            s.push_str(&format!(
                "This entity is {} ({}); see [Proposals](#proposals).\n\n",
                p.kind(),
                prov_short(p)
            ));
        }

        // Diagrams: the renderings relevant to this entity, embedded as relative
        // links into ../diagrams/, so the out directory serves anywhere as-is.
        let views = relevant_views(store, id, &instances);
        if !views.is_empty() {
            s.push_str("## Diagrams\n\n");
            for (vid, v) in &views {
                s.push_str(&embed(store, vid, v, DOCS_DIR));
            }
        }

        let mut rids = store.requirements_referencing(id);
        rids.sort();
        if !rids.is_empty() {
            s.push_str("## Requirements\n\n");
            for rid in &rids {
                let Some(r) = store.graph.requirements.get(rid) else {
                    continue;
                };
                s.push_str(&format!("### `{}`\n\n{}\n\n", rid, r.statement));
                if !r.facets.is_empty() {
                    let fs: Vec<String> = r
                        .facets
                        .iter()
                        .map(|f| match &f.measure {
                            Some(m) => format!("{} ({})", f.facet, m),
                            None => f.facet.clone(),
                        })
                        .collect();
                    s.push_str(&format!("Facets: {}\n\n", fs.join(", ")));
                }
                if !r.edges.is_empty() {
                    let es: Vec<String> = r
                        .edges
                        .iter()
                        .map(|e| {
                            let t = e
                                .rel_type
                                .as_deref()
                                .unwrap_or(crate::model::DEFAULT_REL_TYPE);
                            match &e.cardinality {
                                Some(c) => format!("`{}` → `{}` ({}, {})", e.a, e.b, t, c),
                                None => format!("`{}` → `{}` ({})", e.a, e.b, t),
                            }
                        })
                        .collect();
                    s.push_str(&format!("Edges: {}\n\n", es.join(", ")));
                }
                if let Some(t) = &r.transition {
                    let mut line = format!("Transition: `{}`: {} → {}", t.subject, t.from, t.to);
                    if let Some(tr) = &t.trigger {
                        line.push_str(&format!(" on {}", tr));
                    }
                    if let Some(gu) = &t.guard {
                        line.push_str(&format!(" if {}", gu));
                    }
                    s.push_str(&line);
                    s.push_str("\n\n");
                }
                match r.source.as_ref() {
                    Some(src) => s.push_str(&format!(
                        "> {}\n\nSource: `{}#{}`",
                        src.quote.split_whitespace().collect::<Vec<_>>().join(" "),
                        src.doc,
                        src.section
                    )),
                    None => s.push_str(&format!(
                        "Provenance: {}",
                        crate::session::provenance_line(r)
                    )),
                }
                let others: Vec<String> = r
                    .entities
                    .iter()
                    .filter(|e| store.resolve_id(e) != id.as_str())
                    .map(|e| page_link(store, e))
                    .collect();
                if !others.is_empty() {
                    s.push_str(&format!(" · ties: {}", others.join(", ")));
                }
                s.push_str("\n\n");
                if let Some(v) = vmap.get(rid.as_str()) {
                    let status = v["status"].as_str().unwrap_or("missing");
                    let mut line = format!("Verification: `{}`", status);
                    if let Some(name) = v["name"].as_str() {
                        line.push_str(&format!(
                            " by `{}` ({})",
                            name,
                            v["kind"].as_str().unwrap_or("?")
                        ));
                    }
                    if let Some(t) = v["lastRun"].as_str() {
                        line.push_str(&format!(", last run {}", t));
                    }
                    if let Some(ev) = v["evidence"].as_str() {
                        line.push_str(&format!(
                            "\n\n> {}",
                            ev.split_whitespace().collect::<Vec<_>>().join(" ")
                        ));
                    }
                    s.push_str(&line);
                    s.push_str("\n\n");
                }
            }
        }

        // Relationships: one line per direction-and-type group, the other member
        // linked to its page. Mirrors docs/consumers/docsgen.md#the-requirements-document.
        let mut rels: Vec<String> = Vec::new();
        for (relid, rel) in &store.graph.relationships {
            if !rel.members.contains(id) {
                continue;
            }
            for c in &rel.contributions {
                let end_txt = |e: &String| {
                    if e == id {
                        ent.name.clone()
                    } else {
                        page_link(store, e)
                    }
                };
                let mut line = format!("- {} → {} ({}", end_txt(&c.a), end_txt(&c.b), c.r#type);
                if let Some(card) = &c.cardinality {
                    line.push_str(&format!(", {}", card));
                }
                line.push_str(&format!(
                    ") · `{}` · from {}",
                    relid,
                    c.requirements.join(", ")
                ));
                rels.push(line);
            }
        }
        if !rels.is_empty() {
            s.push_str("## Relationships\n\n");
            s.push_str(&rels.join("\n"));
            s.push_str("\n\n");
        }

        // Proposals: pending ratifications on the entity, its attributes, and its
        // requirements, rendered as prompts with the suggested edit.
        let proposals: Vec<String> = store
            .graph
            .diagnostics
            .iter()
            .filter(|(_, d)| proposes_for(store, d, id, &rids))
            .map(|(did, d)| render_proposal(did, d))
            .collect();
        if !proposals.is_empty() {
            s.push_str("## Proposals\n\n");
            for p in proposals {
                s.push_str(&p);
            }
        }

        let goals = goal_lines(store, id, &rids);
        if !goals.is_empty() {
            s.push_str("## Goals\n\n");
            s.push_str(&goals.join("\n"));
            s.push_str("\n\n");
        }

        let diags: Vec<String> = store
            .graph
            .diagnostics
            .iter()
            .filter(|(_, d)| {
                d.lifecycle == "open"
                    && d.subjects.iter().any(|sj| {
                        let resolved = store.resolve_id(sj);
                        resolved == id.as_str() || rids.iter().any(|r| r == resolved)
                    })
            })
            .map(|(did, d)| {
                let triage = d
                    .triage
                    .as_ref()
                    .map(|t| format!(" · triage: {}", t))
                    .unwrap_or_default();
                format!(
                    "- `{}` [{}] {}: {}{}",
                    did, d.severity, d.rule, d.message, triage
                )
            })
            .collect();
        if !diags.is_empty() {
            s.push_str("## Open diagnostics\n\n");
            s.push_str(&diags.join("\n"));
            s.push_str("\n\n");
        }

        if !ent.mentions.is_empty() {
            s.push_str("## Mentioned in\n\n");
            for m in &ent.mentions {
                s.push_str(&format!(
                    "- `{}#{}`: \"{}\"\n",
                    m.doc,
                    m.section,
                    m.quote.split_whitespace().collect::<Vec<_>>().join(" ")
                ));
            }
        }
        std::fs::write(dir.join(format!("{}.md", slug(id))), s).ok();
        written += 1;
    }

    // The level pages: one per node with a level view and one per scope root with
    // one, nested as the containment tree is. Mirrors docs/consumers/docsgen.md#level-pages.
    written += write_level_pages(store, &dir);
    // The walk: one card per entity, one page per view, from the shared model.
    // Mirrors docs/consumers/docsgen.md#entity-cards.
    let walk = Walk::new(store);
    written += write_cards(store, &walk, &dir);
    written += write_view_pages(store, &walk, &dir);

    // The index: every entity linked, the default class and component views rendered
    // from the graph, every view listed, and the pending proposals grouped by target
    // document. Mirrors docs/consumers/docsgen.md#relationships-view.
    if !store.graph.entities.is_empty() {
        let mut idx = String::new();
        idx.push_str("# Index\n\n");
        for (id, ent) in &store.graph.entities {
            idx.push_str(&format!("- [{}](./{}.md) `{}`\n", ent.name, slug(id), id));
        }

        let class_views: Vec<(&String, &View)> = store
            .graph
            .views
            .iter()
            .filter(|(vid, _)| vid.starts_with("view:class/"))
            .collect();
        let component_views: Vec<(&String, &View)> = store
            .graph
            .views
            .iter()
            .filter(|(vid, _)| vid.starts_with("view:component/"))
            .collect();
        // The scope root's level view is the index's picture of a scope, and its
        // caption links to the scope root's level page, the top of the level pages.
        // Mirrors docs/consumers/docsgen.md#relationships-view.
        let mut root_levels: BTreeMap<String, String> = BTreeMap::new();
        for t in level_targets(store) {
            if let Some(scope) = crate::board::scope_target(&t) {
                if let Some(vid) = crate::derive::level_view_id(store, &t) {
                    root_levels.insert(vid, scope.to_string());
                }
            }
        }
        let mut linked_scopes: BTreeSet<String> = BTreeSet::new();
        if !class_views.is_empty() || !component_views.is_empty() {
            idx.push_str("\n## Diagrams\n\n");
            for (vid, v) in class_views.iter().chain(component_views.iter()) {
                let mut block = embed(store, vid, v, DOCS_DIR);
                if let Some(scope) = root_levels.get(vid.as_str()) {
                    let target = crate::store::scope_root_target(scope);
                    if let Some(link) = level_link(store, &target, DOCS_DIR) {
                        let trimmed = block.trim_end().len();
                        block.truncate(trimmed);
                        block.push_str(&format!(" · level page: {}\n\n", link));
                        linked_scopes.insert(scope.clone());
                    }
                }
                idx.push_str(&block);
            }
        }
        // A scope root whose level view is not stored yet (the next commit derives
        // it) still gets its level page linked, so the top of the levels is always
        // reachable from the index.
        for scope in root_levels.values() {
            if linked_scopes.contains(scope) {
                continue;
            }
            let target = crate::store::scope_root_target(scope);
            if let Some(link) = level_link(store, &target, DOCS_DIR) {
                idx.push_str(&format!("Level page of scope `{}`: {}\n\n", scope, link));
            }
        }

        let mut all_views: Vec<(String, View)> = store
            .graph
            .views
            .iter()
            .map(|(vid, v)| (vid.clone(), v.clone()))
            .collect();
        for m in store.graph.state_machines.values() {
            let vid = format!("view:state/{}", entity_slug(&m.subject));
            if !store.graph.views.contains_key(&vid) {
                if let Some(v) = synthesized_state_view(store, &m.subject) {
                    all_views.push((vid, v));
                }
            }
        }
        all_views.sort_by(|a, b| a.0.cmp(&b.0));
        if !all_views.is_empty() {
            idx.push_str("## Views\n\n");
            for (vid, v) in &all_views {
                let (svg, puml) = diagram_rel(vid).unwrap_or_default();
                let mut line = format!(
                    "- `{}` ({}) {} · {} members · [svg]({}) · [puml]({})",
                    vid,
                    v.kind,
                    v.title,
                    v.members.len(),
                    svg,
                    puml
                );
                if let Some(goal) = over_limit_goal(store, vid, v) {
                    line.push_str(&format!(" · goal `{}`", goal));
                }
                idx.push_str(&line);
                idx.push('\n');
            }
            idx.push('\n');
        }

        // The ratification report: every pending proposal, grouped by the document
        // its edit targets, so an owner reviews one document's proposals together.
        let mut by_doc: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (did, d) in &store.graph.diagnostics {
            if d.lifecycle != "open"
                || (d.rule != "ratification-pending" && d.rule != "invented-choice")
            {
                continue;
            }
            let Some(p) = &d.prompt else { continue };
            let edit = p.options.iter().find_map(|o| o.edit.as_ref());
            let (doc, line) = match edit {
                Some(e) => (
                    e.doc.clone(),
                    format!(
                        "- `{}` ({}): {} → `{}#{}`",
                        did,
                        d.subjects.join(", "),
                        e.new_text,
                        e.doc,
                        e.section
                    ),
                ),
                None => (
                    "(no target)".to_string(),
                    format!("- `{}` ({}): {}", did, d.subjects.join(", "), d.message),
                ),
            };
            by_doc.entry(doc).or_default().push(line);
        }
        if !by_doc.is_empty() {
            idx.push_str("## Ratification\n\n");
            for (doc, lines) in &by_doc {
                idx.push_str(&format!("### `{}`\n\n", doc));
                idx.push_str(&lines.join("\n"));
                idx.push_str("\n\n");
            }
        }
        std::fs::write(dir.join("index.md"), idx).ok();
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn gs(out: &std::path::Path) -> GenSettings {
        GenSettings {
            deliverable: out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        }
    }

    #[test]
    fn renders_and_prunes() {
        let out = std::env::temp_dir().join(format!("jazyk-docsgen-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        s.graph.entities.insert(
            "ent:cart".into(),
            Entity {
                name: "Cart".into(),
                definition: Some("holds items".into()),
                mentions: vec![SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "the Cart".into(),
                }],
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                statement: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "holds\nitems".into(),
                }),
                ..Default::default()
            },
        );
        // A stale file for an entity that no longer exists must be pruned.
        std::fs::create_dir_all(out.join("docsgen")).ok();
        std::fs::write(out.join("docsgen/ghost.md"), "old").ok();
        // A stale card and a stale diagram page are pruned with it.
        std::fs::create_dir_all(out.join("docsgen/entities")).ok();
        std::fs::write(out.join("docsgen/entities/ghost.md"), "old").ok();
        std::fs::create_dir_all(out.join("docsgen/diagrams/class")).ok();
        std::fs::write(out.join("docsgen/diagrams/class/ghost.md"), "old").ok();
        // The document and the card: two pages.
        let n = write_all(&s, &gs(&out));
        assert_eq!(n, 2);
        let doc = std::fs::read_to_string(out.join("docsgen/cart.md")).unwrap();
        assert!(doc.contains("# Cart"));
        assert!(doc.contains("req:shop-1"));
        assert!(
            doc.contains("> holds items"),
            "quote is whitespace-normalized: {}",
            doc
        );
        assert!(doc.contains("[card](./entities/cart.md)"), "{}", doc);
        assert!(!out.join("docsgen/ghost.md").exists());
        assert!(!out.join("docsgen/entities/ghost.md").exists());
        assert!(!out.join("docsgen/diagrams/class/ghost.md").exists());
        let card = std::fs::read_to_string(out.join("docsgen/entities/cart.md")).unwrap();
        assert!(card.contains("# Cart\n\n`ent:cart` · holds items\n"), "{}", card);
        assert!(card.contains("## Inside\n\na leaf\n"), "{}", card);
        assert!(
            card.contains("- [Requirements document](../cart.md) (1 requirement)"),
            "{}",
            card
        );
    }

    // The showcase graph: an entity page embeds the scope class view, the containing
    // system's component view, its own state view, and the flow view naming it, each
    // as a relative image link into ../diagrams/, with captions cross-linking the
    // member pages. Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
    #[test]
    fn showcase_page_embeds_images_and_cross_links() {
        let out =
            std::env::temp_dir().join(format!("jazyk-docsgen-showcase-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        std::fs::create_dir_all(&out).unwrap();
        let mut s = crate::derive::tests::showcase_store();
        s.out = out.clone();
        let mut batch = crate::store::RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        write_all(&s, &gs(&out));

        let page = std::fs::read_to_string(out.join("docsgen/order.md")).unwrap();
        // The images, in the stated order: the level neighborhood (the parent's level
        // view, order-service holding four children; the order itself holds none), own
        // state view, the flow view naming the order. The scope root's level view and
        // the shop's belong to the pages of their members, not to the order's.
        let level = crate::derive::level_view_id(&s, "ent:order-service").unwrap();
        let level_svg = format!("](../diagrams/{}.svg)", &level["view:".len()..]);
        assert!(page.contains(&level_svg), "{}", page);
        assert!(
            !page.contains("](../diagrams/component/public.svg)"),
            "{}",
            page
        );
        assert!(
            !page.contains("](../diagrams/component/shop.svg)"),
            "{}",
            page
        );
        assert!(page.contains("](../diagrams/state/order.svg)"), "{}", page);
        assert!(
            page.contains("](../diagrams/usecase/customer-shop.svg)"),
            "{}",
            page
        );
        let level_pos = page.find(&level_svg).unwrap();
        let state_pos = page.find("../diagrams/state/order.svg").unwrap();
        let flow_pos = page.find("../diagrams/usecase/customer-shop.svg").unwrap();
        assert!(level_pos < state_pos && state_pos < flow_pos);
        // The state caption lists the states; the level caption links the drawn
        // members to their cards and the view to its diagram page.
        assert!(page.contains("3 states: placed, paid, held"), "{}", page);
        assert!(page.contains("(./entities/checkout-api.md)"), "{}", page);
        assert!(!page.contains("](./checkout-api.md)"), "{}", page);
        assert!(
            page.contains(&format!(
                "[page](./diagrams/{}.md)",
                &level["view:".len()..]
            )),
            "{}",
            page
        );
        // The header links the card; the containment line keeps linking documents.
        assert!(page.contains("[card](./entities/order.md)"), "{}", page);
        assert!(
            page.contains("Parent: [Order Service](./order-service.md)"),
            "{}",
            page
        );
        assert!(
            page.contains(&format!(
                "[source](../diagrams/{}.puml)",
                &level["view:".len()..]
            )),
            "{}",
            page
        );
        // The rendered files exist, so the links resolve.
        assert!(out.join("diagrams/state/order.svg").exists());

        // The index embeds the structural level views as images (no mermaid), the
        // scope root's (view:component/public: shop is a system, customer an actor)
        // and the shop's, lists every view, and cross-links the pages.
        let idx = std::fs::read_to_string(out.join("docsgen/index.md")).unwrap();
        assert!(
            idx.contains("](../diagrams/component/public.svg)"),
            "{}",
            idx
        );
        assert!(!idx.contains("](../diagrams/class/public.svg)"), "{}", idx);
        assert!(idx.contains("](../diagrams/component/shop.svg)"), "{}", idx);
        assert!(!idx.contains("mermaid"), "{}", idx);
        assert!(idx.contains("- [Order](./order.md) `ent:order`"), "{}", idx);
        assert!(idx.contains("`view:usecase/customer-shop`"), "{}", idx);
        std::fs::remove_dir_all(&out).ok();
    }

    // The showcase graph's level pages: the scope root and every node with a level
    // view get a page under levels/, a leaf and a one-child node get none, the
    // breadcrumb walks the containment chain up to the root page, the members link
    // down, and the index links the root level page.
    // Mirrors docs/consumers/docsgen.md#level-pages.
    #[test]
    fn level_pages_nest_as_the_containment_tree() {
        let out = std::env::temp_dir().join(format!("jazyk-docsgen-levels-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        std::fs::create_dir_all(&out).unwrap();
        let mut s = crate::derive::tests::showcase_store();
        s.out = out.clone();
        let mut batch = crate::store::RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        // A stale page for a node that lost its level is pruned.
        std::fs::create_dir_all(out.join("docsgen/levels")).unwrap();
        std::fs::write(out.join("docsgen/levels/ghost.md"), "old").unwrap();
        write_all(&s, &gs(&out));
        let levels = out.join("docsgen/levels");

        // A page per level: the scope root, the shop (two services), the order
        // service (four children). No page for the inventory service (one child)
        // nor for a leaf.
        assert!(levels.join("scope-public.md").exists());
        assert!(levels.join("shop.md").exists());
        assert!(levels.join("order-service.md").exists());
        assert!(!levels.join("inventory-service.md").exists());
        assert!(!levels.join("checkout-api.md").exists());
        assert!(!levels.join("ghost.md").exists());

        // The order service's page: the breadcrumb chain root → shop → node (the node
        // unlinked), the header with the entity page link, the level view embedded
        // with paths one directory up, the members linked to their entity pages.
        let page = std::fs::read_to_string(levels.join("order-service.md")).unwrap();
        assert!(
            page.starts_with("[Public](./scope-public.md) › [Shop](./shop.md) › Order Service\n"),
            "{}",
            page
        );
        assert!(page.contains("# Order Service\n"), "{}", page);
        assert!(
            page.contains("[card](../entities/order-service.md) · [entity page](../order-service.md)"),
            "{}",
            page
        );
        let level = crate::derive::level_view_id(&s, "ent:order-service").unwrap();
        let rel = &level["view:".len()..];
        assert!(
            page.contains(&format!("](../../diagrams/{}.svg)", rel)),
            "{}",
            page
        );
        assert!(
            page.contains(&format!("[source](../../diagrams/{}.puml)", rel)),
            "{}",
            page
        );
        assert!(
            page.contains("## Members\n\n- [checkout API](../entities/checkout-api.md) · «interface»"),
            "{}",
            page
        );
        assert!(
            !page.contains("[level](./checkout-api.md)"),
            "a leaf has no link down: {}",
            page
        );

        // The shop's page links down to the order service's level page with its child
        // count and not to the inventory service's; the customer, an outside entity
        // the lifted edge brings into the view, is not a member.
        let shop = std::fs::read_to_string(levels.join("shop.md")).unwrap();
        assert!(
            shop.starts_with("[Public](./scope-public.md) › Shop\n"),
            "{}",
            shop
        );
        assert!(
            shop.contains("- [Order Service](../entities/order-service.md) · «service» · [level](./order-service.md) (4 children)"),
            "{}",
            shop
        );
        assert!(
            shop.contains("- [Inventory Service](../entities/inventory-service.md) · «service»\n"),
            "{}",
            shop
        );
        assert!(!shop.contains("- [Customer](../entities/customer.md)"), "{}", shop);
        assert!(
            shop.contains("](../../diagrams/component/shop.svg)"),
            "{}",
            shop
        );

        // The root page: unlinked breadcrumb, the scope's level view, the shop as a
        // member with its link down, the flow view lifted into the root level.
        let root = std::fs::read_to_string(levels.join("scope-public.md")).unwrap();
        assert!(
            root.starts_with("Public\n\n# Public\n\n`scope:public` · scope `public`"),
            "{}",
            root
        );
        assert!(
            root.contains("](../../diagrams/component/public.svg)"),
            "{}",
            root
        );
        assert!(
            root.contains("- [Shop](../entities/shop.md) · «system» · [level](./shop.md) (2 children)"),
            "{}",
            root
        );
        assert!(root.contains("`view:usecase/customer-shop`"), "{}", root);
        let structural = root.find("component/public.svg").unwrap();
        let flow = root.find("usecase/customer-shop").unwrap();
        assert!(
            structural < flow,
            "the structural view embeds first: {}",
            root
        );

        // The index links the root level page from the scope's caption.
        let idx = std::fs::read_to_string(out.join("docsgen/index.md")).unwrap();
        assert!(
            idx.contains("level page: [Public](./levels/scope-public.md)"),
            "{}",
            idx
        );
        std::fs::remove_dir_all(&out).ok();
    }

    // The showcase graph's walk: a card per entity under entities/ listing one level
    // in every direction with every entity link pointing at a card, and a page per
    // view under diagrams/<kind>/ with the level, the legend, the steps, and the
    // views around. Mirrors docs/consumers/docsgen.md#entity-cards and
    // docs/consumers/docsgen.md#diagram-pages.
    #[test]
    fn cards_and_diagram_pages_walk_the_graph() {
        let out = std::env::temp_dir().join(format!("jazyk-docsgen-walk-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        std::fs::create_dir_all(&out).unwrap();
        let mut s = crate::derive::tests::showcase_store();
        s.out = out.clone();
        let mut batch = crate::store::RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        write_all(&s, &gs(&out));
        let cards = out.join("docsgen/entities");
        let pages = out.join("docsgen/diagrams");

        // The order service's card: the header, the breadcrumb up (the scope root to
        // its level page, the shop to its card), the context and inside views embedded
        // two directories up with card links in the captions, the relationships,
        // the flow at the shop's level, the sibling, the children, the long reads.
        let card = std::fs::read_to_string(cards.join("order-service.md")).unwrap();
        assert!(
            card.starts_with("# Order Service\n\n`ent:order-service` · «service»"),
            "{}",
            card
        );
        assert!(
            card.contains("## Sits in\n\n[Public](../levels/scope-public.md) › [Shop](./shop.md) › Order Service\n"),
            "{}",
            card
        );
        let context = card.find("## In context").unwrap();
        let inside = card.find("## Inside").unwrap();
        assert!(context < inside);
        assert!(
            card[context..inside].contains("](../../diagrams/component/shop.svg)"),
            "{}",
            card
        );
        assert!(
            card[context..inside].contains("[Inventory Service](./inventory-service.md)"),
            "{}",
            card
        );
        assert!(
            card[context..inside].contains("· [page](../diagrams/component/shop.md)"),
            "{}",
            card
        );
        assert!(
            card[inside..].contains("](../../diagrams/component/order-service.svg)"),
            "{}",
            card
        );
        assert!(
            card.contains("- dependency → [stock API](./stock-api.md) · 1 requirement"),
            "{}",
            card
        );
        assert!(
            card.contains("(../diagrams/usecase/shop-customer-shop.md) `view:usecase/shop-customer-shop`"),
            "{}",
            card
        );
        assert!(
            card.contains("## Siblings\n\n- [Inventory Service](./inventory-service.md)\n"),
            "{}",
            card
        );
        assert!(card.contains("## Children\n\n"), "{}", card);
        assert!(card.contains("- [Order](./order.md)\n"), "{}", card);
        assert!(
            card.contains("- [Requirements document](../order-service.md) ("),
            "{}",
            card
        );
        assert!(
            card.contains("- Level page of [Shop](../levels/shop.md)\n"),
            "{}",
            card
        );
        assert!(
            card.contains("- Own level page: [Order Service](../levels/order-service.md)\n"),
            "{}",
            card
        );
        // No link on a card points at a requirements document except the long reads.
        assert!(!card.contains("](../order-service.md#"), "{}", card);
        assert!(!card.contains(".svg]]"), "{}", card);

        // A leaf's card: a leaf inside, no children, its context the parent's level.
        let leaf = std::fs::read_to_string(cards.join("order-item.md")).unwrap();
        assert!(leaf.contains("## Inside\n\na leaf\n"), "{}", leaf);
        assert!(!leaf.contains("## Children"), "{}", leaf);
        assert!(
            leaf.contains("](../../diagrams/component/order-service.svg)"),
            "{}",
            leaf
        );
        assert!(
            leaf.contains("[Public](../levels/scope-public.md) › [Shop](./shop.md) › [Order Service](./order-service.md) › Order Item\n"),
            "{}",
            leaf
        );

        // The shop's level view page: the level line ends in the level page, the image
        // sits three directories up, the legend links every drawn entity to its card
        // and the ones with a level to their level view's page, and the views around
        // are the shop's flow views, the root's view above, the order service's below.
        let page = std::fs::read_to_string(pages.join("component/shop.md")).unwrap();
        assert!(
            page.starts_with("# Shop\n\n`view:component/shop` · component · "),
            "{}",
            page
        );
        assert!(
            page.contains("## Level\n\n[Public](../../levels/scope-public.md) › [Shop](../../entities/shop.md) · level page: [Shop](../../levels/shop.md)\n"),
            "{}",
            page
        );
        assert!(
            page.contains("](../../../diagrams/component/shop.svg)"),
            "{}",
            page
        );
        assert!(
            page.contains("[source](../../../diagrams/component/shop.puml)"),
            "{}",
            page
        );
        assert!(
            page.contains("- [Order Service](../../entities/order-service.md) · «service» · [level below](./order-service.md)\n"),
            "{}",
            page
        );
        assert!(
            page.contains("- [Inventory Service](../../entities/inventory-service.md) · «service»\n"),
            "{}",
            page
        );
        assert!(!page.contains("## Steps"), "{}", page);
        assert!(
            page.contains("(../usecase/shop-customer-shop.md) `view:usecase/shop-customer-shop`"),
            "{}",
            page
        );
        assert!(
            page.contains("- Above: [Public](./public.md) `view:component/public`"),
            "{}",
            page
        );
        assert!(
            page.contains("- Below: [Order Service](./order-service.md) `view:component/order-service`"),
            "{}",
            page
        );

        // The shop level's sequence page: the steps as drawn, each id linked to its
        // block in a requirements document, the ends linked to their cards.
        let seq = std::fs::read_to_string(pages.join("sequence/shop-customer-shop.md")).unwrap();
        assert!(
            seq.contains("## Level\n\n[Public](../../levels/scope-public.md) › [Shop](../../entities/shop.md) · level page: [Shop](../../levels/shop.md)\n"),
            "{}",
            seq
        );
        assert!(
            seq.contains("## Steps\n\n- [`req:shop-1`](../../customer.md#reqshop-1) The customer submits the shopping cart through the checkout API. · [Customer](../../entities/customer.md) → [Order Service](../../entities/order-service.md)\n"),
            "{}",
            seq
        );
        assert!(
            seq.contains("- Same level: ") && seq.contains("`view:component/shop`"),
            "{}",
            seq
        );

        // The state view's page belongs to its machine, not to a level.
        let state = std::fs::read_to_string(pages.join("state/order.md")).unwrap();
        assert!(
            state.contains("## Level\n\nthe state machine of [Order](../../entities/order.md)\n"),
            "{}",
            state
        );
        std::fs::remove_dir_all(&out).ok();
    }

    // A pending ratification proposal renders as its prompt on the entity page, and
    // the index groups proposals by target document.
    // Mirrors docs/consumers/docsgen.md#ratification-proposals.
    #[test]
    fn ratification_proposal_renders_as_a_prompt() {
        let out = std::env::temp_dir().join(format!("jazyk-docsgen-ratify-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        s.graph.entities.insert(
            "ent:pricing".into(),
            Entity {
                name: "Pricing".into(),
                provenance: Some(Provenance::Derived {
                    from: vec!["req:orders-3".into()],
                    reasoning: "a separable concern".into(),
                }),
                ..Default::default()
            },
        );
        s.graph.diagnostics.insert(
            "diag:ratification-pending-1".into(),
            Diagnostic {
                rule: "ratification-pending".into(),
                severity: "warning".into(),
                subjects: vec!["ent:pricing".into()],
                message: "ent:pricing is derived and no document states it.".into(),
                reasoning: Some("req:orders-3 separates pricing".into()),
                lifecycle: "open".into(),
                triage: None,
                prompt: Some(DiagnosticPrompt {
                    question: "Should docs/orders.md state the pricing module?".into(),
                    options: vec![
                        PromptOption {
                            label: "Insert into docs/orders.md /orders/service".into(),
                            edit: Some(SuggestedEdit {
                                doc: "docs/orders.md".into(),
                                section: "/orders/service".into(),
                                old_text: String::new(),
                                new_text: "The order service contains a pricing module.".into(),
                            }),
                            answer: None,
                        },
                        PromptOption {
                            label: "Retract".into(),
                            edit: None,
                            answer: Some("retract".into()),
                        },
                    ],
                    freeform: true,
                }),
                answer: None,
                created: None,
                updated: None,
            },
        );
        write_all(&s, &gs(&out));
        let page = std::fs::read_to_string(out.join("docsgen/pricing.md")).unwrap();
        assert!(page.contains("## Proposals"), "{}", page);
        assert!(
            page.contains("> The order service contains a pricing module."),
            "{}",
            page
        );
        assert!(
            page.contains("Target: `docs/orders.md#/orders/service`"),
            "{}",
            page
        );
        assert!(page.contains("- Answer: Retract"), "{}", page);
        assert!(
            page.contains("This entity is derived"),
            "the header names the provenance: {}",
            page
        );
        let idx = std::fs::read_to_string(out.join("docsgen/index.md")).unwrap();
        assert!(idx.contains("## Ratification"), "{}", idx);
        assert!(idx.contains("### `docs/orders.md`"), "{}", idx);
        std::fs::remove_dir_all(&out).ok();
    }
}
