// The browser transport. The native server routes a method and a path to an axum
// handler; this routes the same method and the same path to the same value
// functions, and answers the same JSON. A route the browser backend cannot serve
// answers 501 naming why, so the frontend degrades one panel at a time rather than
// failing whole. See docs/frontends/wasm.md#one-api-two-transports.

use crate::project::Project;
use crate::store::Store;
use serde_json::{json, Value};

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

// A value, or the refusal the value function names, in the server's error shape.
fn answer(r: Result<Value, super::Refusal>) -> String {
    match r {
        Ok(v) => ok(v),
        Err((status, msg)) => err(status, msg),
    }
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
    #[cfg(not(target_arch = "wasm32"))]
    {
        request_at(&crate::portable::root(), method, path, body)
    }
    #[cfg(target_arch = "wasm32")]
    {
        use futures::FutureExt;
        let route = path.split('?').next().unwrap_or(path);
        if method == "POST"
            && ["/jobs", "/compile", "/prompt", "/inputs"]
                .iter()
                .any(|suffix| route.ends_with(suffix))
        {
            return err(409, "Model execution requires request_async.");
        }
        request_at(&crate::portable::root(), method, path, body)
            .now_or_never()
            .unwrap_or_else(|| err(409, "This operation requires request_async."))
    }
}

pub async fn request_async(method: &str, path: &str, body: &str) -> String {
    request_at_async(&crate::portable::root(), method, path, body).await
}

pub async fn request_at_async(
    root: &std::path::Path,
    method: &str,
    path: &str,
    body: &str,
) -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        request_at(root, method, path, body)
    }
    #[cfg(target_arch = "wasm32")]
    {
        request_at(root, method, path, body).await
    }
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub async fn request_at(root: &std::path::Path, method: &str, path: &str, body: &str) -> String {
    let body: Value = match serde_json::from_str(if body.is_empty() { "null" } else { body }) {
        Ok(v) => v,
        Err(e) => return err(400, e.to_string()),
    };
    let routed = match super::sets::route(path, body) {
        Ok(r) => r,
        Err((s, e)) => return err(s, e),
    };
    let path = &routed.path;
    let route = path.split('?').next().unwrap_or(path);
    let _conversation = if method == "POST" && matches!(route, "/jobs" | "/compile") {
        match crate::conversation::BuildScope::enter(&routed.body) {
            Ok(scope) => Some(scope),
            Err((status, error)) => return err(status, error),
        }
    } else {
        None
    };
    if let Some(result) = crate::task_runtime::api(root, method, route, &routed.body, &routed.path)
    {
        return match result {
            Ok(value) => ok(value),
            Err(message) => err(400, message),
        };
    }
    let workspace = match crate::document_sets::Workspace::load(root) {
        Ok(w) => w,
        Err((status, error)) => {
            if matches!(
                route,
                "/settings" | "/project/files" | "/project/file" | "/project/raw" | "/ping"
            ) && method == "GET"
            {
                return request_scoped(root, method, path, &routed.body, &Project::load(root))
                    .await;
            }
            return err(status, error);
        }
    };
    if route == "/sets" || route == "/sets/preview" {
        return match method {
            "GET" => ok(workspace.summary()),
            "POST" | "PUT" | "DELETE" => answer(crate::document_sets::edit(
                root,
                &routed.body,
                route.ends_with("/preview"),
            )),
            _ => err(405, "unsupported document-set operation"),
        };
    }
    if method == "POST"
        && matches!(route, "/jobs" | "/compile")
        && routed.selector.is_none()
        && routed.body["kind"].as_str().unwrap_or("compile") == "compile"
    {
        let summary = workspace.summary();
        let ids: Vec<String> = if let Some(ids) = routed.body["setIds"].as_array() {
            let mut selected = Vec::new();
            for value in ids {
                let Some(id) = value.as_str() else {
                    return err(400, "setIds must contain strings");
                };
                match workspace.select(id) {
                    Ok(s) => selected.push(s.id.clone()),
                    Err((s, e)) => return err(s, e),
                }
            }
            workspace
                .sets
                .iter()
                .filter(|s| selected.contains(&s.id))
                .map(|s| s.id.clone())
                .collect()
        } else {
            summary["sets"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["dirty"] == true && s["state"] != "empty")
                .filter_map(|s| s["id"].as_str().map(str::to_string))
                .collect()
        };
        let mut results = Vec::new();
        for id in ids {
            if crate::portable::cancel_requested() {
                break;
            }
            let p = workspace.resolve(Some(&id)).unwrap();
            let mut result: Value =
                serde_json::from_str(&request_scoped(root, method, path, &routed.body, &p).await)
                    .unwrap();
            result["setId"] = json!(id);
            results.push(result);
        }
        return ok(json!({"id":1,"kind":"compile","state":"finished","results":results}));
    }
    if method == "GET"
        && routed.selector.is_none()
        && super::sets::aggregate_route(route)
        && !workspace.sets.is_empty()
    {
        return ok(super::sets::aggregate(root, &workspace, path));
    }
    let context = if let Some(id) = routed.selector.as_deref() {
        match workspace.resolve(Some(id)) {
            Ok(p) => p,
            Err((s, e)) => return err(s, e),
        }
    } else if matches!(
        route,
        "/project"
            | "/project/files"
            | "/project/file"
            | "/project/raw"
            | "/project/text"
            | "/project/rename"
            | "/deliverable/raw"
            | "/docs/content"
            | "/docs/document"
            | "/docs/rename"
            | "/settings"
            | "/ping"
            | "/capabilities"
    ) || route.starts_with("/chat/")
    {
        workspace.files_project()
    } else if workspace.sets.len() == 1 {
        workspace.resolve(None).unwrap()
    } else if method == "GET" && workspace.sets.is_empty() {
        workspace.files_project()
    } else {
        return err(
            400,
            "this operation requires setId or a qualified graph reference",
        );
    };
    let response = request_scoped(root, method, path, &routed.body, &context).await;
    let mut envelope: Value = serde_json::from_str(&response).unwrap();
    if route == "/project" {
        envelope["body"]["sets"] = workspace.summary()["sets"].clone();
        envelope["body"]["configHash"] = json!(workspace.hash);
    }
    if let Some(id) = &context.set_id {
        envelope["body"] = super::sets::qualify_value(envelope["body"].take(), id);
    }
    envelope.to_string()
}

