// The viewer: render the graph store into one self-contained HTML file. Reads the same
// shards every frontend reads; never compiles. Mirrors docs2/frontends/viewer.md.
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
    format!("<span class=\"chip sev-{}\">{}</span>", esc(severity), esc(severity))
}

// The searchable text of a card, lowercased into a data attribute.
fn search_attr(parts: &[&str]) -> String {
    esc(&parts.join(" ").to_lowercase())
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
table { border-collapse: collapse; width: 100%; font-size: 13.5px; background: #fff; }
th { text-align: left; font-family: ui-monospace, Menlo, monospace; font-size: 11px;
  text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted);
  border-bottom: 1.5px solid var(--ink); padding: 6px 10px; }
td { border-bottom: 1px solid var(--line); padding: 6px 10px; }
td.num { text-align: right; font-variant-numeric: tabular-nums;
  font-family: ui-monospace, Menlo, monospace; }
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

pub fn render(store: &Store) -> String {
    let g = &store.graph;
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
    h.push_str("<input id=\"q\" type=\"search\" placeholder=\"Filter by id, name, or text\" aria-label=\"Filter\">\n");

    // Entities.
    h.push_str("<h2>Entities</h2>\n");
    for (id, e) in &g.entities {
        let refs = store.requirements_referencing(id);
        let mut body = String::new();
        let _ = write!(body, "<h3 id=\"n-{}\">{} <span class=\"k\">{}</span></h3>", esc(id), esc(&e.name), esc(id));
        if e.scope != "public" {
            let _ = write!(body, "<p><span class=\"k\">scope</span> {}</p>", esc(&e.scope));
        }
        if let Some(d) = &e.definition {
            let _ = write!(body, "<p>{}</p>", esc(d));
        }
        if !e.aliases.is_empty() {
            let _ = write!(body, "<p><span class=\"k\">aliases</span> {}</p>", esc(&e.aliases.join(", ")));
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
            let _ = write!(body, "<p><span class=\"k\">requirements</span> {}</p>", links.join(" "));
        }
        let s = search_attr(&[id, &e.name, e.definition.as_deref().unwrap_or(""), &e.aliases.join(" ")]);
        let _ = write!(h, "<div class=\"card\" data-s=\"{}\">{}</div>\n", s, body);
    }

    // Requirements.
    h.push_str("<h2>Requirements</h2>\n");
    for (id, r) in &g.requirements {
        let mut body = String::new();
        let _ = write!(body, "<h3 id=\"n-{}\">{}</h3>", esc(id), esc(id));
        let _ = write!(body, "<p>{}</p>", esc(&r.ears));
        let links: Vec<String> = r.entities.iter().map(|e| link(e)).collect();
        let _ = write!(body, "<p><span class=\"k\">entities</span> {}</p>", links.join(" "));
        let _ = write!(
            body,
            "<p class=\"q\">{}#{} \u{201c}{}\u{201d}</p>",
            esc(&r.source.doc),
            esc(&r.source.section),
            esc(&truncate(&r.source.quote, 160))
        );
        if !r.edges.is_empty() {
            let edges: Vec<String> = r
                .edges
                .iter()
                .map(|e| {
                    format!(
                        "{} ~ {} ({})",
                        esc(&e.a),
                        esc(&e.b),
                        esc(e.rel_type.as_deref().unwrap_or("reference"))
                    )
                })
                .collect();
            let _ = write!(body, "<p><span class=\"k\">edges</span> {}</p>", edges.join(", "));
        }
        let s = search_attr(&[id, &r.ears, &r.source.doc]);
        let _ = write!(h, "<div class=\"card\" data-s=\"{}\">{}</div>\n", s, body);
    }

    // Relationships.
    h.push_str("<h2>Relationships</h2>\n");
    if g.relationships.is_empty() {
        h.push_str("<p class=\"k\">none derived</p>\n");
    }
    for (id, rel) in &g.relationships {
        let members: Vec<String> = rel.members.iter().map(|m| link(m)).collect();
        let reqs: Vec<String> = rel.requirements.iter().map(|r| link(r)).collect();
        let s = search_attr(&[id, &rel.rel_type, &rel.members.join(" ")]);
        let _ = write!(
            h,
            "<div class=\"card\" data-s=\"{}\"><h3 id=\"n-{}\">{} <span class=\"k\">{}</span></h3><p><span class=\"k\">members</span> {}</p><p><span class=\"k\">requirements</span> {}</p></div>\n",
            s,
            esc(id),
            esc(id),
            esc(&rel.rel_type),
            members.join(" "),
            reqs.join(" ")
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
        let _ = write!(body, "<p><span class=\"k\">subjects</span> {}</p>", subjects.join(" "));
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
        let mut s = Store::default();
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
                ears: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                edges: vec![],
                source: SourceRef { doc: "shop.md".into(), section: "/shop".into(), quote: "holds".into() },
                confidence: None,
                reasoning: None,
                created: None,
                updated: None,
            },
        );
        let text = "# Shop\nbody\n";
        s.docs.insert(
            "shop.md".into(),
            DocRecord { content_hash: hash_hex(text), sections: crate::md::parse_sections(text), coverage: BTreeMap::new() },
        );
        let html = render(&s);
        assert!(html.contains("id=\"n-ent:cart\""));
        assert!(html.contains("Cart &lt;script&gt;"));
        assert!(html.contains("&quot;items&quot; &amp; things"));
        assert!(html.contains("href=\"#n-req:shop-1\""));
        assert!(html.contains("<table>"));
        assert!(!html.contains("<script>alert"));
    }
}
