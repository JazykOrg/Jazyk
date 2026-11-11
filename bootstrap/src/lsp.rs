use crate::vfs::VPathExt;
// The language server: thin and read-only, per docs/frontends/lsp.md. It reads the
// graph store and maps nodes to editor positions. It never compiles; rebuilds run
// through `jazyk compile` or `jazyk watch`, and the store's generation counter tells
// this server when to reload and republish.
use crate::jsonrpc::{read_message, write_message};
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};

mod annotations;
mod commands;
mod document;
mod navigation;
#[cfg(test)]
mod tests;

pub use commands::Compiler;

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

// The semantic token legend: entity mentions are `type` tokens with the
// `jazykEntity` modifier. Mirrors docs/frontends/lsp.md#capabilities.
const SEMANTIC_TOKEN_TYPES: [&str; 1] = ["type"];
const SEMANTIC_TOKEN_MODIFIERS: [&str; 1] = ["jazykEntity"];

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
        let mut lock_seen = crate::layout::lock_file(&out).vexists();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let lock_now = crate::layout::lock_file(&out).vexists();
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
