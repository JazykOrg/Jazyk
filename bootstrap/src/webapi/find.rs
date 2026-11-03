// `GET /api/find?q=`: the editor's project-wide search over files, document
// sections, and the graph. Not the model's search tool, whose results stay as they
// are. Mirrors docs/frontends/gui.md#api.
use crate::store::Store;
use serde_json::{json, Value};
use std::path::Path;

// Hits each kind keeps.
const PER_KIND: usize = 8;
// A file read for its text: at most this size, and at most this much in all.
const FILE_MOST: u64 = 512 * 1024;
const TEXT_BUDGET: u64 = 16 * 1024 * 1024;
// A detail line: the characters kept around the first match.
const DETAIL: usize = 120;

// The order kinds list in, for equal ranks.
const KINDS: [&str; 6] = ["file", "section", "entity", "requirement", "criterion", "view"];

struct Hit {
    rank: u8,
    kind: &'static str,
    key: String,
    value: Value,
}

// The query's words, lowercased. Empty when the query has none.
fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(|w| w.to_lowercase()).collect()
}

// Whether every word occurs in the text, case-insensitively.
fn matches(text: &str, words: &[String]) -> bool {
    let lower = text.to_lowercase();
    words.iter().all(|w| lower.contains(w.as_str()))
}

// One line of text around the first word's first match, trimmed and cut to fit.
fn detail(text: &str, words: &[String]) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = line.to_lowercase();
    let at = words.first().and_then(|w| lower.find(w.as_str())).unwrap_or(0);
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let at = chars.iter().position(|(i, _)| *i >= at).unwrap_or(0);
    let start = at.saturating_sub(DETAIL / 3);
    let end = (start + DETAIL).min(chars.len());
    let mut out: String = chars[start..end].iter().map(|(_, c)| c).collect();
    if start > 0 {
        out = format!("…{}", out.trim_start());
    }
    if end < chars.len() {
        out = format!("{}…", out.trim_end());
    }
    out
}

// The first line of the text where every word occurs, with its zero-based index;
// else the first line holding the first word.
fn matching_line<'a>(text: &'a str, words: &[String]) -> Option<(usize, &'a str)> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .position(|l| matches(l, words))
        .or_else(|| lines.iter().position(|l| words.first().is_some_and(|w| l.to_lowercase().contains(w.as_str()))))
        .map(|i| (i, lines[i]))
}

