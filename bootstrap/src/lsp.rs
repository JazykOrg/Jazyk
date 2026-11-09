use crate::vfs::VPathExt;
// The language server: thin and read-only, per docs/frontends/lsp.md. It reads the
// graph store and maps nodes to editor positions. It never compiles; rebuilds run
// through `jazyk compile` or `jazyk watch`, and the store's generation counter tells
// this server when to reload and republish.
use crate::jsonrpc::{read_message, write_message};
use crate::md;
use crate::model::{Entity, VIEW_KINDS};
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
    // The cheap per-set generations plus the configuration hash the store was
    // loaded at; refresh reloads only when it moved (lsp.md#rebuilds-and-refresh).
    fingerprint: String,
    gen: crate::gen::GenSettings,
    // Open documents: project-relative doc path -> current editor text.
    overlay: HashMap<String, String>,
    // Ids for server-initiated requests (window/showDocument), prefixed so they can
    // never collide with the client's own numeric ids.
    next_srv_id: u64,
    // Where `jazyk.compile` sends a build (lsp.md#commands).
    compiler: Compiler,
}

// How `jazyk.compile` runs: over stdio in a thread under the build lease, with the
// LLM options `jazyk lsp` resolved; inside the GUI server as a GUI job.
// One per server, so the unboxed options cost nothing worth boxing.
#[allow(clippy::large_enum_variant)]
pub enum Compiler {
    Stdio {
        opts: crate::cli::Options,
        running: Option<(
            std::thread::JoinHandle<()>,
            std::sync::Arc<std::sync::atomic::AtomicBool>,
        )>,
    },
    // Submits one set's compile (set id, configuration hash) and answers the job.
    Submit(Box<dyn Fn(String, String) -> Value + Send>),
}

impl Compiler {
    // Cancel a running stdio build and wait for it to stop at a boundary.
    fn finish(&mut self) {
        if let Compiler::Stdio { running, .. } = self {
            if let Some((handle, cancel)) = running.take() {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = handle.join();
            }
        }
    }
}

