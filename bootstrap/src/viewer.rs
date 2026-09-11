// The viewer: render the graph store into one self-contained HTML file. Reads the same
// shards every frontend reads; never compiles. The walk of the docsgen pages (level
// pages, entity cards, diagram pages) runs inside the one file as anchors, and each
// view card embeds its rendering inline with the drill-down anchors rewritten to the
// cards. Mirrors docs/frontends/viewer.md.
use crate::card::{Crumb, Walk};
use crate::gen::GenSettings;
use crate::llm::truncate;
use crate::model::View;
use crate::store::Store;
use std::fmt::Write as _;

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

// A node id as a clickable link to its card.
fn link(id: &str) -> String {
    format!("<a class=\"id\" href=\"#n-{}\">{}</a>", esc(id), esc(id))
}

fn chip(severity: &str) -> String {
    format!(
        "<span class=\"chip sev-{}\">{}</span>",
        esc(severity),
        esc(severity)
    )
}

// Verification status chip. Classes group the seven statuses into four colors.
fn vchip(status: &str) -> String {
    let class = match status {
        "verified" => "v-ok",
        "failing" => "v-bad",
        s if s.starts_with("stale") => "v-stale",
        _ => "v-none",
    };
    format!("<span class=\"chip {}\">{}</span>", class, esc(status))
}

// The searchable text of a card, lowercased into a data attribute.
fn search_attr(parts: &[&str]) -> String {
    esc(&parts.join(" ").to_lowercase())
}

// ---- the walk: links between the cards and the level sections ----

// The anchor of a level section: `#l-<target>`.
fn level_anchor(target: &str) -> String {
    format!("#l-{}", esc(target))
}

// The name a level section shows: the node's name, or the scope's name.
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

// An entity's name linked to its card; the bare id when the store does not hold it.
fn name_link(store: &Store, id: &str) -> String {
    let resolved = store.resolve_id(id);
    match store.graph.entities.get(resolved) {
        Some(e) => format!(
            "<a class=\"n\" href=\"#n-{}\">{}</a>",
            esc(resolved),
            esc(&e.name)
        ),
        None => format!("<span class=\"q\">{}</span>", esc(id)),
    }
}

// A view's title linked to its card, its id beside it; the bare id when unstored.
fn view_link(store: &Store, vid: &str) -> String {
    match store.graph.views.get(vid) {
        Some(v) => format!(
            "<a class=\"n\" href=\"#n-{0}\">{1}</a> <span class=\"k\">{0}</span>",
            esc(vid),
            esc(&v.title)
        ),
        None => format!("<span class=\"id\">{}</span>", esc(vid)),
    }
}

// The target's name linked to its level section, when it holds a level.
fn level_link(store: &Store, target: &str) -> Option<String> {
    crate::derive::level_view_id(store, target)?;
    Some(format!(
        "<a class=\"n\" href=\"{}\">{}</a>",
        level_anchor(target),
        esc(&level_name(store, target))
    ))
}

// A breadcrumb as links: each ancestor to its card (the scope root to its level
// section), the last crumb unlinked unless `link_last`. With `to_levels`, an ancestor
// holding a level links to its level section instead of its card.
fn crumb_links(store: &Store, crumbs: &[Crumb], to_levels: bool, link_last: bool) -> String {
    let n = crumbs.len();
    crumbs
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i + 1 == n && !link_last {
                return esc(&c.name);
            }
            if crate::board::scope_target(&c.id).is_some() {
                return level_link(store, &c.id).unwrap_or_else(|| esc(&c.name));
            }
            if to_levels {
                if let Some(l) = level_link(store, &c.id) {
                    return l;
                }
            }
            name_link(store, &c.id)
        })
        .collect::<Vec<_>>()
        .join(" › ")
}

fn kind_of(view_id: &str) -> &str {
    view_id
        .strip_prefix("view:")
        .and_then(|r| r.split('/').next())
        .unwrap_or("view")
}

fn step_count(store: &Store, view_id: &str) -> usize {
    store
        .graph
        .views
        .get(view_id)
        .map(|v| v.members.len())
        .unwrap_or(0)
}

// One line per flow cluster: the title, the use case and sequence view cards as
// links, and the step count. A cluster is one line, never one per kind. The same
// grouping docsgen's cards use.
fn cluster_lines(store: &Store, view_ids: &[String]) -> Vec<String> {
    let mut clusters: Vec<(String, Vec<String>)> = Vec::new();
    for vid in view_ids {
        let slug = vid.rsplit('/').next().unwrap_or(vid).to_string();
        match clusters.iter_mut().find(|(s, _)| *s == slug) {
            Some((_, ids)) => ids.push(vid.clone()),
            None => clusters.push((slug, vec![vid.clone()])),
        }
    }
    clusters
        .into_iter()
        .map(|(_, mut ids)| {
            ids.sort_by_key(|v| match kind_of(v) {
                "usecase" => 0,
                "sequence" => 1,
                _ => 2,
            });
            let title = ids
                .iter()
                .find_map(|v| store.graph.views.get(v).map(|x| x.title.clone()))
                .unwrap_or_default();
            let kinds: Vec<String> = ids
                .iter()
                .map(|v| {
                    let label = match kind_of(v) {
                        "usecase" => "use case".to_string(),
                        k => k.replace('-', " "),
                    };
                    if store.graph.views.contains_key(v) {
                        format!("<a class=\"n\" href=\"#n-{}\">{}</a>", esc(v), esc(&label))
                    } else {
                        format!("<span class=\"id\">{}</span>", esc(v))
                    }
                })
                .collect();
            let steps = ids.iter().map(|v| step_count(store, v)).max().unwrap_or(0);
            format!("{} · {} ({} steps)", esc(&title), kinds.join(" · "), steps)
        })
        .collect()
}

// A list of lines as a compact bullet list, or `none`.
fn list(lines: &[String]) -> String {
    if lines.is_empty() {
        return "<span class=\"k\">none</span>".to_string();
    }
    let items: Vec<String> = lines.iter().map(|l| format!("<li>{}</li>", l)).collect();
    format!("<ul class=\"w\">{}</ul>", items.join(""))
}

// The structural level view of a target as stored: the derived id, or the same level
// under the other structural kind when a store from before a stereotype change still
// holds it (the next commit rewrites it). `false` when no shard holds it.
fn stored_level_view(store: &Store, target: &str) -> Option<(String, bool)> {
    let id = crate::derive::level_view_id(store, target)?;
    let sibling = match id.split_once('/') {
        Some(("view:class", slug)) => format!("view:component/{}", slug),
        Some(("view:component", slug)) => format!("view:class/{}", slug),
        _ => id.clone(),
    };
    Some(if store.graph.views.contains_key(&id) {
        (id, true)
    } else if store.graph.views.contains_key(&sibling) {
        (sibling, true)
    } else {
        (id, false)
    })
}

// The views of a level in the order a level section lists them: the structural view,
// then the flow views derived for the level (use case, then sequence), by id.
fn level_views(store: &Store, walk: &Walk, target: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some((id, true)) = stored_level_view(store, target) {
        out.push(id);
    }
    let mut flows: Vec<String> = walk
        .flow_levels
        .iter()
        .filter(|(_, t)| t.as_str() == target)
        .map(|(v, _)| v.clone())
        .collect();
    flows.sort_by(|a, b| {
        let rank = |v: &str| match kind_of(v) {
            "usecase" => 0,
            "sequence" => 1,
            _ => 2,
        };
        rank(a).cmp(&rank(b)).then(a.cmp(b))
    });
    out.extend(flows);
    out
}

// The direct children of a level in document order: the level view's members are
// the children first, then the outside entities the lifted edges bring in, and only
// the children are members of the section.
fn level_children(store: &Store, target: &str) -> Vec<String> {
    let direct: std::collections::BTreeSet<String> =
        crate::board::level_members(store, target).into_iter().collect();
    crate::derive::level_view_members(store, target)
        .into_iter()
        .filter(|m| direct.contains(m))
        .collect()
}

