use super::document::{line_len, occurrences};
use super::Lsp;
use crate::md;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

impl Lsp {
    // One lens above each requirement's located quote: the attachment made visible
    // without hovering. The title is the requirement id, plus its verification status
    // when the ledger has one. Emitted only where the quote locates, so a broken
    // quote never shows a misplaced lens.
    // Mirrors docs/frontends/lsp.md#capabilities.
    pub(super) fn on_code_lens(&self, params: &Value) -> Value {
        let Some(doc) = self.param_doc(params) else {
            return json!([]);
        };
        let text = self.doc_text(&doc);
        let statuses = self.requirement_statuses();
        let mut lenses: Vec<(usize, usize, Value)> = Vec::new();
        for (rid, r) in &self.store.graph.requirements {
            let Some(src) = r.source.as_ref().filter(|s| s.doc == doc) else {
                continue;
            };
            let Some((sl, sc, _, _)) = md::locate(&text, &src.quote) else {
                continue;
            };
            let mut title = rid.clone();
            if let Some(st) = statuses.get(rid.as_str()) {
                title.push_str(&format!(" · {}", st.word));
            }
            lenses.push((
                sl,
                sc,
                json!({
                    "range": self.range((sl, sc, sl, sc)),
                    "command": {
                        "title": title,
                        "command": "jazyk.openRequirement",
                        "arguments": [rid]
                    }
                }),
            ));
        }
        for (rid, line) in self.code_requirements(&doc) {
            let title = match statuses.get(&rid) {
                Some(st) => format!("{} · {}", rid, st.word),
                None => rid.clone(),
            };
            lenses.push((line, 0, json!({
                "range": self.range((line, 0, line, 0)),
                "command": {"title": title, "command": "jazyk.openRequirement", "arguments": [rid]}
            })));
        }
        // A section whose text moved since the last build lenses on its heading.
        // Mirrors docs/frontends/lsp.md#capabilities.
        for line in self.changed_sections(&doc, &text) {
            lenses.push((
                line,
                0,
                json!({
                    "range": self.range((line, 0, line, 0)),
                    "command": {
                        "title": "Changed since last build",
                        "command": "jazyk.showLastBuild",
                        "arguments": [{"uri": self.doc_to_uri(&doc)}]
                    }
                }),
            ));
        }
        lenses.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        json!(lenses.into_iter().map(|(_, _, l)| l).collect::<Vec<_>>())
    }