#[cfg_attr(not(target_arch = "wasm32"), maybe_async::must_be_sync)]
pub(crate) async fn request_scoped(
    root: &std::path::Path,
    method: &str,
    path: &str,
    body: &Value,
    context: &Project,
) -> String {
    let (path, q) = split_query(path);
    let path = path.strip_prefix("/api").unwrap_or(path);
    let project = || context.clone();
    let out = || context.out.clone();

    match (method, path) {
        ("GET", "/ping") => ok(json!({
            "app": if crate::vfs::is_scoped() { "jazyk-hosted" } else { "jazyk-wasm" },
            "version": env!("CARGO_PKG_VERSION"),
            "protocol": crate::protocol::EDITOR_PROTOCOL_VERSION,
            "root": root.to_string_lossy(),
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
                "llm": { "model": crate::portable::model_name(), "baseUrl": "in-page" },
                "version": env!("CARGO_PKG_VERSION"),
                // What this transport is, so the app can hide what it cannot serve.
                "chatInputs": true,
                "conversationJobs": true,
                "backend": if crate::vfs::is_scoped() { "hosted" } else { "wasm" },
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
        ("POST", "/board") => {
            let p = project();
            match super::board::selected_board_value(&p, &p.out, body) {
                Ok(v) => ok(v),
                Err(e) => err(400, e),
            }
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
                None => err(
                    404,
                    format!("`{}` names no goal and no known target", target),
                ),
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
            let input_hashes = crate::control::current_doc_hashes(&p);
            let docs: Vec<Value> = p
                .doc_files()
                .iter()
                .map(|f| {
                    let rel = super::status::rel_doc(&p.root, f);
                    let hash = input_hashes
                        .get(&rel)
                        .cloned()
                        .unwrap_or_else(|| "unreadable".into());
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

        ("GET", "/docs/document") => answer(super::binary_documents::read(
            &project(),
            &param(&q, "path").unwrap_or_default(),
        )),
        ("PUT", "/docs/document") => answer(super::binary_documents::write(
            &project(),
            &param(&q, "path").unwrap_or_default(),
            body,
        )),

        ("GET", "/docs/content") => {
            let rel = param(&q, "path").unwrap_or_default();
            match super::docs::doc_content(&project(), &rel) {
                super::docs::DocRead::Found(v) => ok(v),
                super::docs::DocRead::Missing(m) => err(404, m),
                super::docs::DocRead::Invalid(m) => err(super::docs::invalid_status(&m), m),
            }
        }

        ("PUT", "/docs/content") => {
            let rel = param(&q, "path").unwrap_or_default();
            let text = body["text"].as_str().unwrap_or_default();
            let base = body["baseHash"].as_str();
            match super::docs::doc_write(&project(), &rel, text, base) {
                super::docs::DocWrite::Written(v) => ok(v),
                super::docs::DocWrite::Conflict(v) => reply(409, v),
                super::docs::DocWrite::Invalid(m) => err(super::docs::invalid_status(&m), m),
                super::docs::DocWrite::Failed(m) => err(500, m),
            }
        }

        ("DELETE", "/docs/content") => answer(super::docs::doc_delete(
            &project(),
            &param(&q, "path").unwrap_or_default(),
        )),
        ("POST", "/docs/rename") => answer(super::docs::doc_rename(
            &project(),
            body["from"].as_str().unwrap_or_default(),
            body["to"].as_str().unwrap_or_default(),
        )),

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
            if !body["selection"].is_null() {
                return err(
                    501,
                    "the browser backend runs no selected build; build the whole set",
                );
            }
            let p = project();
            let llm = crate::portable::llm();
            let trace = crate::portable::trace();
            // The answers recorded since the last build run first: a tab runs no
            // answer session in the background (docs/frontends/wasm.md#the-inbox).
            let answers = crate::inprocess::run_answers(&p, &llm, &p.out, &trace).await;
            let report = crate::reconcile::compile_with(
                &p,
                &llm,
                &p.out,
                &trace,
                body["sessions"].as_u64().map(|n| n as usize),
            )
            .await;
            ok(json!({
                "id": 1,
                "kind": "compile",
                "state": "finished",
                "report": serde_json::to_value(&report).unwrap_or(Value::Null),
                "answers": answers,
            }))
        }

        // The whole graph, as the shards hold it.
        ("GET", "/graph") => {
            let p = project();
            let store = Store::load(&out());
            let locations = crate::diagnostic_locations::locations(
                &store,
                &p.root,
                &crate::gen::GenSettings::resolve(&p),
            );
            ok(json!({
                "generation": store.status.generation,
                "entities": store.graph.entities,
                "requirements": store.graph.requirements,
                "criteria": store.graph.criteria,
                "views": store.graph.views,
                "relationships": store.graph.relationships,
                "stateMachines": store.graph.state_machines,
                "diagnostics": store.graph.diagnostics,
                "diagnosticLocations": locations,
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

        ("GET", "/find") => {
            let p = project();
            let g = crate::gen::GenSettings::resolve(&p);
            let listing =
                super::projfiles::project_files(&p.root, &g.deliverable, &p.out, &p.docs_glob);
            ok(super::find::find(
                &Store::load(&out()),
                &p.root,
                &super::find::file_paths(&listing),
                &param(&q, "q").unwrap_or_default(),
            ))
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
        ("GET", "/benchmarks") => ok(json!({ "caseSetHash": "", "results": [] })),
        ("GET", "/benchmarks/models") => ok(json!({ "models": [] })),
        ("GET", "/trace") => ok(json!({ "traces": [] })),

        // The product the project carries, read from the tree with the server's
        // own values (docs/frontends/wasm.md#the-product).
        ("GET", "/gen/pending") => {
            let p = project();
            let store = Store::load(&p.out);
            ok(super::product::gen_pending_value(
                &store,
                &crate::gen::GenSettings::resolve(&p),
            ))
        }
        ("GET", path) if path.starts_with("/gen/package/") => {
            let id = decode(path.trim_start_matches("/gen/package/"));
            let p = project();
            let store = Store::load(&p.out);
            answer(super::product::gen_package_value(
                &store,
                &id,
                &crate::gen::GenSettings::resolve(&p),
            ))
        }
        ("GET", "/verify/pending") => {
            let p = project();
            let store = Store::load(&p.out);
            ok(super::product::verify_pending_value(
                &store,
                &crate::gen::GenSettings::resolve(&p),
                param(&q, "filter").as_deref(),
                param(&q, "entity").as_deref(),
            ))
        }
        // The export answers its text as the body, so the page reads it the way it
        // reads the server's file.
        ("GET", "/verify/matrix") => {
            let p = project();
            let store = Store::load(&p.out);
            let gs = crate::gen::GenSettings::resolve(&p);
            match param(&q, "format").as_deref() {
                None | Some("json") => ok(super::product::matrix_value(&store, &gs)),
                Some(f) => {
                    let (rows, _) = super::product::matrix_rows(&store, &gs);
                    match super::product::matrix_export(&rows, f) {
                        Some((body, _, _)) => ok(Value::String(body)),
                        None => err(400, format!("unknown format {}; csv, md, or json", f)),
                    }
                }
            }
        }
        ("GET", "/deliverable") => {
            let p = project();
            ok(super::deliverable::deliverable_value(
                &p.out,
                &crate::gen::GenSettings::resolve(&p),
            ))
        }
        ("GET", "/deliverable/file") => {
            let p = project();
            answer(super::deliverable::deliverable_file(
                &p.out,
                &crate::gen::GenSettings::resolve(&p),
                &param(&q, "path").unwrap_or_default(),
            ))
        }
        // The envelope carries JSON, so a product file's bytes travel base64-encoded
        // and the page decodes them into what the server sends as the body.
        ("GET", "/deliverable/raw") => {
            use base64::Engine as _;
            let p = project();
            let rel = param(&q, "path").unwrap_or_default();
            answer(
                super::deliverable::deliverable_raw(&crate::gen::GenSettings::resolve(&p), &rel)
                    .map(|bytes| {
                        json!({
                            "path": rel,
                            "size": bytes.len(),
                            "contentType": super::outfiles::content_type(&rel),
                            "base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
                        })
                    }),
            )
        }
        ("GET", "/product/overview") => {
            let p = project();
            ok(super::overview::overview_value(
                &p,
                &crate::gen::GenSettings::resolve(&p),
            ))
        }
        ("GET", "/deliverable/baseline") => answer(super::deliverable::deliverable_baseline(
            &out(),
            &param(&q, "path").unwrap_or_default(),
        )),
        ("GET", path) if path.starts_with("/docsgen/") => {
            let slug = decode(path.trim_start_matches("/docsgen/"));
            answer(super::outfiles::docsgen_value(&out(), &slug))
        }

        // The workflow modes, the tab's own control plane
        // (docs/frontends/wasm.md#workflow-modes-in-a-tab).
        ("GET", "/watch") => {
            let p = project();
            ok(super::workers::watch_value(&crate::control::Control::load(
                &p, &p.out,
            )))
        }
        ("PUT", "/watch") => {
            let p = project();
            answer(super::workers::watch_write(&p, &p.out, body))
        }
        // The inspector's fact edits, the server's own core
        // (docs/frontends/gui.md#api, facts).
        ("POST", path) if path.starts_with("/facts/") && path.ends_with("/edit") => {
            let id = decode(path.trim_start_matches("/facts/").trim_end_matches("/edit"));
            answer(super::facts::edit_fact_core(&project(), &out(), &id, body))
        }
        // The Inbox, the server's own cores (docs/frontends/wasm.md#the-inbox).
        ("GET", "/inbox") => {
            let p = project();
            let store = Store::load(&p.out);
            ok(super::inbox::inbox_value(
                &p,
                &crate::gen::GenSettings::resolve(&p),
                &p.out,
                &store,
            ))
        }
        ("GET", "/questions") => ok(super::inbox::questions_value(&Store::load(&out()))),
        ("POST", path) if path.starts_with("/diagnostics/") && path.ends_with("/triage") => {
            let id = decode(
                path.trim_start_matches("/diagnostics/")
                    .trim_end_matches("/triage"),
            );
            answer(super::inbox::triage_value(&out(), &id, body))
        }
        // A reply that records `handling` waits for the next build, which runs its
        // answer session; the reply says so.
        ("POST", path) if path.starts_with("/questions/") && path.ends_with("/answer") => {
            let id = decode(
                path.trim_start_matches("/questions/")
                    .trim_end_matches("/answer"),
            );
            let p = project();
            answer(
                super::inbox::answer_value(&p, &p.out, &id, body).map(|mut v| {
                    if v["status"] == "handling" {
                        v["note"] = json!("recorded; the next build acts on the answer");
                    }
                    v
                }),
            )
        }
        ("GET", "/settings") => ok(crate::project::settings_read(root)),
        // The setup wizard's write (docs/frontends/gui.md#api, `PUT /api/settings`): the
        // file lands in the tree, and the page mirrors it the way any change goes.
        ("PUT", "/settings") => match crate::project::settings_write(root, body) {
            Ok(v) => ok(v),
            Err((code, msg)) => err(code, msg),
        },

        ("GET", "/project/files") => {
            let p = project();
            let g = crate::gen::GenSettings::resolve(&p);
            ok(super::projfiles::project_files(
                &p.root,
                &g.deliverable,
                &p.out,
                &p.docs_glob,
            ))
        }
        ("GET", "/project/text") => answer(super::projfiles::project_text(
            &project().root,
            &param(&q, "path").unwrap_or_default(),
        )),
        ("PUT", "/project/text") => {
            let p = project();
            answer(super::projfiles::write_project_text(
                &p.root,
                &p.out,
                &param(&q, "path").unwrap_or_default(),
                body,
            ))
        }
        ("DELETE", "/project/file") => {
            let p = project();
            answer(super::projfiles::delete_project_file(
                &p.root,
                &p.out,
                &param(&q, "path").unwrap_or_default(),
            ))
        }
        ("POST", "/project/rename") => {
            let p = project();
            answer(super::projfiles::rename_project_file(
                &p.root,
                &p.out,
                body["from"].as_str().unwrap_or_default(),
                body["to"].as_str().unwrap_or_default(),
            ))
        }
        ("PUT", "/project/file") => {
            let p = project();
            answer(super::projfiles::create_project_file(
                &p.root,
                &p.out,
                &param(&q, "path").unwrap_or_default(),
                body,
            ))
        }
        // Any project file's bytes, text or not, for a preview; base64 in the envelope
        // like a product file's.
        ("GET", "/project/raw") => {
            use base64::Engine as _;
            let rel = param(&q, "path").unwrap_or_default();
            answer(
                super::projfiles::project_file(&project().root, &rel).map(|bytes| {
                    json!({
                        "path": rel,
                        "size": bytes.len(),
                        "contentType": super::outfiles::content_type(&rel),
                        "base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
                    })
                }),
            )
        }
        ("GET", "/project/file") => {
            let rel = param(&q, "path").unwrap_or_default();
            match super::projfiles::project_file(&project().root, &rel) {
                Ok(bytes) if param(&q, "binary").as_deref() == Some("true") => {
                    use base64::Engine as _;
                    ok(
                        json!({"base64":base64::engine::general_purpose::STANDARD.encode(&bytes),
                        "contentType":super::outfiles::content_type(&rel)}),
                    )
                }
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => ok(Value::String(text)),
                    Err(_) => err(
                        501,
                        format!(
                            "the browser backend serves text files only; {} is not text",
                            rel
                        ),
                    ),
                },
                Err((status, msg)) => err(status, msg),
            }
        }

        ("GET", "/out/list") => answer(super::outfiles::out_list(
            &out(),
            &param(&q, "path").unwrap_or_default(),
        )),

        // The file's text is the body, the way the server's bytes are. The envelope
        // carries JSON, so a file that is not text is not served.
        ("GET", "/out/file") => {
            let rel = param(&q, "path").unwrap_or_default();
            match super::outfiles::out_file(&out(), &rel) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => ok(Value::String(text)),
                    Err(_) => err(
                        501,
                        format!(
                            "the browser backend serves text files only; {} is not text",
                            rel
                        ),
                    ),
                },
                Err((status, msg)) => err(status, msg),
            }
        }

        // The chat pane: conversations run in the worker on the page's model, and
        // their progress is posted to the page as events while the prompt request
        // is still running. See docs/frontends/wasm.md#chat.
        ("GET", "/chat/history") => ok(crate::conversation::export_history()),
        ("PUT", "/chat/history") => match crate::conversation::import_history(body) {
            Ok(value) => ok(value),
            Err(message) => err(409, message),
        },
        ("POST", "/chat/sessions") => ok(crate::conversation::create(body)),
        ("GET", "/chat/sessions") => ok(crate::conversation::list()),
        ("GET", p) if p.starts_with("/chat/sessions/") => {
            let id = decode(p.trim_start_matches("/chat/sessions/"));
            match crate::conversation::get(&id) {
                Some(v) => ok(v),
                None => err(404, format!("no chat session {}", id)),
            }
        }
        ("DELETE", p) if p.starts_with("/chat/sessions/") => {
            let id = decode(p.trim_start_matches("/chat/sessions/"));
            if crate::conversation::remove(&id) {
                ok(json!({ "ok": true }))
            } else {
                err(404, format!("no chat session {}", id))
            }
        }
        ("POST", p) if p.starts_with("/chat/sessions/") && p.ends_with("/prompt") => {
            let id = decode(
                p.trim_start_matches("/chat/sessions/")
                    .trim_end_matches("/prompt"),
            );
            let (status, v) = crate::conversation::prompt(&id, body).await;
            reply(status, v)
        }
        ("POST", p) if p.starts_with("/chat/sessions/") && p.ends_with("/inputs") => {
            let id = decode(
                p.trim_start_matches("/chat/sessions/")
                    .trim_end_matches("/inputs"),
            );
            let (status, value) = crate::conversation::prompt(&id, body).await;
            reply(status, value)
        }
        // The page answers a cancel while a turn runs; one that reaches the worker
        // found no turn running, so there is nothing to stop.
        ("POST", p) if p.starts_with("/chat/sessions/") && p.ends_with("/cancel") => {
            reply(202, json!({ "ok": true }))
        }
        ("POST", "/chat/permissions") => err(
            409,
            "a chat in this tab never asks permission, so there is nothing to answer",
        ),

        _ => err(
            501,
            format!("the browser backend does not serve {} {}", method, path),
        ),
    }
}

#[cfg(test)]
mod tests {
    // The browser transport serves the editor's project-wide search
    // (docs/frontends/gui.md#api): a file by its name and by its text.
    #[test]
    fn find_answers_files_by_name_and_text() {
        let dir = std::env::temp_dir().join(format!("jazyk-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in [".jazyk", "docs", "code"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        crate::vfs::write(dir.join(".jazyk/jazyk.toml"), "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\n").unwrap();
        crate::vfs::write(dir.join("docs/main.md"), "# Main\n\nA sort utility.\n").unwrap();
        crate::vfs::write(dir.join("code/main.js"), "export {}\n").unwrap();
        crate::vfs::write(dir.join("code/util.js"), "// the main loop\n").unwrap();
        // Compiled state under the set's out directory is not a file to find.
        std::fs::create_dir_all(dir.join(".jazyk/ds_main/graph/entity")).unwrap();
        crate::vfs::write(
            dir.join(".jazyk/ds_main/graph/entity/ent~main.yaml"),
            "name: Main\n",
        )
        .unwrap();
        let reply: serde_json::Value =
            serde_json::from_str(&super::request_at(&dir, "GET", "/api/find?q=main", "")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let hits = reply["body"]["hits"]
            .as_array()
            .unwrap_or_else(|| panic!("{reply}"));
        let found: Vec<(&str, Option<u64>)> = hits
            .iter()
            .map(|h| (h["path"].as_str().unwrap(), h["line"].as_u64()))
            .collect();
        assert_eq!(
            found,
            vec![
                ("code/main.js", None),
                ("docs/main.md", None),
                (".jazyk/jazyk.toml", Some(4)),
                ("code/util.js", Some(1))
            ],
            "{reply}"
        );
    }

    #[test]
    fn project_raw_answers_any_file_as_base64_and_file_keeps_text_only() {
        let dir = std::env::temp_dir().join(format!("jazyk-project-raw-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = [0x89u8, b'P', b'N', b'G', 0, 1, 2];
        crate::vfs::write(dir.join("logo.png"), png).unwrap();
        let raw: serde_json::Value = serde_json::from_str(&super::request_at(
            &dir,
            "GET",
            "/api/project/raw?path=logo.png",
            "",
        ))
        .unwrap();
        let text = raw.to_string();
        assert!(text.contains("\"contentType\":\"image/png\""), "{text}");
        assert!(text.contains("\"base64\":\"iVBORwABAg==\""), "{text}");
        assert!(text.contains("\"size\":7"), "{text}");
        let file = super::request_at(&dir, "GET", "/api/project/file?path=logo.png", "");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(file.contains("501"), "{file}");
    }
}
