// The language server: thin and read-only, per docs/frontends/lsp.md. It reads the
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
    gen: crate::gen::GenSettings,
    // Open documents: project-relative doc path -> current editor text.
    overlay: HashMap<String, String>,
    // Ids for server-initiated requests (window/showDocument), prefixed so they can
    // never collide with the client's own numeric ids.
    next_srv_id: u64,
}

impl Lsp {
    pub fn new(root: PathBuf, out: PathBuf, gen: crate::gen::GenSettings) -> Lsp {
        let store = Store::load(&out);
        let generation = store.status.generation;
        Lsp { root, out, store, generation, gen, overlay: HashMap::new(), next_srv_id: 1 }
    }

    pub fn run(&mut self) {
        // Two producers, one consumer: a stdin reader thread and a store poller thread
        // feed one channel, so a committed build repaints every open document the
        // moment it lands, without waiting for editor activity.
        // Mirrors docs/frontends/lsp.md#rebuilds-and-refresh.
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        let tx_in = tx.clone();
        std::thread::spawn(move || {
            let stdin = io::stdin();
            let mut reader = BufReader::new(stdin.lock());
            while let Some(m) = read_message(&mut reader) {
                if tx_in.send(Event::Client(m)).is_err() {
                    return;
                }
            }
            tx_in.send(Event::Eof).ok();
        });
        spawn_store_watcher(self.out.clone(), tx);
        let stdout = io::stdout();
        loop {
            let msg = match rx.recv() {
                Ok(Event::Client(m)) => m,
                Ok(Event::StoreChanged) => {
                    let mut out = stdout.lock();
                    self.refresh(&mut out);
                    continue;
                }
                Ok(Event::Eof) | Err(_) => break,
            };
            let mut out = stdout.lock();
            if !self.handle(msg, &mut out) {
                break;
            }
        }
    }

    // Dispatch one client message. Transport-agnostic: the GUI serves the same
    // sessions over WebSocket. Returns false on `exit`.
    pub(crate) fn handle<W: Write>(&mut self, msg: Value, out: &mut W) -> bool {
        // A message without a method is the client's response to a server-initiated
        // request (window/showDocument); never answer it, or its id would collide
        // with a pending client request.
        if msg.get("method").is_none() {
            return true;
        }
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("").to_string();
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        // The store is the single source of truth: reload when a compile moved it.
        self.refresh(out);
        match method.as_str() {
            "initialize" => reply(out, id, self.capabilities()),
            "initialized" => {}
            "shutdown" => reply(out, id, Value::Null),
            "exit" => return false,
            "textDocument/didOpen" => {
                if let Some(doc) = self.sync_open(&params) {
                    self.publish(out, &doc);
                }
            }
            "textDocument/didChange" => {
                if let Some(doc) = self.sync_change(&params) {
                    self.publish(out, &doc);
                }
            }
            "textDocument/didSave" => self.publish_all(out),
            // The client's native file watcher saw the store move; the refresh
            // before dispatch already reloaded and republished. Nothing more.
            "workspace/didChangeWatchedFiles" => {}
            "textDocument/didClose" => {
                if let Some(doc) = self.param_doc(&params) {
                    self.overlay.remove(&doc);
                }
            }
            "textDocument/definition" => {
                let r = self.on_definition(&params);
                reply(out, id, r);
            }
            "textDocument/references" => {
                let r = self.on_references(&params);
                reply(out, id, r);
            }
            "textDocument/hover" => {
                let r = self.on_hover(&params);
                reply(out, id, r);
            }
            "textDocument/completion" => {
                let r = self.on_completion();
                reply(out, id, r);
            }
            "textDocument/documentLink" => {
                let r = self.on_document_links(&params);
                reply(out, id, r);
            }
            "textDocument/codeLens" => {
                let r = self.on_code_lens(&params);
                reply(out, id, r);
            }
            "textDocument/codeAction" => {
                let r = self.on_code_action(&params);
                reply(out, id, r);
            }
            "workspace/executeCommand" => {
                let r = self.on_execute_command(&params, out);
                reply(out, id, r);
            }
            _ => {
                if id.is_some() {
                    reply(out, id, Value::Null);
                }
            }
        }
        true
    }