    // jazyk.openRequirement <rid>: ask the client to show the requirement's heading
    // in its entity's requirements document under <out>/docsgen/. Server-driven via
    // window/showDocument, so any LSP client navigates without client-side commands.
    // The prompted diagnostics whose anchor intersects the requested range become
    // code actions: one per option, running jazyk.answerDiagnostic server-side.
    // Freeform needs a client input surface, so base clients get the options only.
    // Mirrors docs/frontends/lsp.md#capabilities.
    pub(super) fn on_code_action(&self, params: &Value) -> Value {
        let Some(doc) = self.param_doc(params) else {
            return json!([]);
        };
        let start = params["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
        let end = params["range"]["end"]["line"].as_u64().unwrap_or(u64::MAX) as usize;
        let mut actions: Vec<Value> = Vec::new();
        for (did, d) in &self.store.graph.diagnostics {
            if !crate::diagnostic_locations::visible(d) {
                continue;
            }
            let Some(p) = &d.prompt else { continue };
            if d.answer
                .as_ref()
                .map(|a| a.status != "failed")
                .unwrap_or(false)
            {
                continue;
            }
            let in_range = |r: (usize, usize, usize, usize)| r.0 <= end && r.2 >= start;
            let intersects = crate::diagnostic_locations::document_locations(&self.store, d)
                .into_iter()
                .filter(|l| l.path == doc)
                .any(|l| {
                    in_range(self.anchor(
                        &doc,
                        l.quote.as_deref().unwrap_or(""),
                        "",
                        l.section.as_deref(),
                    ))
                });
            if !intersects {
                continue;
            }
            for (i, o) in p.options.iter().enumerate() {
                let title = if o.edit.is_some() {
                    format!("Apply: {}", o.label)
                } else {
                    format!("Answer: {}", o.label)
                };
                actions.push(json!({
                    "title": title,
                    "kind": "quickfix",
                    "isPreferred": i == 0 && o.edit.is_some(),
                    "command": {
                        "title": o.label,
                        "command": "jazyk.answerDiagnostic",
                        "arguments": [{"id": did, "option": i}]
                    }
                }));
            }
        }
        json!(actions)
    }

    // ---- status words ----

    // Whether this server serves one set's out directory rather than the project.
    pub(super) fn serves_one_set(&self) -> bool {
        self.out
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(crate::document_sets::valid_id)
    }

    // Each requirement's derived status, reason, and status word. A project-wide
    // server reads each set's own ledger, loading a set only when it has one.
    // Mirrors docs/consumers/gen.md#status-words.
    fn requirement_statuses(&self) -> HashMap<String, ReqStatus> {
        let mut raw = crate::verify::status_map(&self.store, &self.gen);
        if !self.serves_one_set() {
            if let Ok(w) = crate::document_sets::Workspace::load(&self.root) {
                for set in &w.sets {
                    let Ok(p) = w.resolve(Some(&set.id)) else {
                        continue;
                    };
                    if crate::gen::Ledger::load(&p.out).requirements.is_empty() {
                        continue;
                    }
                    let store = Store::load(&p.out);
                    let gs = crate::gen::GenSettings::resolve(&p);
                    for (rid, v) in crate::verify::status_map(&store, &gs) {
                        raw.insert(crate::webapi::sets::qualify(&rid, &set.id), v);
                    }
                }
            }
        }
        let questions = crate::verify::questioned(&self.store);
        raw.into_iter()
            .map(|(rid, v)| {
                let status = v["status"].as_str().unwrap_or("missing").to_string();
                let reason = v["reason"].as_str().unwrap_or("").to_string();
                let word = crate::verify::status_word(&status, &reason, questions.contains(&rid));
                (
                    rid,
                    ReqStatus {
                        status,
                        reason,
                        word,
                    },
                )
            })
            .collect()
    }

    // The requirements sourced in a document whose quote locates in its current
    // text, each with its located range, in text order.
    fn located_requirements(
        &self,
        doc: &str,
        text: &str,
    ) -> Vec<(String, (usize, usize, usize, usize))> {
        let mut found: Vec<(String, (usize, usize, usize, usize))> = self
            .store
            .graph
            .requirements
            .iter()
            .filter_map(|(rid, r)| {
                let src = r.source.as_ref().filter(|s| s.doc == doc)?;
                md::locate(text, &src.quote).map(|range| (rid.clone(), range))
            })
            .collect();
        found.sort_by_key(|f| f.1);
        found
    }

    // One hint at the end of each located requirement quote inside the requested
    // range: the status word, the raw status in the tooltip.
    // Mirrors docs/frontends/lsp.md#capabilities.
    pub(super) fn on_inlay_hints(&self, params: &Value) -> Value {
        let Some(doc) = self.param_doc(params) else {
            return json!([]);
        };
        let at = |p: &Value, default: usize| {
            (
                p["line"].as_u64().map(|v| v as usize).unwrap_or(default),
                p["character"]
                    .as_u64()
                    .map(|v| v as usize)
                    .unwrap_or(default),
            )
        };
        let from = at(&params["range"]["start"], 0);
        let to = at(&params["range"]["end"], usize::MAX);
        let text = self.doc_text(&doc);
        let statuses = self.requirement_statuses();
        // A project that never generated has nothing to tell apart: every hint would
        // read `not built`. Only questions show there.
        let built = statuses
            .values()
            .any(|s| !(s.status == "missing" && s.reason == "not-generated"));
        let mut hints: Vec<Value> = Vec::new();
        for (rid, (_, _, el, ec)) in self.located_requirements(&doc, &text) {
            if (el, ec) < from || (el, ec) > to {
                continue;
            }
            let Some(st) = statuses.get(&rid) else {
                continue;
            };
            if !built && st.word == "not built" {
                continue;
            }
            hints.push(json!({
                "position": {"line": el, "character": ec},
                "label": format!("· {}", st.word),
                "paddingLeft": true,
                "tooltip": format!("{}: {} ({})", rid, st.status, st.reason),
            }));
        }
        json!(hints)
    }

    // Non-overlapping whole-word entity mentions in `text` as (line, column, length,
    // entity id), UTF-16 columns. The longest name wins on overlaps, then text order;
    // `keep` filters entities before overlaps resolve. The result is in acceptance
    // order: longest first.
    pub(super) fn entity_mentions(
        &self,
        text: &str,
        keep: impl Fn(&str) -> bool,
    ) -> Vec<(usize, usize, usize, String)> {
        let mut cands: Vec<(usize, usize, usize, String)> = Vec::new();
        for (id, e) in &self.store.graph.entities {
            if !keep(id) {
                continue;
            }
            for n in std::iter::once(&e.name).chain(e.aliases.iter()) {
                for (line, col, len) in occurrences(text, n) {
                    cands.push((line, col, len, id.clone()));
                }
            }
        }
        cands.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
        let mut taken: Vec<(usize, usize, usize, String)> = Vec::new();
        for c in cands {
            let end = c.1 + c.2;
            if taken
                .iter()
                .any(|t| t.0 == c.0 && c.1 < t.1 + t.2 && t.1 < end)
            {
                continue;
            }
            taken.push(c);
        }
        taken
    }

    // Every entity mention as one semantic token, so themes color them. Document
    // links stay the navigation. Mirrors docs/frontends/lsp.md#capabilities.
    pub(super) fn on_semantic_tokens(&self, params: &Value) -> Value {
        let Some(doc) = self.param_doc(params) else {
            return json!({ "data": [] });
        };
        let text = self.doc_text(&doc);
        let mut mentions = self.entity_mentions(&text, |_| true);
        mentions.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut data: Vec<usize> = Vec::with_capacity(mentions.len() * 5);
        let (mut prev_line, mut prev_col) = (0usize, 0usize);
        for (line, col, len, _) in mentions {
            let delta_line = line - prev_line;
            let delta_col = if delta_line == 0 { col - prev_col } else { col };
            // Token type 0 (`type`), modifier bit 0 (`jazykEntity`).
            data.extend([delta_line, delta_col, len, 0, 1]);
            prev_line = line;
            prev_col = col;
        }
        json!({ "data": data })
    }

    // Inside a requirement's located quote: the whole quote, with the located quotes
    // of its criteria in the same document. Mirrors docs/frontends/lsp.md#capabilities.
    pub(super) fn on_document_highlight(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else {
            return json!([]);
        };
        let Some((rid, r)) = self.requirement_at(&doc, line, ch) else {
            return json!([]);
        };
        let text = self.doc_text(&doc);
        let mut quotes: Vec<&str> = r.source.iter().map(|s| s.quote.as_str()).collect();
        let criteria = self.store.criteria_of(&rid);
        for cid in &criteria {
            if let Some(src) = self
                .store
                .graph
                .criteria
                .get(cid)
                .and_then(|c| c.source.as_ref())
                .filter(|s| s.doc == doc)
            {
                quotes.push(&src.quote);
            }
        }
        let ranges: BTreeSet<(usize, usize, usize, usize)> = quotes
            .into_iter()
            .filter_map(|q| md::locate(&text, q))
            .collect();
        json!(ranges
            .into_iter()
            .map(|r| json!({ "range": self.range(r), "kind": 1 }))
            .collect::<Vec<_>>())
    }

    // The stored section tree of the document as nested symbols, headings only,
    // each with its requirement count and read state. Ranges come from the current
    // text where the section still parses at the same reference.
    // Mirrors docs/frontends/lsp.md#capabilities.
    pub(super) fn on_document_symbols(&self, params: &Value) -> Value {
        let Some(doc) = self.param_doc(params) else {
            return json!([]);
        };
        let Some(record) = self.store.docs.get(&doc) else {
            return json!([]);
        };
        let text = self.doc_text(&doc);
        let current = md::parse_document(Path::new(&doc), &text);
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for r in self.store.graph.requirements.values() {
            if let Some(src) = r.source.as_ref().filter(|s| s.doc == doc) {
                *counts.entry(src.section.as_str()).or_default() += 1;
            }
        }
        let mut children: HashMap<Option<&str>, Vec<(&str, &crate::model::Section)>> =
            HashMap::new();
        for (reference, s) in &record.sections {
            if matches!(s.kind.as_str(), "root" | "heading") {
                children
                    .entry(s.parent.as_deref())
                    .or_default()
                    .push((reference, s));
            }
        }
        for list in children.values_mut() {
            list.sort_by_key(|(_, s)| (s.order, s.lines[0]));
        }
        let ctx = SymbolContext {
            text: &text,
            total: text.lines().count(),
            current: &current,
            record,
            counts: &counts,
            children: &children,
        };
        let (symbols, _) = self.section_symbols(&ctx, None);
        json!(symbols)
    }

    // The symbols under one parent, and the last line any of them covers.
    fn section_symbols(&self, ctx: &SymbolContext, parent: Option<&str>) -> (Vec<Value>, usize) {
        let mut symbols = Vec::new();
        let mut last = 0usize;
        for (reference, s) in ctx.children.get(&parent).into_iter().flatten() {
            let lines = ctx
                .current
                .get(*reference)
                .filter(|c| c.kind == s.kind)
                .map(|c| c.lines)
                .unwrap_or(s.lines);
            let max_line = ctx.total.saturating_sub(1);
            let start = lines[0].min(max_line);
            let own_end = lines[1].saturating_sub(1).clamp(start, max_line.max(start));
            let (kids, kids_end) = self.section_symbols(ctx, Some(reference));
            let end = own_end.max(kids_end);
            last = last.max(end);
            let n = ctx.counts.get(reference).copied().unwrap_or(0);
            let read = match ctx
                .record
                .coverage
                .get(*reference)
                .map(|c| c.state.as_str())
            {
                Some("covered") => "read",
                Some("non-normative") => "not normative",
                _ => "not read yet",
            };
            let title = if s.title.trim().is_empty() {
                reference.to_string()
            } else {
                s.title.clone()
            };
            symbols.push(json!({
                "name": title,
                "detail": format!(
                    "{} requirement{} · {}",
                    n,
                    if n == 1 { "" } else { "s" },
                    read
                ),
                "kind": 15,
                "range": self.range((start, 0, end, line_len(ctx.text, end))),
                "selectionRange": self.range((start, 0, start, line_len(ctx.text, start))),
                "children": kids,
            }));
        }
        (symbols, last)
    }

    // Entities by name or alias and requirements by statement or id, across every
    // set the server reads, each at its defining mention or quote.
    // Mirrors docs/frontends/lsp.md#capabilities.
    pub(super) fn on_workspace_symbols(&self, params: &Value) -> Value {
        const LIMIT: usize = 200;
        let query = params["query"].as_str().unwrap_or("").to_lowercase();
        let hit = |s: &str| s.to_lowercase().contains(&query);
        let mut symbols: Vec<Value> = Vec::new();
        for (id, e) in &self.store.graph.entities {
            if symbols.len() >= LIMIT {
                return json!(symbols);
            }
            if !(hit(&e.name) || e.aliases.iter().any(|a| hit(a))) {
                continue;
            }
            let Some(m) = e.mentions.first() else {
                continue;
            };
            let r = self.anchor(&m.doc, &m.quote, &e.name, Some(&m.section));
            symbols.push(json!({
                "name": e.name, "kind": 5, "containerName": id,
                "location": { "uri": self.doc_to_uri(&m.doc), "range": self.range(r) }
            }));
        }
        for (rid, r) in &self.store.graph.requirements {
            if symbols.len() >= LIMIT {
                break;
            }
            if !(hit(&r.statement) || hit(rid)) {
                continue;
            }
            let Some(src) = &r.source else {
                continue;
            };
            let range = self.anchor(&src.doc, &src.quote, "", Some(&src.section));
            symbols.push(json!({
                "name": r.statement, "kind": 15, "containerName": rid,
                "location": { "uri": self.doc_to_uri(&src.doc), "range": self.range(range) }
            }));
        }
        json!(symbols)
    }

    // The lines a `Changed since last build` lens sits on: the start line, in the
    // current text, of each heading-level section whose hash differs from its stored
    // section, or that the last build did not store. A document the last build never
    // stored has none. Mirrors docs/frontends/lsp.md#capabilities.
    fn changed_sections(&self, doc: &str, text: &str) -> Vec<usize> {
        let Some(record) = self.store.docs.get(doc) else {
            return Vec::new();
        };
        if record.sections.values().any(|s| s.kind == "blob") {
            return Vec::new();
        }
        let mut lines: Vec<usize> = md::parse_document(Path::new(doc), text)
            .iter()
            .filter(|(_, s)| matches!(s.kind.as_str(), "preamble" | "root" | "heading"))
            .filter(|(reference, s)| {
                record
                    .sections
                    .get(*reference)
                    .is_none_or(|old| old.hash != s.hash)
            })
            .map(|(_, s)| s.lines[0])
            .collect();
        lines.sort_unstable();
        lines
    }
}

// A requirement's derived status, its reason, and its status word.
struct ReqStatus {
    status: String,
    reason: String,
    word: &'static str,
}

// What building the document symbols of one document reads.
struct SymbolContext<'a> {
    text: &'a str,
    total: usize,
    current: &'a std::collections::BTreeMap<String, crate::model::Section>,
    record: &'a crate::model::DocRecord,
    counts: &'a HashMap<&'a str, usize>,
    children: &'a HashMap<Option<&'a str>, Vec<(&'a str, &'a crate::model::Section)>>,
}
