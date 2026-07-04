// The language server: thin and read-only, per docs2/frontends/lsp.md. It reads the
// graph store and maps nodes to editor positions. It never compiles; rebuilds run
// through `jazyk compile` or `jazyk watch`, and the store's generation counter tells
// this server when to reload and republish.
use crate::context::{self, Focus};
use crate::jsonrpc::{read_message, write_message};
use crate::md;
use crate::model::Entity;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};

pub struct Lsp {
    root: PathBuf,
    out: PathBuf,
    store: Store,
    generation: u64,
    // Open documents: project-relative doc path -> current editor text.
    overlay: HashMap<String, String>,
}

impl Lsp {
    pub fn new(root: PathBuf, out: PathBuf) -> Lsp {
        let store = Store::load(&out);
        let generation = store.status.generation;
        Lsp { root, out, store, generation, overlay: HashMap::new() }
    }

    pub fn run(&mut self) {
        spawn_build_logger(self.out.clone());
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        let stdout = io::stdout();
        loop {
            let msg = match read_message(&mut reader) {
                Some(m) => m,
                None => break,
            };
            let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("").to_string();
            let id = msg.get("id").cloned();
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let mut out = stdout.lock();
            // The store is the single source of truth: reload when a compile moved it.
            self.refresh(&mut out);
            match method.as_str() {
                "initialize" => reply(&mut out, id, self.capabilities()),
                "initialized" => {}
                "shutdown" => reply(&mut out, id, Value::Null),
                "exit" => break,
                "textDocument/didOpen" => {
                    if let Some(doc) = self.sync_open(&params) {
                        self.publish(&mut out, &doc);
                    }
                }
                "textDocument/didChange" => {
                    if let Some(doc) = self.sync_change(&params) {
                        self.publish(&mut out, &doc);
                    }
                }
                "textDocument/didSave" => self.publish_all(&mut out),
                "textDocument/didClose" => {
                    if let Some(doc) = self.param_doc(&params) {
                        self.overlay.remove(&doc);
                    }
                }
                "textDocument/definition" => {
                    let r = self.on_definition(&params);
                    reply(&mut out, id, r);
                }
                "textDocument/references" => {
                    let r = self.on_references(&params);
                    reply(&mut out, id, r);
                }
                "textDocument/hover" => {
                    let r = self.on_hover(&params);
                    reply(&mut out, id, r);
                }
                "textDocument/completion" => {
                    let r = self.on_completion();
                    reply(&mut out, id, r);
                }
                "textDocument/documentLink" => {
                    let r = self.on_document_links(&params);
                    reply(&mut out, id, r);
                }
                _ => {
                    if id.is_some() {
                        reply(&mut out, id, Value::Null);
                    }
                }
            }
        }
    }

    fn capabilities(&self) -> Value {
        json!({
            "capabilities": {
                "textDocumentSync": 1, // full
                "definitionProvider": true,
                "referencesProvider": true,
                "hoverProvider": true,
                "completionProvider": { "triggerCharacters": ["`", "["] },
                "documentLinkProvider": { "resolveProvider": false }
            },
            "serverInfo": { "name": "jazyk", "version": env!("CARGO_PKG_VERSION") }
        })
    }

    // Reload the store when the generation counter moved, and republish every open
    // document so the editor reflects the new build.
    fn refresh<W: Write>(&mut self, out: &mut W) {
        let current = Store::load(&self.out);
        if current.status.generation != self.generation {
            eprintln!(
                "[jazyk-lsp] store generation {} -> {}; reloading",
                self.generation, current.status.generation
            );
            self.generation = current.status.generation;
            self.store = current;
            self.publish_all(out);
        }
    }

    // ---- document sync ----

    fn sync_open(&mut self, params: &Value) -> Option<String> {
        let td = params.get("textDocument")?;
        let doc = self.uri_to_doc(td.get("uri")?.as_str()?)?;
        let text = td.get("text")?.as_str()?.to_string();
        self.overlay.insert(doc.clone(), text);
        Some(doc)
    }

