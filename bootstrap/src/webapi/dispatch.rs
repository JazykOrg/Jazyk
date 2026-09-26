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

        // The matched documents with their on-disk hash, the reconciled hash,
        // staleness, open diagnostics by severity, and open goals counted by kind.
        ("GET", "/docs") => {
            let p = project();
            let store = Store::load(&p.out);
            let by_doc = super::status::diag_counts_by_doc(&store);
            let board = crate::board::Board::compute(&p, &p.out);
            let mut goals_by_doc: std::collections::BTreeMap<
                String,
                std::collections::BTreeMap<String, usize>,
            > = Default::default();
            for g in board.open_goals() {
                if let Some(doc) = crate::board::target_doc(&g.target) {
                    *goals_by_doc
                        .entry(doc)
                        .or_default()
                        .entry(g.kind.clone())
                        .or_default() += 1;
                }
            }
            let docs: Vec<Value> = p
                .doc_files()
                .iter()
                .map(|f| {
                    let rel = super::status::rel_doc(&p.root, f);
                    let text = crate::vfs::read_to_string(f).unwrap_or_default();
                    let hash = crate::model::hash_hex(&text);
                    let graph_hash = store.docs.get(&rel).map(|r| r.content_hash.clone());
                    json!({
                        "path": rel,
                        "contentHash": hash,
                        "graphHash": graph_hash,
                        "stale": graph_hash.as_deref() != Some(hash.as_str()),
                        "diagnostics": by_doc.get(&rel),
                        "goals": goals_by_doc.get(&rel),
                    })
                })
                .collect();
            ok(json!({ "docs": docs }))
        }

        ("GET", "/docs/content") => {
            let rel = param(&q, "path").unwrap_or_default();
            match super::docs::doc_content(&project(), &rel) {
                super::docs::DocRead::Found(v) => ok(v),
                super::docs::DocRead::Missing(m) => err(404, m),
                super::docs::DocRead::Invalid(m) => err(400, m),
            }
        }

        ("PUT", "/docs/content") => {
            let rel = param(&q, "path").unwrap_or_default();
            let text = body["text"].as_str().unwrap_or_default();
            let base = body["baseHash"].as_str();
            match super::docs::doc_write(&project(), &rel, text, base) {
                super::docs::DocWrite::Written(v) => ok(v),
                super::docs::DocWrite::Conflict(v) => reply(409, v),
                super::docs::DocWrite::Invalid(m) => err(400, m),
                super::docs::DocWrite::Failed(m) => err(500, m),
            }
        }

        ("GET", "/docs/baseline") => {
            let rel = param(&q, "path").unwrap_or_default();
            match super::docs::doc_baseline(&Store::load(&out()), &rel) {
                Some(v) => ok(v),
                None => err(404, format!("{} has never reconciled", rel)),
            }
        }

        // A build, synchronous. The server runs one on its job manager and answers
        // immediately; here the worker is the job, and the reply is the report. The
        // trace streams to the page as it is emitted, so the wait is watchable.
        ("POST", "/jobs") | ("POST", "/compile") => {
            let kind = body["kind"].as_str().unwrap_or("compile");
            if kind != "compile" {
                return err(
                    501,
                    format!("the browser backend runs no {} job: it has no shell", kind),
                );
            }
            let p = project();
            let llm = crate::wasm::llm();
            let trace = crate::wasm::trace();
            let report = crate::reconcile::compile(&p, &llm, &p.out, &trace);
            ok(json!({
                "id": 1,
                "kind": "compile",
                "state": "finished",
                "report": serde_json::to_value(&report).unwrap_or(Value::Null),
            }))
        }

        // The whole graph, as the shards hold it.
        ("GET", "/graph") => {
            let store = Store::load(&out());
            ok(json!({
                "generation": store.status.generation,
                "entities": store.graph.entities,
                "requirements": store.graph.requirements,
                "criteria": store.graph.criteria,
                "views": store.graph.views,
                "relationships": store.graph.relationships,
                "stateMachines": store.graph.state_machines,
                "diagnostics": store.graph.diagnostics,
                "redirects": store.graph.redirects,
            }))
        }

        ("GET", "/coverage") => ok(json!(Store::load(&out()).docs)),

        ("GET", "/overview") => {
            let p = project();
            let store = Store::load(&p.out);
            let statuses = crate::verify::status_map(&store, &crate::gen::GenSettings::resolve(&p));
            let mut by_status: std::collections::BTreeMap<String, usize> = Default::default();
            for v in statuses.values() {
                *by_status
                    .entry(v["status"].as_str().unwrap_or("unknown").to_string())
                    .or_default() += 1;
            }
            let mut v = super::status::status_value(&store);
            v["verification"] = json!(by_status);
            ok(v)
        }

        ("GET", "/journal") => {
            let out = out();
            let generation = Store::load(&out).status.generation;
            let to = param(&q, "to")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(generation)
                .min(generation);
            let limit = param(&q, "limit")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(50)
                .clamp(1, 500);
            let from = param(&q, "from")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(1)
                .max(1);
            // Generations without a journal file exist (checks and GC entries are
            // sparse), so the range walks down and `limit` caps the count.
            let mut entries: Vec<Value> = Vec::new();
            let mut g = to;
            while g >= from && (entries.len() as u64) < limit {
                let f = out.join("journal").join(format!("g{}.yaml", g));
                if let Ok(text) = crate::vfs::read_to_string(&f) {
                    if let Ok(mut v) = serde_norway::from_str::<Value>(&text) {
                        v["generation"] = json!(g);
                        entries.push(v);
                    }
                }
                if g == 0 {
                    break;
                }
                g -= 1;
            }
            ok(json!({ "generation": generation, "entries": entries }))
        }

        ("GET", "/search") => {
            let store = Store::load(&out());
            let hits: Vec<Value> = store
                .search(&param(&q, "q").unwrap_or_default())
                .into_iter()
                .map(|(id, name, definition)| {
                    json!({ "id": id, "name": name, "definition": definition })
                })
                .collect();
            ok(json!({ "hits": hits }))
        }

        ("GET", p) if p.starts_with("/requirements/") => {
            let id = decode(p.trim_start_matches("/requirements/"));
            let store = Store::load(&out());
            let id = store.resolve_id(&id).to_string();
            match store.graph.requirements.get(&id) {
                Some(r) => ok(json!(r)),
                None => err(404, format!("no requirement {}", id)),
            }
        }

        ("GET", "/feedback") => {
            let limit = param(&q, "limit")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(200)
                .clamp(1, 2000);
            ok(json!({ "entries": crate::feedback::read(&out(), limit) }))
        }

        // Nothing in a tab holds work the way the server's job manager does: a build
        // runs inside one request. The shapes stay valid so the panels render empty.
        ("GET", "/jobs") => ok(json!({ "jobs": [] })),
        ("GET", "/workers") => {
            let p = project();
            let c = crate::control::Control::load(&p, &p.out);
            ok(super::workers::workers_snapshot(
                &p,
                &p.out,
                &crate::gen::GenSettings::resolve(&p),
                &c,
            ))
        }
        ("GET", "/questions") => ok(json!({ "questions": [] })),
        ("GET", "/gen/pending") => ok(json!({ "pending": [] })),
        // Each empty answer keeps the native handler's shape (gui.md#api): the pages
        // index `rows` and `counts` as objects and crash on an array or a hole.
        ("GET", "/verify/pending") => ok(json!({ "pending": [], "counts": {} })),
        ("GET", "/verify/matrix") => ok(json!({ "rows": {}, "counts": {} })),
        ("GET", "/benchmarks") => ok(json!({ "caseSetHash": "", "results": [] })),
        ("GET", "/benchmarks/models") => ok(json!({ "models": [] })),
        ("GET", "/deliverable") => ok(json!({ "root": "", "files": [], "medium": null, "build": null })),
        ("GET", "/trace") => ok(json!({ "traces": [] })),

        ("GET", "/watch") => ok(json!({ "watch": false })),
        ("GET", "/settings") => ok(crate::project::settings_read(&crate::wasm::root())),

        ("GET", "/out/list") => {
            let out = out();
            let mut files: Vec<Value> = Vec::new();
            for p in crate::vfs::list_all() {
                if let Ok(rel) = p.strip_prefix(&out) {
                    files.push(json!({
                        "path": rel.to_string_lossy().replace('\\', "/"),
                        "size": crate::vfs::read(&p).map(|b| b.len()).unwrap_or(0),
                    }));
                }
            }
            ok(json!({ "files": files }))
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
