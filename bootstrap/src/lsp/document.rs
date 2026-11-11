use super::Lsp;
use crate::jsonrpc::write_message;
use crate::md;
use crate::model::Entity;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

impl Lsp {
    // ---- document sync ----

    pub(super) fn sync_open(&mut self, params: &Value) -> Option<String> {
        let td = params.get("textDocument")?;
        let doc = self.uri_to_doc(td.get("uri")?.as_str()?)?;
        let text = td.get("text")?.as_str()?.to_string();
        self.overlay.insert(doc.clone(), text);
        Some(doc)
    }

    pub(super) fn sync_change(&mut self, params: &Value) -> Option<String> {
        let doc = self.param_doc(params)?;
        // Full sync: the last content change carries the whole text.
        let text = params
            .get("contentChanges")?
            .as_array()?
            .last()?
            .get("text")?
            .as_str()?
            .to_string();
        self.overlay.insert(doc.clone(), text);
        Some(doc)
    }

    pub(super) fn param_doc(&self, params: &Value) -> Option<String> {
        let uri = params.get("textDocument")?.get("uri")?.as_str()?;
        self.uri_to_doc(uri)
    }

    // ---- path mapping ----

    pub(super) fn uri_to_doc(&self, uri: &str) -> Option<String> {
        let path = uri_to_path(uri)?;
        let root = uri_to_path(&path_to_uri(&self.root))?;
        let rel = path.strip_prefix(&root).ok()?;
        Some(rel.to_string_lossy().replace('\\', "/"))
    }

    pub(super) fn doc_to_uri(&self, doc: &str) -> String {
        path_to_uri(&self.root.join(doc))
    }

    pub(super) fn doc_text(&self, doc: &str) -> String {
        if let Some(t) = self.overlay.get(doc) {
            return t.clone();
        }
        md::read_doc(&self.root.join(doc)).unwrap_or_default()
    }

    // ---- anchoring ----

    // Range of a quote in a document: exact match first, then the first whole-word
    // occurrence of a fallback name, then the section's first line, then line 0.
    pub(super) fn anchor(
        &self,
        doc: &str,
        quote: &str,
        name: &str,
        section: Option<&str>,
    ) -> (usize, usize, usize, usize) {
        let text = self.doc_text(doc);
        if let Some(r) = md::locate(&text, quote) {
            return r;
        }
        if !name.is_empty() {
            if let Some((l, c, len)) = occurrences(&text, name).into_iter().next() {
                return (l, c, l, c + len);
            }
        }
        if let Some(sec) = section {
            if let Some(s) = self.store.docs.get(doc).and_then(|d| d.sections.get(sec)) {
                return section_anchor(&text, s).unwrap_or((s.lines[0], 0, s.lines[0], 0));
            }
        }
        (0, 0, 0, 0)
    }

    pub(super) fn range(&self, r: (usize, usize, usize, usize)) -> Value {
        json!({
            "start": {"line": r.0, "character": r.1},
            "end": {"line": r.2, "character": r.3}
        })
    }

    // ---- diagnostics ----

    pub(super) fn publish_all<W: Write>(&self, out: &mut W) {
        let open: Vec<String> = self.overlay.keys().cloned().collect();
        for doc in open {
            self.publish(out, &doc);
        }
    }