    fn sync_change(&mut self, params: &Value) -> Option<String> {
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

    fn param_doc(&self, params: &Value) -> Option<String> {
        let uri = params.get("textDocument")?.get("uri")?.as_str()?;
        self.uri_to_doc(uri)
    }

    // ---- path mapping ----

    fn uri_to_doc(&self, uri: &str) -> Option<String> {
        let path = uri_to_path(uri)?;
        let rel = path.strip_prefix(&self.root).ok()?;
        Some(rel.to_string_lossy().replace('\\', "/"))
    }

    fn doc_to_uri(&self, doc: &str) -> String {
        path_to_uri(&self.root.join(doc))
    }

    fn doc_text(&self, doc: &str) -> String {
        if let Some(t) = self.overlay.get(doc) {
            return t.clone();
        }
        std::fs::read_to_string(self.root.join(doc)).unwrap_or_default()
    }

    // ---- anchoring ----

    // Range of a quote in a document: exact match first, then the first whole-word
    // occurrence of a fallback name, then the section's first line, then line 0.
    fn anchor(&self, doc: &str, quote: &str, name: &str, section: Option<&str>) -> (usize, usize, usize, usize) {
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
                return (s.lines[0], 0, s.lines[0], 0);
            }
        }
        (0, 0, 0, 0)
    }

    fn range(&self, r: (usize, usize, usize, usize)) -> Value {
        json!({
            "start": {"line": r.0, "character": r.1},
            "end": {"line": r.2, "character": r.3}
        })
    }

    // ---- diagnostics ----

    fn publish_all<W: Write>(&self, out: &mut W) {
        let open: Vec<String> = self.overlay.keys().cloned().collect();
        for doc in open {
            self.publish(out, &doc);
        }
    }

    // Publish the open diagnostics that anchor to one document. Suppressed triage stays
    // out of the editor; resolved findings are never shown.
    fn publish<W: Write>(&self, out: &mut W, doc: &str) {
        let mut items: Vec<Value> = Vec::new();
        for d in self.store.graph.diagnostics.values() {
            if d.lifecycle != "open" || d.triage.as_deref() == Some("suppressed") {
                continue;
            }
            let severity = match d.severity.as_str() {
                "error" => 1,
                "warning" => 2,
                "info" => 3,
                _ => 4, // none: shown as a hint
            };
            for subject in &d.subjects {
                let anchor = self.subject_anchor(subject, doc);
                let Some(range) = anchor else { continue };
                items.push(json!({
                    "range": self.range(range),
                    "severity": severity,
                    "source": "jazyk",
                    "code": d.rule,
                    "message": format!("{}: {}", d.rule, d.message)
                }));
            }
        }
        let msg = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": { "uri": self.doc_to_uri(doc), "diagnostics": items }
        });
        write_message(out, &msg);
    }

    // Where a diagnostic subject anchors inside `doc`, if it does at all.
    fn subject_anchor(&self, subject: &str, doc: &str) -> Option<(usize, usize, usize, usize)> {
        let resolved = self.store.resolve_id(subject).to_string();
        if let Some(r) = self.store.graph.requirements.get(&resolved) {
            if r.source.doc == doc {
                return Some(self.anchor(doc, &r.source.quote, "", Some(&r.source.section)));
            }
            return None;
        }
        if let Some(e) = self.store.graph.entities.get(&resolved) {
            let m = e.mentions.iter().find(|m| m.doc == doc)?;
            return Some(self.anchor(doc, &m.quote, &e.name, Some(&m.section)));
        }
        // A section reference subject: "doc.md#/ref".
        if let Some((sdoc, sec)) = crate::model::split_section_ref(&resolved) {
            if sdoc == doc {
                let s = self.store.docs.get(doc)?.sections.get(&sec)?;
                return Some((s.lines[0], 0, s.lines[0], 0));
            }
        }
        None
    }

    // ---- entity under cursor ----

    fn pos(&self, params: &Value) -> Option<(String, usize, usize)> {
        let doc = self.param_doc(params)?;
        let p = params.get("position")?;
        let line = p.get("line")?.as_u64()? as usize;
        let ch = p.get("character")?.as_u64()? as usize;
        Some((doc, line, ch))
    }

    // The entity whose name or alias occurrence covers (line, character); longest match
    // wins. Any entity is eligible, not only ones mentioned in this document, so a doc
    // that references a concept without a stored mention still navigates.
    fn entity_at(&self, doc: &str, line: usize, character: usize) -> Option<(String, &Entity)> {
        let text = self.doc_text(doc);
        let mut best: Option<(usize, String)> = None;
        for (id, e) in &self.store.graph.entities {
            let mut names = vec![e.name.clone()];
            names.extend(e.aliases.iter().cloned());
            for n in names {
                for (l, c, len) in occurrences(&text, &n) {
                    if l == line && character >= c && character < c + len {
                        if best.as_ref().map(|(bl, _)| len > *bl).unwrap_or(true) {
                            best = Some((len, id.clone()));
                        }
                    }
                }
            }
        }
        let (_, id) = best?;
        self.store.graph.entities.get(&id).map(|e| (id, e))
    }

    // ---- request handlers ----

    fn on_definition(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else { return Value::Null };
        let Some((_, e)) = self.entity_at(&doc, line, ch) else { return Value::Null };
        // The defining mention is the first one recorded.
        let Some(m) = e.mentions.first() else { return Value::Null };
        let r = self.anchor(&m.doc, &m.quote, &e.name, Some(&m.section));
        json!({ "uri": self.doc_to_uri(&m.doc), "range": self.range(r) })
    }

    fn on_references(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else { return json!([]) };
        let Some((_, e)) = self.entity_at(&doc, line, ch) else { return json!([]) };
        let mut locs: Vec<Value> = Vec::new();
        let mut seen: BTreeSet<(String, usize)> = BTreeSet::new();
        for m in &e.mentions {
            let r = self.anchor(&m.doc, &m.quote, &e.name, Some(&m.section));
            if seen.insert((m.doc.clone(), r.0)) {
                locs.push(json!({ "uri": self.doc_to_uri(&m.doc), "range": self.range(r) }));
            }
        }
        json!(locs)
    }

    // Hover shows the same rendered pack the compiler and the MCP server see.
    fn on_hover(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else { return Value::Null };
        let Some((id, _)) = self.entity_at(&doc, line, ch) else { return Value::Null };
        match context::assemble(&self.store, &id, &Focus::default(), 4000) {
            Ok(pack) => json!({ "contents": { "kind": "markdown", "value": pack.pack } }),
            Err(_) => Value::Null,
        }
    }

    // Every whole-word occurrence of an entity name or alias links to that entity's
    // requirements document under <out>/docsgen/. Links are emitted only when the
    // target file exists, so they never dangle. Longest name wins on overlaps, like
    // entity_at; at most 200 links per document.
    fn on_document_links(&self, params: &Value) -> Value {
        let Some(doc) = self.param_doc(params) else { return json!([]) };
        let text = self.doc_text(&doc);
        struct Cand {
            line: usize,
            col: usize,
            len: usize,
            id: String,
            name: String,
        }
        let mut cands: Vec<Cand> = Vec::new();
        for (id, e) in &self.store.graph.entities {
            let slug = id.strip_prefix("ent:").unwrap_or(id);
            if !self.out.join("docsgen").join(format!("{}.md", slug)).exists() {
                continue;
            }
            let mut names = vec![e.name.clone()];
            names.extend(e.aliases.iter().cloned());
            for n in names {
                for (line, col, len) in occurrences(&text, &n) {
                    cands.push(Cand { line, col, len, id: id.clone(), name: e.name.clone() });
                }
            }
        }
        cands.sort_by(|a, b| b.len.cmp(&a.len).then(a.line.cmp(&b.line)).then(a.col.cmp(&b.col)));
        let mut taken: Vec<(usize, usize, usize)> = Vec::new(); // (line, start, end)
        let mut links: Vec<Value> = Vec::new();
        for c in cands {
            if links.len() >= 200 {
                break;
            }
            let end = c.col + c.len;
            if taken.iter().any(|(l, s, e)| *l == c.line && c.col < *e && *s < end) {
                continue;
            }
            taken.push((c.line, c.col, end));
            let slug = c.id.strip_prefix("ent:").unwrap_or(&c.id);
            let target = path_to_uri(&self.out.join("docsgen").join(format!("{}.md", slug)));
            links.push(json!({
                "range": self.range((c.line, c.col, c.line, end)),
                "target": target,
                "tooltip": format!("{}: requirements document", c.name)
            }));
        }
        json!(links)
    }

    fn on_completion(&self) -> Value {
        let mut items: Vec<Value> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for e in self.store.graph.entities.values() {
            let mut names = vec![e.name.clone()];
            names.extend(e.aliases.iter().cloned());
            for n in names {
                if seen.insert(n.clone()) {
                    items.push(json!({
                        "label": n,
                        "kind": 6,
                        "detail": e.definition.clone().unwrap_or_default()
                    }));
                }
            }
        }
        json!({ "isIncomplete": false, "items": items })
    }
}

