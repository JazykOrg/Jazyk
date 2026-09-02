// The viewer: render the graph store into one self-contained HTML file. Reads the same
// shards every frontend reads; never compiles. Mirrors docs/frontends/viewer.md.
use crate::gen::GenSettings;
use crate::llm::truncate;
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
        // The structural view as stored: the derived id, or the same level under the
        // other structural kind when a store from before a stereotype change still
        // holds it (the next commit rewrites it). A level view no shard holds prints
        // its id without a link.
        let structural = crate::derive::level_view_id(store, target).map(|id| {
            let sibling = match id.split_once('/') {
                Some(("view:class", slug)) => format!("view:component/{}", slug),
                Some(("view:component", slug)) => format!("view:class/{}", slug),
                _ => id.clone(),
            };
            if store.graph.views.contains_key(&id) {
                (id, true)
            } else if store.graph.views.contains_key(&sibling) {
                (sibling, true)
            } else {
                (id, false)
            }
        });
        let mut views: Vec<(String, bool)> = structural.clone().into_iter().collect();
        views.extend(
            flows
                .get(target)
                .into_iter()
                .flatten()
                .map(|v| (v.clone(), true)),
        );
        let count = match structural {
            Some(_) => format!(" <span class=\"k\">{} children</span>", children.len()),
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

    // Views: the stored half of each diagram, with a link to its rendering when the
    // .svg exists beside this file's default location under the out directory.
    h.push_str("<h2>Views</h2>\n");
    if g.views.is_empty() {
        h.push_str("<p class=\"k\">none</p>\n");
    }
    for (id, v) in &g.views {
        let members: Vec<String> = v.members.iter().map(|m| link(m)).collect();
        let svg_link = id
            .strip_prefix("view:")
            .and_then(|rel| rel.split_once('/'))
            .filter(|(kind, slug)| {
                store
                    .out
                    .join("diagrams")
                    .join(kind)
                    .join(format!("{}.svg", slug))
                    .exists()
            })
            .map(|(kind, slug)| {
                format!(
                    " · <a class=\"id\" href=\"diagrams/{}/{}.svg\">svg</a>",
                    esc(kind),
                    esc(slug)
                )
            })
            .unwrap_or_default();
        let s = search_attr(&[id, &v.kind, &v.title]);
        let _ = write!(
            h,
            "<div class=\"card\" data-s=\"{}\"><h3 id=\"n-{}\">{} <span class=\"k\">{} · {}</span>{}</h3><p><span class=\"k\">members</span> {}</p></div>\n",
            s,
            esc(id),
            esc(&v.title),
            esc(id),
            esc(&v.kind),
            svg_link,
            members.join(" ")
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
