use super::document::path_to_uri;
use super::Lsp;
use crate::jsonrpc::write_message;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

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
    pub(super) fn finish(&mut self) {
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

    pub(super) fn on_execute_command<W: Write>(&mut self, params: &Value, out: &mut W) -> Value {
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
}