// Every target with a level section: the scope roots with a level view first, then
// every node with one, by id.
fn level_targets(store: &Store) -> Vec<String> {
    let scopes: std::collections::BTreeSet<&str> = store
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

// What a view with no level belongs to: its machine's subject, its instances' type,
// or nothing (a curated view).
fn belongs_to(store: &Store, view: &View) -> String {
    match view.kind.as_str() {
        "state" => match view.members.first() {
            Some(subject) => format!("the state machine of {}", name_link(store, subject)),
            None => "a state view with no subject".to_string(),
        },
        "object" => {
            let types = crate::derive::instance_types(store);
            match view
                .members
                .iter()
                .find_map(|m| types.get(store.resolve_id(m)))
            {
                Some(t) => format!("the object view of {}", name_link(store, t)),
                None => "an object view with no type".to_string(),
            }
        }
        _ => "a curated view; it belongs to no level".to_string(),
    }
}

// ---- the rendering, inline ----

// A rendering read from `<out>/diagrams/<kind>/<slug>.svg`, ready to sit inside the
// page: sanitized, scalable, its drill-down anchors pointing at the cards in this
// file. `width` is the rendering's own width in px, the figure's maximum.
struct InlineSvg {
    markup: String,
    width: Option<u32>,
}

struct SvgRules {
    pi: regex::Regex,
    doctype: regex::Regex,
    script: regex::Regex,
    script_empty: regex::Regex,
    foreign: regex::Regex,
    on_attr: regex::Regex,
    js_href: regex::Regex,
    size_attr: regex::Regex,
    width_attr: regex::Regex,
    view_box: regex::Regex,
    style_attr: regex::Regex,
    card_href: regex::Regex,
    diagram_href: regex::Regex,
}

fn svg_rules() -> &'static SvgRules {
    use regex::Regex;
    static RULES: std::sync::OnceLock<SvgRules> = std::sync::OnceLock::new();
    RULES.get_or_init(|| SvgRules {
        pi: Regex::new(r"(?s)<\?.*?\?>").unwrap(),
        doctype: Regex::new(r"(?is)<!DOCTYPE[^>]*>").unwrap(),
        script: Regex::new(r"(?is)<script\b[^>]*>.*?</script\s*>").unwrap(),
        script_empty: Regex::new(r"(?is)<script\b[^>]*/>").unwrap(),
        foreign: Regex::new(r"(?is)<foreignObject\b[^>]*>.*?</foreignObject\s*>").unwrap(),
        on_attr: Regex::new(r#"(?i)\s+on[a-z]+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)"#).unwrap(),
        js_href: Regex::new(r#"(?i)\s+(?:xlink:)?href\s*=\s*("\s*javascript:[^"]*"|'\s*javascript:[^']*')"#)
            .unwrap(),
        size_attr: Regex::new(r#"\s+(?:width|height)\s*=\s*"[^"]*""#).unwrap(),
        width_attr: Regex::new(r#"\swidth\s*=\s*"\s*([0-9.]+)\s*(?:px)?\s*""#).unwrap(),
        view_box: Regex::new(r#"\sviewBox\s*=\s*"\s*[0-9.-]+[\s,]+[0-9.-]+[\s,]+([0-9.]+)"#).unwrap(),
        style_attr: Regex::new(r#"\sstyle\s*=\s*"([^"]*)""#).unwrap(),
        card_href: Regex::new(r#"((?:xlink:)?href)="\.\./\.\./docsgen/entities/([^"/]+)\.md""#).unwrap(),
        diagram_href: Regex::new(r#"((?:xlink:)?href)="\.\./([^"/]+)/([^"/]+)\.svg""#).unwrap(),
    })
}

// Sanitizes one rendering for the page and rewrites its links to this file's
// anchors: `script` and `foreignObject` elements, `on*` attributes, `javascript:`
// links, the XML declaration and processing instructions stripped; the root's hard
// `width` and `height` dropped (the attributes and the same declarations in its
// `style`) with the `viewBox` kept; `../../docsgen/entities/<slug>.md` becomes
// `#n-ent:<slug>`, a collapsed node's `../<kind>/<slug>.svg` becomes
// `#n-view:<kind>/<slug>`. None when the text holds no `<svg` root. Mirrors
// docs/frontends/viewer.md#what-it-shows.
fn inline_svg(raw: &str) -> Option<InlineSvg> {
    let r = svg_rules();
    let start = raw.find("<svg")?;
    let mut s = raw[start..].to_string();
    s = r.pi.replace_all(&s, "").into_owned();
    s = r.doctype.replace_all(&s, "").into_owned();
    s = r.script.replace_all(&s, "").into_owned();
    s = r.script_empty.replace_all(&s, "").into_owned();
    s = r.foreign.replace_all(&s, "").into_owned();
    s = r.on_attr.replace_all(&s, "").into_owned();
    s = r.js_href.replace_all(&s, "").into_owned();
    // The root tag alone loses its size; a nested element keeps its own.
    let end = s.find('>')?;
    let tag = s[..=end].to_string();
    let width = r
        .width_attr
        .captures(&tag)
        .or_else(|| r.view_box.captures(&tag))
        .and_then(|c| c[1].parse::<f64>().ok())
        .map(|w| w.round() as u32);
    let mut root = r.size_attr.replace_all(&tag, "").into_owned();
    if let Some(c) = r.style_attr.captures(&root) {
        let kept: Vec<&str> = c[1]
            .split(';')
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .filter(|d| {
                let key = d.split(':').next().unwrap_or("").trim();
                key != "width" && key != "height"
            })
            .collect();
        let whole = c[0].to_string();
        let replacement = if kept.is_empty() {
            String::new()
        } else {
            format!(" style=\"{};\"", kept.join(";"))
        };
        root = root.replacen(&whole, &replacement, 1);
    }
    s.replace_range(..=end, &root);
    s = r.card_href.replace_all(&s, "$1=\"#n-ent:$2\"").into_owned();
    s = r
        .diagram_href
        .replace_all(&s, "$1=\"#n-view:$2/$3\"")
        .into_owned();
    Some(InlineSvg { markup: s, width })
}

// The `.svg` of a view under the out directory, when it exists: `(kind, slug)` and
// the file.
fn svg_path(store: &Store, view_id: &str) -> Option<(String, String, std::path::PathBuf)> {
    let (kind, slug) = view_id.strip_prefix("view:")?.split_once('/')?;
    let path = store
        .out
        .join("diagrams")
        .join(kind)
        .join(format!("{}.svg", slug));
    path.exists()
        .then(|| (kind.to_string(), slug.to_string(), path))
}

// The figure a view card embeds: the sanitized rendering, capped at its own width.
fn figure(store: &Store, view_id: &str) -> Option<String> {
    let (_, _, path) = svg_path(store, view_id)?;
    let raw = std::fs::read_to_string(path).ok()?;
    let svg = inline_svg(&raw)?;
    let cap = svg
        .width
        .map(|w| format!(" style=\"max-width:{}px\"", w))
        .unwrap_or_default();
    Some(format!("<figure class=\"dia\"{}>{}</figure>", cap, svg.markup))
}

// The containment tree the `parent` field makes, one root per scope printed as
// `scope:<scope>`, every node indented per depth. A node with a level prints its child
// count and its level view ids (the structural view, then the flow views lifted into
// the level), each linked to its view card; a leaf prints its name alone. Reading down
// the tree is reading down the levels. Mirrors docs/frontends/viewer.md#what-it-shows.
fn tree(store: &Store) -> String {
    use std::collections::{BTreeMap, BTreeSet};
    let g = &store.graph;

    // Children per target: an entity whose parent is not a live entity hangs off its
    // scope root, so nothing on disk goes unprinted.
    let mut by_parent: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut scopes: BTreeSet<String> = BTreeSet::new();
    for (id, e) in &g.entities {
        scopes.insert(e.scope.clone());
        let key = e
            .parent
            .as_deref()
            .map(|p| store.resolve_id(p))
            .filter(|p| g.entities.contains_key(*p))
            .map(str::to_string)
            .unwrap_or_else(|| crate::store::scope_root_target(&e.scope));
        by_parent.entry(key).or_default().push(id.clone());
    }
    // Children in the order the level view lists them (document order); the level
    // members start with the children, so their index orders the list.
    for (target, children) in by_parent.iter_mut() {
        let members = crate::derive::level_view_members(store, target);
        children.sort_by_key(|c| {
            (
                members.iter().position(|m| m == c).unwrap_or(usize::MAX),
                c.clone(),
            )
        });
    }
    // The flow views of each level, by the level they were derived for.
    let mut flows: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (vid, v) in &g.views {
        if !crate::derive::FLOW_KINDS.contains(&v.kind.as_str()) {
            continue;
        }
        if let Some(level) = crate::derive::flow_view_level(store, vid) {
            flows.entry(level).or_default().push(vid.clone());
        }
    }

    fn line(
        store: &Store,
        target: &str,
        name: &str,
        depth: usize,
        by_parent: &BTreeMap<String, Vec<String>>,
        flows: &BTreeMap<String, Vec<String>>,
        seen: &mut BTreeSet<String>,
        out: &mut String,
    ) {
        if !seen.insert(target.to_string()) {
            return;
        }
        // The scope root prints its address; an entity prints its name linked to its
        // card.
        let label = match crate::board::scope_target(target) {
            Some(_) => format!("<span class=\"root\">{}</span>", esc(target)),
            None => format!(
                "<a class=\"n\" href=\"#n-{}\">{}</a>",
                esc(target),
                esc(name)
            ),
        };
        let children = by_parent.get(target).map(Vec::as_slice).unwrap_or(&[]);
        // The structural view as stored; a level view no shard holds prints its id
        // without a link.
        let structural = stored_level_view(store, target);
        let mut views: Vec<(String, bool)> = structural.clone().into_iter().collect();
        views.extend(
            flows
                .get(target)
                .into_iter()
                .flatten()
                .map(|v| (v.clone(), true)),
        );
        // The child count links to the node's level section.
        let count = match structural {
            Some(_) => format!(
                " <a class=\"k\" href=\"{}\">{} children</a>",
                level_anchor(target),
                children.len()
            ),
            None => String::new(),
        };
        let ids: Vec<String> = views
            .iter()
            .map(|(v, stored)| {
                if *stored {
                    link(v)
                } else {
                    format!("<span class=\"id\">{}</span>", esc(v))
                }
            })
            .collect();
        let mut s: Vec<&str> = vec![target, name];
        s.extend(views.iter().map(|(v, _)| v.as_str()));
        let _ = write!(
            out,
            "<div class=\"tn\" data-s=\"{}\" style=\"padding-left:{}px\">{}{}{}{}</div>\n",
            search_attr(&s),
            depth * 20,
            label,
            count,
            if ids.is_empty() { "" } else { " " },
            ids.join(" ")
        );
        for c in children {
            let name = &store.graph.entities[c].name;
            line(store, c, name, depth + 1, by_parent, flows, seen, out);
        }
    }

    let mut out = String::new();
    let mut seen = BTreeSet::new();
    for scope in &scopes {
        let target = crate::store::scope_root_target(scope);
        line(
            store, &target, &target, 0, &by_parent, &flows, &mut seen, &mut out,
        );
    }
    out
}

const STYLE: &str = "
:root { --ink:#1d2523; --muted:#5b6763; --line:#dde3e0; --accent:#0e7a6d;
  --err:#c24333; --warn:#a8731c; --info:#2a6fa8; --none:#7a827f; }
* { box-sizing: border-box; }
body { font-family: -apple-system, 'Segoe UI', 'Helvetica Neue', sans-serif;
  color: var(--ink); background: #f7f8f7; margin: 0; line-height: 1.5; font-size: 15px; }
.wrap { max-width: 1020px; margin: 0 auto; padding: 28px 24px 80px; }
h1 { font-size: 26px; margin: 0 0 4px; }
h2 { font-size: 18px; margin: 36px 0 10px; border-bottom: 2px solid var(--ink); padding-bottom: 4px; }
.stats { font-family: ui-monospace, Menlo, monospace; font-size: 12.5px; color: var(--muted); margin: 0 0 18px; }
input#q { width: 100%; padding: 9px 12px; font-size: 14px; border: 1.5px solid var(--ink);
  border-radius: 4px; background: #fff; }
input#q:focus { outline: 2px solid var(--accent); }
.card { background: #fff; border: 1px solid var(--line); border-radius: 5px;
  padding: 10px 14px; margin: 8px 0; }
.card h3 { margin: 0 0 4px; font-size: 14px; font-family: ui-monospace, Menlo, monospace; }
.card p { margin: 3px 0; font-size: 13.5px; }
.k { color: var(--muted); font-size: 12px; font-family: ui-monospace, Menlo, monospace; }
.q { font-family: ui-monospace, Menlo, monospace; font-size: 12px; color: var(--muted); }
.id { font-family: ui-monospace, Menlo, monospace; font-size: 12px; color: var(--accent);
  text-decoration: none; }
.id:hover, .id:focus-visible { text-decoration: underline; }
.chip { display: inline-block; font-family: ui-monospace, Menlo, monospace; font-size: 10.5px;
  padding: 1px 8px; border-radius: 9px; border: 1px solid currentColor; margin-right: 6px; }
.sev-error { color: var(--err); } .sev-warning { color: var(--warn); }
.sev-info { color: var(--info); } .sev-none { color: var(--none); }
.v-ok { color: var(--accent); } .v-bad { color: var(--err); }
.v-stale { color: var(--warn); } .v-none { color: var(--none); }
.card.agg-ok { border-left: 4px solid var(--accent); }
.card.agg-bad { border-left: 4px solid var(--err); }
.card.agg-stale { border-left: 4px solid var(--warn); }
.card.agg-none { border-left: 4px solid var(--line); }
table { border-collapse: collapse; width: 100%; font-size: 13.5px; background: #fff; }
th { text-align: left; font-family: ui-monospace, Menlo, monospace; font-size: 11px;
  text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted);
  border-bottom: 1.5px solid var(--ink); padding: 6px 10px; }
td { border-bottom: 1px solid var(--line); padding: 6px 10px; }
td.num { text-align: right; font-variant-numeric: tabular-nums;
  font-family: ui-monospace, Menlo, monospace; }
.tree { background: #fff; border: 1px solid var(--line); border-radius: 5px;
  padding: 10px 14px; margin: 8px 0; font-family: ui-monospace, Menlo, monospace;
  font-size: 12.5px; overflow-x: auto; }
.tn { white-space: nowrap; line-height: 1.7; }
.tn .root { color: var(--muted); }
.tn .n { color: var(--ink); text-decoration: none; }
.tn .n:hover, .tn .n:focus-visible { text-decoration: underline; }
.tn .id { margin-left: 6px; }
.tn a.k { text-decoration: none; }
.tn a.k:hover, .tn a.k:focus-visible { text-decoration: underline; }
.card a.n { color: var(--accent); text-decoration: none; }
.card a.n:hover, .card a.n:focus-visible { text-decoration: underline; }
.card p.crumbs { font-size: 13px; color: var(--muted); }
ul.w { margin: 2px 0 6px; padding-left: 22px; font-size: 13.5px; }
ul.w li { margin: 1px 0; }
.dia { margin: 10px 0; padding: 0; overflow-x: auto; }
.dia svg { display: block; width: 100%; height: auto; }
:target { outline: 2px solid var(--accent); outline-offset: 2px; }
";

const SCRIPT: &str = "
const q = document.getElementById('q');
q.addEventListener('input', () => {
  const needle = q.value.trim().toLowerCase();
  for (const card of document.querySelectorAll('[data-s]')) {
    card.style.display = !needle || card.dataset.s.includes(needle) ? '' : 'none';
  }
});
";

pub fn render(store: &Store, gs: &GenSettings) -> String {
    let g = &store.graph;
    let vmap = crate::verify::status_map(store, gs);
    let walk = Walk::new(store);
    let mut h = String::with_capacity(64 * 1024);

    // Header stats mirror `jazyk status`.
    let (mut errs, mut warns, mut infos, mut nones) = (0usize, 0usize, 0usize, 0usize);
    for d in g.diagnostics.values() {
        if d.lifecycle == "open" {
            match d.severity.as_str() {
                "error" => errs += 1,
                "warning" => warns += 1,
                "info" => infos += 1,
                _ => nones += 1,
            }
        }
    }
    let (mut total_secs, mut covered_secs) = (0usize, 0usize);
    for rec in store.docs.values() {
        for (r, sec) in &rec.sections {
            if sec.raw.lines().skip(1).all(|l| l.trim().is_empty()) {
                continue;
            }
            total_secs += 1;
            if rec.coverage.contains_key(r) {
                covered_secs += 1;
            }
        }
    }

    h.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    h.push_str("<title>Jazyk graph</title>\n<style>");
    h.push_str(STYLE);
    h.push_str("</style>\n</head>\n<body>\n<div class=\"wrap\">\n");
    h.push_str("<h1>Jazyk graph</h1>\n");
    let _ = write!(
        h,
        "<p class=\"stats\">{} entities · {} requirements · {} relationships · open diagnostics: {} error, {} warning, {} info, {} none · coverage {}/{} sections · generation {}</p>\n",
        g.entities.len(),
        g.requirements.len(),
        g.relationships.len(),
        errs,
        warns,
        infos,
        nones,
        covered_secs,
        total_secs,
        store.status.generation
    );
    // Verification summary, when any ledger row exists.
    {
        let (mut ok, mut bad, mut stale, mut unv, mut not_gen) =
            (0usize, 0usize, 0usize, 0usize, 0usize);
        for v in vmap.values() {
            match v["status"].as_str().unwrap_or("") {
                "verified" => ok += 1,
                "failing" => bad += 1,
                s if s.starts_with("stale") => stale += 1,
                "unverified" => unv += 1,
                _ => not_gen += 1,
            }
        }
        if ok + bad + stale + unv > 0 {
            let _ = write!(
                h,
                "<p class=\"stats\">verification: <span class=\"v-ok\">{} verified</span> · <span class=\"v-bad\">{} failing</span> · <span class=\"v-stale\">{} stale</span> · {} unverified · {} not generated</p>\n",
                ok, bad, stale, unv, not_gen
            );
        }
    }
    h.push_str("<input id=\"q\" type=\"search\" placeholder=\"Filter by id, name, or text\" aria-label=\"Filter\">\n");

    // The tree: the drill-down, one root per scope, level view ids beside each node.
    h.push_str("<h2>Tree</h2>\n");
    if g.entities.is_empty() {
        h.push_str("<p class=\"k\">none</p>\n");
    } else {
        let _ = write!(h, "<div class=\"tree\">\n{}</div>\n", tree(store));
    }

    // Levels: one anchored section per level, the file's form of the docsgen level
    // page: the breadcrumb, the header, the views as links, the members.
    h.push_str("<h2>Levels</h2>\n");
    let targets = level_targets(store);
    if targets.is_empty() {
        h.push_str("<p class=\"k\">none</p>\n");
    }
    for target in &targets {
        let mut body = String::new();
        let name = level_name(store, target);
        let _ = write!(
            body,
            "<h3 id=\"l-{}\">{} <span class=\"k\">{}</span></h3>",
            esc(target),
            esc(&name),
            esc(target)
        );
        let crumbs: Vec<Crumb> = match crate::board::scope_target(target) {
            Some(_) => vec![Crumb {
                id: target.clone(),
                name: name.clone(),
            }],
            None => crate::card::breadcrumb(store, target),
        };
        let _ = write!(
            body,
            "<p class=\"crumbs\">{}</p>",
            crumb_links(store, &crumbs, true, false)
        );
        match crate::board::scope_target(target) {
            Some(scope) => {
                let _ = write!(body, "<p><span class=\"k\">scope</span> {}</p>", esc(scope));
            }
            None => {
                let ent = &g.entities[target];
                let mut head = String::new();
                if let Some(st) = &ent.stereotype {
                    let _ = write!(head, "«{}» ", esc(st));
                }
                if let Some(d) = &ent.definition {
                    head.push_str(&esc(d));
                    head.push(' ');
                }
                let _ = write!(head, "· {}", name_link(store, target));
                let _ = write!(body, "<p>{}</p>", head);
                if let Some(p) = &ent.provenance {
                    let _ = write!(body, "<p class=\"q\">this entity is {}</p>", esc(p.kind()));
                }
            }
        }
        let views: Vec<String> = level_views(store, &walk, target)
            .iter()
            .map(|v| view_link(store, v))
            .collect();
        let _ = write!(
            body,
            "<p><span class=\"k\">diagrams</span> {}</p>",
            if views.is_empty() {
                "<span class=\"k\">none stored</span>".to_string()
            } else {
                views.join(" · ")
            }
        );
        let children = level_children(store, target);
        let members: Vec<String> = children
            .iter()
            .filter_map(|c| {
                let e = g.entities.get(c)?;
                let mut line = name_link(store, c);
                if let Some(st) = &e.stereotype {
                    let _ = write!(line, " «{}»", esc(st));
                }
                if let Some(d) = &e.definition {
                    let _ = write!(line, " · {}", esc(d));
                }
                if crate::derive::level_view_id(store, c).is_some() {
                    let _ = write!(
                        line,
                        " · <a class=\"n\" href=\"{}\">level</a> <span class=\"k\">({} children)</span>",
                        level_anchor(c),
                        crate::board::level_members(store, c).len()
                    );
                }
                Some(line)
            })
            .collect();
        let _ = write!(body, "<p><span class=\"k\">members</span></p>{}", list(&members));
        let mut s: Vec<&str> = vec![target, &name];
        let member_names: Vec<&str> = children
            .iter()
            .filter_map(|c| g.entities.get(c).map(|e| e.name.as_str()))
            .collect();
        s.extend(member_names);
        let _ = write!(
            h,
            "<div class=\"card\" data-s=\"{}\">{}</div>\n",
            search_attr(&s),
            body
        );
    }

    // Entities.
    h.push_str("<h2>Entities</h2>\n");
    for (id, e) in &g.entities {
        let refs = store.requirements_referencing(id);
        let mut body = String::new();
        let _ = write!(
            body,
            "<h3 id=\"n-{}\">{} <span class=\"k\">{}</span></h3>",
            esc(id),
            esc(&e.name),
            esc(id)
        );
        if e.scope != "public" {
            let _ = write!(
                body,
                "<p><span class=\"k\">scope</span> {}</p>",
                esc(&e.scope)
            );
        }
        if let Some(d) = &e.definition {
            let _ = write!(body, "<p>{}</p>", esc(d));
        }
        if !e.aliases.is_empty() {
            let _ = write!(
                body,
                "<p><span class=\"k\">aliases</span> {}</p>",
                esc(&e.aliases.join(", "))
            );
        }
        for m in &e.mentions {
            let _ = write!(
                body,
                "<p class=\"q\">{}#{} \u{201c}{}\u{201d}</p>",
                esc(&m.doc),
                esc(&m.section),
                esc(&truncate(&m.quote, 160))
            );
        }
        if !refs.is_empty() {
            let links: Vec<String> = refs.iter().map(|r| link(r)).collect();
            let _ = write!(
                body,
                "<p><span class=\"k\">requirements</span> {}</p>",
                links.join(" ")
            );
        }
        // The card's sections, one level in every direction, every link an anchor
        // to another card in this file. Mirrors docs/frontends/viewer.md#what-it-shows.
        if let Some(c) = crate::card::entity_card(store, &walk, id) {
            let _ = write!(
                body,
                "<p><span class=\"k\">sits in</span> {}</p>",
                crumb_links(store, &c.breadcrumb, false, false)
            );
            let context = match c.context.as_deref().filter(|v| g.views.contains_key(*v)) {
                Some(v) => view_link(store, v),
                None => "<span class=\"k\">no level view: the level above holds this entity alone</span>".to_string(),
            };
            let _ = write!(body, "<p><span class=\"k\">in context</span> {}</p>", context);
            match c.inside.as_deref().filter(|v| g.views.contains_key(*v)) {
                Some(v) => {
                    let mut inside = view_link(store, v);
                    let flows = cluster_lines(store, &c.inside_flows);
                    if !flows.is_empty() {
                        let _ = write!(inside, " · flows of this level: {}", list(&flows));
                    }
                    let _ = write!(body, "<p><span class=\"k\">inside</span> {}</p>", inside);
                }
                None => body.push_str("<p><span class=\"k\">inside</span> <span class=\"k\">a leaf</span></p>"),
            }
            let rels: Vec<String> = c
                .relationships
                .iter()
                .map(|r| {
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
                    format!(
                        "{} {} {} <span class=\"k\">· {}</span>",
                        esc(&r.r#type),
                        arrow,
                        name_link(store, &r.other),
                        n
                    )
                })
                .collect();
            let _ = write!(body, "<p><span class=\"k\">relationships</span> {}</p>", list(&rels));
            let _ = write!(
                body,
                "<p><span class=\"k\">flows</span> {}</p>",
                list(&cluster_lines(store, &c.flows))
            );
            let kin = |k: &crate::card::Kin| {
                let mut line = name_link(store, &k.id);
                if k.child_count >= 2 {
                    let _ = write!(line, " <span class=\"k\">({} children)</span>", k.child_count);
                }
                line
            };
            let siblings: Vec<String> = c.siblings.iter().map(kin).collect();
            let _ = write!(body, "<p><span class=\"k\">siblings</span> {}</p>", list(&siblings));
            if !c.children.is_empty() {
                let children: Vec<String> = c.children.iter().map(kin).collect();
                let _ = write!(body, "<p><span class=\"k\">children</span> {}</p>", list(&children));
            }
            let mut levels: Vec<String> = Vec::new();
            if let Some(parent) = c.breadcrumb.iter().rev().nth(1) {
                if let Some(l) = level_link(store, &parent.id) {
                    levels.push(format!("level of {}", l));
                }
            }
            if c.inside.is_some() {
                if let Some(l) = level_link(store, id) {
                    levels.push(format!("own level {}", l));
                }
            }
            if !levels.is_empty() {
                let _ = write!(
                    body,
                    "<p><span class=\"k\">levels</span> {}</p>",
                    levels.join(" · ")
                );
            }
        }
        // Aggregate verification over the entity's requirements: any failing reads
        // red, any stale amber, all verified green, none generated gray.
        let agg = {
            let statuses: Vec<&str> = refs
                .iter()
                .filter_map(|r| vmap.get(r).and_then(|v| v["status"].as_str()))
                .collect();
            if statuses.iter().any(|s| *s == "failing") {
                "agg-bad"
            } else if statuses.iter().any(|s| s.starts_with("stale")) {
                "agg-stale"
            } else if !statuses.is_empty() && statuses.iter().all(|s| *s == "verified") {
                "agg-ok"
            } else {
                "agg-none"
            }
        };
        let s = search_attr(&[
            id,
            &e.name,
            e.definition.as_deref().unwrap_or(""),
            &e.aliases.join(" "),
        ]);
        let _ = write!(
            h,
            "<div class=\"card {}\" data-s=\"{}\">{}</div>\n",
            agg, s, body
        );
    }

    // Requirements.
    h.push_str("<h2>Requirements</h2>\n");
    for (id, r) in &g.requirements {
        let mut body = String::new();
        let _ = write!(body, "<h3 id=\"n-{}\">{}</h3>", esc(id), esc(id));
        let _ = write!(body, "<p>{}</p>", esc(&r.statement));
        if let Some(v) = vmap.get(id) {
            let status = v["status"].as_str().unwrap_or("missing");
            let mut line = vchip(status);
            if let Some(k) = v["kind"].as_str() {
                let _ = write!(line, " <span class=\"k\">{}</span>", esc(k));
            }
            if let Some(run) = v["run"].as_str() {
                let _ = write!(line, " <span class=\"q\">{}</span>", esc(run));
            }
            if let Some(ev) = v["evidence"].as_str() {
                let _ = write!(
                    line,
                    "<br><span class=\"q\">{}</span>",
                    esc(&truncate(ev, 140))
                );
            }
            let _ = write!(body, "<p>{}</p>", line);
        }
        let links: Vec<String> = r.entities.iter().map(|e| link(e)).collect();
        let _ = write!(
            body,
            "<p><span class=\"k\">entities</span> {}</p>",
            links.join(" ")
        );
        match r.source.as_ref() {
            Some(src) => {
                let _ = write!(
                    body,
                    "<p class=\"q\">{}#{} \u{201c}{}\u{201d}</p>",
                    esc(&src.doc),
                    esc(&src.section),
                    esc(&truncate(&src.quote, 160))
                );
            }
            None => {
                let _ = write!(
                    body,
                    "<p class=\"q\">{}</p>",
                    esc(&crate::session::provenance_line(r))
                );
            }
        }
        if !r.edges.is_empty() {
            let edges: Vec<String> = r
                .edges
                .iter()
                .map(|e| {
                    let t = e
                        .rel_type
                        .as_deref()
                        .unwrap_or(crate::model::DEFAULT_REL_TYPE);
                    match &e.cardinality {
                        Some(c) => {
                            format!("{} → {} ({}, {})", esc(&e.a), esc(&e.b), esc(t), esc(c))
                        }
                        None => format!("{} → {} ({})", esc(&e.a), esc(&e.b), esc(t)),
                    }
                })
                .collect();
            let _ = write!(
                body,
                "<p><span class=\"k\">edges</span> {}</p>",
                edges.join(", ")
            );
        }
        if !r.facets.is_empty() {
            let facets: Vec<String> = r
                .facets
                .iter()
                .map(|f| match &f.measure {
                    Some(m) => format!("{} ({})", f.facet, m),
                    None => f.facet.clone(),
                })
                .collect();
            let _ = write!(
                body,
                "<p><span class=\"k\">facets</span> {}</p>",
                esc(&facets.join(", "))
            );
        }
        if let Some(t) = &r.transition {
            let mut tl = format!("{}: {} → {}", t.subject, t.from, t.to);
            if let Some(tr) = &t.trigger {
                tl.push_str(&format!(" on {}", tr));
            }
            if let Some(gu) = &t.guard {
                tl.push_str(&format!(" if {}", gu));
            }
            let _ = write!(
                body,
                "<p><span class=\"k\">transition</span> {}</p>",
                esc(&tl)
            );
        }
        let doc = r
            .source
            .as_ref()
            .map(|s| s.doc.as_str())
            .unwrap_or_default();
        let s = search_attr(&[id, &r.statement, doc]);
        let _ = write!(h, "<div class=\"card\" data-s=\"{}\">{}</div>\n", s, body);
    }

    // Relationships.
    h.push_str("<h2>Relationships</h2>\n");
    if g.relationships.is_empty() {
        h.push_str("<p class=\"k\">none derived</p>\n");
    }
    for (id, rel) in &g.relationships {
        let members: Vec<String> = rel.members.iter().map(|m| link(m)).collect();
        // Each contribution group as stored: direction, type, cardinality, and the
        // contributing requirement ids. Mirrors docs/frontends/viewer.md#what-it-shows.
        let mut groups = String::new();
        for c in &rel.contributions {
            let card = c
                .cardinality
                .as_ref()
                .map(|x| format!(", {}", x))
                .unwrap_or_default();
            let reqs: Vec<String> = c.requirements.iter().map(|r| link(r)).collect();
            let _ = write!(
                groups,
                "<p class=\"q\">{} → {} ({}{}) · {}</p>",
                esc(&c.a),
                esc(&c.b),
                esc(&c.r#type),
                esc(&card),
                reqs.join(" ")
            );
        }
        let s = search_attr(&[id, rel.strongest(), &rel.members.join(" ")]);
        let _ = write!(
            h,
            "<div class=\"card\" data-s=\"{}\"><h3 id=\"n-{}\">{} <span class=\"k\">{}</span></h3><p><span class=\"k\">members</span> {}</p>{}</div>\n",
            s,
            esc(id),
            esc(id),
            esc(rel.strongest()),
            members.join(" "),
            groups
        );
    }

    // Views: the stored half of each diagram, a link to its rendering when the .svg
    // exists beside this file's default location under the out directory, and the
    // diagram page: the level, the rendering inline, the legend, the steps, the views
    // around. Mirrors docs/frontends/viewer.md#what-it-shows.
    h.push_str("<h2>Views</h2>\n");
    if g.views.is_empty() {
        h.push_str("<p class=\"k\">none</p>\n");
    }
    for (id, v) in &g.views {
        let members: Vec<String> = v.members.iter().map(|m| link(m)).collect();
        let svg_link = svg_path(store, id)
            .map(|(kind, slug, _)| {
                format!(
                    " · <a class=\"id\" href=\"diagrams/{}/{}.svg\">svg</a>",
                    esc(&kind),
                    esc(&slug)
                )
            })
            .unwrap_or_default();
        let mut body = String::new();
        let _ = write!(
            body,
            "<h3 id=\"n-{}\">{} <span class=\"k\">{} · {}</span>{}</h3><p><span class=\"k\">members</span> {}</p>",
            esc(id),
            esc(&v.title),
            esc(id),
            esc(&v.kind),
            svg_link,
            members.join(" ")
        );
        let mut drawn_names: Vec<String> = Vec::new();
        if let Some(p) = crate::card::view_page(store, &walk, id) {
            let level = match &p.level {
                Some(l) => {
                    let mut line = crumb_links(store, &l.breadcrumb, false, true);
                    // A node's chain ends in its card; the level section follows.
                    // The scope root's chain is its level section already.
                    if crate::board::scope_target(&l.target).is_none() {
                        if let Some(link) = level_link(store, &l.target) {
                            let _ = write!(line, " · level: {}", link);
                        }
                    }
                    line
                }
                None => belongs_to(store, v),
            };
            let _ = write!(body, "<p><span class=\"k\">level</span> {}</p>", level);
            if let Some(fig) = figure(store, id) {
                body.push_str(&fig);
            }
            let drawn: Vec<String> = p
                .drawn
                .iter()
                .map(|d| {
                    drawn_names.push(d.name.clone());
                    let mut line = name_link(store, &d.id);
                    if let Some(st) = &d.stereotype {
                        let _ = write!(line, " «{}»", esc(st));
                    }
                    if let Some(lv) = &d.level_view {
                        if g.views.contains_key(lv) {
                            let _ = write!(
                                line,
                                " · <a class=\"n\" href=\"#n-{}\">level below</a>",
                                esc(lv)
                            );
                        }
                    }
                    line
                })
                .collect();
            let _ = write!(body, "<p><span class=\"k\">drawn</span> {}</p>", list(&drawn));
            if crate::derive::FLOW_KINDS.contains(&p.kind.as_str()) {
                let steps: Vec<String> = p
                    .steps
                    .iter()
                    .map(|st| {
                        format!(
                            "{} {} <span class=\"k\">·</span> {} → {}",
                            link(&st.requirement),
                            esc(&st.statement),
                            name_link(store, &st.from),
                            name_link(store, &st.to)
                        )
                    })
                    .collect();
                let _ = write!(body, "<p><span class=\"k\">steps</span> {}</p>", list(&steps));
            }
            let views = |ids: &[String]| -> String {
                ids.iter()
                    .map(|x| view_link(store, x))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let mut around: Vec<String> = Vec::new();
            if !p.around.same_level.is_empty() {
                around.push(format!("same level: {}", views(&p.around.same_level)));
            }
            if let Some(a) = &p.around.above {
                around.push(format!("above: {}", views(std::slice::from_ref(a))));
            }
            if !p.around.below.is_empty() {
                around.push(format!("below: {}", views(&p.around.below)));
            }
            let _ = write!(body, "<p><span class=\"k\">around</span> {}</p>", list(&around));
        }
        let mut s: Vec<&str> = vec![id, &v.kind, &v.title];
        s.extend(drawn_names.iter().map(String::as_str));
        let _ = write!(
            h,
            "<div class=\"card\" data-s=\"{}\">{}</div>\n",
            search_attr(&s),
            body
        );
    }

    // Diagnostics.
    h.push_str("<h2>Diagnostics</h2>\n");
    if g.diagnostics.is_empty() {
        h.push_str("<p class=\"k\">none</p>\n");
    }
    for (id, d) in &g.diagnostics {
        let subjects: Vec<String> = d
            .subjects
            .iter()
            .map(|sj| {
                let resolved = store.resolve_id(sj);
                if g.entities.contains_key(resolved) || g.requirements.contains_key(resolved) {
                    link(resolved)
                } else {
                    format!("<span class=\"q\">{}</span>", esc(sj))
                }
            })
            .collect();
        let mut body = String::new();
        let _ = write!(
            body,
            "<h3 id=\"n-{}\">{}{} <span class=\"k\">{} · {}</span></h3>",
            esc(id),
            chip(&d.severity),
            esc(&d.rule),
            esc(id),
            esc(&d.lifecycle)
        );
        let _ = write!(body, "<p>{}</p>", esc(&d.message));
        if let Some(rsn) = &d.reasoning {
            let _ = write!(body, "<p class=\"q\">{}</p>", esc(rsn));
        }
        let _ = write!(
            body,
            "<p><span class=\"k\">subjects</span> {}</p>",
            subjects.join(" ")
        );
        let s = search_attr(&[id, &d.rule, &d.severity, &d.message]);
        let _ = write!(h, "<div class=\"card\" data-s=\"{}\">{}</div>\n", s, body);
    }

    // Coverage.
    h.push_str("<h2>Coverage</h2>\n<table>\n<thead><tr><th>Document</th><th>Covered</th><th>Non-normative</th><th>Unprocessed</th></tr></thead>\n<tbody>\n");
    for (doc, rec) in &store.docs {
        let (mut covered, mut nonnorm, mut unproc) = (0usize, 0usize, 0usize);
        for (r, sec) in &rec.sections {
            if sec.raw.lines().skip(1).all(|l| l.trim().is_empty()) {
                continue;
            }
            match rec.coverage.get(r).map(|c| c.state.as_str()) {
                Some("covered") => covered += 1,
                Some("non-normative") => nonnorm += 1,
                _ => unproc += 1,
            }
        }
        let _ = write!(
            h,
            "<tr data-s=\"{}\"><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>\n",
            search_attr(&[doc]),
            esc(doc),
            covered,
            nonnorm,
            unproc
        );
    }
    h.push_str("</tbody>\n</table>\n");

    h.push_str("</div>\n<script>");
    h.push_str(SCRIPT);
    h.push_str("</script>\n</body>\n</html>\n");
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use std::collections::BTreeMap;

    #[test]
    fn renders_and_escapes() {
        let out = std::env::temp_dir().join(format!("jazyk-viewer-test-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        std::fs::create_dir_all(out.join("diagrams/class")).unwrap();
        std::fs::write(out.join("diagrams/class/public.svg"), "<svg/>").unwrap();
        let mut s = Store {
            out: out.clone(),
            ..Default::default()
        };
        s.graph.entities.insert(
            "ent:cart".into(),
            Entity {
                name: "Cart <script>".into(),
                definition: Some("holds \"items\" & things".into()),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                statement: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                transition: Some(Transition {
                    subject: "ent:cart".into(),
                    from: "empty".into(),
                    to: "filled".into(),
                    trigger: Some("an item lands".into()),
                    guard: None,
                }),
                facets: vec![Facet {
                    facet: "quality".into(),
                    reasoning: "bounded".into(),
                    measure: Some("2 seconds".into()),
                }],
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop".into(),
                    quote: "holds".into(),
                }),
                ..Default::default()
            },
        );
        s.graph.relationships.insert(
            "rel:cart~item".into(),
            Relationship {
                members: vec!["ent:cart".into(), "ent:item".into()],
                contributions: vec![
                    Contribution {
                        a: "ent:cart".into(),
                        b: "ent:item".into(),
                        r#type: "composition".into(),
                        cardinality: Some("1..*".into()),
                        requirements: vec!["req:shop-1".into()],
                    },
                    Contribution {
                        a: "ent:item".into(),
                        b: "ent:cart".into(),
                        r#type: "dependency".into(),
                        cardinality: None,
                        requirements: vec!["req:shop-2".into()],
                    },
                ],
            },
        );
        s.graph.views.insert(
            "view:class/public".into(),
            View {
                kind: "class".into(),
                title: "Public".into(),
                members: vec!["ent:cart".into()],
                ..Default::default()
            },
        );
        let text = "# Shop\nbody\n";
        s.docs.insert(
            "shop.md".into(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
        let html = render(
            &s,
            &GenSettings {
                deliverable: std::path::PathBuf::from("/nonexistent"),
                worker: "agentic".into(),
                code: Vec::new(),
            },
        );
        assert!(html.contains("id=\"n-ent:cart\""));
        assert!(html.contains("Cart &lt;script&gt;"));
        assert!(html.contains("&quot;items&quot; &amp; things"));
        assert!(html.contains("href=\"#n-req:shop-1\""));
        assert!(html.contains("The Cart shall hold items."));
        // Facets and transition as stated.
        assert!(html.contains("quality (2 seconds)"), "{}", html);
        assert!(
            html.contains("ent:cart: empty → filled on an item lands"),
            "{}",
            html
        );
        // Each contribution group with direction, type, cardinality, requirements.
        assert!(
            html.contains("ent:cart → ent:item (composition, 1..*)"),
            "{}",
            html
        );
        assert!(
            html.contains("ent:item → ent:cart (dependency)"),
            "{}",
            html
        );
        // The view card links its rendering, which exists on disk.
        assert!(html.contains("id=\"n-view:class/public\""), "{}", html);
        assert!(
            html.contains("href=\"diagrams/class/public.svg\""),
            "{}",
            html
        );
        assert!(html.contains("<table>"));
        assert!(!html.contains("<script>alert"));
        std::fs::remove_dir_all(&out).ok();
    }

    fn gs() -> GenSettings {
        GenSettings {
            deliverable: std::path::PathBuf::from("/nonexistent"),
            worker: "agentic".into(),
            code: Vec::new(),
        }
    }

    fn entity(name: &str, stereotype: Option<&str>, parent: Option<&str>) -> Entity {
        Entity {
            name: name.into(),
            stereotype: stereotype.map(str::to_string),
            parent: parent.map(str::to_string),
            ..Default::default()
        }
    }

    fn behavior(entities: &[&str]) -> Requirement {
        Requirement {
            statement: "The User asks the API.".into(),
            entities: entities.iter().map(|e| e.to_string()).collect(),
            facets: vec![Facet {
                facet: "behavior".into(),
                reasoning: "a flow".into(),
                measure: None,
            }],
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop".into(),
                quote: "asks".into(),
            }),
            ..Default::default()
        }
    }

    // The tree lines of a rendering, in order, tags stripped, whitespace collapsed.
    fn tree_lines(html: &str) -> Vec<(usize, String)> {
        html.lines()
            .filter(|l| l.starts_with("<div class=\"tn\""))
            .map(|l| {
                let depth = l
                    .split("padding-left:")
                    .nth(1)
                    .and_then(|s| s.split("px").next())
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0)
                    / 20;
                let mut text = String::new();
                let mut in_tag = false;
                for c in l.chars() {
                    match c {
                        '<' => in_tag = true,
                        '>' => in_tag = false,
                        _ if !in_tag => text.push(c),
                        _ => {}
                    }
                }
                (depth, text.split_whitespace().collect::<Vec<_>>().join(" "))
            })
            .collect()
    }

    #[test]
    fn tree_prints_root_node_and_leaf_with_level_view_ids() {
        let mut s = Store::default();
        s.graph.entities.insert(
            "ent:backend".into(),
            entity("Backend", Some("system"), None),
        );
        s.graph
            .entities
            .insert("ent:user".into(), entity("User", Some("actor"), None));
        s.graph.entities.insert(
            "ent:api".into(),
            entity("API Server", None, Some("ent:backend")),
        );
        s.graph.entities.insert(
            "ent:db".into(),
            entity("Database", None, Some("ent:backend")),
        );
        s.graph
            .requirements
            .insert("req:shop-1".into(), behavior(&["ent:user", "ent:api"]));
        s.graph
            .requirements
            .insert("req:shop-2".into(), behavior(&["ent:user", "ent:api"]));
        // The stored views a commit would derive: the root's structural view, the
        // backend's, and the root's flow cluster (User in shop.md).
        for (id, kind) in [
            ("view:component/public", "component"),
            ("view:component/backend", "component"),
            ("view:usecase/user-shop", "use-case"),
        ] {
            s.graph.views.insert(
                id.into(),
                View {
                    kind: kind.into(),
                    title: id.into(),
                    default: true,
                    ..Default::default()
                },
            );
        }
        let html = render(&s, &gs());
        let lines = tree_lines(&html);
        assert_eq!(lines.len(), 5, "{:?}", lines);

        // The root first, at depth zero, with its own level view ids: the structural
        // view, then the flow view lifted into the level.
        assert_eq!(lines[0].0, 0);
        assert_eq!(
            lines[0].1,
            "scope:public 2 children view:component/public view:usecase/user-shop"
        );
        // A node with children: its count and its structural view, one level in.
        let backend = lines
            .iter()
            .find(|(_, t)| t.starts_with("Backend"))
            .expect("backend line");
        assert_eq!(backend.0, 1);
        assert_eq!(backend.1, "Backend 2 children view:component/backend");
        // Its children print one level deeper, as leaves, names alone.
        let api = lines
            .iter()
            .find(|(_, t)| t == "API Server")
            .expect("api line");
        let db = lines
            .iter()
            .find(|(_, t)| t == "Database")
            .expect("db line");
        assert_eq!((api.0, db.0), (2, 2));
        // A parentless leaf prints plainly at depth one.
        let user = lines.iter().find(|(_, t)| t == "User").expect("user line");
        assert_eq!(user.0, 1);
        // Children immediately follow their parent, and the root lists its children in
        // document order: User, named in shop.md, before the unmentioned Backend.
        let idx = |t: &str| lines.iter().position(|(_, x)| x.starts_with(t)).unwrap();
        let b = idx("Backend");
        assert_eq!(
            [idx("API Server"), idx("Database")].iter().min().copied(),
            Some(b + 1)
        );
        assert_eq!(
            [idx("API Server"), idx("Database")].iter().max().copied(),
            Some(b + 2)
        );
        assert!(idx("User") < b, "{:?}", lines);

        // Ids link to cards: the node to its entity card, each view to its view card.
        assert!(
            html.contains("href=\"#n-ent:backend\">Backend</a>"),
            "{}",
            html
        );
        assert!(html.contains("href=\"#n-view:component/backend\">view:component/backend</a>"));
        assert!(html.contains("href=\"#n-view:usecase/user-shop\">view:usecase/user-shop</a>"));
        // Each line filters by its id, name, and view ids.
        assert!(html.contains("data-s=\"ent:api api server\""), "{}", html);
        assert!(html.contains(
            "data-s=\"scope:public scope:public view:component/public view:usecase/user-shop\""
        ));
    }

    #[test]
    fn tree_hangs_a_child_of_a_missing_parent_off_the_root_and_survives_a_cycle() {
        let mut s = Store::default();
        s.graph.entities.insert(
            "ent:orphan".into(),
            entity("Orphan", None, Some("ent:gone")),
        );
        s.graph
            .entities
            .insert("ent:a".into(), entity("A", None, Some("ent:b")));
        s.graph
            .entities
            .insert("ent:b".into(), entity("B", None, Some("ent:a")));
        let html = render(&s, &gs());
        let lines = tree_lines(&html);
        // The orphan prints under the root; the cycle never reaches the root, and the
        // walk terminates without printing either member twice.
        assert_eq!(lines[0].1, "scope:public");
        assert!(
            lines.iter().any(|(d, t)| *d == 1 && t == "Orphan"),
            "{:?}",
            lines
        );
        assert!(lines.iter().filter(|(_, t)| t == "A").count() <= 1);
    }

    #[test]
    fn tree_links_the_stored_sibling_kind_and_never_dangles() {
        let mut s = Store::default();
        s.graph.entities.insert(
            "ent:backend".into(),
            entity("Backend", Some("system"), None),
        );
        s.graph
            .entities
            .insert("ent:user".into(), entity("User", None, None));
        s.graph.entities.insert(
            "ent:api".into(),
            entity("API Server", None, Some("ent:backend")),
        );
        s.graph.entities.insert(
            "ent:db".into(),
            entity("Database", None, Some("ent:backend")),
        );
        // The root derives component (Backend is a «system») but the store still holds
        // the class view of an earlier commit; the backend level holds nothing yet.
        s.graph.views.insert(
            "view:class/public".into(),
            View {
                kind: "class".into(),
                title: "Public".into(),
                default: true,
                ..Default::default()
            },
        );
        let html = render(&s, &gs());
        let lines = tree_lines(&html);
        assert_eq!(lines[0].1, "scope:public 2 children view:class/public");
        assert!(html.contains("href=\"#n-view:class/public\">view:class/public</a>"));
        assert!(!html.contains("href=\"#n-view:component/public\""));
        // The backend's derived id prints as text, not a link, until a commit stores it.
        assert!(
            html.contains("<span class=\"id\">view:component/backend</span>"),
            "{}",
            html
        );
        assert!(!html.contains("href=\"#n-view:component/backend\""));
    }

    // Mirrors docs/frontends/viewer.md#what-it-shows and #navigation: the walk runs
    // inside the one file. The showcase store with a fake out directory holding one
    // rendering: the view card embeds it inline, sanitized, its drill-down anchor
    // rewritten to the card; the entity card lists its sections as anchors; the level
    // section carries the breadcrumb and the members.
    #[test]
    fn walk_runs_inside_the_file_with_the_rendering_inline() {
        let out = std::env::temp_dir().join(format!("jazyk-viewer-walk-{}", std::process::id()));
        std::fs::remove_dir_all(&out).ok();
        std::fs::create_dir_all(out.join("diagrams/component")).unwrap();
        std::fs::write(
            out.join("diagrams/component/shop.svg"),
            concat!(
                "<?xml version=\"1.0\" encoding=\"us-ascii\"?><?plantuml 1.2026.2?>",
                "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"",
                " height=\"120px\" style=\"width:300px;height:120px;background:#FFFFFF;\"",
                " viewBox=\"0 0 300 120\" width=\"300px\"><defs/>",
                "<script type=\"text/javascript\">alert('x')</script>",
                "<g onclick=\"alert('y')\" ONLOAD='alert(1)'>",
                "<a href=\"../../docsgen/entities/order-service.md\" xlink:href=\"../../docsgen/entities/order-service.md\">",
                "<g class=\"entity\" data-qualified-name=\"order_service\"><rect width=\"80\" height=\"30\"/></g></a>",
                "<a href=\"javascript:alert(2)\"><text>bad</text></a>",
                "<a href=\"../class/leaves.svg\" xlink:href=\"../class/leaves.svg\"><g class=\"entity\"><rect/></g></a>",
                "<foreignObject><body xmlns=\"http://www.w3.org/1999/xhtml\">html</body></foreignObject>",
                "</g></svg>"
            ),
        )
        .unwrap();
        let mut s = crate::derive::tests::showcase_store();
        s.out = out.clone();
        let mut batch = crate::store::RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        let html = render(&s, &gs());

        // The view card: the rendering inline, sanitized, scalable, its anchors
        // pointing at the cards in this file, the svg link kept.
        let card = |anchor: &str| -> &str {
            let start = html.find(anchor).unwrap_or_else(|| panic!("no {}", anchor));
            let end = html[start..].find("</div>\n").unwrap() + start;
            &html[start..end]
        };
        let shop_view = card("id=\"n-view:component/shop\"");
        assert!(shop_view.contains("<figure class=\"dia\" style=\"max-width:300px\"><svg "), "{}", shop_view);
        assert!(shop_view.contains("viewBox=\"0 0 300 120\""));
        assert!(!shop_view.contains("width=\"300px\"") && !shop_view.contains("height=\"120px\""), "{}", shop_view);
        assert!(shop_view.contains("style=\"background:#FFFFFF;\""), "{}", shop_view);
        // A nested element keeps its own size.
        assert!(shop_view.contains("<rect width=\"80\" height=\"30\"/>"));
        assert!(!shop_view.contains("<script") && !shop_view.contains("alert("), "{}", shop_view);
        assert!(!shop_view.to_lowercase().contains("onclick") && !shop_view.to_lowercase().contains("onload"));
        assert!(!shop_view.contains("<?") && !shop_view.contains("foreignObject"));
        assert!(
            shop_view.contains("<a href=\"#n-ent:order-service\" xlink:href=\"#n-ent:order-service\">"),
            "{}",
            shop_view
        );
        assert!(shop_view.contains("<a href=\"#n-view:class/leaves\" xlink:href=\"#n-view:class/leaves\">"));
        assert!(shop_view.contains("href=\"diagrams/component/shop.svg\">svg</a>"));
        // The level breadcrumb, the legend with the level below, the views around.
        assert!(shop_view.contains("<span class=\"k\">level</span> <a class=\"n\" href=\"#l-scope:public\">Public</a> › <a class=\"n\" href=\"#n-ent:shop\">Shop</a> · level: <a class=\"n\" href=\"#l-ent:shop\">Shop</a>"), "{}", shop_view);
        assert!(shop_view.contains("<span class=\"k\">drawn</span> <ul class=\"w\">"));
        assert!(
            shop_view.contains("<li><a class=\"n\" href=\"#n-ent:order-service\">Order Service</a> «service» · <a class=\"n\" href=\"#n-view:component/order-service\">level below</a></li>"),
            "{}",
            shop_view
        );
        assert!(shop_view.contains("above: <a class=\"n\" href=\"#n-view:component/public\">"), "{}", shop_view);
        assert!(shop_view.contains("below: <a class=\"n\" href=\"#n-view:component/order-service\">"));
        assert!(!shop_view.contains("<span class=\"k\">steps</span>"));
        // A view whose .svg is missing keeps the card without the image or the link.
        let os_view = card("id=\"n-view:component/order-service\"");
        assert!(!os_view.contains("<figure") && !os_view.contains(".svg\">svg</a>"), "{}", os_view);
        // A flow view lists its steps as drawn, each end a card link.
        let seq = card("id=\"n-view:sequence/shop-customer-shop\"");
        assert!(seq.contains("<span class=\"k\">steps</span> <ul class=\"w\"><li><a class=\"id\" href=\"#n-req:shop-1\">"), "{}", seq);
        assert!(seq.contains("<a class=\"n\" href=\"#n-ent:customer\">Customer</a> → <a class=\"n\" href=\"#n-ent:order-service\">Order Service</a>"), "{}", seq);

        // The entity card: one level in every direction, every link an anchor.
        let os = card("id=\"n-ent:order-service\"");
        assert!(
            os.contains("<span class=\"k\">sits in</span> <a class=\"n\" href=\"#l-scope:public\">Public</a> › <a class=\"n\" href=\"#n-ent:shop\">Shop</a> › Order Service"),
            "{}",
            os
        );
        assert!(os.contains("<span class=\"k\">in context</span> <a class=\"n\" href=\"#n-view:component/shop\">"), "{}", os);
        assert!(os.contains("<span class=\"k\">inside</span> <a class=\"n\" href=\"#n-view:component/order-service\">"), "{}", os);
        assert!(os.contains("dependency → <a class=\"n\" href=\"#n-ent:inventory-service\">Inventory Service</a>"), "{}", os);
        assert!(os.contains("<span class=\"k\">flows</span> <ul class=\"w\"><li>"), "{}", os);
        assert!(os.contains("<a class=\"n\" href=\"#n-view:usecase/shop-customer-shop\">use case</a> · <a class=\"n\" href=\"#n-view:sequence/shop-customer-shop\">sequence</a>"), "{}", os);
        assert!(os.contains("<span class=\"k\">siblings</span> <ul class=\"w\"><li><a class=\"n\" href=\"#n-ent:inventory-service\">"), "{}", os);
        assert!(os.contains("<span class=\"k\">children</span> <ul class=\"w\">") && os.contains("href=\"#n-ent:order\">Order</a>"), "{}", os);
        assert!(os.contains("<span class=\"k\">levels</span> level of <a class=\"n\" href=\"#l-ent:shop\">Shop</a> · own level <a class=\"n\" href=\"#l-ent:order-service\">Order Service</a>"), "{}", os);
        let leaf = card("id=\"n-ent:order-item\"");
        assert!(leaf.contains("<span class=\"k\">inside</span> <span class=\"k\">a leaf</span>"), "{}", leaf);
        assert!(!leaf.contains("<span class=\"k\">children</span>"));

        // The level section: the breadcrumb up (an ancestor to its level section), the
        // views as links, the members down with their level sections.
        let shop_level = card("id=\"l-ent:shop\"");
        assert!(shop_level.contains("<p class=\"crumbs\"><a class=\"n\" href=\"#l-scope:public\">Public</a> › Shop</p>"), "{}", shop_level);
        assert!(shop_level.contains("«system»") && shop_level.contains("· <a class=\"n\" href=\"#n-ent:shop\">Shop</a>"), "{}", shop_level);
        assert!(shop_level.contains("<span class=\"k\">diagrams</span> <a class=\"n\" href=\"#n-view:component/shop\">"), "{}", shop_level);
        assert!(shop_level.contains("href=\"#n-view:usecase/shop-customer-shop\">") && shop_level.contains("href=\"#n-view:sequence/shop-customer-shop\">"), "{}", shop_level);
        assert!(
            shop_level.contains("<li><a class=\"n\" href=\"#n-ent:order-service\">Order Service</a> «service»") && shop_level.contains("· <a class=\"n\" href=\"#l-ent:order-service\">level</a> <span class=\"k\">(4 children)</span>"),
            "{}",
            shop_level
        );
        // The customer sits outside the level through a lifted edge: drawn, not a member.
        assert!(!shop_level.contains("href=\"#n-ent:customer\""), "{}", shop_level);
        // The tree's child count links to the level section.
        assert!(html.contains("<a class=\"k\" href=\"#l-ent:shop\">2 children</a>"), "{}", html);
        // Nothing inside the file points outside it but the svg file link.
        assert!(!html.contains("docsgen/entities/"), "{}", html);
        std::fs::remove_dir_all(&out).ok();
    }

    #[test]
    fn tree_prints_none_for_an_empty_store() {
        let html = render(&Store::default(), &gs());
        assert!(
            html.contains("<h2>Tree</h2>\n<p class=\"k\">none</p>"),
            "{}",
            html
        );
        assert!(tree_lines(&html).is_empty());
    }
}
