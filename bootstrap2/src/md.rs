// Parsing: split a markdown document into a tree of sections (deterministic, no LLM).
// Mirrors docs2/compiler/parsing.md.
use crate::model::{hash_hex, Section};
use std::collections::{BTreeMap, HashMap};

// Locate `needle` in `text` (possibly multi-line). Returns the 0-based
// (start_line, start_col, end_line, end_col) in character columns, or None. Exact
// substring first; a quote wrapped across source lines locates whitespace-insensitively,
// the same doctrine the store applies to quote containment. Character offsets are never
// stored.
pub fn locate(text: &str, needle: &str) -> Option<(usize, usize, usize, usize)> {
    let needle = needle.trim();
    if needle.is_empty() {
        return None;
    }
    let (byte, end) = match text.find(needle) {
        Some(b) => (b, b + needle.len()),
        None => locate_tokens(text, needle)?,
    };
    let (sl, sc) = line_col(text, byte);
    let (el, ec) = line_col(text, end);
    Some((sl, sc, el, ec))
}

// Match the needle's whitespace-separated tokens in order, any whitespace between.
// Returns the matched byte range.
fn locate_tokens(text: &str, needle: &str) -> Option<(usize, usize)> {
    let tokens: Vec<&str> = needle.split_whitespace().collect();
    let first = tokens.first()?;
    for (start, _) in text.match_indices(first) {
        let mut pos = start + first.len();
        let mut ok = true;
        for token in &tokens[1..] {
            let rest = &text[pos..];
            let skipped = rest.len() - rest.trim_start().len();
            if skipped == 0 {
                ok = false;
                break;
            }
            let at = pos + skipped;
            if text[at..].starts_with(token) {
                pos = at + token.len();
            } else {
                ok = false;
                break;
            }
        }
        if ok {
            return Some((start, pos));
        }
    }
    None
}

// 0-based (line, char column) of a byte offset within `text`.
pub fn line_col(text: &str, byte: usize) -> (usize, usize) {
    let before = &text[..byte.min(text.len())];
    let line = before.matches('\n').count();
    let col = before.rsplit('\n').next().unwrap_or("").chars().count();
    (line, col)
}

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

struct Head {
    level: usize,
    title: String,
    line: usize,
}

pub fn parse_sections(text: &str) -> BTreeMap<String, Section> {
    let lines: Vec<&str> = text.lines().collect();
    let mut heads: Vec<Head> = Vec::new();
    let mut in_code = false;
    for (i, l) in lines.iter().enumerate() {
        if l.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let t = l.trim_start();
        let hashes = t.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && t.chars().nth(hashes) == Some(' ') {
            heads.push(Head {
                level: hashes,
                title: t[hashes..].trim().to_string(),
                line: i,
            });
        }
    }

    let mut sections: BTreeMap<String, Section> = BTreeMap::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut sibling_counts: HashMap<String, usize> = HashMap::new();
    for (idx, h) in heads.iter().enumerate() {
        while let Some(top) = stack.last() {
            if top.0 >= h.level {
                stack.pop();
            } else {
                break;
            }
        }
        let parent_ref = if stack.is_empty() {
            None
        } else {
            Some(format!("/{}", stack.iter().map(|(_, s)| s.clone()).collect::<Vec<_>>().join("/")))
        };
        let sl = slug(&h.title);
        let path: Vec<String> = stack
            .iter()
            .map(|(_, s)| s.clone())
            .chain(std::iter::once(sl.clone()))
            .collect();
        let reference = format!("/{}", path.join("/"));
        let pkey = parent_ref.clone().unwrap_or_else(|| "/".to_string());
        let order = {
            let c = sibling_counts.entry(pkey).or_insert(0);
            let v = *c;
            *c += 1;
            v
        };
        let end = if idx + 1 < heads.len() {
            heads[idx + 1].line
        } else {
            lines.len()
        };
        let raw = lines[h.line..end].join("\n");
        let kind = if stack.is_empty() && idx == 0 { "root" } else { "heading" };
        sections.insert(
            reference,
            Section {
                title: h.title.clone(),
                kind: kind.to_string(),
                order,
                parent: parent_ref,
                hash: hash_hex(&raw),
                raw,
                lines: [h.line, end],
            },
        );
        stack.push((h.level, sl));
    }
    sections
}

// Relative markdown links to other .md files inside `text`, resolved against the linking
// document's directory. Feeds the reconciler's level scheduling (the document link graph).
pub fn doc_links(text: &str, from_doc: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b'(' {
            if let Some(close) = text[i + 2..].find(')') {
                let target = &text[i + 2..i + 2 + close];
                let target = target.split('#').next().unwrap_or("");
                if target.ends_with(".md") && !target.starts_with("http") {
                    if let Some(resolved) = resolve_rel(from_doc, target) {
                        out.push(resolved);
                    }
                }
                i += 2 + close;
            }
        }
        i += 1;
    }
    out.sort();
    out.dedup();
    out
}

// Resolve a relative link target against the directory of `from_doc` (both '/'-separated,
// relative to the project root). Returns None if the path escapes the root.
fn resolve_rel(from_doc: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = from_doc.split('/').collect();
    parts.pop(); // drop the file name
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn locate_wrapped_quote() {
        let text = "intro text\nAn Order shall be paid within 21 days of placement; otherwise the system shall\ncancel it.\nmore";
        let needle = "An Order shall be paid within 21 days of placement; otherwise the system shall cancel it.";
        let (sl, sc, el, ec) = super::locate(text, needle).expect("wrapped quote locates");
        assert_eq!((sl, sc), (1, 0));
        assert_eq!(el, 2);
        assert_eq!(ec, "cancel it.".chars().count());
    }

    use super::*;

    #[test]
    fn parses_tree_with_refs_and_hashes() {
        let text = "# Top\nintro\n\n## A\nbody a\n\n### A1\ndeep\n\n## B\nbody b\n";
        let s = parse_sections(text);
        assert_eq!(s.len(), 4);
        assert_eq!(s["/top"].kind, "root");
        assert_eq!(s["/top/a"].parent.as_deref(), Some("/top"));
        assert_eq!(s["/top/a/a1"].parent.as_deref(), Some("/top/a"));
        assert_eq!(s["/top/b"].order, 1);
        assert!(s["/top/a"].raw.contains("body a"));
        assert!(!s["/top/a"].raw.contains("deep"));
        assert_ne!(s["/top/a"].hash, s["/top/b"].hash);
    }

    #[test]
    fn ignores_headings_in_code_blocks() {
        let text = "# Top\n```\n# not a heading\n```\n## Real\n";
        let s = parse_sections(text);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn extracts_doc_links() {
        let text = "see [a](./sub/a.md) and [b](../b.md#anchor) and [x](http://x.md)";
        let links = doc_links(text, "docs/main.md");
        assert_eq!(links, vec!["b.md".to_string(), "docs/sub/a.md".to_string()]);
    }

    #[test]
    fn locates_quotes() {
        let text = "line one\nthe exact quote here\nline three";
        assert!(locate(text, "exact quote").is_some());
        assert!(locate(text, "not there").is_none());
    }
}