    fn capabilities(&self) -> Value {
        json!({
            "capabilities": {
                "textDocumentSync": 1, // full
                "definitionProvider": true,
                "referencesProvider": true,
                "hoverProvider": true,
                "completionProvider": { "triggerCharacters": ["`", "["] },
                "documentLinkProvider": { "resolveProvider": false },
                "codeLensProvider": { "resolveProvider": false },
                "codeActionProvider": true,
                "executeCommandProvider": { "commands": ["jazyk.openRequirement", "jazyk.answerDiagnostic"] }
            },
            "serverInfo": { "name": "jazyk", "version": env!("CARGO_PKG_VERSION") }
        })
    }

    // Reload the store when the generation counter moved, and republish every open
    // document so the editor reflects the new build.
    pub(crate) fn refresh<W: Write>(&mut self, out: &mut W) {
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
        for (did, d) in &self.store.graph.diagnostics {
            if d.lifecycle != "open" || d.triage.as_deref() == Some("suppressed") {
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
            for subject in &d.subjects {
                let anchor = self.subject_anchor(subject, doc);
                let Some(range) = anchor else { continue };
                let message = match &question {
                    Some(q) => format!("{}: {}\nQ: {}", d.rule, d.message, q),
                    None => format!("{}: {}", d.rule, d.message),
                };
                items.push(json!({
                    "range": self.range(range),
                    "severity": severity,
                    "source": "jazyk",
                    "code": d.rule,
                    "message": message,
                    "data": { "jazykDiag": did }
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

    // Hover shows the same rendered pack the compiler and the MCP server see, with a
    // verification summary from the ledger. Hovering inside a requirement's located
    // quote shows that requirement's own status.
    // Mirrors docs/frontends/lsp.md#capabilities.
    fn on_hover(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else { return Value::Null };
        if let Some((id, _)) = self.entity_at(&doc, line, ch) {
            let mut value = match context::assemble(&self.store, &id, &Focus::default(), 4000) {
                Ok(pack) => pack.pack,
                Err(_) => return Value::Null,
            };
            let vmap = crate::verify::status_map(&self.store, &self.gen);
            let refs = self.store.requirements_referencing(&id);
            let statuses: Vec<&str> = refs
                .iter()
                .filter_map(|r| vmap.get(r).and_then(|v| v["status"].as_str()))
                .collect();
            if !statuses.is_empty() {
                let ok = statuses.iter().filter(|s| **s == "verified").count();
                let bad = statuses.iter().filter(|s| **s == "failing").count();
                let stale = statuses.iter().filter(|s| s.starts_with("stale")).count();
                value.push_str(&format!(
                    "\n\n---\nverification: {}/{} verified, {} failing, {} stale",
                    ok,
                    statuses.len(),
                    bad,
                    stale
                ));
            }
            return json!({ "contents": { "kind": "markdown", "value": value } });
        }
        // No entity under the cursor: a requirement's quote, maybe.
        if let Some((rid, r)) = self.requirement_at(&doc, line, ch) {
            let value = self.requirement_card(&rid, r);
            let mut hover = json!({ "contents": { "kind": "markdown", "value": value } });
            // The hover range is the located quote, so the whole statement highlights.
            if let Some(q) = md::locate(&self.doc_text(&doc), &r.source.quote) {
                hover["range"] = self.range(q);
            }
            return hover;
        }
        Value::Null
    }

    // The requirement card: the requirement, the code, and the test, each linked, with
    // the derived verification status. Links are absolute `file://` URIs with an
    // `#L<line>` fragment so any client navigates; the requirement link carries the id
    // as `?req=` for clients that route to the node itself.
    // Mirrors docs/frontends/lsp.md#capabilities.
    fn requirement_card(&self, rid: &str, r: &crate::model::Requirement) -> String {
        let ledger = crate::gen::Ledger::load(&self.out);
        let row = ledger.requirements.get(rid);
        let (status, reason) = match row {
            Some(row) => crate::verify::status_of(&self.store, rid, row, &self.gen),
            None => ("missing".to_string(), "not-generated".to_string()),
        };
        let mut s = format!("**`{}`** · {} {}\n\n{}\n\n", rid, status_glyph(&status), status, r.ears);
        if let Some(link) = self.docsgen_link(rid, r) {
            s.push_str(&format!("[the requirement →]({})\n\n", link));
        }
        s.push_str("---\n\n");

        // The code: anchored sites first, each relocated against the file as it stands,
        // then manifest files that carry no site.
        let mut code: Vec<String> = Vec::new();
        if let Some(row) = row {
            let mut anchored: BTreeSet<&str> = BTreeSet::new();
            for site in &row.sites {
                anchored.insert(site.file.as_str());
                let abs = self.gen.deliverable.join(&site.file);
                let text = std::fs::read_to_string(&abs).unwrap_or_default();
                match crate::gen::locate_head(&text, &site.head, site.line) {
                    Some((l, exact)) => code.push(format!(
                        "- [`{}:{}`]({}#L{}){}",
                        site.file,
                        l,
                        path_to_uri(&abs),
                        l,
                        if exact { "" } else { " · moved" }
                    )),
                    None => code.push(format!(
                        "- [`{}`]({}) · site lost",
                        site.file,
                        path_to_uri(&abs)
                    )),
                }
            }
            for f in &row.files {
                if anchored.contains(f.as_str()) {
                    continue;
                }
                let abs = self.gen.deliverable.join(f);
                code.push(format!("- [`{}`]({})", f, path_to_uri(&abs)));
            }
        }
        if code.is_empty() {
            s.push_str("**code** · not generated\n\n");
        } else {
            s.push_str("**code**\n\n");
            s.push_str(&code.join("\n"));
            s.push_str("\n\n");
        }

        // The test: the artifact at the line its name sits on, then the status.
        s.push_str("---\n\n");
        let Some(row) = row else {
            s.push_str("**test** · not generated\n");
            return s;
        };
        let t = &row.test;
        let artifact = crate::gen::artifact_path(&self.out, &self.gen, t);
        let label = if t.label == t.kind { t.kind.clone() } else { format!("{} · {}", t.kind, t.label) };
        s.push_str(&format!("**test** · {}\n\n", label));
        let line = if t.name.is_empty() {
            None
        } else {
            std::fs::read_to_string(&artifact)
                .ok()
                .and_then(|text| text.lines().position(|l| l.contains(&t.name)).map(|i| i + 1))
        };
        // An llm test's criteria are metadata under the out directory, not part of the
        // product: the link carries the id, so a client with no page for the artifact
        // lands on the requirement's verification detail instead.
        let target = if t.kind == "llm" {
            format!("{}?req={}", path_to_uri(&artifact), rid)
        } else {
            path_to_uri(&artifact)
        };
        match line {
            Some(l) => s.push_str(&format!(
                "- [`{}:{}`]({}#L{}) · `{}`\n",
                t.artifact, l, target, l, t.name
            )),
            None => s.push_str(&format!(
                "- [`{}`]({}){}\n",
                t.artifact,
                target,
                if artifact.exists() { "" } else { " · artifact gone" }
            )),
        }
        s.push_str(&format!("- {} {} · {}", status_glyph(&status), status, reason));
        if let Some(last) = &row.last_run {
            s.push_str(&format!(" · last run {}", last));
        }
        s.push('\n');
        s.push_str(&format!("- run `{}`\n", t.run));
        if let Some(ev) = &row.evidence {
            s.push_str(&format!("\n> {}\n", ev.split_whitespace().collect::<Vec<_>>().join(" ")));
        }
        s
    }

    // Link to a requirement's heading in its entity's requirements document. None when
    // the document does not exist, so the card never dangles.
    fn docsgen_link(&self, rid: &str, r: &crate::model::Requirement) -> Option<String> {
        let ent = r.entities.first()?;
        let slug = self.store.resolve_id(ent).strip_prefix("ent:").unwrap_or(ent).to_string();
        let path = self.out.join("docsgen").join(format!("{}.md", slug));
        let text = std::fs::read_to_string(&path).ok()?;
        let heading = format!("### `{}`", rid);
        let line = text.lines().position(|l| l.trim() == heading).map(|i| i + 1).unwrap_or(1);
        Some(format!("{}?req={}#L{}", path_to_uri(&path), rid, line))
    }

    // The requirement whose located quote contains the position, if any.
    fn requirement_at(&self, doc: &str, line: usize, character: usize) -> Option<(String, &crate::model::Requirement)> {
        let text = self.doc_text(doc);
        for (rid, r) in &self.store.graph.requirements {
            if r.source.doc != doc {
                continue;
            }
            if let Some((sl, sc, el, ec)) = md::locate(&text, &r.source.quote) {
                let after_start = line > sl || (line == sl && character >= sc);
                let before_end = line < el || (line == el && character <= ec);
                if after_start && before_end {
                    return Some((rid.clone(), r));
                }
            }
        }
        None
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

    // One lens above each requirement's located quote: the attachment made visible
    // without hovering. The title is the requirement id, plus its verification status
    // when the ledger has one. Emitted only where the quote locates, so a broken
    // quote never shows a misplaced lens.
    // Mirrors docs/frontends/lsp.md#capabilities.
    fn on_code_lens(&self, params: &Value) -> Value {
        let Some(doc) = self.param_doc(params) else { return json!([]) };
        let text = self.doc_text(&doc);
        let vmap = crate::verify::status_map(&self.store, &self.gen);
        let mut lenses: Vec<(usize, usize, Value)> = Vec::new();
        for (rid, r) in &self.store.graph.requirements {
            if r.source.doc != doc {
                continue;
            }
            let Some((sl, sc, _, _)) = md::locate(&text, &r.source.quote) else { continue };
            let mut title = rid.clone();
            if let Some(s) = vmap.get(rid.as_str()).and_then(|v| v["status"].as_str()) {
                title.push_str(&format!(" · {}", s));
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
    fn on_code_action(&self, params: &Value) -> Value {
        let Some(doc) = self.param_doc(params) else { return json!([]) };
        let start = params["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
        let end = params["range"]["end"]["line"].as_u64().unwrap_or(u64::MAX) as usize;
        let mut actions: Vec<Value> = Vec::new();
        for (did, d) in &self.store.graph.diagnostics {
            if d.lifecycle != "open" || d.triage.as_deref() == Some("suppressed") {
                continue;
            }
            let Some(p) = &d.prompt else { continue };
            if d.answer.as_ref().map(|a| a.status != "failed").unwrap_or(false) {
                continue;
            }
            let intersects = d.subjects.iter().any(|s| {
                self.subject_anchor(s, &doc)
                    .map(|(sl, _, el, _)| sl <= end && el >= start)
                    .unwrap_or(false)
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

    // Answer a prompted diagnostic: the LSP's one explicit, human-initiated write
    // path. An edit option applies as a dual write and resolves immediately; any
    // other reply records handling and a background answer session acts on it.
    // Mirrors docs/frontends/lsp.md and docs/compiler/model/diagnostic.md#answers.
    fn on_answer_diagnostic<W: Write>(&mut self, params: &Value, out: &mut W) -> Value {
        let arg = params["arguments"][0].clone();
        let Some(did) = arg["id"].as_str() else {
            return json!({"error": "jazyk.answerDiagnostic needs {id, option|text}"});
        };
        let reply = if let Some(i) = arg["option"].as_u64() {
            crate::answer::Reply::Choice(i as usize)
        } else if let Some(t) = arg["text"].as_str() {
            crate::answer::Reply::Text(t.to_string())
        } else {
            return json!({"error": "pass option (an index into the prompt's options) or text (a freeform reply)"});
        };
        let mut project = crate::project::Project::load(&self.root);
        project.out = self.out.clone();
        match crate::answer::answer(&project, &self.out, did, reply, None) {
            Ok(v) => {
                if v["status"] == "handling" {
                    crate::answer::spawn_handler(project, self.out.clone(), did.to_string());
                }
                // The store moved (answer recorded, possibly resolved): repaint now.
                self.refresh(out);
                v
            }
            Err(e) => json!({"error": e}),
        }
    }

    fn on_execute_command<W: Write>(&mut self, params: &Value, out: &mut W) -> Value {
        let cmd = params.get("command").and_then(|c| c.as_str()).unwrap_or("");
        if cmd == "jazyk.answerDiagnostic" {
            return self.on_answer_diagnostic(params, out);
        }
        if cmd != "jazyk.openRequirement" {
            return Value::Null;
        }
        let Some(rid) = params
            .get("arguments")
            .and_then(|a| a.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
        else {
            return Value::Null;
        };
        let resolved = self.store.resolve_id(rid).to_string();
        let Some(r) = self.store.graph.requirements.get(&resolved) else { return Value::Null };
        let Some(ent) = r.entities.first() else { return Value::Null };
        let slug = self.store.resolve_id(ent).strip_prefix("ent:").unwrap_or(ent).to_string();
        let path = self.out.join("docsgen").join(format!("{}.md", slug));
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!("[jazyk-lsp] {}: no requirements document at {}", resolved, path.display());
            return Value::Null;
        };
        let heading = format!("### `{}`", resolved);
        let line = text.lines().position(|l| l.trim() == heading).unwrap_or(0);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": format!("jazyk-srv-{}", self.next_srv_id),
            "method": "window/showDocument",
            "params": {
                "uri": path_to_uri(&path),
                "takeFocus": true,
                "selection": {
                    "start": {"line": line, "character": 0},
                    "end": {"line": line, "character": 0}
                }
            }
        });
        self.next_srv_id += 1;
        write_message(out, &msg);
        Value::Null
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

// One glyph per derived verification status, so the card reads at a glance in clients
// that render no colour. Mirrors docs/consumers/gen.md#status-is-derived-never-stored.
fn status_glyph(status: &str) -> &'static str {
    match status {
        "verified" => "✓",
        "failing" => "✗",
        "unverified" => "○",
        s if s.starts_with("stale") => "↻",
        _ => "–",
    }
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

// Events feeding the main loop: a client message, a store change, or stdin closing.
enum Event {
    Client(Value),
    StoreChanged,
    Eof,
}

// Tail build activity into the log channel (stderr) and nudge the main loop on every
// generation bump: the store lock marks builds starting and ending, and each bump
// replays the new journal entries, one line per committed mutation.
// Mirrors docs/frontends/lsp.md#build-activity-in-the-log.
fn spawn_store_watcher(out: PathBuf, tx: std::sync::mpsc::Sender<Event>) {
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
            if tx.send(Event::StoreChanged).is_err() {
                return;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::{GenSettings, Ledger, ReqRow, RowHashes, Site, TestRef};
    use crate::model::{Requirement, SourceRef};

    fn requirement() -> Requirement {
        Requirement {
            ears: "The Cart shall hold items.".into(),
            entities: vec!["ent:cart".into()],
            edges: vec![],
            source: SourceRef { doc: "shop.md".into(), section: "/shop".into(), quote: "holds".into() },
            confidence: None,
            reasoning: None,
            created: None,
            updated: None,
        }
    }

    // The card: the requirement linked to its docsgen heading, the code linked at the
    // line each site relocates to, and the test linked at its name.
    // Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn requirement_card_links_the_requirement_code_and_test() {
        let tmp = std::env::temp_dir().join(format!("jazyk-lsp-card-{}", std::process::id()));
        std::fs::remove_dir_all(&tmp).ok();
        let root = tmp.join("proj");
        let out = root.join("jazyk-out");
        let deliv = tmp.join("product");
        std::fs::create_dir_all(deliv.join("src")).unwrap();
        std::fs::create_dir_all(deliv.join("tests")).unwrap();
        std::fs::create_dir_all(out.join("docsgen")).unwrap();
        // The site anchored at line 1; an inserted header moved it to line 3.
        std::fs::write(deliv.join("src/cart.rs"), "// header\n\nfn hold(i: Item) {}\n").unwrap();
        std::fs::write(deliv.join("tests/cart.rs"), "#[test]\nfn req_shop_1_abcd() {}\n").unwrap();
        std::fs::write(out.join("docsgen").join("cart.md"), "# Cart\n\n### `req:shop-1`\n").unwrap();

        let mut ledger = Ledger::default();
        ledger.requirements.insert(
            "req:shop-1".into(),
            ReqRow {
                entity: "ent:cart".into(),
                files: vec!["src/cart.rs".into(), "src/checkout.rs".into()],
                sites: vec![Site { file: "src/cart.rs".into(), line: 1, head: "fn hold(i: Item) {}".into() }],
                test: TestRef {
                    kind: "programmatic".into(),
                    label: "unit".into(),
                    artifact: "tests/cart.rs".into(),
                    name: "req_shop_1_abcd".into(),
                    run: "cargo test req_shop_1_abcd".into(),
                    cwd: ".".into(),
                },
                hashes: RowHashes::default(),
                verdict: "none".into(),
                last_run: None,
                exit_code: None,
                evidence: None,
            },
        );
        ledger.save(&out);

        let r = requirement();
        let mut store = Store { out: out.clone(), ..Default::default() };
        store.graph.requirements.insert("req:shop-1".into(), r.clone());
        let lsp = Lsp {
            root,
            out,
            store,
            generation: 0,
            gen: GenSettings { deliverable: deliv, worker: "agentic".into(), code: Vec::new() },
            overlay: HashMap::new(),
            next_srv_id: 1,
        };
        let card = lsp.requirement_card("req:shop-1", &r);

        assert!(card.contains("**`req:shop-1`**"), "{}", card);
        assert!(card.contains("The Cart shall hold items."), "{}", card);
        // The requirement link: the docsgen heading, with the id for clients that route
        // to the node itself.
        assert!(card.contains("docsgen/cart.md?req=req:shop-1#L3"), "{}", card);
        // The code: the site relocated from line 1 to line 3, and marked moved. The
        // manifest file with no site links to the file itself.
        assert!(card.contains("[`src/cart.rs:3`]"), "{}", card);
        assert!(card.contains("#L3) · moved"), "{}", card);
        assert!(card.contains("[`src/checkout.rs`]"), "{}", card);
        // The test: the artifact at the line its name sits on, and the derived status
        // (the row's requirement hash is empty, so the statement moved under it).
        assert!(card.contains("[`tests/cart.rs:2`]"), "{}", card);
        assert!(card.contains("`req_shop_1_abcd`"), "{}", card);
        assert!(card.contains("↻ stale-requirement · requirement-changed"), "{}", card);
        assert!(card.contains("run `cargo test req_shop_1_abcd`"), "{}", card);
        std::fs::remove_dir_all(&tmp).ok();
    }

    // No ledger row: the requirement still shows, the other two parts say so.
    #[test]
    fn requirement_card_without_a_ledger_row() {
        let tmp = std::env::temp_dir().join(format!("jazyk-lsp-card-none-{}", std::process::id()));
        std::fs::remove_dir_all(&tmp).ok();
        let out = tmp.join("jazyk-out");
        std::fs::create_dir_all(&out).unwrap();
        let r = requirement();
        let mut store = Store { out: out.clone(), ..Default::default() };
        store.graph.requirements.insert("req:shop-1".into(), r.clone());
        let lsp = Lsp {
            root: tmp.clone(),
            out,
            store,
            generation: 0,
            gen: GenSettings { deliverable: tmp.join("product"), worker: "agentic".into(), code: Vec::new() },
            overlay: HashMap::new(),
            next_srv_id: 1,
        };
        let card = lsp.requirement_card("req:shop-1", &r);
        assert!(card.contains("– missing"), "{}", card);
        assert!(card.contains("**code** · not generated"), "{}", card);
        assert!(card.contains("**test** · not generated"), "{}", card);
        std::fs::remove_dir_all(&tmp).ok();
    }

    // The interactive path over the wire: a prompted finding publishes with its
    // question, its options come back as code actions, and one executeCommand
    // applies the edit and resolves it. Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn prompted_diagnostic_publishes_offers_actions_and_applies() {
        use crate::model::{Diagnostic, DiagnosticPrompt, PromptOption, SuggestedEdit};
        use crate::store::Op;
        let tmp = std::env::temp_dir().join(format!("jazyk-lsp-answer-{}", std::process::id()));
        std::fs::remove_dir_all(&tmp).ok();
        let root = tmp.join("proj");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("jazyk.toml"), "[docs]\nglob = [\"docs/**/*.md\"]\n").unwrap();
        std::fs::write(root.join("docs/pay.md"), "# Pay\n\nAn Order shall be paid within 30 days.\n").unwrap();
        let project = crate::project::Project::load(&root);
        let out = project.out.clone();
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let mut s = Store::load(&out);
        s.sync_docs(&parsed);
        let sec = s.docs["docs/pay.md"].sections.keys().next().unwrap().clone();
        s.apply(
            vec![Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: Diagnostic {
                    rule: "contradiction".into(),
                    severity: "warning".into(),
                    subjects: vec![format!("docs/pay.md#{}", sec)],
                    message: "21 vs 30 days".into(),
                    reasoning: None,
                    lifecycle: "open".into(),
                    triage: None,
                    prompt: Some(DiagnosticPrompt {
                        question: "Which bound holds?".into(),
                        options: vec![
                            PromptOption {
                                label: "21 days; fix this file".into(),
                                edit: Some(SuggestedEdit {
                                    doc: "docs/pay.md".into(),
                                    section: sec.clone(),
                                    old_text: "within 30 days".into(),
                                    new_text: "within 21 days".into(),
                                }),
                                answer: None,
                            },
                            PromptOption { label: "30 days is right".into(), edit: None, answer: Some("keep 30".into()) },
                        ],
                        freeform: true,
                    }),
                    answer: None,
                    created: None,
                    updated: None,
                },
            }],
            &crate::model::WorkItem { task: "seed".into(), target: "t".into(), dirty_sections: vec![], stale_anchors: vec![], proposals: Vec::new() },
            0,
            0,
        );
        let id = s.graph.diagnostics.keys().next().unwrap().clone();
        drop(s);

        let mut lsp = Lsp::new(root.clone(), out.clone(), crate::gen::GenSettings::resolve(&project));
        let uri = path_to_uri(&root.join("docs/pay.md"));
        let mut wire: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri,
                "text": std::fs::read_to_string(root.join("docs/pay.md")).unwrap()}}}),
            &mut wire,
        );
        let published = String::from_utf8_lossy(&wire).to_string();
        assert!(published.contains("Which bound holds?"), "question published inline: {}", published);

        let mut wire2: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"id": 7, "method": "textDocument/codeAction", "params": {
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 99, "character": 0}}}}),
            &mut wire2,
        );
        let actions = String::from_utf8_lossy(&wire2).to_string();
        assert!(actions.contains("Apply: 21 days; fix this file"), "{}", actions);
        assert!(actions.contains("Answer: 30 days is right"), "{}", actions);

        let mut wire3: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"id": 8, "method": "workspace/executeCommand", "params": {
                "command": "jazyk.answerDiagnostic", "arguments": [{"id": id, "option": 0}]}}),
            &mut wire3,
        );
        let applied = String::from_utf8_lossy(&wire3).to_string();
        assert!(applied.contains("applied"), "{}", applied);
        let text = std::fs::read_to_string(root.join("docs/pay.md")).unwrap();
        assert!(text.contains("within 21 days"), "{}", text);
        // The refresh inside the command republished without the resolved finding.
        assert!(applied.contains("publishDiagnostics"), "{}", applied);
        assert!(!applied.contains("Which bound holds?"), "{}", applied);
        std::fs::remove_dir_all(&tmp).ok();
    }
}