// Search the project: `files` are the project's file paths with their sizes (the
// Files tree's list), read under `root`. Deterministic for the same project and query.
pub fn find(store: &Store, root: &Path, files: &[(String, u64)], query: &str) -> Value {
    let words = words(query);
    if words.is_empty() {
        return json!({ "hits": [], "more": [], "truncated": false });
    }
    let mut hits: Vec<Hit> = Vec::new();

    // Files: by path, then by text. A document set input is searched by its sections.
    let mut budget = TEXT_BUDGET;
    let mut truncated = false;
    for (path, size) in files {
        // The compiled state under `.jazyk/` is searched through its nodes; only the
        // project's own files there are files to find.
        if path.strip_prefix(".jazyk/").is_some_and(|rest| rest.contains('/')) {
            continue;
        }
        let size = *size;
        let name = path.rsplit('/').next().unwrap_or(path);
        if matches(path, &words) {
            let rank = if matches(name, &words) { 0 } else { 1 };
            hits.push(Hit { rank, kind: "file", key: path.clone(), value: json!({ "kind": "file", "path": path, "title": name, "detail": path }) });
            continue;
        }
        if store.docs.contains_key(path.as_str()) {
            continue;
        }
        if size > FILE_MOST {
            continue;
        }
        if size > budget {
            truncated = true;
            continue;
        }
        budget -= size;
        let Ok(Value::Object(text)) = super::projfiles::project_text(root, path) else { continue };
        let Some(text) = text.get("text").and_then(Value::as_str) else { continue };
        if !matches(text, &words) {
            continue;
        }
        if let Some((i, line)) = matching_line(text, &words) {
            hits.push(Hit {
                rank: 2,
                kind: "file",
                key: path.clone(),
                value: json!({ "kind": "file", "path": path, "line": i + 1, "title": name, "detail": detail(line, &words) }),
            });
        }
    }

    // Document sections: by title, then by text.
    for (path, doc) in &store.docs {
        for (id, section) in &doc.sections {
            let by_title = !section.title.is_empty() && matches(&section.title, &words);
            if !by_title && !matches(&section.raw, &words) {
                continue;
            }
            let (offset, line) = if by_title {
                (0, section.raw.lines().nth(1).unwrap_or(&section.title))
            } else {
                matching_line(&section.raw, &words).unwrap_or((0, ""))
            };
            let title = if section.title.is_empty() { path.rsplit('/').next().unwrap_or(path).to_string() } else { section.title.clone() };
            hits.push(Hit {
                rank: if by_title { 0 } else { 2 },
                kind: "section",
                key: format!("{path}#{id}"),
                value: json!({ "kind": "section", "path": path, "section": id, "line": section.lines[0] + offset + 1, "title": title, "detail": detail(line, &words) }),
            });
        }
    }

    // Entities: by name or alias, then by definition.
    for (id, e) in &store.graph.entities {
        let named = matches(&e.name, &words) || e.aliases.iter().any(|a| matches(a, &words));
        let definition = e.definition.clone().unwrap_or_default();
        if !named && !matches(&definition, &words) {
            continue;
        }
        hits.push(Hit {
            rank: if named { 0 } else { 2 },
            kind: "entity",
            key: id.clone(),
            value: json!({ "kind": "entity", "id": id, "title": e.name, "detail": detail(&definition, &words) }),
        });
    }

    // Requirements and criteria: by statement.
    for (id, r) in &store.graph.requirements {
        if matches(&r.statement, &words) {
            hits.push(Hit { rank: 2, kind: "requirement", key: id.clone(), value: json!({ "kind": "requirement", "id": id, "title": detail(&r.statement, &words), "detail": "" }) });
        }
    }
    for (id, c) in &store.graph.criteria {
        if matches(&c.statement, &words) {
            hits.push(Hit { rank: 2, kind: "criterion", key: id.clone(), value: json!({ "kind": "criterion", "id": id, "requirement": c.requirement, "title": detail(&c.statement, &words), "detail": "" }) });
        }
    }

    // Views: by title.
    for (id, v) in &store.graph.views {
        if matches(&v.title, &words) {
            hits.push(Hit { rank: 0, kind: "view", key: id.clone(), value: json!({ "kind": "view", "id": id, "title": v.title, "detail": v.kind }) });
        }
    }

    let kind_order = |k: &str| KINDS.iter().position(|x| *x == k).unwrap_or(KINDS.len());
    hits.sort_by(|a, b| (a.rank, kind_order(a.kind), &a.key).cmp(&(b.rank, kind_order(b.kind), &b.key)));
    let mut kept: Vec<Value> = Vec::new();
    let mut more: Vec<&str> = Vec::new();
    for kind in KINDS {
        let of_kind = hits.iter().filter(|h| h.kind == kind).count();
        if of_kind > PER_KIND {
            more.push(kind);
        }
    }
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for h in hits {
        let n = counts.entry(h.kind).or_insert(0);
        if *n < PER_KIND {
            *n += 1;
            kept.push(h.value);
        }
    }
    json!({ "hits": kept, "more": more, "truncated": truncated })
}

