// Parsing: split a markdown document into a tree of sections (deterministic, no LLM).
// Mirrors docs/compiler/parsing.md.
use crate::model::{hash_hex, Section};
use std::collections::{BTreeMap, HashMap};

// Locate `needle` in `text` (possibly multi-line). Returns the 0-based
// (start_line, start_col, end_line, end_col) in character columns, or None. Exact
// substring first; a quote wrapped across source lines locates whitespace-insensitively,
// the same doctrine the store applies to quote containment. Character offsets are never
// stored.
// The byte range of the located needle: what a prose edit splices against.
pub fn locate_bytes(text: &str, needle: &str) -> Option<(usize, usize)> {
    let needle = needle.trim();
    if needle.is_empty() {
        return None;
    }
    match text.find(needle) {
        Some(b) => Some((b, b + needle.len())),
        None => locate_tokens(text, needle),
    }
}

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

// Whether the Markdown handler claims a file: by its extension, `.md` or `.markdown`.
// A matched file no handler claims is `unsupported-format` territory.
// Mirrors docs/compiler/parsing.md#format-handlers.
pub fn supported_format(path: &std::path::Path) -> bool {
    #[cfg(all(feature = "native-documents", not(target_arch = "wasm32")))]
    if crate::documents::supported(path) {
        return true;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"))
        .unwrap_or(false)
}

// Read a document for its handler: the text, or why it cannot be read (unreadable, or
// not valid UTF-8), the message a `parse-error` diagnostic carries.
pub fn read_doc(path: &std::path::Path) -> Result<String, String> {
    #[cfg(all(feature = "native-documents", not(target_arch = "wasm32")))]
    if crate::documents::supported(path) {
        return crate::documents::read(path).map(|d| d.text);
    }
    let bytes = crate::vfs::read(path).map_err(|e| format!("cannot read the file: {}", e))?;
    String::from_utf8(bytes).map_err(|e| format!("not valid UTF-8: {}", e.utf8_error()))
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

// A fence line: the fence character (backtick or tilde), its length, and the info
// string after it. A closing fence carries an empty info string.
fn fence(line: &str) -> Option<(char, usize, &str)> {
    let t = line.trim_start();
    let c = t.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = t.chars().take_while(|&x| x == c).count();
    if n < 3 {
        return None;
    }
    Some((c, n, t[n..].trim()))
}

// Whether `line` closes a fence opened with `c` repeated `n` times.
fn closes(line: &str, c: char, n: usize) -> bool {
    matches!(fence(line), Some((fc, fn_, info)) if fc == c && fn_ >= n && info.is_empty())
}

// One fenced block: its line range (the fences included), its kind, and its title.
struct Block {
    start: usize,
    end: usize,
    kind: &'static str,
    title: String,
}

// The fenced blocks inside `lines[start..end]`, in order. A block is a `diagram`
// when its info string names PlantUML or its first line opens one (`@start...`),
// else a `code-block`. An unclosed fence runs to the end of the range.
// Mirrors docs/compiler/parsing.md#section-tree.
fn blocks(lines: &[&str], start: usize, end: usize) -> Vec<Block> {
    let mut out = Vec::new();
    let mut i = start;
    while i < end {
        let Some((c, n, info)) = fence(lines[i]) else {
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < end && !closes(lines[j], c, n) {
            j += 1;
        }
        let close = if j < end { j + 1 } else { end };
        let lang = info.split_whitespace().next().unwrap_or("").to_lowercase();
        let first = lines.get(i + 1).map(|l| l.trim_start()).unwrap_or("");
        let diagram =
            matches!(lang.as_str(), "plantuml" | "puml" | "uml") || first.starts_with("@start");
        let kind = if diagram { "diagram" } else { "code-block" };
        let title = if info.is_empty() && diagram {
            "plantuml".to_string()
        } else {
            info.to_string()
        };
        out.push(Block {
            start: i,
            end: close,
            kind,
            title,
        });
        i = close;
    }
    out
}

pub fn parse_sections(text: &str) -> BTreeMap<String, Section> {
    let lines: Vec<&str> = text.lines().collect();
    let mut heads: Vec<Head> = Vec::new();
    let mut in_code: Option<(char, usize)> = None;
    for (i, l) in lines.iter().enumerate() {
        match in_code {
            Some((c, n)) => {
                if closes(l, c, n) {
                    in_code = None;
                }
                continue;
            }
            None => {
                if let Some((c, n, _)) = fence(l) {
                    in_code = Some((c, n));
                    continue;
                }
            }
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
    // Content before the first heading, or a whole document without headings, forms a
    // preamble section at `/`, so no prose is invisible to extraction. Mirrors
    // docs/compiler/parsing.md#section-tree.
    let pre_end = heads.first().map(|h| h.line).unwrap_or(lines.len());
    if lines[..pre_end].iter().any(|l| !l.trim().is_empty()) {
        let raw = lines[..pre_end].join("\n");
        sections.insert(
            "/".to_string(),
            Section {
                title: String::new(),
                kind: "preamble".to_string(),
                order: 0,
                parent: None,
                hash: hash_hex(&raw),
                raw,
                lines: [0, pre_end],
            },
        );
    }
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
            Some(format!(
                "/{}",
                stack
                    .iter()
                    .map(|(_, s)| s.clone())
                    .collect::<Vec<_>>()
                    .join("/")
            ))
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
        let kind = if stack.is_empty() && idx == 0 {
            "root"
        } else {
            "heading"
        };
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
    // Fenced blocks are sections of their own under the section whose body holds
    // them, ordered before that section's subheadings; the parent keeps its whole
    // body, so a quote locates in the section that states it.
    // Mirrors docs/compiler/parsing.md#section-tree.
    let holders: Vec<(String, usize, usize)> = sections
        .iter()
        .filter(|(_, s)| matches!(s.kind.as_str(), "preamble" | "root" | "heading"))
        .map(|(r, s)| (r.clone(), s.lines[0], s.lines[1]))
        .collect();
    let mut block_sections: Vec<(String, Section)> = Vec::new();
    let mut shifts: HashMap<String, usize> = HashMap::new();
    for (parent, start, end) in holders {
        let found = blocks(&lines, start, end);
        let mut per_kind: HashMap<&str, usize> = HashMap::new();
        for (order, b) in found.iter().enumerate() {
            let n = per_kind.entry(b.kind).or_insert(0);
            *n += 1;
            let reference = if parent == "/" {
                format!("/{}-{}", b.kind, n)
            } else {
                format!("{}/{}-{}", parent, b.kind, n)
            };
            let raw = lines[b.start..b.end].join("\n");
            block_sections.push((
                reference,
                Section {
                    title: b.title.clone(),
                    kind: b.kind.to_string(),
                    order,
                    parent: Some(parent.clone()),
                    hash: hash_hex(&raw),
                    raw,
                    lines: [b.start, b.end],
                },
            ));
        }
        if !found.is_empty() {
            shifts.insert(parent, found.len());
        }
    }
    for s in sections.values_mut() {
        if let Some(n) = s.parent.as_ref().and_then(|p| shifts.get(p)) {
            s.order += n;
        }
    }
    sections.extend(block_sections);
    sections
}

// Link positions are read-time projections, not persistent graph identities.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LinkPosition {
    pub line: usize,
    pub character: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LinkRange {
    pub start: LinkPosition,
    pub end: LinkPosition,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DocumentLink {
    pub target: String,
    pub href: String,
    pub range: LinkRange,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LinkDiagnostic {
    pub rule: &'static str,
    pub severity: &'static str,
    pub target: String,
    pub href: String,
    pub message: String,
    pub range: LinkRange,
}

fn link_position(text: &str, byte: usize) -> LinkPosition {
    let before = &text[..byte];
    LinkPosition {
        line: before.bytes().filter(|b| *b == b'\n').count(),
        character: before.rsplit('\n').next().unwrap_or("").encode_utf16().count(),
    }
}

// Resolve only a project-contained relative Markdown URL. The query and fragment
// do not name a file; percent-encoded separators are normalized after decoding.
pub fn resolve_document_link(from_doc: &str, href: &str) -> Option<String> {
    let path = href.split(['?', '#']).next()?;
    let bytes = path.as_bytes();
    if bytes.iter().enumerate().any(|(i, b)| *b == b'%' &&
        !(bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit) && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit))) {
        return None;
    }
    let path = percent_encoding::percent_decode_str(path).decode_utf8().ok()?;
    if path.is_empty() || path.starts_with('/') || path.contains('\\')
        || path.chars().any(char::is_control)
        || path.split('/').next()?.contains(':')
        || !supported_format(std::path::Path::new(path.as_ref())) {
        return None;
    }
    resolve_rel(from_doc, &path)
}

// The CommonMark parser excludes images, code and raw HTML and resolves reference
// link definitions. Offsets cover the occurrence, including reference-link labels.
pub fn document_links(text: &str, from_doc: &str) -> Vec<DocumentLink> {
    pulldown_cmark::Parser::new(text).into_offset_iter().filter_map(|(event, bytes)| {
        let pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) = event else { return None };
        Some(DocumentLink {
            target: resolve_document_link(from_doc, &dest_url)?,
            href: dest_url.into_string(),
            range: LinkRange { start: link_position(text, bytes.start), end: link_position(text, bytes.end) },
        })
    }).collect()
}

// The same document link graph feeds scheduling and missing-target diagnostics.
pub fn doc_links(text: &str, from_doc: &str) -> Vec<String> {
    let mut out: Vec<_> = document_links(text, from_doc).into_iter().map(|link| link.target).collect();
    out.sort();
    out.dedup();
    out
}

// Current files decide existence, never stale graph records. VFS keeps this check
// identical for local folders, WASM workspaces and isolated hosted snapshots.
pub fn missing_document_links(root: &std::path::Path, doc: &str, text: &str) -> Vec<LinkDiagnostic> {
    use crate::vfs::VPathExt;
    document_links(text, doc).into_iter().filter(|link| !root.join(&link.target).vis_file()).map(|link| {
        LinkDiagnostic {
            rule: "broken-link",
            severity: "warning",
            message: format!("{}:{}:{} links to {} which does not exist. Create the document or fix the link.",
                doc, link.range.start.line + 1, link.range.start.character + 1, link.target),
            target: link.target,
            href: link.href,
            range: link.range,
        }
    }).collect()
}

// Resolve a relative link target against the directory of `from_doc` (both '/'-separated,
// relative to the project root). Returns None if the path escapes the root.
pub fn resolve_rel(from_doc: &str, target: &str) -> Option<String> {
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
        assert_eq!(s.len(), 3, "{:?}", s.keys().collect::<Vec<_>>());
        assert_eq!(s["/top/code-block-1"].kind, "code-block");
        assert_eq!(s["/top/real"].kind, "heading");
        // A tilde fence closes only on tildes, so a backtick line inside stays.
        let text = "# Top\n~~~\n```\n# still code\n~~~\n## Real\n";
        let s = parse_sections(text);
        assert_eq!(s.len(), 3, "{:?}", s.keys().collect::<Vec<_>>());
    }

    // A fenced block is a section of its own under the section whose body holds it:
    // a PlantUML block is a `diagram` (by info string or an `@start` first line), any
    // other fence a `code-block`. The parent keeps its whole body, block orders come
    // before subheadings, references count per kind, and every kind is in the
    // model's list. Mirrors docs/compiler/parsing.md#section-tree.
    #[test]
    fn fenced_blocks_become_diagram_and_code_block_sections() {
        let text = "Intro.\n\n```yaml\nkey: 1\n```\n\n# Top\n\nThe Cart holds items.\n\n```plantuml\n@startuml\nCart --> Item : holds\n@enduml\n```\n\nMore prose.\n\n```\n@startuml\nA -> B\n@enduml\n```\n\n```rust\nfn x() {}\n```\n\n## Sub\nbody\n";
        let s = parse_sections(text);
        let keys: Vec<&String> = s.keys().collect();
        assert!(s.contains_key("/code-block-1"), "{:?}", keys);
        assert_eq!(s["/code-block-1"].parent.as_deref(), Some("/"));
        assert_eq!(s["/code-block-1"].title, "yaml");
        let d1 = &s["/top/diagram-1"];
        assert_eq!(d1.kind, "diagram");
        assert_eq!(d1.title, "plantuml");
        assert_eq!(d1.parent.as_deref(), Some("/top"));
        assert!(d1.raw.starts_with("```plantuml\n@startuml"));
        assert!(d1.raw.ends_with("```"));
        assert_eq!(d1.lines, [10, 15]);
        let d2 = &s["/top/diagram-2"];
        assert_eq!(d2.title, "plantuml", "a bare fence opening with @startuml");
        assert_eq!(d2.order, 1);
        let c = &s["/top/code-block-1"];
        assert_eq!(c.kind, "code-block");
        assert_eq!(c.title, "rust");
        assert_eq!(c.order, 2);
        // The subheading follows the three blocks; the parent's raw still holds them.
        assert_eq!(s["/top/sub"].order, 3);
        assert!(s["/top"].raw.contains("Cart --> Item"));
        assert!(s["/top"].raw.contains("More prose."));
        assert_ne!(d1.hash, d2.hash);
        for sec in s.values() {
            assert!(
                crate::model::SECTION_KINDS.contains(&sec.kind.as_str()),
                "{}",
                sec.kind
            );
        }
        // An unclosed fence runs to the end of its section.
        let s = parse_sections("# Top\n```\nopen\n## Next\nbody\n");
        assert_eq!(s["/top/code-block-1"].lines, [1, 5]);
        assert!(!s.contains_key("/top/next"), "{:?}", s.keys().collect::<Vec<_>>());
    }

    #[test]
    fn preamble_and_headingless_content_is_captured() {
        // Prose before the first heading lands in a preamble section at `/`.
        let s = parse_sections("Intro line before any heading.\n\n# Top\nbody\n");
        assert_eq!(s["/"].kind, "preamble");
        assert!(s["/"].raw.contains("Intro line"));
        assert_eq!(s["/top"].kind, "root");
        assert_eq!(s.len(), 2);
        // A heading-less document is one preamble holding everything.
        let n = parse_sections("Just prose, no headings at all.\n");
        assert_eq!(n.len(), 1);
        assert!(n["/"].raw.contains("Just prose"));
        // A blank-only file still yields no sections: empty-file check territory.
        assert!(parse_sections("\n\n").is_empty());
    }

    // A handler claims a file by its extension; a file it cannot read says why.
    // Mirrors docs/compiler/parsing.md#format-handlers.
    #[test]
    fn handler_claims_markdown_extensions_and_reports_unreadable_files() {
        use std::path::Path;
        assert!(supported_format(Path::new("docs/a.md")));
        assert!(supported_format(Path::new("docs/A.MARKDOWN")));
        assert!(!supported_format(Path::new("docs/a.txt")));
        assert!(!supported_format(Path::new("docs/README")));
        let dir = std::env::temp_dir().join(format!("jazyk-read-doc-test-{}", crate::sys::process_id()));
        crate::vfs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.md");
        crate::vfs::write(&good, "# T\nbody\n").unwrap();
        assert_eq!(read_doc(&good).unwrap(), "# T\nbody\n");
        let bad = dir.join("bad.md");
        crate::vfs::write(&bad, b"# T\n\xff\xfe not utf-8\n").unwrap();
        let err = read_doc(&bad).unwrap_err();
        assert!(err.contains("not valid UTF-8"), "{}", err);
        let missing = dir.join("missing.md");
        assert!(read_doc(&missing).unwrap_err().contains("cannot read"));
        crate::vfs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extracts_doc_links() {
        let text = "see [a](./sub/a.md) and [b](../b.md#anchor) and [x](http://x.md)";
        let links = doc_links(text, "docs/main.md");
        assert_eq!(links, vec!["b.md".to_string(), "docs/sub/a.md".to_string()]);
    }

    #[test]
    fn document_links_follow_markdown_syntax_and_preserve_utf16_occurrence_ranges() {
        let text = "# Slides\n😀 See [Intro](slide-intro.md?mode=read#top \"Title\") and [More][more].\n\n[more]: <../More%20Info.MARKDOWN> \"More\"\n\n`[code](inline.md)`\n\n```md\n[fenced](code.md)\n```\n\n![asset](image.md) [site](https://host/a.md) [mail](mailto:a.md) [anchor](#here) [pdf](notes.pdf)\n";
        let links = document_links(text, "slides/main.md");
        let binary_formats = cfg!(all(feature = "native-documents", not(target_arch = "wasm32")));
        assert_eq!(links.len(), if binary_formats { 3 } else { 2 }, "{:?}", links);
        if binary_formats {
            assert_eq!(links[2].target, "slides/notes.pdf");
        }
        assert_eq!(links[0].target, "slides/slide-intro.md");
        assert_eq!(links[0].href, "slide-intro.md?mode=read#top");
        assert_eq!(links[0].range.start, LinkPosition { line: 1, character: 7 });
        assert_eq!(links[1].target, "More Info.MARKDOWN");
        let start = text.find("[More][more]").unwrap();
        assert_eq!(links[1].range.start, link_position(text, start));
        assert_eq!(links[1].range.end, link_position(text, start + "[More][more]".len()));
    }

    #[test]
    fn document_link_resolution_rejects_external_and_escaping_targets() {
        for href in ["#intro", "?query=notes.md", "//host/a.md", "/root.md", "file:a.md",
            "https://host/a.md", "../outside.md", "%2e%2e/outside.md", "C:/file.md",
            "dir\\file.md", "bad%00.md", "bad%0A.md", "bad%FF.md", "bad%xy.md", "%2Foutside.md",
            "https%3Aother.md", "image.svg"] {
            assert!(resolve_document_link("main.md", href).is_none(), "{}", href);
        }
        assert_eq!(resolve_document_link("slides/main.md", "../notes%20one.md?x=1#top").as_deref(), Some("notes one.md"));
        assert_eq!(resolve_document_link("slides/main.md", "./sub/../intro.md").as_deref(), Some("slides/intro.md"));
    }

    #[test]
    fn locates_quotes() {
        let text = "line one\nthe exact quote here\nline three";
        assert!(locate(text, "exact quote").is_some());
        assert!(locate(text, "not there").is_none());
    }
}
