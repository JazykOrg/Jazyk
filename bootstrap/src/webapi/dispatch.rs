// The browser transport. The native server routes a method and a path to an axum
// handler; this routes the same method and the same path to the same value
// functions, and answers the same JSON. A route the browser backend cannot serve
// answers 501 naming why, so the frontend degrades one panel at a time rather than
// failing whole. See docs/frontends/wasm.md#one-api-two-transports.

use crate::project::Project;
use crate::store::Store;
use serde_json::{json, Value};
use std::path::PathBuf;

// The reply envelope, so the frontend's fetch shim can rebuild a Response.
fn reply(status: u16, body: Value) -> String {
    json!({ "status": status, "body": body }).to_string()
}

fn ok(body: Value) -> String {
    reply(200, body)
}

fn err(status: u16, msg: impl Into<String>) -> String {
    reply(status, json!({ "error": msg.into() }))
}

fn project() -> Project {
    Project::load(&crate::wasm::root())
}

fn out() -> PathBuf {
    project().out
}

// `/api/views/v:class:x?detail=2` splits into the path and its query pairs.
fn split_query(path: &str) -> (&str, Vec<(String, String)>) {
    match path.split_once('?') {
        None => (path, Vec::new()),
        Some((p, q)) => (
            p,
            q.split('&')
                .filter(|s| !s.is_empty())
                .filter_map(|pair| {
                    let (k, v) = pair.split_once('=')?;
                    Some((k.to_string(), decode(v)))
                })
                .collect(),
        ),
    }
}

