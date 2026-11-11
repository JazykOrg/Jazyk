// Paths and links between the pages, the embedded renderings, and the anchors and
// quotes the requirements documents and reports link through.
use crate::derive::view_edge_count;
use crate::ids::entity_slug;
use crate::model::{StateMachine, View};
use crate::store::Store;
use crate::vfs::VPathExt;

// The directories the pages live in, under `<out>/derived`, beside `diagrams/`. Every
// link is a relative path from the page's directory to the target's path under
// `<out>/derived`, so the folder serves anywhere as-is. Mirrors docs/consumers/docsgen.md#diagrams-on-entity-pages.
pub(super) const DOCS_DIR: &str = "docsgen";
pub(super) const LEVELS_DIR: &str = "docsgen/levels";
pub(super) const CARDS_DIR: &str = "docsgen/entities";

pub(super) fn view_pages_dir(kind: &str) -> String {
    format!("docsgen/diagrams/{}", kind)
}

// The relative path from a page in `from_dir` to `to`, both under `<out>`: `./x.md`
// for a sibling, `../` per directory left otherwise.
pub(super) fn rel(from_dir: &str, to: &str) -> String {
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
pub(super) fn doc_path(id: &str) -> String {
    format!("{}/{}.md", DOCS_DIR, entity_slug(id))
}

pub(super) fn card_path(id: &str) -> String {
    format!("{}/{}.md", CARDS_DIR, entity_slug(id))
}

// `(kind, slug)` of a view id: `view:usecase/checkout` → `("usecase", "checkout")`.
pub(super) fn view_parts(view_id: &str) -> Option<(&str, &str)> {
    view_id.strip_prefix("view:")?.split_once('/')
}

pub(super) fn view_page_path(view_id: &str) -> Option<String> {
    let (kind, s) = view_parts(view_id)?;
    Some(format!("{}/{}.md", view_pages_dir(kind), s))
}

// "[Name](<requirements document>)" for a live entity, the bare id otherwise.
pub(super) fn page_link(store: &Store, id: &str) -> String {
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
pub(super) fn card_link(store: &Store, id: &str, from: &str) -> String {
    let resolved = store.resolve_id(id);
    match store.graph.entities.get(resolved) {
        Some(e) => format!("[{}]({})", e.name, rel(from, &card_path(resolved))),
        None => format!("`{}`", id),
    }
}

// "[title](<diagram page>)" for a stored view, "[id](<diagram page>)" for one the
// store does not hold yet, the bare id when the id has no kind.
pub(super) fn view_page_link(store: &Store, view_id: &str, from: &str) -> String {
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
pub(super) fn diagram_rel(view_id: &str) -> Option<(String, String)> {
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
    crate::layout::diagrams_dir(&store.out)
        .join(kind)
        .join(format!("{}.svg", s))
        .vexists()
}

pub(super) const FLOW_KINDS: [&str; 4] = ["use-case", "activity", "sequence", "communication"];

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
pub(super) fn over_limit_goal(store: &Store, view_id: &str, view: &View) -> Option<String> {
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
pub(super) fn caption(store: &Store, view_id: &str, view: &View, from: &str) -> String {
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
pub(super) fn image(store: &Store, view_id: &str, view: &View, from: &str) -> String {
    match diagram_files(view_id, from) {
        Some((svg, _)) if svg_exists(store, view_id) => {
            format!("![{}]({})\n\n", view.title, svg)
        }
        _ => String::new(),
    }
}

// One embedded rendering on a page in `from`: the image, then the caption with the
// link to the view's diagram page.
pub(super) fn embed(store: &Store, view_id: &str, view: &View, from: &str) -> String {
    if view_parts(view_id).is_none() {
        return String::new();
    }
    let mut s = image(store, view_id, view, from);
    s.push_str(&caption(store, view_id, view, from));
    s.push_str(&format!(
        " · [page]({})",
        rel(from, &view_page_path(view_id).unwrap_or_default())
    ));
    s.push_str("\n\n");
    s
}

// The GitHub anchor of the heading "### `req:x`" in a requirements document: the
// backticks and the colon drop, the rest lowercases.
// GitHub's heading anchor: lowercased, punctuation dropped, spaces to hyphens, so a
// heading `main.ts` anchors as `maints`. Mirrors docs/consumers/docsgen.md#glossary.
pub(super) fn github_anchor(title: &str) -> String {
    let mut out = String::new();
    for c in title.trim().to_lowercase().chars() {
        if c.is_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else if c.is_whitespace() {
            out.push('-');
        }
    }
    out
}

// A quote's relative links resolve from the source document; on the page they must
// resolve from `docsgen/`, two levels under the project root when the out directory
// sits where the default puts it. Mirrors docs/consumers/docsgen.md#the-requirements-document.
pub(super) fn source_prefix(store: &Store) -> Option<&'static str> {
    if store.out.file_name().and_then(|f| f.to_str()) == Some(".jazyk") {
        Some("../../")
    } else if store
        .out
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|f| f.to_str())
        == Some(".jazyk")
    {
        Some("../../../")
    } else {
        None
    }
}

pub(super) fn relink_quote(quote: &str, doc: &str, store: &Store) -> String {
    let prefix = source_prefix(store);
    if prefix.is_none() || !quote.contains("](") {
        return quote.to_string();
    }
    let mut out = String::new();
    let mut rest = quote;
    while let Some(i) = rest.find("](") {
        out.push_str(&rest[..i + 2]);
        rest = &rest[i + 2..];
        let Some(close) = rest.find(')') else { break };
        let target = &rest[..close];
        let (path, anchor) = match target.find('#') {
            Some(h) => (&target[..h], &target[h..]),
            None => (target, ""),
        };
        let relative = !path.is_empty()
            && !path.contains("://")
            && !path.starts_with('/')
            && !path.starts_with("mailto:");
        if path.is_empty() && !anchor.is_empty() {
            // An anchor-only link points into the source document.
            out.push_str(&format!("{}{}{}", prefix.unwrap(), doc, anchor));
        } else {
            match (relative, crate::md::resolve_rel(doc, path)) {
                (true, Some(resolved)) => {
                    out.push_str(&format!("{}{}{}", prefix.unwrap(), resolved, anchor));
                }
                _ => out.push_str(target),
            }
        }
        rest = &rest[close..];
    }
    out.push_str(rest);
    out
}

fn heading_anchor(rid: &str) -> String {
    rid.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase()
}

// "[`req:x`](<document>#anchor)": the requirement's block in the document of the
// first entity it names, the bare id when it names none.
pub(super) fn requirement_link(store: &Store, rid: &str, from: &str) -> String {
    let doc = store.graph.requirements.get(rid).and_then(|r| {
        r.entities
            .iter()
            .map(|e| store.resolve_id(e).to_string())
            .find(|e| store.graph.entities.contains_key(e))
    });
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