// The project's file paths and sizes from the Files tree's listing value.
pub fn file_paths(listing: &Value) -> Vec<(String, u64)> {
    listing["files"]
        .as_array()
        .map(|files| files.iter().filter_map(|f| Some((f["path"].as_str()?.to_string(), f["size"].as_u64().unwrap_or(0)))).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_must_all_occur_case_insensitively() {
        let w = words("Main  SORT");
        assert!(matches("the main sort routine", &w));
        assert!(!matches("the main routine", &w));
        assert!(words("   ").is_empty());
    }

    #[test]
    fn detail_centers_on_the_match_and_marks_cuts() {
        let long = format!("{} needle {}", "a ".repeat(100), "b ".repeat(100));
        let d = detail(&long, &words("needle"));
        assert!(d.contains("needle"));
        assert!(d.starts_with('…') && d.ends_with('…'));
        assert!(d.chars().count() <= DETAIL + 2);
        assert_eq!(detail("short line", &words("short")), "short line");
    }

    // The project's kinds in one search: a file by name and by text, a section by
    // title and by text, an entity by name and by definition, a requirement, a
    // criterion, and a view; a document's text only through its sections.
    #[test]
    fn find_ranks_names_before_text_across_every_kind() {
        use crate::model::{Criterion, DocRecord, Entity, Requirement, Section, View};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("code")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("code/order.js"), "// totals\nfunction receipt() { return 'order receipt' }\n").unwrap();
        std::fs::write(root.join("code/receipt.js"), "export {}\n").unwrap();
        std::fs::write(root.join("docs/shop.md"), "# Shop\n\nEvery order gets a receipt.\n").unwrap();
        let mut s = Store::default();
        let section = |title: &str, raw: &str, start: usize| Section { title: title.into(), kind: "heading".into(), order: 0, parent: None, raw: raw.into(), hash: String::new(), lines: [start, start + raw.lines().count()] };
        let mut doc = DocRecord { content_hash: String::new(), sections: Default::default(), coverage: Default::default() };
        doc.sections.insert("/shop".into(), section("Shop", "# Shop\n\nEvery order gets a receipt.", 0));
        doc.sections.insert("/shop/receipts".into(), section("Receipts", "## Receipts\nPrinted.", 3));
        s.docs.insert("docs/shop.md".into(), doc);
        s.graph.entities.insert("ent:receipt".into(), Entity { name: "Receipt".into(), ..Default::default() });
        s.graph.entities.insert("ent:order".into(), Entity { name: "Order".into(), definition: Some("What a customer buys; it ends with a receipt.".into()), ..Default::default() });
        s.graph.requirements.insert("req:1".into(), Requirement { statement: "A customer must receive an order receipt.".into(), ..Default::default() });
        s.graph.criteria.insert("crit:1".into(), Criterion { requirement: "req:1".into(), statement: "Given an order, a receipt is printed.".into(), ..Default::default() });
        s.graph.views.insert("view:receipts".into(), View { kind: "class".into(), title: "Receipts".into(), ..Default::default() });
        let files = vec![("code/order.js".to_string(), 60), ("code/receipt.js".to_string(), 10), ("docs/shop.md".to_string(), 40)];
        let v = find(&s, root, &files, "RECEIPT");
        let hits: Vec<(String, String)> = v["hits"].as_array().unwrap().iter()
            .map(|h| (h["kind"].as_str().unwrap().to_string(), h["path"].as_str().or(h["id"].as_str()).unwrap().to_string()))
            .collect();
        let pair = |k: &str, p: &str| (k.to_string(), p.to_string());
        assert_eq!(hits, vec![
            pair("file", "code/receipt.js"),
            pair("section", "docs/shop.md"),
            pair("entity", "ent:receipt"),
            pair("view", "view:receipts"),
            pair("file", "code/order.js"),
            pair("section", "docs/shop.md"),
            pair("entity", "ent:order"),
            pair("requirement", "req:1"),
            pair("criterion", "crit:1"),
        ]);
        let text_hit = &v["hits"][4];
        assert_eq!(text_hit["line"], 2);
        assert!(text_hit["detail"].as_str().unwrap().contains("receipt"));
        let section_text = &v["hits"][5];
        assert_eq!(section_text["section"], "/shop");
        assert_eq!(section_text["line"], 3);
        assert_eq!(v["hits"][8]["requirement"], "req:1");
        assert_eq!(v["more"], json!([]));
        assert_eq!(v["truncated"], false);
        assert_eq!(find(&s, root, &files, "  ")["hits"], json!([]));
        // Every word must occur.
        assert!(find(&s, root, &files, "receipt nowhere")["hits"].as_array().unwrap().is_empty());
    }

    #[test]
    fn matching_line_prefers_a_line_with_every_word() {
        let text = "one main\nmain and sort here\nsort";
        assert_eq!(matching_line(text, &words("main sort")), Some((1, "main and sort here")));
        assert_eq!(matching_line("only main\nnothing", &words("main sort")), Some((0, "only main")));
    }
}