fn reply<W: Write>(out: &mut W, id: Option<Value>, result: Value) {
    let msg = json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": result });
    write_message(out, &msg);
}

// file:// URI -> path (handles the common file:///abs/path form).
fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    Some(PathBuf::from(percent_decode(rest)))
}

fn path_to_uri(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{}", s)
    } else {
        format!("file:///{}", s)
    }
}

// Whole-word, case-insensitive occurrences of `needle` in `text`, as
// (line, start_col, len) in 0-based char columns. Editor-position mapping only.
fn occurrences(text: &str, needle: &str) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    if needle.trim().is_empty() {
        return out;
    }
    let nlow = needle.to_lowercase();
    let nlen = needle.chars().count();
    for (lineno, line) in text.lines().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let lower: String = line.to_lowercase();
        let mut start = 0usize;
        while let Some(byte_idx) = lower[start..].find(&nlow) {
            let abs = start + byte_idx;
            let col = lower[..abs].chars().count();
            let before_ok = col == 0 || !chars[col - 1].is_alphanumeric();
            let after_idx = col + nlen;
            let after_ok = after_idx >= chars.len() || !chars[after_idx].is_alphanumeric();
            if before_ok && after_ok {
                out.push((lineno, col, nlen));
            }
            start = abs + nlow.len();
            if start > lower.len() {
                break;
            }
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

// Tail build activity into the log channel (stderr): the store lock marks builds
// starting and ending, and each generation bump replays the new journal entries, one
// line per committed mutation. Mirrors docs2/frontends/lsp.md#build-activity-in-the-log.
fn spawn_build_logger(out: PathBuf) {
    std::thread::spawn(move || {
        let read_generation = |out: &Path| -> u64 {
            std::fs::read_to_string(out.join("status.yaml"))
                .ok()
                .and_then(|t| {
                    t.lines()
                        .find(|l| l.starts_with("generation:"))
                        .and_then(|l| l.split(':').nth(1))
                        .and_then(|v| v.trim().parse::<u64>().ok())
                })
                .unwrap_or(0)
        };
        let mut last_gen = read_generation(&out);
        let mut lock_seen = out.join(".lock").exists();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let lock_now = out.join(".lock").exists();
            if lock_now != lock_seen {
                lock_seen = lock_now;
                eprintln!(
                    "[jazyk-build] {}",
                    if lock_now { "build started (lock acquired)" } else { "build ended (lock released)" }
                );
            }
            let gen_now = read_generation(&out);
            if gen_now <= last_gen {
                continue;
            }
            for g in (last_gen + 1)..=gen_now {
                let path = out.join("journal").join(format!("g{}.yaml", g));
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let Ok(entry) = serde_norway::from_str::<Value>(&text) else { continue };
                let task = entry["workItem"]["task"].as_str().unwrap_or("?");
                let target = entry["workItem"]["target"].as_str().unwrap_or("?");
                let muts = entry["mutations"].as_array().cloned().unwrap_or_default();
                eprintln!("[jazyk-build] g{} {} {} ({} mutation(s))", g, task, target, muts.len());
                for m in &muts {
                    eprintln!(
                        "[jazyk-build]   {} {}",
                        m["op"].as_str().unwrap_or("?"),
                        m["id"].as_str().unwrap_or("")
                    );
                }
            }
            last_gen = gen_now;
        }
    });
}