    // Publish the open diagnostics that anchor to one document. Suppressed triage stays
    // out of the editor; resolved findings are never shown.
    pub(super) fn publish<W: Write>(&self, out: &mut W, doc: &str) {
        let items = self.document_diagnostics(doc);
        let msg = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": { "uri": self.doc_to_uri(doc), "diagnostics": items }
        });
        write_message(out, &msg);
    }

    // Shared with the optional custom-document proof. Ranges refer to the reader's
    // text projection; only the custom view maps them to binary document locations.
    pub fn document_diagnostics(&self, doc: &str) -> Vec<Value> {
        let mut items: Vec<Value> = Vec::new();
        if md::supported_format(Path::new(doc)) {
            for link in md::missing_document_links(&self.root, doc, &self.doc_text(doc)) {
                let stored = self.store.graph.diagnostics.iter().find(|(_, d)| {
                    d.rule == "broken-link"
                        && d.subjects.first().is_some_and(|s| s == doc)
                        && (d.subjects.len() == 1 || d.subjects.get(1) == Some(&link.target))
                        && d.lifecycle == "open"
                });
                if stored.is_some_and(|(_, d)| d.triage.as_deref() == Some("suppressed")) {
                    continue;
                }
                let mut data = json!({ "target": link.target });
                if let Some((id, _)) = stored {
                    data["jazykDiag"] = json!(id);
                }
                items.push(json!({
                    "range": link.range, "severity": 2, "source": "jazyk",
                    "code": link.rule, "message": link.message, "data": data,
                }));
            }
        }
        for (did, d) in &self.store.graph.diagnostics {
            if d.rule == "broken-link" || !crate::diagnostic_locations::visible(d) {
                continue;
            }
            let severity = match d.severity.as_str() {
                "error" => 1,
                "warning" => 2,
                "info" => 3,
                _ => 4, // none: shown as a hint
            };
            // An unanswered prompt travels with the finding, so the question sits
            // inline where the finding is and code actions offer its options.
            // Mirrors docs/frontends/lsp.md#capabilities.
            let question = match (&d.prompt, &d.answer) {
                (Some(p), None) => Some(p.question.clone()),
                (Some(p), Some(a)) if a.status == "failed" => Some(p.question.clone()),
                _ => None,
            };
            let message = match &question {
                Some(q) => format!("{}: {}\nQ: {}", d.rule, d.message, q),
                None => format!("{}: {}", d.rule, d.message),
            };
            let mut ranges = BTreeSet::new();
            for location in crate::diagnostic_locations::document_locations(&self.store, d) {
                if location.path != doc {
                    continue;
                }
                let range = self.anchor(
                    doc,
                    location.quote.as_deref().unwrap_or(""),
                    "",
                    location.section.as_deref(),
                );
                if !ranges.insert(range) {
                    continue;
                }
                items.push(json!({
                    "range": self.range(range), "severity": severity,
                    "source": diagnostic_source(&self.root,did), "code": d.rule, "message": message,
                    "data": { "jazykDiag": did, "setId": crate::webapi::sets::unqualify(did).0, "related": location.related }
                }));
            }
        }
        items
    }

    // ---- entity under cursor ----

    pub(super) fn pos(&self, params: &Value) -> Option<(String, usize, usize)> {
        let doc = self.param_doc(params)?;
        let p = params.get("position")?;
        let line = p.get("line")?.as_u64()? as usize;
        let ch = p.get("character")?.as_u64()? as usize;
        Some((doc, line, ch))
    }

    // The entity whose name or alias occurrence covers (line, character); longest match
    // wins. Any entity is eligible, not only ones mentioned in this document, so a doc
    // that references a concept without a stored mention still navigates.
    pub(super) fn entity_at(
        &self,
        doc: &str,
        line: usize,
        character: usize,
    ) -> Option<(String, &Entity)> {
        let text = self.doc_text(doc);
        let mut best: Option<(usize, String)> = None;
        for (id, e) in &self.store.graph.entities {
            let mut names = vec![e.name.clone()];
            names.extend(e.aliases.iter().cloned());
            for n in names {
                for (l, c, len) in occurrences(&text, &n) {
                    if l == line
                        && character >= c
                        && character < c + len
                        && best.as_ref().map(|(bl, _)| len > *bl).unwrap_or(true)
                    {
                        best = Some((len, id.clone()));
                    }
                }
            }
        }
        let (_, id) = best?;
        self.store.graph.entities.get(&id).map(|e| (id, e))
    }
}

// file:// URI -> path (handles the common file:///abs/path form).
pub(super) fn uri_to_path(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

// The length of one line in UTF-16 code units, 0 past the end.
pub(super) fn line_len(text: &str, line: usize) -> usize {
    text.lines()
        .nth(line)
        .map(|l| l.encode_utf16().count())
        .unwrap_or(0)
}

pub(super) fn path_to_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .map(|uri| uri.to_string())
        .unwrap_or_default()
}