// The one set `jazyk.compile` builds: `setId`, or the project's only set. In a project
// of several sets a call without one is refused (lsp.md#commands).
pub(crate) fn compile_target(root: &Path, arg: &Value) -> Result<(String, String), String> {
    let w = crate::document_sets::Workspace::load(root).map_err(|e| e.1)?;
    if let Some(id) = arg["setId"].as_str() {
        return w
            .select(id)
            .map(|s| (s.id.clone(), w.hash.clone()))
            .map_err(|e| e.1);
    }
    match w.sets.len() {
        0 => Err(
            "this project has no document set yet; add one with `jazyk sets add NAME --docs PATH`"
                .into(),
        ),
        1 => Ok((w.sets[0].id.clone(), w.hash.clone())),
        n => Err(format!(
            "choose a document set: this project has {} ({}); pass setId",
            n,
            w.sets
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

impl Lsp {
    pub fn new(root: PathBuf, out: PathBuf, gen: crate::gen::GenSettings) -> Lsp {
        let fingerprint = store_fingerprint(&root, &out);
        let store = read_project_store(&root, &out);
        let generation = store.status.generation;
        Lsp {
            root,
            out,
            store,
            generation,
            fingerprint,
            gen,
            overlay: HashMap::new(),
            next_srv_id: 1,
            compiler: Compiler::Stdio {
                opts: Default::default(),
                running: None,
            },
        }
    }

    // The options `jazyk lsp` was started with: a stdio compile resolves its LLM from
    // them like any CLI build.
    pub fn with_cli_options(mut self, opts: crate::cli::Options) -> Lsp {
        self.compiler = Compiler::Stdio {
            opts,
            running: None,
        };
        self
    }

    // Inside the GUI server a compile is a GUI job.
    pub fn with_job_submitter(
        mut self,
        submit: Box<dyn Fn(String, String) -> Value + Send>,
    ) -> Lsp {
        self.compiler = Compiler::Submit(submit);
        self
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
        spawn_store_watcher(self.root.clone(), self.out.clone(), tx);
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
        // A build started here stops at a boundary, never cut off mid-commit.
        self.compiler.finish();
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
        let method = msg
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        // The store is the single source of truth: reload when a compile moved it.
        self.refresh(out);
        match method.as_str() {
            "initialize" => reply(out, id, self.capabilities()),
            "jazyk/document" => {
                let result = params["uri"]
                    .as_str()
                    .and_then(|uri| self.uri_to_doc(uri))
                    .ok_or((400, "expected a document URI inside this project".into()))
                    .and_then(|doc| {
                        crate::webapi::binary_documents::read(
                            &crate::project::Project::load(&self.root),
                            &doc,
                        )
                    });
                match result {
                    Ok(value) => reply(out, id, value),
                    Err((code, message)) => write_message(
                        out,
                        &json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32602, "message":message, "data":{"status":code}}}),
                    ),
                }
            }
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
            // A new or removed link target changes source diagnostics even when
            // no build has changed the graph generation.
            "workspace/didChangeWatchedFiles" => self.publish_all(out),
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
            "textDocument/typeDefinition" => {
                let r = self.on_type_definition(&params);
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
            "textDocument/inlayHint" => {
                let r = self.on_inlay_hints(&params);
                reply(out, id, r);
            }
            "textDocument/semanticTokens/full" => {
                let r = self.on_semantic_tokens(&params);
                reply(out, id, r);
            }
            "textDocument/documentHighlight" => {
                let r = self.on_document_highlight(&params);
                reply(out, id, r);
            }
            "textDocument/documentSymbol" => {
                let r = self.on_document_symbols(&params);
                reply(out, id, r);
            }
            "workspace/symbol" => {
                let r = self.on_workspace_symbols(&params);
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
                "typeDefinitionProvider": true,
                "referencesProvider": true,
                "hoverProvider": true,
                "completionProvider": { "triggerCharacters": ["`", "["] },
                "documentLinkProvider": { "resolveProvider": false },
                "codeLensProvider": { "resolveProvider": false },
                "codeActionProvider": true,
                "inlayHintProvider": { "resolveProvider": false },
                "semanticTokensProvider": {
                    "legend": { "tokenTypes": SEMANTIC_TOKEN_TYPES, "tokenModifiers": SEMANTIC_TOKEN_MODIFIERS },
                    "full": true
                },
                "documentHighlightProvider": true,
                "documentSymbolProvider": true,
                "workspaceSymbolProvider": true,
                "executeCommandProvider": { "commands": ["jazyk.openRequirement", "jazyk.showLastBuild", "jazyk.answerDiagnostic", "jazyk.documentSets", "jazyk.previewDocumentSet", "jazyk.saveDocumentSet", "jazyk.compile"] }
            },
            "serverInfo": { "name": "jazyk", "version": env!("CARGO_PKG_VERSION") }
        })
    }

    // Reload the store when a set's generation counter or the configuration moved,
    // and republish every open document so the editor reflects the new build. The
    // comparison reads one line per set; the graph loads only on a change.
    pub(crate) fn refresh<W: Write>(&mut self, out: &mut W) {
        let now = store_fingerprint(&self.root, &self.out);
        if now == self.fingerprint {
            return;
        }
        self.fingerprint = now;
        let current = read_project_store(&self.root, &self.out);
        eprintln!(
            "[jazyk-lsp] store generation {} -> {}; reloading",
            self.generation, current.status.generation
        );
        self.generation = current.status.generation;
        self.store = current;
        self.publish_all(out);
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
        let root = uri_to_path(&path_to_uri(&self.root))?;
        let rel = path.strip_prefix(&root).ok()?;
        Some(rel.to_string_lossy().replace('\\', "/"))
    }

    fn doc_to_uri(&self, doc: &str) -> String {
        path_to_uri(&self.root.join(doc))
    }

    fn doc_text(&self, doc: &str) -> String {
        if let Some(t) = self.overlay.get(doc) {
            return t.clone();
        }
        md::read_doc(&self.root.join(doc)).unwrap_or_default()
    }

    // ---- anchoring ----

    // Range of a quote in a document: exact match first, then the first whole-word
    // occurrence of a fallback name, then the section's first line, then line 0.
    fn anchor(
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

    // ---- rendered views ----

    // The rendered `.svg` of a view, only when the build output exists on disk. The
    // hover never renders a view itself. Mirrors docs/frontends/lsp.md#capabilities.
    fn view_svg_path(&self, view_id: &str) -> Option<PathBuf> {
        let rel = view_id.strip_prefix("view:")?;
        let (kind, slug) = rel.split_once('/')?;
        let p = self
            .out
            .join("diagrams")
            .join(kind)
            .join(format!("{}.svg", slug));
        p.vexists().then_some(p)
    }

    // ---- the walk ----
    // The pages docsgen writes under <out>/docsgen/, by the same slugs it uses, so a
    // hover link and a type definition land on the file the last commit rendered.
    // Mirrors docs/consumers/docsgen.md#entity-cards and #diagram-pages.

    // An entity's card: `docsgen/entities/<slug>.md`.
    fn card_path(&self, id: &str) -> PathBuf {
        self.out
            .join("docsgen")
            .join("entities")
            .join(format!("{}.md", crate::ids::entity_slug(id)))
    }

    // An entity's requirements document: `docsgen/<slug>.md`.
    fn requirements_doc_path(&self, id: &str) -> PathBuf {
        self.out
            .join("docsgen")
            .join(format!("{}.md", crate::ids::entity_slug(id)))
    }

    // A view's diagram page: `docsgen/diagrams/<kind>/<slug>.md`.
    fn diagram_page_path(&self, view_id: &str) -> Option<PathBuf> {
        let rel = view_id.strip_prefix("view:")?;
        let (kind, slug) = rel.split_once('/')?;
        Some(
            self.out
                .join("docsgen")
                .join("diagrams")
                .join(kind)
                .join(format!("{}.md", slug)),
        )
    }

    // A level's page: `docsgen/levels/<slug>.md`, or `levels/scope-<scope>.md` for a
    // scope root.
    fn level_page_path(&self, target: &str) -> PathBuf {
        self.out
            .join("docsgen")
            .join(crate::docsgen::level_page(target))
    }

    // The entity's card in short, read from the shared model (card.rs) so the hover
    // shows what docsgen and the GUI show. In order: the header, `Sits in`, the
    // in-context image (the parent level's structural view), the `Inside` link with
    // the child count (never a second image), one line of links to the long reads,
    // and a derived entity's pending proposal. Blocks are separated by blank lines,
    // so every markdown renderer keeps them apart. None for an unknown id.
    // Mirrors docs/frontends/lsp.md#capabilities.
    fn entity_card_short(&self, id: &str) -> Option<String> {
        let walk = crate::card::Walk::new(&self.store);
        let card = crate::card::entity_card(&self.store, &walk, id)?;
        let mut s = format!("**{}**", card.name);
        if let Some(st) = &card.stereotype {
            s.push_str(&format!(" «{}»", st));
        }
        if !card.definition.is_empty() {
            s.push_str(&format!(" · {}", card.definition));
        }
        // Sits in: the breadcrumb as text, the entity itself last and bold.
        let n = card.breadcrumb.len();
        let crumbs: Vec<String> = card
            .breadcrumb
            .iter()
            .enumerate()
            .map(|(i, c)| {
                if i + 1 == n {
                    format!("**{}**", c.name)
                } else {
                    c.name.clone()
                }
            })
            .collect();
        s.push_str(&format!("\n\nSits in: {}", crumbs.join(" › ")));
        // In context: the build output the last commit rendered, captioned with the
        // view id. A missing rendering leaves the caption alone, linked to the
        // view's page; the hover never renders a view itself.
        if let Some(ctx) = &card.context {
            match self.view_svg_path(ctx) {
                Some(svg) => s.push_str(&format!("\n\n![{}]({})", ctx, path_to_uri(&svg))),
                None => {
                    let page = self
                        .diagram_page_path(ctx)
                        .map(|p| path_to_uri(&p))
                        .unwrap_or_default();
                    s.push_str(&format!(
                        "\n\nIn context: [{}]({}) (not rendered)",
                        ctx, page
                    ));
                }
            }
        }
        // Inside: the entity's own level, as a link to its diagram page.
        if let Some(inside) = &card.inside {
            let page = self
                .diagram_page_path(inside)
                .map(|p| path_to_uri(&p))
                .unwrap_or_default();
            let k = card.children.len();
            s.push_str(&format!(
                "\n\nInside: [{}]({}) ({} {})",
                inside,
                page,
                k,
                if k == 1 { "child" } else { "children" }
            ));
        }
        // The long reads: the card, the requirements document, the level page of the
        // parent's level (only when the parent has one, as its context view says).
        let mut links = vec![
            format!("[card]({})", path_to_uri(&self.card_path(&card.id))),
            format!(
                "[requirements ({})]({})",
                card.requirement_count,
                path_to_uri(&self.requirements_doc_path(&card.id))
            ),
        ];
        if card.context.is_some() && n >= 2 {
            let parent = &card.breadcrumb[n - 2];
            links.push(format!(
                "[level: {}]({})",
                parent.name,
                path_to_uri(&self.level_page_path(&parent.id))
            ));
        }
        s.push_str(&format!("\n\n{}", links.join(" · ")));
        // A derived or decreed entity says so and names its pending proposal.
        if card.provenance != "quote" {
            match &card.proposal {
                Some(did) => s.push_str(&format!(
                    "\n\n{} · proposal pending: `{}`",
                    card.provenance, did
                )),
                None => s.push_str(&format!("\n\n{} · no proposal pending", card.provenance)),
            }
        }
        Some(s)
    }

    // The image of the smallest flow view listing a requirement as a member, so the
    // step shows in its flow above the card. Same tie rule as for entities.
    fn flow_view_image(&self, rid: &str) -> Option<String> {
        const FLOW: [&str; 4] = ["use-case", "activity", "sequence", "communication"];
        let mut best: Option<(usize, usize, String)> = None;
        for (vid, v) in &self.store.graph.views {
            if !FLOW.contains(&v.kind.as_str()) {
                continue;
            }
            if !v.members.iter().any(|m| self.store.resolve_id(m) == rid) {
                continue;
            }
            if self.view_svg_path(vid).is_none() {
                continue;
            }
            let pos = VIEW_KINDS
                .iter()
                .position(|k| *k == v.kind)
                .unwrap_or(usize::MAX);
            let key = (v.members.len(), pos, vid.clone());
            if best.as_ref().map(|b| key < *b).unwrap_or(true) {
                best = Some(key);
            }
        }
        let (_, _, vid) = best?;
        let v = &self.store.graph.views[&vid];
        let svg = self.view_svg_path(&vid)?;
        Some(format!(
            "![{} ({})]({})\n\n",
            v.title,
            v.kind,
            path_to_uri(&svg)
        ))
    }

    // ---- request handlers ----

    fn on_definition(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else {
            return Value::Null;
        };
        let Some((_, e)) = self.entity_at(&doc, line, ch) else {
            return Value::Null;
        };
        // The defining mention is the first one recorded.
        let Some(m) = e.mentions.first() else {
            return Value::Null;
        };
        let r = self.anchor(&m.doc, &m.quote, &e.name, Some(&m.section));
        json!({ "uri": self.doc_to_uri(&m.doc), "range": self.range(r) })
    }

    fn on_references(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else {
            return json!([]);
        };
        let Some((_, e)) = self.entity_at(&doc, line, ch) else {
            return json!([]);
        };
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

    // Go to type definition: the entity's card at line 0. Definition stays the
    // defining mention in the prose; the type definition is the walk.
    // Mirrors docs/frontends/lsp.md#capabilities.
    fn on_type_definition(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else {
            return Value::Null;
        };
        let Some((id, _)) = self.entity_at(&doc, line, ch) else {
            return Value::Null;
        };
        json!({
            "uri": path_to_uri(&self.card_path(&id)),
            "range": self.range((0, 0, 0, 0))
        })
    }

    // Hover on an entity renders its card in short, with a verification summary from
    // the ledger. Hovering inside a requirement's located quote shows that
    // requirement's own card. Mirrors docs/frontends/lsp.md#capabilities.
    fn on_hover(&self, params: &Value) -> Value {
        let Some((doc, line, ch)) = self.pos(params) else {
            return Value::Null;
        };
        if let Some((id, _)) = self.entity_at(&doc, line, ch) {
            let Some(mut value) = self.entity_card_short(&id) else {
                return Value::Null;
            };
            let vmap = crate::verify::status_map(&self.store, &self.gen);
            let refs = self.store.requirements_referencing(&id);
            // Only requirements the ledger holds a row for count: a project that
            // never generated shows no verification line at all.
            let statuses: Vec<&str> = refs
                .iter()
                .filter_map(|r| vmap.get(r).and_then(|v| v["status"].as_str()))
                .filter(|s| *s != "missing")
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
            if let Some(q) = r
                .source
                .as_ref()
                .and_then(|s| md::locate(&self.doc_text(&doc), &s.quote))
            {
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
        if let (Some(id), local) = crate::webapi::sets::unqualify(rid) {
            if let Ok(p) = crate::project::Project::load_set(&self.root, &id) {
                let scoped = Self::new(
                    p.root.clone(),
                    p.out.clone(),
                    crate::gen::GenSettings::resolve(&p),
                );
                if let Some(r) = scoped.store.graph.requirements.get(&local) {
                    return format!(
                        "**{}**\n\n{}",
                        p.set_name.unwrap_or(id),
                        scoped.requirement_card(&local, r)
                    );
                }
            }
        }
        let ledger = crate::gen::Ledger::load(&self.out);
        let row = ledger.requirements.get(rid);
        let (status, reason) = match row {
            Some(row) => crate::verify::status_of(&self.store, rid, row, &self.gen),
            None => ("missing".to_string(), "not-generated".to_string()),
        };
        let mut s = String::new();
        // The smallest flow view listing this requirement sits above the card.
        if let Some(img) = self.flow_view_image(rid) {
            s.push_str(&img);
        }
        // The header reads the status word; the test part keeps the raw status.
        let word = crate::verify::status_word(
            &status,
            &reason,
            crate::verify::questioned(&self.store).contains(rid),
        );
        s.push_str(&format!(
            "**`{}`** · {} {}\n\n{}\n\n",
            rid,
            word_glyph(word),
            word,
            r.statement
        ));
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
                let text = crate::vfs::read_to_string(&abs).unwrap_or_default();
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
        let label = if t.label == t.kind {
            t.kind.clone()
        } else {
            format!("{} · {}", t.kind, t.label)
        };
        s.push_str(&format!("**test** · {}\n\n", label));
        let line = if t.name.is_empty() {
            None
        } else {
            crate::vfs::read_to_string(&artifact).ok().and_then(|text| {
                text.lines()
                    .position(|l| l.contains(&t.name))
                    .map(|i| i + 1)
            })
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
                if artifact.vexists() {
                    ""
                } else {
                    " · artifact gone"
                }
            )),
        }
        s.push_str(&format!(
            "- {} {} · {}",
            status_glyph(&status),
            status,
            reason
        ));
        if let Some(last) = &row.last_run {
            s.push_str(&format!(" · last run {}", last));
        }
        s.push('\n');
        s.push_str(&format!("- run `{}`\n", t.run));
        if let Some(ev) = &row.evidence {
            s.push_str(&format!(
                "\n> {}\n",
                ev.split_whitespace().collect::<Vec<_>>().join(" ")
            ));
        }
        s
    }

    // Link to a requirement's heading in its entity's requirements document. None when
    // the document does not exist, so the card never dangles.
    fn docsgen_link(&self, rid: &str, r: &crate::model::Requirement) -> Option<String> {
        let ent = r.entities.first()?;
        let slug = self
            .store
            .resolve_id(ent)
            .strip_prefix("ent:")
            .unwrap_or(ent)
            .to_string();
        let path = self.out.join("docsgen").join(format!("{}.md", slug));
        let text = crate::vfs::read_to_string(&path).ok()?;
        let heading = format!("### `{}`", rid);
        let line = text
            .lines()
            .position(|l| l.trim() == heading)
            .map(|i| i + 1)
            .unwrap_or(1);
        Some(format!("{}?req={}#L{}", path_to_uri(&path), rid, line))
    }

    // The requirement whose located quote contains the position, if any.
    fn requirement_at(
        &self,
        doc: &str,
        line: usize,
        character: usize,
    ) -> Option<(String, &crate::model::Requirement)> {
        let text = self.doc_text(doc);
        for (rid, r) in &self.store.graph.requirements {
            let Some(src) = r.source.as_ref().filter(|s| s.doc == doc) else {
                continue;
            };
            if let Some((sl, sc, el, ec)) = md::locate(&text, &src.quote) {
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
        let Some(doc) = self.param_doc(params) else {
            return json!([]);
        };
        let text = self.doc_text(&doc);
        let docsgen = |id: &str| {
            let slug = id.strip_prefix("ent:").unwrap_or(id);
            self.out.join("docsgen").join(format!("{}.md", slug))
        };
        let links: Vec<Value> = self
            .entity_mentions(&text, |id| docsgen(id).vexists())
            .into_iter()
            .take(200)
            .map(|(line, col, len, id)| {
                let name = self
                    .store
                    .graph
                    .entities
                    .get(&id)
                    .map(|e| e.name.as_str())
                    .unwrap_or(&id);
                json!({
                    "range": self.range((line, col, line, col + len)),
                    "target": path_to_uri(&docsgen(&id)),
                    "tooltip": format!("{}: requirements document", name)
                })
            })
            .collect();
        json!(links)
    }

    // One lens above each requirement's located quote: the attachment made visible
    // without hovering. The title is the requirement id, plus its verification status
    // when the ledger has one. Emitted only where the quote locates, so a broken
    // quote never shows a misplaced lens.
    // Mirrors docs/frontends/lsp.md#capabilities.
    fn on_code_lens(&self, params: &Value) -> Value {
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
    fn on_code_action(&self, params: &Value) -> Value {
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
        let mut project = self
            .out
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|id| crate::project::Project::load_set(&self.root, id).ok())
            .unwrap_or_else(|| crate::project::Project::load(&self.root));
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

    // `jazyk.compile`: one set, through the frontend's build path (lsp.md#commands).
    fn on_compile(&mut self, arg: &Value) -> Value {
        let (id, hash) = match compile_target(&self.root, arg) {
            Ok(t) => t,
            Err(e) => return json!({"error": e}),
        };
        match &mut self.compiler {
            Compiler::Submit(submit) => submit(id, hash),
            Compiler::Stdio { opts, running } => {
                if running.as_ref().is_some_and(|(h, _)| !h.is_finished()) {
                    return json!({"error": "a compile started here is still running"});
                }
                let p = match crate::project::Project::load_set(&self.root, &id) {
                    Ok(p) => p,
                    Err(e) => return json!({"error": e.1}),
                };
                let llm = crate::cli::resolve_llm(
                    opts,
                    &p.llm,
                    &crate::project::load_global_llm(),
                    |n| std::env::var(n).ok(),
                );
                let trace = crate::session::Trace::stderr(crate::session::TraceLevel::Normal);
                let cancel = trace.cancel_flag();
                let handle = std::thread::spawn(move || {
                    // The configuration the caller saw is the one built.
                    if crate::document_sets::Workspace::load(&p.root).is_ok_and(|w| w.hash == hash)
                    {
                        crate::reconcile::compile(&p, &llm, &p.out, &trace);
                    }
                });
                *running = Some((handle, cancel));
                json!({"started": [id]})
            }
        }
    }

    fn on_execute_command<W: Write>(&mut self, params: &Value, out: &mut W) -> Value {
        let cmd = params.get("command").and_then(|c| c.as_str()).unwrap_or("");
        if cmd == "jazyk.documentSets" {
            return crate::document_sets::Workspace::load(&self.root)
                .map(|w| w.summary())
                .unwrap_or_else(|e| json!({"error":e.1}));
        }
        if matches!(cmd, "jazyk.previewDocumentSet" | "jazyk.saveDocumentSet") {
            return crate::document_sets::edit(
                &self.root,
                &params["arguments"][0],
                cmd == "jazyk.previewDocumentSet",
            )
            .unwrap_or_else(|e| json!({"error":e.1}));
        }
        if cmd == "jazyk.compile" {
            return self.on_compile(&params["arguments"][0]);
        }
        if cmd == "jazyk.showLastBuild" {
            return self.on_show_last_build(&params["arguments"][0], out);
        }
        if !self
            .out
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(crate::document_sets::valid_id)
        {
            let routed = match crate::webapi::sets::route("/", params.clone()) {
                Ok(r) => r,
                Err(e) => return json!({"error":e.1}),
            };
            if let Some(id) = routed.selector {
                let p = match crate::project::Project::load_set(&self.root, &id) {
                    Ok(p) => p,
                    Err(e) => return json!({"error":e.1}),
                };
                let mut local = Self::new(
                    p.root.clone(),
                    p.out.clone(),
                    crate::gen::GenSettings::resolve(&p),
                );
                local.overlay = self.overlay.clone();
                local.next_srv_id = self.next_srv_id;
                let value = local.on_execute_command(&routed.body, out);
                self.next_srv_id = local.next_srv_id;
                return crate::webapi::sets::qualify_value(value, &id);
            }
            if cmd == "jazyk.answerDiagnostic" {
                return json!({"error":"use the qualified diagnostic ID from diagnostics"});
            }
        }
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
        let Some(r) = self.store.graph.requirements.get(&resolved) else {
            return Value::Null;
        };
        let Some(ent) = r.entities.first() else {
            return Value::Null;
        };
        let slug = self
            .store
            .resolve_id(ent)
            .strip_prefix("ent:")
            .unwrap_or(ent)
            .to_string();
        let path = self.out.join("docsgen").join(format!("{}.md", slug));
        let Ok(text) = crate::vfs::read_to_string(&path) else {
            eprintln!(
                "[jazyk-lsp] {}: no requirements document at {}",
                resolved,
                path.display()
            );
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

    // ---- status words ----

    // Whether this server serves one set's out directory rather than the project.
    fn serves_one_set(&self) -> bool {
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
    fn on_inlay_hints(&self, params: &Value) -> Value {
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
        let mut hints: Vec<Value> = Vec::new();
        for (rid, (_, _, el, ec)) in self.located_requirements(&doc, &text) {
            if (el, ec) < from || (el, ec) > to {
                continue;
            }
            let Some(st) = statuses.get(&rid) else {
                continue;
            };
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
    fn entity_mentions(
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
    fn on_semantic_tokens(&self, params: &Value) -> Value {
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
    fn on_document_highlight(&self, params: &Value) -> Value {
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
    fn on_document_symbols(&self, params: &Value) -> Value {
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
    fn on_workspace_symbols(&self, params: &Value) -> Value {
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

    // The out directory of the set that holds a document: this server's own when it
    // serves one set, else the first set whose documents match the path.
    fn set_out_for(&self, doc: &str) -> PathBuf {
        if self.serves_one_set() {
            return self.out.clone();
        }
        crate::document_sets::Workspace::load(&self.root)
            .ok()
            .and_then(|w| {
                let id = w
                    .memberships(doc)
                    .into_iter()
                    .find(|m| m["role"] == "docs")?["setId"]
                    .as_str()?
                    .to_string();
                w.resolve(Some(&id)).ok().map(|p| p.out)
            })
            .unwrap_or_else(|| self.out.clone())
    }

    // jazyk.showLastBuild {uri}: write the document as the last build stored it to
    // `<set out>/last-build/<doc>` and ask the client to open it. A projection, safe
    // to delete; the graph is never touched. Mirrors docs/frontends/lsp.md#commands.
    fn on_show_last_build<W: Write>(&mut self, arg: &Value, out: &mut W) -> Value {
        let Some(doc) = arg["uri"].as_str().and_then(|u| self.uri_to_doc(u)) else {
            return json!({"error": "jazyk.showLastBuild needs {uri} of a project document"});
        };
        let Some(record) = self.store.docs.get(&doc) else {
            return json!({"error": format!("{} has no last build", doc)});
        };
        let mut parts: Vec<&crate::model::Section> = record
            .sections
            .values()
            .filter(|s| matches!(s.kind.as_str(), "preamble" | "root" | "heading"))
            .collect();
        parts.sort_by_key(|s| s.lines[0]);
        let mut built = parts
            .iter()
            .map(|s| s.raw.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if !built.is_empty() {
            built.push('\n');
        }
        let path = self.set_out_for(&doc).join("last-build").join(&doc);
        if let Some(dir) = path.parent() {
            if let Err(e) = crate::vfs::create_dir_all(dir) {
                return json!({"error": format!("cannot create {}: {}", dir.display(), e)});
            }
        }
        if let Err(e) = crate::vfs::write(&path, built) {
            return json!({"error": format!("cannot write {}: {}", path.display(), e)});
        }
        let uri = path_to_uri(&path);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": format!("jazyk-srv-{}", self.next_srv_id),
            "method": "window/showDocument",
            "params": { "uri": uri, "takeFocus": true }
        });
        self.next_srv_id += 1;
        write_message(out, &msg);
        json!({ "uri": uri })
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
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

// The semantic token legend: entity mentions are `type` tokens with the
// `jazykEntity` modifier. Mirrors docs/frontends/lsp.md#capabilities.
const SEMANTIC_TOKEN_TYPES: [&str; 1] = ["type"];
const SEMANTIC_TOKEN_MODIFIERS: [&str; 1] = ["jazykEntity"];

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

// The length of one line in UTF-16 code units, 0 past the end.
fn line_len(text: &str, line: usize) -> usize {
    text.lines()
        .nth(line)
        .map(|l| l.encode_utf16().count())
        .unwrap_or(0)
}

// One glyph per status word, so a word reads at a glance in clients that render no
// colour. Mirrors docs/consumers/gen.md#status-words.
fn word_glyph(word: &str) -> &'static str {
    match word {
        "verified" => "✓",
        "failing" => "✗",
        "stale" => "↻",
        "question" => "?",
        _ => "–",
    }
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
    url::Url::from_file_path(path)
        .map(|uri| uri.to_string())
        .unwrap_or_default()
}

// Whole-word, case-insensitive occurrences of `needle` in `text`, as
// (line, start_col, len) in 0-based UTF-16 columns, the LSP's position encoding.
fn occurrences(text: &str, needle: &str) -> Vec<(usize, usize, usize)> {
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
fn spawn_store_watcher(root: PathBuf, out: PathBuf, tx: std::sync::mpsc::Sender<Event>) {
    std::thread::spawn(move || {
        let mut last_gen = store_fingerprint(&root, &out);
        let mut lock_seen = out.join(".lock").vexists();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let lock_now = out.join(".lock").vexists();
            if lock_now != lock_seen {
                lock_seen = lock_now;
                eprintln!(
                    "[jazyk-build] {}",
                    if lock_now {
                        "build started (lock acquired)"
                    } else {
                        "build ended (lock released)"
                    }
                );
            }
            let gen_now = store_fingerprint(&root, &out);
            if gen_now != last_gen {
                last_gen = gen_now;
                if tx.send(Event::StoreChanged).is_err() {
                    return;
                }
            }
        }
    });
}

// What a reload keys on: the configuration hash and one line of each set's
// status.yaml (or the one set's generation when the out directory is a set).
// Mirrors docs/frontends/lsp.md#rebuilds-and-refresh.
fn store_fingerprint(root: &Path, out: &Path) -> String {
    let own = || crate::store::read_generation(out).to_string();
    if out
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(crate::document_sets::valid_id)
    {
        return own();
    }
    let Ok(w) = crate::document_sets::Workspace::load(root) else {
        return own();
    };
    if !w.exists {
        return own();
    }
    let mut v = w.hash.clone();
    for set in &w.sets {
        if let Ok(p) = w.resolve(Some(&set.id)) {
            v.push_str(&format!(
                "{}:{};",
                set.id,
                crate::store::read_generation(&p.out)
            ));
        }
    }
    v
}

// An in-memory projection only. It is never saved or passed to a graph mutator.
fn read_project_store(root: &Path, out: &Path) -> Store {
    if out
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(crate::document_sets::valid_id)
    {
        return Store::load(out);
    }
    let Ok(w) = crate::document_sets::Workspace::load(root) else {
        return Store::load(out);
    };
    if !w.exists {
        return Store::load(out);
    }
    let mut combined = Store {
        out: out.into(),
        ..Default::default()
    };
    let mut graph = serde_json::to_value(&combined.graph).unwrap();
    let mut version = w.hash.clone();
    for set in &w.sets {
        let p = w.resolve(Some(&set.id)).unwrap();
        let store = Store::load(&p.out);
        version.push_str(&format!("{}:{};", set.id, store.status.generation));
        let value = crate::webapi::sets::qualify_value(
            serde_json::to_value(&store.graph).unwrap(),
            &set.id,
        );
        for (kind, map) in value.as_object().unwrap() {
            if let (Some(to), Some(from)) = (graph[kind].as_object_mut(), map.as_object()) {
                to.extend(from.clone());
            }
        }
        for (path, doc) in store.docs {
            combined.docs.entry(path).or_insert(doc);
        }
    }
    combined.graph = serde_json::from_value(graph).unwrap_or_default();
    combined.status.generation =
        u64::from_str_radix(&crate::model::hash_hex(&version)[..16], 16).unwrap_or(0);
    combined
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
fn section_anchor(text: &str, s: &crate::model::Section) -> Option<(usize, usize, usize, usize)> {
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
fn section_lead(raw: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    #[test]
    fn section_findings_anchor_on_heading_or_first_sentence() {
        assert_eq!(
            section_lead("## Payments\n\nA payment is captured.\n").as_deref(),
            Some("## Payments")
        );
        assert_eq!(
            section_lead("\nThe shop sells books online. A customer fills a cart.\n").as_deref(),
            Some("The shop sells books online.")
        );
        assert_eq!(
            section_lead("Orders ship\nin one day. Then more.\n\nNext.").as_deref(),
            Some("Orders ship in one day.")
        );
        assert_eq!(
            section_lead("A list of things\n").as_deref(),
            Some("A list of things")
        );
        assert_eq!(
            section_lead("```plantuml\n@startuml\n```").as_deref(),
            Some("```plantuml")
        );
        let text = "# Shop\n\nIntro.\n\n## Payments\n\nA payment is captured.\n";
        let sec: crate::model::Section = serde_json::from_value(serde_json::json!({
            "title": "Payments", "kind": "heading", "order": 0,
            "raw": "## Payments\n\nA payment is captured.\n", "hash": "x"
        }))
        .unwrap();
        assert_eq!(section_anchor(text, &sec), Some((4, 0, 4, 11)));
    }

    // The refresh keys on one line per set plus the configuration hash, and loads
    // the graph only when one moved (docs/frontends/lsp.md#rebuilds-and-refresh).
    #[test]
    fn refresh_reloads_only_when_a_set_generation_or_the_configuration_moved() {
        let tmp = std::env::temp_dir().join(format!("jz-lsp-fingerprint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".jazyk")).unwrap();
        let config = |extra: &str| {
            format!(
                "version = 1\n\n[[sets]]\nid = \"ds_a\"\nname = \"A\"\ndocs = [\"a/**\"]\n\n[[sets]]\nid = \"ds_b\"\nname = \"B\"\ndocs = [\"b/**\"]\n{}",
                extra
            )
        };
        std::fs::write(tmp.join(".jazyk/jazyk.toml"), config("")).unwrap();
        for id in ["ds_a", "ds_b"] {
            let mut s = crate::store::Store::load(&tmp.join(".jazyk").join(id));
            s.status.generation = 1;
            s.save_for_test();
        }
        let out = tmp.join(".jazyk");
        let first = super::store_fingerprint(&tmp, &out);
        assert_eq!(first, super::store_fingerprint(&tmp, &out));
        let gs = crate::gen::GenSettings {
            configured: false,
            deliverable: tmp.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        let mut lsp = super::Lsp::new(tmp.clone(), out.clone(), gs);
        let loaded = lsp.generation;
        let mut sink = Vec::new();
        lsp.refresh(&mut sink);
        assert_eq!(lsp.generation, loaded);
        assert!(sink.is_empty(), "no reload, nothing republished");
        // One set's generation moves: the fingerprint and the loaded store follow.
        let mut s = crate::store::Store::load(&out.join("ds_b"));
        s.status.generation = 2;
        s.save_for_test();
        let second = super::store_fingerprint(&tmp, &out);
        assert_ne!(first, second);
        lsp.refresh(&mut sink);
        assert_ne!(lsp.generation, loaded);
        assert_eq!(lsp.fingerprint, second);
        // The configuration moves without any generation: a reload too.
        std::fs::write(
            tmp.join(".jazyk/jazyk.toml"),
            config("\n[llm]\nmodel = \"m\"\n"),
        )
        .unwrap();
        assert_ne!(second, super::store_fingerprint(&tmp, &out));
        // A set's own out directory keys on its generation alone.
        assert_eq!(super::store_fingerprint(&tmp, &out.join("ds_b")), "2");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn occurrences_report_columns_on_the_original_text() {
        // Columns are UTF-16 units: an emoji counts two.
        assert_eq!(super::occurrences("😀 order", "order"), vec![(0, 3, 5)]);
        assert_eq!(
            super::occurrences("a\n𝔸 Order 😀x", "order"),
            vec![(1, 3, 5)]
        );
        let hits = super::occurrences("İstanbul order Order", "order");
        assert_eq!(hits, vec![(0, 9, 5), (0, 15, 5)]);
        let hits = super::occurrences("orders order", "order");
        assert_eq!(hits, vec![(0, 7, 5)]);
        assert!(super::occurrences("İİİ", "x").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn file_urls_find_documents_under_a_verbatim_windows_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let lsp = lsp_at(root.clone(), root.join(".jazyk"), Store::default());
        let uri = super::path_to_uri(&root.join("docs/main.md"));
        assert_eq!(lsp.uri_to_doc(&uri), Some("docs/main.md".into()));
    }

    // `jazyk.compile` builds one set: the named one, or the only one; in a project of
    // several sets a call without setId starts nothing. Inside the GUI it is a job
    // (docs/frontends/lsp.md#commands).
    #[test]
    fn the_compile_command_addresses_one_set_through_the_frontends_build_path() {
        use std::sync::{Arc, Mutex};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let add = |id: &str| {
            let w = crate::document_sets::Workspace::load(&root).unwrap();
            crate::document_sets::edit(&root, &json!({"baseHash": w.hash, "set": {"id": id, "name": id, "docs": [format!("{}.md", id)]}}), false).unwrap();
        };
        add("ds_one");
        let submitted: Arc<Mutex<Vec<(String, String)>>> = Default::default();
        let sink = submitted.clone();
        let mut lsp = Lsp::new(
            root.clone(),
            root.join(".jazyk"),
            GenSettings::resolve(&crate::project::Project::load(&root)),
        )
        .with_job_submitter(Box::new(move |id, hash| {
            sink.lock().unwrap().push((id.clone(), hash));
            json!({"started": [id], "jobId": 7})
        }));
        let call = |lsp: &mut Lsp, args: Value| {
            let mut out = Vec::new();
            lsp.on_execute_command(
                &json!({"command": "jazyk.compile", "arguments": [args]}),
                &mut out,
            )
        };
        let reply = call(&mut lsp, json!({}));
        assert_eq!(reply["started"], json!(["ds_one"]));
        assert_eq!(reply["jobId"], 7);
        add("ds_two");
        let reply = call(&mut lsp, json!({}));
        assert!(
            reply["error"]
                .as_str()
                .unwrap()
                .starts_with("choose a document set"),
            "{}",
            reply
        );
        let reply = call(&mut lsp, json!({"setId": "ds_two"}));
        assert_eq!(reply["started"], json!(["ds_two"]));
        let calls = submitted.lock().unwrap().clone();
        assert_eq!(
            calls.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
            vec!["ds_one", "ds_two"]
        );
        assert_eq!(
            calls[1].1,
            crate::document_sets::Workspace::load(&root).unwrap().hash
        );
        // Over stdio the build runs in a thread the server waits for on exit.
        let mut stdio = Lsp::new(
            root.clone(),
            root.join(".jazyk"),
            GenSettings::resolve(&crate::project::Project::load(&root)),
        );
        assert!(call(&mut stdio, json!({})).get("error").is_some());
        let reply = call(&mut stdio, json!({"setId": "ds_one"}));
        assert_eq!(reply["started"], json!(["ds_one"]));
        stdio.compiler.finish();
        assert!(matches!(
            &stdio.compiler,
            Compiler::Stdio { running: None, .. }
        ));
    }

    #[test]
    fn file_urls_round_trip_native_paths_with_unicode_and_delimiters() {
        for name in ["a b.md", "%aé.md", "café.md", "%.md", "%2.md", "a#b.md"] {
            let path = std::env::temp_dir().join(name);
            let uri = super::path_to_uri(&path);
            assert_eq!(super::uri_to_path(&uri), Some(path), "{uri}");
        }
        assert!(super::uri_to_path("https://example.org/a.md").is_none());
    }

    use super::*;
    use crate::gen::{GenSettings, Ledger, ReqRow, RowHashes, Site, TestRef};
    use crate::model::{Requirement, SourceRef};

    #[cfg(all(feature = "native-documents", not(target_arch = "wasm32")))]
    #[test]
    fn docx_warning_publishes_against_the_native_projection() {
        let root =
            std::env::temp_dir().join(format!("jazyk-lsp-docx-{}", crate::sys::process_id()));
        crate::vfs::create_dir_all(&root).unwrap();
        crate::vfs::write(
            root.join("contract.docx"),
            include_bytes!("../tests/fixtures/document-proof/contract.docx"),
        )
        .unwrap();
        let project = crate::project::Project::load(&root);
        let mut lsp = Lsp::new(
            root.clone(),
            root.join(".jazyk"),
            GenSettings::resolve(&project),
        );
        let mut bytes = Vec::new();
        lsp.publish(&mut bytes, "contract.docx");
        let msg = read_message(&mut std::io::BufReader::new(bytes.as_slice())).unwrap();
        assert_eq!(msg["method"], "textDocument/publishDiagnostics");
        let warning = msg["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["code"] == "broken-link")
            .unwrap();
        let text = lsp.doc_text("contract.docx");
        let line = text
            .lines()
            .nth(warning["range"]["start"]["line"].as_u64().unwrap() as usize)
            .unwrap();
        assert_eq!(line, "Reference: [contract](missing.md)");
        assert_eq!(warning["range"]["start"]["character"], 11);
        let mut response = Vec::new();
        lsp.handle(json!({"jsonrpc":"2.0", "id":1, "method":"jazyk/document", "params":{"uri":lsp.doc_to_uri("contract.docx")}}), &mut response);
        let msg = read_message(&mut std::io::BufReader::new(response.as_slice())).unwrap();
        assert_eq!(msg["result"]["format"], "docx");
        assert_eq!(msg["result"]["projection"]["text"], text);
        assert!(!msg["result"]["diagnostics"][0]["locations"]
            .as_array()
            .unwrap()
            .is_empty());
        crate::vfs::remove_dir_all(&root).unwrap();
    }

    fn requirement() -> Requirement {
        Requirement {
            statement: "The Cart shall hold items.".into(),
            entities: vec!["ent:cart".into()],
            edges: vec![],
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop".into(),
                quote: "holds".into(),
            }),
            ..Default::default()
        }
    }

    // The card: the requirement linked to its docsgen heading, the code linked at the
    // line each site relocates to, and the test linked at its name.
    // Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn requirement_card_links_the_requirement_code_and_test() {
        let tmp = std::env::temp_dir().join(format!("jazyk-lsp-card-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&tmp).ok();
        let root = tmp.join("proj");
        let out = root.join(".jazyk");
        let deliv = tmp.join("product");
        crate::vfs::create_dir_all(deliv.join("src")).unwrap();
        crate::vfs::create_dir_all(deliv.join("tests")).unwrap();
        crate::vfs::create_dir_all(out.join("docsgen")).unwrap();
        // The site anchored at line 1; an inserted header moved it to line 3.
        crate::vfs::write(
            deliv.join("src/cart.rs"),
            "// header\n\nfn hold(i: Item) {}\n",
        )
        .unwrap();
        crate::vfs::write(
            deliv.join("tests/cart.rs"),
            "#[test]\nfn req_shop_1_abcd() {}\n",
        )
        .unwrap();
        crate::vfs::write(
            out.join("docsgen").join("cart.md"),
            "# Cart\n\n### `req:shop-1`\n",
        )
        .unwrap();

        let mut ledger = Ledger::default();
        ledger.requirements.insert(
            "req:shop-1".into(),
            ReqRow {
                tests: std::collections::BTreeMap::new(),
                entity: "ent:cart".into(),
                files: vec!["src/cart.rs".into(), "src/checkout.rs".into()],
                sites: vec![Site {
                    file: "src/cart.rs".into(),
                    line: 1,
                    head: "fn hold(i: Item) {}".into(),
                }],
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
        ledger.save(&out).unwrap();

        let r = requirement();
        let mut store = Store {
            out: out.clone(),
            ..Default::default()
        };
        store
            .graph
            .requirements
            .insert("req:shop-1".into(), r.clone());
        let fingerprint = store_fingerprint(&root, &out);
        let lsp = Lsp {
            root,
            out,
            store,
            generation: 0,
            fingerprint,
            gen: GenSettings {
                configured: true,
                deliverable: deliv,
                worker: "agentic".into(),
                code: Vec::new(),
            },
            overlay: HashMap::new(),
            next_srv_id: 1,
            compiler: Compiler::Stdio {
                opts: Default::default(),
                running: None,
            },
        };
        let card = lsp.requirement_card("req:shop-1", &r);

        assert!(card.contains("**`req:shop-1`**"), "{}", card);
        assert!(card.contains("The Cart shall hold items."), "{}", card);
        // The requirement link: the docsgen heading, with the id for clients that route
        // to the node itself.
        assert!(
            card.contains("docsgen/cart.md?req=req:shop-1#L3"),
            "{}",
            card
        );
        // The code: the site relocated from line 1 to line 3, and marked moved. The
        // manifest file with no site links to the file itself.
        assert!(card.contains("[`src/cart.rs:3`]"), "{}", card);
        assert!(card.contains("#L3) · moved"), "{}", card);
        assert!(card.contains("[`src/checkout.rs`]"), "{}", card);
        // The test: the artifact at the line its name sits on, and the derived status
        // (the row's requirement hash is empty, so the statement moved under it).
        assert!(card.contains("[`tests/cart.rs:2`]"), "{}", card);
        assert!(card.contains("`req_shop_1_abcd`"), "{}", card);
        assert!(
            card.contains("↻ stale-requirement · requirement-changed"),
            "{}",
            card
        );
        assert!(
            card.contains("run `cargo test req_shop_1_abcd`"),
            "{}",
            card
        );
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // No ledger row: the requirement still shows, the other two parts say so.
    #[test]
    fn requirement_card_without_a_ledger_row() {
        let tmp =
            std::env::temp_dir().join(format!("jazyk-lsp-card-none-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&tmp).ok();
        let out = tmp.join(".jazyk");
        crate::vfs::create_dir_all(&out).unwrap();
        let r = requirement();
        let mut store = Store {
            out: out.clone(),
            ..Default::default()
        };
        store
            .graph
            .requirements
            .insert("req:shop-1".into(), r.clone());
        let fingerprint = store_fingerprint(&tmp, &out);
        let lsp = Lsp {
            root: tmp.clone(),
            out,
            store,
            generation: 0,
            fingerprint,
            gen: GenSettings {
                configured: true,
                deliverable: tmp.join("product"),
                worker: "agentic".into(),
                code: Vec::new(),
            },
            overlay: HashMap::new(),
            next_srv_id: 1,
            compiler: Compiler::Stdio {
                opts: Default::default(),
                running: None,
            },
        };
        let card = lsp.requirement_card("req:shop-1", &r);
        assert!(card.contains("– not built"), "{}", card);
        assert!(card.contains("**code** · not generated"), "{}", card);
        assert!(card.contains("**test** · not generated"), "{}", card);
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // The interactive path over the wire: a prompted finding publishes with its
    // question, its options come back as code actions, and one executeCommand
    // applies the edit and resolves it. Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn prompted_diagnostic_publishes_offers_actions_and_applies() {
        use crate::model::{Diagnostic, DiagnosticPrompt, PromptOption, SuggestedEdit};
        use crate::store::Op;
        let tmp =
            std::env::temp_dir().join(format!("jazyk-lsp-answer-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&tmp).ok();
        let root = tmp.join("proj");
        crate::vfs::create_dir_all(root.join("docs")).unwrap();
        crate::vfs::write(
            { let path = root.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        crate::vfs::write(
            root.join("docs/pay.md"),
            "# Pay\n\nAn Order shall be paid within 30 days.\n",
        )
        .unwrap();
        let project = crate::project::Project::load(&root);
        let out = project.out.clone();
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let mut s = Store::load(&out);
        s.sync_docs(&parsed);
        let sec = s.docs["docs/pay.md"]
            .sections
            .keys()
            .next()
            .unwrap()
            .clone();
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
                                    facts: Vec::new(),
                                }),
                                answer: None,
                            },
                            PromptOption {
                                label: "30 days is right".into(),
                                edit: None,
                                answer: Some("keep 30".into()),
                            },
                        ],
                        freeform: true,
                    }),
                    answer: None,
                    created: None,
                    updated: None,
                },
            }],
            &crate::model::WorkItem {
                task: "seed".into(),
                target: "t".into(),
                dirty_sections: vec![],
                stale_anchors: vec![],
                proposals: Vec::new(),
            }
            .commit(0, None),
        );
        let id = s.graph.diagnostics.keys().next().unwrap().clone();
        drop(s);

        let mut lsp = Lsp::new(
            root.clone(),
            out.clone(),
            crate::gen::GenSettings::resolve(&project),
        );
        let uri = path_to_uri(&root.join("docs/pay.md"));
        let mut wire: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri,
                "text": crate::vfs::read_to_string(root.join("docs/pay.md")).unwrap()}}}),
            &mut wire,
        );
        let published = String::from_utf8_lossy(&wire).to_string();
        assert!(
            published.contains("Which bound holds?"),
            "question published inline: {}",
            published
        );

        let mut wire2: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"id": 7, "method": "textDocument/codeAction", "params": {
                "textDocument": {"uri": uri},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 99, "character": 0}}}}),
            &mut wire2,
        );
        let actions = String::from_utf8_lossy(&wire2).to_string();
        assert!(
            actions.contains("Apply: 21 days; fix this file"),
            "{}",
            actions
        );
        assert!(actions.contains("Answer: 30 days is right"), "{}", actions);

        let mut wire3: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"id": 8, "method": "workspace/executeCommand", "params": {
                "command": "jazyk.answerDiagnostic", "arguments": [{"id": id, "option": 0}]}}),
            &mut wire3,
        );
        let applied = String::from_utf8_lossy(&wire3).to_string();
        assert!(applied.contains("applied"), "{}", applied);
        let text = crate::vfs::read_to_string(root.join("docs/pay.md")).unwrap();
        assert!(text.contains("within 21 days"), "{}", text);
        // The refresh inside the command republished without the resolved finding.
        assert!(applied.contains("publishDiagnostics"), "{}", applied);
        assert!(!applied.contains("Which bound holds?"), "{}", applied);
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn criterion_locations_publish_own_and_related_requirement_ranges_once() {
        let mut store = Store::default();
        store.graph.requirements.insert("req:one".into(), serde_json::from_value(json!({"statement":"Rule", "entities":[], "source":{"doc":"rules.md","section":"/rules","quote":"Orders complete."}})).unwrap());
        store.graph.criteria.insert("crit:one".into(), serde_json::from_value(json!({"requirement":"req:one", "statement":"Example", "source":{"doc":"examples.md","section":"/examples","quote":"Returns 200."}})).unwrap());
        store.graph.diagnostics.insert("diag:one".into(), serde_json::from_value(json!({"rule":"contradiction","severity":"error","subjects":["crit:one","req:one"],"message":"Conflict"})).unwrap());
        let mut lsp = lsp_at(
            std::env::temp_dir(),
            std::env::temp_dir().join("diagnostic-lsp"),
            store,
        );
        lsp.overlay
            .insert("rules.md".into(), "# Rules\n\nOrders complete.\n".into());
        lsp.overlay
            .insert("examples.md".into(), "# Examples\n\nReturns 200.\n".into());
        lsp.store.graph.diagnostics.get_mut("diag:one").unwrap().prompt = Some(serde_json::from_value(json!({
            "question":"Which is correct?", "options":[{"label":"Explain", "answer":"Explain"}], "freeform":true
        })).unwrap());
        for doc in ["rules.md", "examples.md"] {
            let diagnostics = lsp.document_diagnostics(doc);
            assert_eq!(diagnostics.len(), 1, "{doc}");
            assert_eq!(diagnostics[0]["range"]["start"]["line"], 2);
            assert_eq!(diagnostics[0]["data"]["jazykDiag"], "diag:one");
            let actions = lsp.on_code_action(&json!({"textDocument":{"uri": lsp.doc_to_uri(doc)},
                "range":{"start":{"line":2,"character":0},"end":{"line":2,"character":4}}}));
            assert_eq!(actions.as_array().unwrap().len(), 1, "{doc}");
        }
        lsp.store
            .graph
            .diagnostics
            .get_mut("diag:one")
            .unwrap()
            .triage = Some("suppressed".into());
        assert!(lsp.document_diagnostics("rules.md").is_empty());
    }

    fn lsp_at(root: std::path::PathBuf, out: std::path::PathBuf, store: Store) -> Lsp {
        // The store is in memory only: key the refresh to disk so it never reloads.
        let fingerprint = store_fingerprint(&root, &out);
        Lsp {
            root,
            out: out.clone(),
            store,
            generation: 0,
            fingerprint,
            gen: GenSettings {
                configured: true,
                deliverable: out.join("product"),
                worker: "agentic".into(),
                code: Vec::new(),
            },
            overlay: HashMap::new(),
            next_srv_id: 1,
            compiler: Compiler::Stdio {
                opts: Default::default(),
                running: None,
            },
        }
    }

    fn write_svg(out: &std::path::Path, kind: &str, slug: &str) {
        let dir = out.join("diagrams").join(kind);
        crate::vfs::create_dir_all(&dir).unwrap();
        crate::vfs::write(dir.join(format!("{}.svg", slug)), "<svg/>").unwrap();
    }

    // The showcase graph with its derived data, its out directory under `tmp`.
    fn shop_at(out: &std::path::Path) -> Store {
        let mut s = crate::derive::tests::showcase_store();
        s.out = out.to_path_buf();
        s.graph
            .entities
            .get_mut("ent:order-service")
            .unwrap()
            .definition = Some("Takes and fulfils orders.".into());
        let mut batch = crate::store::RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        s
    }

    // Hover on an entity with a level: the header, `Sits in`, the in-context image
    // of the parent's level view captioned with its id, the `Inside` link with the
    // child count (never a second image), and the line of links to the card, the
    // requirements document, and the parent's level page.
    // Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn entity_hover_renders_the_card_in_short_with_its_level() {
        let tmp = std::env::temp_dir().join(format!("jazyk-lsp-walk-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&tmp).ok();
        let out = tmp.join(".jazyk");
        crate::vfs::create_dir_all(&out).unwrap();
        write_svg(&out, "component", "shop");
        write_svg(&out, "component", "order-service");
        let lsp = lsp_at(tmp.clone(), out.clone(), shop_at(&out));
        let card = lsp.entity_card_short("ent:order-service").unwrap();
        let o = path_to_uri(&out);
        let blocks: Vec<&str> = card.split("\n\n").collect();
        assert_eq!(
            blocks,
            ["**Order Service** «service» · Takes and fulfils orders.".to_string(),
                "Sits in: Public › Shop › **Order Service**".to_string(),
                format!("![view:component/shop]({}/diagrams/component/shop.svg)", o),
                format!(
                    "Inside: [view:component/order-service]({}/docsgen/diagrams/component/order-service.md) (4 children)",
                    o
                ),
                format!(
                    "[card]({o}/docsgen/entities/order-service.md) · [requirements (2)]({o}/docsgen/order-service.md) · [level: Shop]({o}/docsgen/levels/shop.md)",
                    o = o
                )]
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
        );
        // The inside view is rendered too, and still only the context is an image.
        assert_eq!(card.matches("![").count(), 1);
        // A parentless entity sits in the scope root: its level page is the scope's.
        let root = lsp.entity_card_short("ent:shop").unwrap();
        assert!(root.contains("Sits in: Public › **Shop**"), "{}", root);
        assert!(
            root.contains(&format!(
                "[level: Public]({}/docsgen/levels/scope-public.md)",
                o
            )),
            "{}",
            root
        );
        assert!(lsp.entity_card_short("ent:nobody").is_none());
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // A leaf: no `Inside` line, its context is its parent's level, and when that
    // rendering is missing the caption stands alone linked to the diagram page.
    // Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn entity_hover_on_a_leaf_has_no_inside_and_survives_a_missing_rendering() {
        let tmp = std::env::temp_dir().join(format!("jazyk-lsp-leaf-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&tmp).ok();
        let out = tmp.join(".jazyk");
        crate::vfs::create_dir_all(&out).unwrap();
        write_svg(&out, "component", "order-service");
        let lsp = lsp_at(tmp.clone(), out.clone(), shop_at(&out));
        let o = path_to_uri(&out);
        let card = lsp.entity_card_short("ent:order-item").unwrap();
        assert!(card.starts_with("**Order Item**\n\n"), "{}", card);
        assert!(
            card.contains("Sits in: Public › Shop › Order Service › **Order Item**"),
            "{}",
            card
        );
        assert!(
            card.contains(&format!(
                "![view:component/order-service]({}/diagrams/component/order-service.svg)",
                o
            )),
            "{}",
            card
        );
        assert!(!card.contains("Inside:"), "{}", card);
        assert!(
            card.contains(&format!(
                "[level: Order Service]({}/docsgen/levels/order-service.md)",
                o
            )),
            "{}",
            card
        );
        crate::vfs::remove_file(out.join("diagrams/component/order-service.svg")).unwrap();
        let card = lsp.entity_card_short("ent:order-item").unwrap();
        assert!(!card.contains("!["), "{}", card);
        assert!(
            card.contains(&format!(
                "In context: [view:component/order-service]({}/docsgen/diagrams/component/order-service.md) (not rendered)",
                o
            )),
            "{}",
            card
        );
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // A derived grouping says so and names its pending ratification proposal.
    // Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn entity_hover_on_a_derived_grouping_names_its_proposal() {
        use crate::model::{Diagnostic, Entity, Provenance};
        let tmp =
            std::env::temp_dir().join(format!("jazyk-lsp-derived-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&tmp).ok();
        let out = tmp.join(".jazyk");
        crate::vfs::create_dir_all(&out).unwrap();
        let mut s = crate::derive::tests::showcase_store();
        s.out = out.clone();
        s.graph.entities.insert(
            "ent:promotions".into(),
            Entity {
                name: "Promotions".into(),
                parent: Some("ent:shop".into()),
                provenance: Some(Provenance::Derived {
                    from: vec!["ent:promo".into(), "ent:coupon".into()],
                    reasoning: "both discount an order".into(),
                }),
                ..Default::default()
            },
        );
        for (id, name) in [("ent:promo", "Promo"), ("ent:coupon", "Coupon")] {
            s.graph.entities.insert(
                id.into(),
                Entity {
                    name: name.into(),
                    parent: Some("ent:promotions".into()),
                    ..Default::default()
                },
            );
        }
        s.graph.diagnostics.insert(
            "diag:ratification-pending-1".into(),
            Diagnostic {
                rule: "ratification-pending".into(),
                severity: "info".into(),
                subjects: vec!["ent:promotions".into()],
                message: "Promotions groups Promo and Coupon".into(),
                reasoning: None,
                lifecycle: "open".into(),
                triage: None,
                prompt: None,
                answer: None,
                created: None,
                updated: None,
            },
        );
        let mut batch = crate::store::RecordBatch::new(1);
        crate::derive::recompute(&mut s, "g1", &mut batch);
        let lsp = lsp_at(tmp.clone(), out.clone(), s);
        let card = lsp.entity_card_short("ent:promotions").unwrap();
        assert!(
            card.ends_with("derived · proposal pending: `diag:ratification-pending-1`"),
            "{}",
            card
        );
        assert!(card.contains("(2 children)"), "{}", card);
        // A quoted entity carries no such line.
        let plain = lsp.entity_card_short("ent:order").unwrap();
        assert!(!plain.contains("proposal"), "{}", plain);
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // A cursor position is in UTF-16 code units, the LSP's default encoding: an
    // astral character before a name counts two, so the name under the cursor is the
    // one the editor shows there, not the one a character count would find.
    // Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn cursor_positions_count_astral_characters_as_two_units() {
        let tmp =
            std::env::temp_dir().join(format!("jazyk-lsp-astral-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&tmp).ok();
        let out = tmp.join(".jazyk");
        crate::vfs::create_dir_all(&out).unwrap();
        let mut lsp = lsp_at(tmp.clone(), out.clone(), shop_at(&out));
        let mut wire: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"id": 1, "method": "initialize", "params": {}}),
            &mut wire,
        );
        let uri = path_to_uri(&tmp.join("shop.md"));
        lsp.handle(
            json!({"method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri,
                "text": "\u{1F600}\u{1F600} The Order Service takes.\n\u{1D538} second line Order Service\n"}}}),
            &mut wire,
        );
        let mut id = 10;
        let mut hits = |line: u64, character: u64| -> bool {
            id += 1;
            let mut wire: Vec<u8> = Vec::new();
            lsp.handle(
                json!({"id": id, "method": "textDocument/typeDefinition", "params": {
                    "textDocument": {"uri": uri}, "position": {"line": line, "character": character}}}),
                &mut wire,
            );
            let reply = String::from_utf8_lossy(&wire).to_string();
            reply.contains("order-service.md")
        };
        // Line 0: two emoji (four units) and "The " put the name at units 9..22.
        assert!(!hits(0, 8), "a character count put the name at 7");
        assert!(hits(0, 9));
        assert!(hits(0, 21), "the name's last unit");
        assert!(!hits(0, 22));
        // Line 1: one astral letter (two units) puts the name at units 15..28.
        assert!(!hits(1, 14));
        assert!(hits(1, 15));
        assert!(hits(1, 27));
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // Over the wire: the capability is advertised, hover carries the short card, and
    // type definition lands on the entity's card at line 0 while definition stays the
    // defining mention. Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn type_definition_opens_the_card() {
        let tmp =
            std::env::temp_dir().join(format!("jazyk-lsp-typedef-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&tmp).ok();
        let out = tmp.join(".jazyk");
        crate::vfs::create_dir_all(&out).unwrap();
        let mut lsp = lsp_at(tmp.clone(), out.clone(), shop_at(&out));
        let mut wire: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"id": 1, "method": "initialize", "params": {}}),
            &mut wire,
        );
        let init = String::from_utf8_lossy(&wire).to_string();
        assert!(init.contains("\"typeDefinitionProvider\":true"), "{}", init);

        let uri = path_to_uri(&tmp.join("shop.md"));
        let mut wire: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri,
                "text": "The Order Service takes orders.\n"}}}),
            &mut wire,
        );
        let at = json!({"textDocument": {"uri": uri}, "position": {"line": 0, "character": 6}});
        let mut wire: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"id": 2, "method": "textDocument/typeDefinition", "params": at}),
            &mut wire,
        );
        let td = String::from_utf8_lossy(&wire).to_string();
        assert!(
            td.contains(&format!(
                "\"uri\":\"{}/docsgen/entities/order-service.md\"",
                path_to_uri(&out)
            )),
            "{}",
            td
        );
        assert!(
            td.contains("\"start\":{\"line\":0,\"character\":0}"),
            "{}",
            td
        );

        let mut wire: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"id": 3, "method": "textDocument/hover", "params": at}),
            &mut wire,
        );
        let hover = String::from_utf8_lossy(&wire).to_string();
        assert!(
            hover.contains("Sits in: Public › Shop › **Order Service**"),
            "{}",
            hover
        );
        assert!(
            hover.contains("docsgen/entities/order-service.md"),
            "{}",
            hover
        );

        // Nothing under the cursor: no type definition.
        let mut wire: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"id": 4, "method": "textDocument/typeDefinition", "params": {
                "textDocument": {"uri": uri}, "position": {"line": 0, "character": 25}}}),
            &mut wire,
        );
        assert!(
            String::from_utf8_lossy(&wire).contains("\"result\":null"),
            "{}",
            String::from_utf8_lossy(&wire)
        );
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // The requirement card says the statement and embeds the smallest flow view
    // listing the requirement. Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn requirement_card_embeds_the_smallest_flow_view_and_statement() {
        use crate::model::View;
        let tmp = std::env::temp_dir().join(format!("jazyk-lsp-flow-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&tmp).ok();
        let out = tmp.join(".jazyk");
        crate::vfs::create_dir_all(&out).unwrap();
        write_svg(&out, "usecase", "all");
        write_svg(&out, "sequence", "one");

        let r = requirement();
        let mut store = Store {
            out: out.clone(),
            ..Default::default()
        };
        store
            .graph
            .requirements
            .insert("req:shop-1".into(), r.clone());
        let view = |kind: &str, title: &str, members: &[&str]| View {
            kind: kind.into(),
            title: title.into(),
            members: members.iter().map(|m| m.to_string()).collect(),
            ..Default::default()
        };
        store.graph.views.insert(
            "view:usecase/all".into(),
            view("use-case", "All", &["req:shop-1", "req:shop-2"]),
        );
        store.graph.views.insert(
            "view:sequence/one".into(),
            view("sequence", "One", &["req:shop-1"]),
        );
        let lsp = lsp_at(tmp.clone(), out.clone(), store);
        let card = lsp.requirement_card("req:shop-1", &r);
        assert!(card.contains("The Cart shall hold items."), "{}", card);
        assert!(card.contains("![One (sequence)]"), "{}", card);
        assert!(card.contains("diagrams/sequence/one.svg"), "{}", card);
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // The five reader-facing words, first match wins.
    // Mirrors docs/consumers/gen.md#status-words.
    #[test]
    fn status_words_map_the_derived_statuses() {
        use crate::verify::status_word as w;
        assert_eq!(w("verified", "passed", true), "question");
        assert_eq!(w("failing", "failed", false), "failing");
        assert_eq!(w("partial", "criteria-failing", false), "failing");
        assert_eq!(w("stale-code", "code-changed", false), "stale");
        assert_eq!(w("missing", "artifact-gone", false), "stale");
        assert_eq!(w("verified", "passed", false), "verified");
        for (s, r) in [
            ("missing", "not-generated"),
            ("unimplemented", "unimplemented"),
            ("unverified", "never-run"),
        ] {
            assert_eq!(w(s, r, false), "not built");
        }
    }

    const BUILT: &str = "# Shop\n\nThe Cart shall hold items. Orders ship daily.\n\n## Checkout\n\nA Checkout Service takes payment.\n";

    // A built shop document: two entities, one requirement with a criterion quoted in
    // the same document, an open question naming the requirement, and coverage.
    fn editor_fixture(name: &str) -> (std::path::PathBuf, Lsp) {
        use crate::model::{Coverage, Criterion, DocRecord};
        let tmp =
            std::env::temp_dir().join(format!("jazyk-lsp-{}-{}", name, crate::sys::process_id()));
        crate::vfs::remove_dir_all(&tmp).ok();
        let out = tmp.join(".jazyk");
        crate::vfs::create_dir_all(&out).unwrap();
        crate::vfs::write(tmp.join("shop.md"), BUILT).unwrap();
        let mut store = Store {
            out: out.clone(),
            ..Default::default()
        };
        let mut coverage = std::collections::BTreeMap::new();
        coverage.insert(
            "/shop".to_string(),
            Coverage {
                state: "covered".into(),
                note: None,
                claimed_by: None,
            },
        );
        coverage.insert(
            "/shop/checkout".to_string(),
            Coverage {
                state: "non-normative".into(),
                note: None,
                claimed_by: None,
            },
        );
        store.docs.insert(
            "shop.md".into(),
            DocRecord {
                content_hash: crate::model::hash_hex(BUILT),
                sections: md::parse_sections(BUILT),
                coverage,
            },
        );
        let mention = |quote: &str, section: &str| SourceRef {
            doc: "shop.md".into(),
            section: section.into(),
            quote: quote.into(),
        };
        store.graph.entities.insert(
            "ent:cart".into(),
            Entity {
                name: "Cart".into(),
                mentions: vec![mention("The Cart shall hold items.", "/shop")],
                ..Default::default()
            },
        );
        store.graph.entities.insert(
            "ent:checkout-service".into(),
            Entity {
                name: "Checkout Service".into(),
                aliases: vec!["checkout".into()],
                mentions: vec![mention(
                    "A Checkout Service takes payment.",
                    "/shop/checkout",
                )],
                ..Default::default()
            },
        );
        store.graph.requirements.insert(
            "req:hold".into(),
            Requirement {
                statement: "The Cart shall hold items.".into(),
                entities: vec!["ent:cart".into()],
                source: Some(mention("The Cart shall hold items.", "/shop")),
                ..Default::default()
            },
        );
        store.graph.criteria.insert(
            "crit:hold-1".into(),
            Criterion {
                requirement: "req:hold".into(),
                statement: "Orders ship daily.".into(),
                source: Some(mention("Orders ship daily.", "/shop")),
                ..Default::default()
            },
        );
        let lsp = lsp_at(tmp.clone(), out, store);
        (tmp, lsp)
    }

    fn open(lsp: &mut Lsp, text: &str) -> String {
        let uri = lsp.doc_to_uri("shop.md");
        let mut sink: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri,
                "languageId": "markdown", "version": 1, "text": text}}}),
            &mut sink,
        );
        uri
    }

    fn ask(lsp: &mut Store, rid: &str) {
        lsp.graph.diagnostics.insert(
            "diag:ask".into(),
            crate::model::Diagnostic {
                rule: "ambiguity".into(),
                severity: "warning".into(),
                subjects: vec![rid.into()],
                message: "which cart?".into(),
                reasoning: None,
                lifecycle: "open".into(),
                triage: None,
                prompt: Some(crate::model::DiagnosticPrompt {
                    question: "Which cart?".into(),
                    options: vec![],
                    freeform: true,
                }),
                answer: None,
                created: None,
                updated: None,
            },
        );
    }

    // The initialize result declares every new capability and the legend.
    // Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn initialize_declares_the_editor_capabilities() {
        let (tmp, lsp) = editor_fixture("caps");
        let caps = &lsp.capabilities()["capabilities"];
        assert_eq!(caps["inlayHintProvider"]["resolveProvider"], false);
        assert_eq!(caps["documentHighlightProvider"], true);
        assert_eq!(caps["documentSymbolProvider"], true);
        assert_eq!(caps["workspaceSymbolProvider"], true);
        assert_eq!(caps["semanticTokensProvider"]["full"], true);
        assert_eq!(
            caps["semanticTokensProvider"]["legend"],
            json!({"tokenTypes": ["type"], "tokenModifiers": ["jazykEntity"]})
        );
        let commands = caps["executeCommandProvider"]["commands"].to_string();
        assert!(commands.contains("jazyk.showLastBuild"), "{}", commands);
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // The status word sits at the end of each located quote, inside the requested
    // range only; the code lens carries the same word.
    // Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn inlay_hints_and_lenses_read_the_status_word() {
        let (tmp, mut lsp) = editor_fixture("hints");
        let uri = open(&mut lsp, BUILT);
        let all = json!({"textDocument": {"uri": uri},
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 0}}});
        let hints = lsp.on_inlay_hints(&all);
        assert_eq!(
            hints,
            json!([{
                "position": {"line": 2, "character": 26},
                "label": "· not built",
                "paddingLeft": true,
                "tooltip": "req:hold: missing (not-generated)",
            }])
        );
        let outside = json!({"textDocument": {"uri": uri},
            "range": {"start": {"line": 4, "character": 0}, "end": {"line": 9, "character": 0}}});
        assert_eq!(lsp.on_inlay_hints(&outside), json!([]));
        // An open question outranks every other word.
        ask(&mut lsp.store, "req:hold");
        assert_eq!(lsp.on_inlay_hints(&all)[0]["label"], "· question");
        // A suppressed question no longer counts.
        lsp.store
            .graph
            .diagnostics
            .get_mut("diag:ask")
            .unwrap()
            .triage = Some("suppressed".into());
        assert_eq!(lsp.on_inlay_hints(&all)[0]["label"], "· not built");
        lsp.store
            .graph
            .diagnostics
            .get_mut("diag:ask")
            .unwrap()
            .triage = None;
        let lenses = lsp.on_code_lens(&json!({"textDocument": {"uri": uri}}));
        assert_eq!(lenses[0]["command"]["title"], "req:hold · question");
        // A quote that no longer locates shows no hint.
        let uri = open(&mut lsp, "# Shop\n\nThe basket holds items.\n");
        let all = json!({"textDocument": {"uri": uri}});
        assert_eq!(lsp.on_inlay_hints(&all), json!([]));
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // Every entity mention is one token, longest name first, encoded relative.
    // Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn semantic_tokens_mark_every_entity_mention() {
        let (tmp, mut lsp) = editor_fixture("tokens");
        let uri = open(&mut lsp, BUILT);
        let tokens = lsp.on_semantic_tokens(&json!({"textDocument": {"uri": uri}}));
        // `Cart` at 2:4, `Checkout` (the heading, an alias) at 4:3, `Checkout
        // Service` at 6:2 wins over the alias inside it.
        assert_eq!(
            tokens["data"],
            json!([2, 4, 4, 0, 1, 2, 3, 8, 0, 1, 2, 2, 16, 0, 1])
        );
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // Inside the requirement's quote: the quote and its criterion's quote.
    // Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn document_highlight_covers_the_requirement_and_its_criteria() {
        let (tmp, mut lsp) = editor_fixture("highlight");
        let uri = open(&mut lsp, BUILT);
        let at = |line: usize, character: usize| json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}});
        assert_eq!(
            lsp.on_document_highlight(&at(2, 10)),
            json!([
                {"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 26}}, "kind": 1},
                {"range": {"start": {"line": 2, "character": 27}, "end": {"line": 2, "character": 45}}, "kind": 1},
            ])
        );
        assert_eq!(lsp.on_document_highlight(&at(6, 3)), json!([]));
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // The stored tree as nested symbols with counts and read state, positioned in
    // the current text. Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn document_symbols_nest_the_stored_sections() {
        let (tmp, mut lsp) = editor_fixture("symbols");
        // A line inserted above moves both sections down by two.
        let uri = open(&mut lsp, &format!("Intro.\n\n{}", BUILT));
        let symbols = lsp.on_document_symbols(&json!({"textDocument": {"uri": uri}}));
        assert_eq!(symbols.as_array().unwrap().len(), 1);
        let shop = &symbols[0];
        assert_eq!(shop["name"], "Shop");
        assert_eq!(shop["detail"], "1 requirement · read");
        assert_eq!(shop["selectionRange"]["start"]["line"], 2);
        assert_eq!(shop["range"]["end"]["line"], 8);
        let checkout = &shop["children"][0];
        assert_eq!(checkout["name"], "Checkout");
        assert_eq!(checkout["detail"], "0 requirements · not normative");
        assert_eq!(checkout["range"]["start"]["line"], 6);
        assert_eq!(
            checkout["selectionRange"]["end"],
            json!({"line": 6, "character": 11})
        );
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // Entities by name or alias and requirements by statement, each located.
    // Mirrors docs/frontends/lsp.md#capabilities.
    #[test]
    fn workspace_symbols_find_entities_and_requirements() {
        let (tmp, lsp) = editor_fixture("wsym");
        let found = lsp.on_workspace_symbols(&json!({"query": "CHECKOUT"}));
        assert_eq!(found.as_array().unwrap().len(), 1);
        assert_eq!(found[0]["name"], "Checkout Service");
        assert_eq!(found[0]["kind"], 5);
        assert_eq!(found[0]["location"]["range"]["start"]["line"], 6);
        let found = lsp.on_workspace_symbols(&json!({"query": "hold items"}));
        assert_eq!(found.as_array().unwrap().len(), 1);
        assert_eq!(found[0]["containerName"], "req:hold");
        assert_eq!(found[0]["location"]["range"]["start"]["line"], 2);
        assert_eq!(
            lsp.on_workspace_symbols(&json!({"query": ""}))
                .as_array()
                .unwrap()
                .len(),
            3
        );
        crate::vfs::remove_dir_all(&tmp).ok();
    }

    // An edited section lenses on its heading; the command writes the last-built
    // text as a projection and asks the client to show it.
    // Mirrors docs/frontends/lsp.md#capabilities and #commands.
    #[test]
    fn changed_sections_lens_and_show_the_last_build() {
        let (tmp, mut lsp) = editor_fixture("changed");
        let uri = open(&mut lsp, BUILT);
        let changed = |lsp: &Lsp| {
            lsp.on_code_lens(&json!({"textDocument": {"uri": uri}}))
                .as_array()
                .unwrap()
                .iter()
                .filter(|l| l["command"]["command"] == "jazyk.showLastBuild")
                .map(|l| l["range"]["start"]["line"].as_u64().unwrap())
                .collect::<Vec<_>>()
        };
        assert!(changed(&lsp).is_empty());
        open(&mut lsp, &BUILT.replace("takes payment", "takes cards"));
        assert_eq!(changed(&lsp), [4]);
        let mut sink: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"id": 9, "method": "workspace/executeCommand", "params": {
                "command": "jazyk.showLastBuild", "arguments": [{"uri": uri}]}}),
            &mut sink,
        );
        let projection = tmp.join(".jazyk/last-build/shop.md");
        assert_eq!(crate::vfs::read_to_string(&projection).unwrap(), BUILT);
        let wire = String::from_utf8(sink).unwrap();
        assert!(
            wire.contains("\"method\":\"window/showDocument\""),
            "{}",
            wire
        );
        assert!(wire.contains("last-build/shop.md"), "{}", wire);
        crate::vfs::remove_dir_all(&tmp).ok();
    }
}