// Percent decoding, enough for the ids and targets the API takes.
fn decode(s: &str) -> String {
    let bytes = s.replace('+', " ").into_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&String::from_utf8_lossy(&bytes[i + 1..i + 3]), 16) {
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

fn param(q: &[(String, String)], key: &str) -> Option<String> {
    q.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

pub fn request(method: &str, path: &str, body: &str) -> String {
    let (path, q) = split_query(path);
    let path = path.strip_prefix("/api").unwrap_or(path);
    let body: Value = serde_json::from_str(body).unwrap_or(Value::Null);

    match (method, path) {
        ("GET", "/ping") => ok(json!({
            "app": "jazyk-wasm",
            "version": env!("CARGO_PKG_VERSION"),
            "root": crate::wasm::root().to_string_lossy(),
        })),

        ("GET", "/project") => {
            let p = project();
            ok(json!({
                "root": p.root.to_string_lossy(),
                "out": p.out.to_string_lossy(),
                "docsGlob": p.docs_glob,
                "roots": p.roots,
                "deliverable": crate::gen::GenSettings::resolve(&p).deliverable.to_string_lossy(),
                "budgets": {
                    "sessionRounds": crate::limits::SESSION_ROUNDS,
                    "sessionMutations": crate::limits::SESSION_MUTATIONS,
                    "contextBudget": crate::limits::CONTEXT_BUDGET,
                    "buildSessionFactor": crate::limits::BUILD_SESSION_FACTOR,
                },
                "executors": p.executors.by_kind,
                "llm": { "model": crate::wasm::model_name(), "baseUrl": "in-page" },
                "version": env!("CARGO_PKG_VERSION"),
                // What this transport is, so the app can hide what it cannot serve.
                "backend": "wasm",
            }))
        }

        ("GET", "/status") => {
            let p = project();
            let store = Store::load(&p.out);
            let mut v = super::status::status_value(&store);
            v["board"] = json!(crate::board::Board::compute(&p, &p.out).counts());
            ok(v)
        }

        ("GET", "/board") => {
            let p = project();
            ok(super::board::board_value(&p, &p.out))
        }

        ("GET", "/preview") => {
            let p = project();
            let target = param(&q, "goal").unwrap_or_default();
            ok(super::board::preview_value(&p, &p.out, &target))
        }

        ("GET", "/explain") => {
            let p = project();
            let target = param(&q, "target").unwrap_or_default();
            match super::board::explain_value(&p, &p.out, &target) {
                Some(v) => ok(v),
                None => err(404, format!("`{}` names no goal and no known target", target)),
            }
        }

        ("GET", "/ripple") => {
            let root = match (param(&q, "generation"), param(&q, "target")) {
                (Some(g), _) => format!("g{}", g),
                (None, Some(t)) if !t.trim().is_empty() => t.trim().to_string(),
                _ => return err(400, "pass generation= or target= to root the DAG"),
            };
            let back = param(&q, "back").as_deref() == Some("true");
            match super::board::ripple_value(&out(), &root, back) {
                Some(v) => ok(v),
                None => err(404, "the journal holds no entry touching that root"),
            }
        }

        ("GET", "/views") => ok(super::views::views_value(&Store::load(&out()))),

        ("GET", "/tree") => ok(super::views::tree_value(&Store::load(&out()))),

        ("GET", p) if p.starts_with("/views/") => {
            let id = decode(p.trim_start_matches("/views/"));
            let store = Store::load(&out());
            if let Some(rest) = id.strip_suffix("/page") {
                return match super::card::page_value(&store, rest) {
                    Some(v) => ok(v),
                    None => err(404, format!("no view {}", rest)),
                };
            }
            let detail = param(&q, "detail")
                .and_then(|d| d.parse::<usize>().ok())
                .unwrap_or(0);
            match super::views::view_value(&store, &id, detail) {
                Some(v) => ok(v),
                None => err(404, format!("no view {}", id)),
            }
        }

        ("GET", p) if p.starts_with("/entities/") => {
            let id = decode(p.trim_start_matches("/entities/"));
            let store = Store::load(&out());
            let id = id.strip_suffix("/card").unwrap_or(&id).to_string();
            match super::card::card_value(&store, &id) {
                Some(v) => ok(v),
                None => err(404, format!("no entity {}", id)),
            }
        }

        // The documents the tab is compiling, and their text.
        ("GET", "/docs") => {
            let p = project();
            let store = Store::load(&p.out);
            let files = p.doc_files();
            let docs: Vec<Value> = files
                .iter()
                .map(|f| {
                    let rel = super::status::rel_doc(&p.root, f);
                    let text = crate::vfs::read_to_string(f).unwrap_or_default();
                    json!({
                        "path": rel,
                        "bytes": text.len(),
                        "reconciled": store.docs.contains_key(&rel),
                    })
                })
                .collect();
            ok(json!({ "docs": docs }))
        }

        ("GET", "/docs/file") => {
            let rel = param(&q, "path").unwrap_or_default();
            let p = project();
            match crate::vfs::read_to_string(p.root.join(&rel)) {
                Ok(text) => ok(json!({ "path": rel, "text": text })),
                Err(e) => err(404, format!("{}: {}", rel, e)),
            }
        }

        ("PUT", "/docs/file") => {
            let rel = body["path"].as_str().unwrap_or_default().to_string();
            let text = body["text"].as_str().unwrap_or_default().to_string();
            if rel.is_empty() {
                return err(400, "pass path and text");
            }
            let p = project();
            match crate::vfs::write(p.root.join(&rel), text.as_bytes()) {
                Ok(()) => ok(json!({ "path": rel, "bytes": text.len() })),
                Err(e) => err(500, format!("{}: {}", rel, e)),
            }
        }

        // One build, synchronous: the tab is already running it on a worker.
        ("POST", "/compile") => {
            let p = project();
            let llm = crate::wasm::llm();
            let trace = crate::wasm::trace();
            let report = crate::reconcile::compile(&p, &llm, &p.out, &trace);
            ok(serde_json::to_value(&report).unwrap_or(Value::Null))
        }

        ("GET", "/journal") => {
            let store = Store::load(&out());
            ok(json!({ "generation": store.status.generation }))
        }

        ("GET", "/out/file") => {
            let rel = param(&q, "path").unwrap_or_default();
            match read_out(&rel) {
                Some(text) => ok(json!({ "path": rel, "text": text })),
                None => err(404, format!("no {} in the out directory", rel)),
            }
        }

        _ => err(
            501,
            format!("the browser backend does not serve {} {}", method, path),
        ),
    }
}

// A file under the out directory, as text. Diagrams are the common case.
pub fn read_out(rel: &str) -> Option<String> {
    crate::vfs::read_to_string(out().join(rel)).ok()
}