// Whole-word, case-insensitive occurrences of `needle` in `text`, as
// (line, start_col, len) in 0-based UTF-16 columns, the LSP's position encoding.
pub(super) fn occurrences(text: &str, needle: &str) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for (line, col, len) in char_occurrences(text, needle) {
        let Some(chars) = text.lines().nth(line) else {
            continue;
        };
        let units = |n: usize| chars.chars().take(n).map(char::len_utf16).sum::<usize>();
        let start = units(col);
        out.push((line, start, units(col + len) - start));
    }
    out
}

// The same occurrences in char columns.
fn char_occurrences(text: &str, needle: &str) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    if needle.trim().is_empty() {
        return out;
    }
    let nlow = needle.to_lowercase();
    for (lineno, line) in text.lines().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        // Lowercasing can change a character's length (one `İ` becomes two chars),
        // so every byte of the lowered line maps back to the original char it came
        // from; columns are reported on the original.
        let mut lower = String::new();
        let mut origin: Vec<usize> = Vec::new();
        for (ci, c) in chars.iter().enumerate() {
            for lc in c.to_lowercase() {
                let from = lower.len();
                lower.push(lc);
                origin.extend(std::iter::repeat_n(ci, lower.len() - from));
            }
        }
        let mut start = 0usize;
        while let Some(byte_idx) = lower[start..].find(&nlow) {
            let abs = start + byte_idx;
            let col = origin[abs];
            let end = abs + nlow.len();
            let after_idx = if end < origin.len() {
                origin[end]
            } else {
                chars.len()
            };
            let before_ok = col == 0 || !chars[col - 1].is_alphanumeric();
            let after_ok = after_idx >= chars.len() || !chars[after_idx].is_alphanumeric();
            if before_ok && after_ok && after_idx > col {
                out.push((lineno, col, after_idx - col));
            }
            start = end;
            if start > lower.len() {
                break;
            }
        }
    }
    out
}

fn diagnostic_source(root: &Path, id: &str) -> String {
    crate::webapi::sets::unqualify(id)
        .0
        .and_then(|id| {
            crate::document_sets::Workspace::load(root)
                .ok()?
                .select(&id)
                .ok()
                .map(|s| format!("jazyk · {}", s.name))
        })
        .unwrap_or_else(|| "jazyk".into())
}

// Where a finding about a whole section sits: its heading, or its first sentence
// when it has none, or a diagram's opening fence; never across its body. Located in
// the current text from the section's start line, so it follows edits above it.
// Mirrors docs/frontends/lsp.md#capabilities.
pub(super) fn section_anchor(
    text: &str,
    s: &crate::model::Section,
) -> Option<(usize, usize, usize, usize)> {
    let needle = section_lead(&s.raw)?;
    // Prefer the occurrence at or after the section's recorded start line.
    let from = s.lines[0].min(text.lines().count());
    let offset: usize = text.split_inclusive('\n').take(from).map(str::len).sum();
    if let Some((a, b, c, d)) = md::locate(&text[offset..], &needle) {
        return Some((a + from, b, c + from, d));
    }
    md::locate(text, &needle)
}

// The text a section-wide finding underlines: the heading line, a diagram's fence
// line, or the first sentence of the first paragraph (its first line when no
// sentence ends inside the paragraph).
pub(super) fn section_lead(raw: &str) -> Option<String> {
    let body: Vec<&str> = raw.lines().skip_while(|l| l.trim().is_empty()).collect();
    let first = body.first()?.trim();
    if first.starts_with('#') || first.starts_with("```") || first.starts_with("~~~") {
        return Some(first.to_string());
    }
    let paragraph: Vec<&str> = body
        .iter()
        .take_while(|l| !l.trim().is_empty())
        .map(|l| l.trim())
        .collect();
    let joined = paragraph.join(" ");
    let chars: Vec<(usize, char)> = joined.char_indices().collect();
    for (i, (at, c)) in chars.iter().enumerate() {
        let ends = matches!(c, '.' | '!' | '?')
            && chars.get(i + 1).is_none_or(|(_, n)| n.is_whitespace())
            && i > 0
            && chars[i - 1].1.is_alphanumeric();
        if ends {
            return Some(joined[..at + c.len_utf8()].to_string());
        }
    }
    Some(first.to_string())
}
