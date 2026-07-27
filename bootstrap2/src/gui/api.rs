// JSON API handlers. Mirrors docs2/frontends/gui.md#api.
use super::state::SharedState;
use crate::store::Store;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

// Uniform error shape: the server's words, rendered verbatim by the client.
pub fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

// Load the store off the async runtime. Reads are lock-free with generation retry.
pub async fn load_store(st: &SharedState) -> Store {
    let out = st.out.clone();
    tokio::task::spawn_blocking(move || Store::load(&out))
        .await
        .expect("store load task panicked")
}

// Unauthenticated: lets a second instance (and a curious human) see which project a
// running server owns. Nothing secret: the root path of a localhost-only process.
pub async fn ping(State(st): State<SharedState>) -> Json<Value> {
    Json(json!({
        "app": "jazyk-gui",
        "version": env!("CARGO_PKG_VERSION"),
        "root": st.proj().root.to_string_lossy(),
    }))
}

pub async fn project(State(st): State<SharedState>) -> Json<Value> {
    let p = st.proj();
    Json(json!({
        "root": p.root.to_string_lossy(),
        "out": st.out.to_string_lossy(),
        "docsGlob": p.docs_glob,
        "roots": p.roots,
        "deliverable": st.gs().deliverable.to_string_lossy(),
        "limits": {
            "turnRounds": p.limits.turn_rounds,
            "turnMutations": p.limits.turn_mutations,
            "contextBudget": p.limits.context_budget,
            "buildTurnFactor": p.limits.build_turn_factor,
        },
        // Never the api key.
        "llm": { "model": st.llm().model, "baseUrl": st.llm().base_url },
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

// The `jazyk status` summary as JSON: status.yaml plus counts derived from the shards.
pub async fn status(State(st): State<SharedState>) -> Json<Value> {
    let store = load_store(&st).await;
    Json(status_value(&store))
}

pub fn status_value(store: &Store) -> Value {
    let (mut total, mut covered) = (0usize, 0usize);
    for rec in store.docs.values() {
        for (r, sec) in &rec.sections {
            if sec.raw.lines().skip(1).all(|l| l.trim().is_empty()) {
                continue;
            }
            total += 1;
            if rec.coverage.contains_key(r) {
                covered += 1;
            }
        }
    }
    let mut by_sev: std::collections::BTreeMap<&str, usize> = Default::default();
    for d in store.graph.diagnostics.values() {
        if d.lifecycle == "open" && d.triage.as_deref() != Some("suppressed") {
            *by_sev.entry(d.severity.as_str()).or_default() += 1;
        }
    }
    json!({
        "generation": store.status.generation,
        "verdict": store.status.verdict,
        "spent": store.status.spent,
        "parked": store.status.parked,
        "counts": {
            "entities": store.graph.entities.len(),
            "requirements": store.graph.requirements.len(),
            "relationships": store.graph.relationships.len(),
        },
        "coverage": { "covered": covered, "total": total },
        "diagnostics": by_sev,
    })
}

pub async fn shutdown(State(st): State<SharedState>) -> Json<Value> {
    st.shutdown.notify_waiters();
    Json(json!({ "ok": true }))
}

// The project settings for the form: set keys, effective defaults, unknown keys, and
// the file hash for the conditional write. Mirrors docs2/frontends/gui.md#api.
pub async fn settings_get(State(st): State<SharedState>) -> Json<Value> {
    let root = st.root.clone();
    Json(tokio::task::spawn_blocking(move || crate::project::settings_read(&root)).await.expect("settings read"))
}

// Rewrite jazyk.toml from the form values and apply live: the running server reloads
// the project, the LLM resolution, and the generation settings without a restart.
pub async fn settings_put(State(st): State<SharedState>, Json(body): Json<Value>) -> Response {
    let root = st.root.clone();
    let result = tokio::task::spawn_blocking(move || {
        let current = crate::project::settings_read(&root);
        if current["hash"] != body["baseHash"] {
            return Err((StatusCode::CONFLICT, "jazyk.toml changed on disk since it was read".to_string()));
        }
        let unknown = current["unknown"].as_array().cloned().unwrap_or_default();
        if !unknown.is_empty() {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "jazyk.toml holds keys the settings form does not know ({}); edit the file directly",
                    unknown.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", ")
                ),
            ));
        }
        let text = crate::project::settings_render(&root, &body["settings"])
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        std::fs::write(root.join("jazyk.toml"), &text)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("cannot write jazyk.toml: {}", e)))?;
        Ok(crate::project::settings_read(&root))
    })
    .await
    .expect("settings write");
    match result {
        Ok(v) => {
            super::reload_project(&st);
            st.events.emit("settings.changed", json!({}));
            Json(v).into_response()
        }
        Err((code, msg)) => err(code, msg),
    }
}

// The human triage decision on a diagnostic, committed through the store as a
// journaled changeset. The compiler never overwrites human triage.
pub async fn triage(
    State(st): State<SharedState>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let triage = match &body["triage"] {
        Value::Null => None,
        Value::String(s) if ["acknowledged", "suppressed", "wontfix"].contains(&s.as_str()) => {
            Some(s.clone())
        }
        _ => {
            return err(
                StatusCode::BAD_REQUEST,
                "triage must be acknowledged, suppressed, wontfix, or null",
            )
        }
    };
    let out = st.out.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut store = Store::load(&out);
        let rid = store.resolve_id(&id).to_string();
        if !store.graph.diagnostics.contains_key(&rid) {
            return Err(format!("no diagnostic {}", rid));
        }
        let item = crate::model::WorkItem {
            task: "triage".into(),
            target: rid.clone(),
            dirty_sections: vec![],
            stale_anchors: vec![],
        };
        store.apply(vec![crate::store::Op::TriageDiagnostic { id: rid.clone(), triage }], &item, 0, 0);
        Ok(json!({ "id": rid, "diagnostic": store.graph.diagnostics.get(&rid) }))
    })
    .await
    .expect("triage task panicked");
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, e),
    }
}

pub async fn watch_get(State(st): State<SharedState>) -> Json<Value> {
    Json(json!({ "mode": st.watch_mode.lock().unwrap().clone() }))
}

pub async fn watch_put(State(st): State<SharedState>, Json(body): Json<Value>) -> Response {
    let mode = body["mode"].as_str().unwrap_or_default();
    if !["off", "queue", "watch"].contains(&mode) {
        return err(StatusCode::BAD_REQUEST, "mode must be off, queue, or watch");
    }
    *st.watch_mode.lock().unwrap() = mode.to_string();
    st.events.emit("watch.state", json!({ "mode": mode }));
    Json(json!({ "mode": mode })).into_response()
}

pub async fn graph(State(st): State<SharedState>) -> Response {
    let store = load_store(&st).await;
    Json(json!({
        "generation": store.status.generation,
        "entities": store.graph.entities,
        "requirements": store.graph.requirements,
        "relationships": store.graph.relationships,
        "diagnostics": store.graph.diagnostics,
        "redirects": store.graph.redirects,
    }))
    .into_response()
}

// The entity page aggregate: the node, the requirements referencing it, its
// relationships, and the verification status of each of those requirements.
pub async fn entity(State(st): State<SharedState>, UrlPath(id): UrlPath<String>) -> Response {
    let store = load_store(&st).await;
    let id = store.resolve_id(&id).to_string();
    let Some(ent) = store.graph.entities.get(&id) else {
        return err(StatusCode::NOT_FOUND, format!("no entity {}", id));
    };
    let req_ids = store.requirements_referencing(&id);
    let requirements: BTreeMap<&String, &crate::model::Requirement> =
        req_ids.iter().filter_map(|r| store.graph.requirements.get(r).map(|q| (r, q))).collect();
    let relationships: BTreeMap<&String, &crate::model::Relationship> = store
        .graph
        .relationships
        .iter()
        .filter(|(_, rel)| rel.members.contains(&id))
        .collect();
    let statuses = crate::verify::status_map(&store, &st.gs());
    let verify: BTreeMap<&String, &Value> =
        req_ids.iter().filter_map(|r| statuses.get(r).map(|v| (r, v))).collect();
    Json(json!({
        "id": id,
        "entity": ent,
        "requirements": requirements,
        "relationships": relationships,
        "verify": verify,
    }))
    .into_response()
}

pub async fn requirement(State(st): State<SharedState>, UrlPath(id): UrlPath<String>) -> Response {
    let store = load_store(&st).await;
    let id = store.resolve_id(&id).to_string();
    let Some(req) = store.graph.requirements.get(&id) else {
        return err(StatusCode::NOT_FOUND, format!("no requirement {}", id));
    };
    let statuses = crate::verify::status_map(&store, &st.gs());
    Json(json!({ "id": id, "requirement": req, "verify": statuses.get(&id) })).into_response()
}

#[derive(Deserialize)]
pub struct SearchQ {
    q: String,
}

pub async fn search(State(st): State<SharedState>, Query(p): Query<SearchQ>) -> Json<Value> {
    let store = load_store(&st).await;
    let hits: Vec<Value> = store
        .search(&p.q)
        .into_iter()
        .map(|(id, name, definition)| json!({ "id": id, "name": name, "definition": definition }))
        .collect();
    Json(json!({ "hits": hits }))
}

#[derive(Deserialize)]
pub struct ContextQ {
    target: String,
    focus: Option<String>,
    budget: Option<usize>,
}

pub async fn context(State(st): State<SharedState>, Query(p): Query<ContextQ>) -> Response {
    let store = load_store(&st).await;
    let focus = p.focus.as_deref().map(crate::context::Focus::parse).unwrap_or_default();
    let budget = p.budget.unwrap_or(st.proj().limits.context_budget);
    let result = if p.target.starts_with("h:") {
        crate::context::expand(&store, &p.target, budget)
    } else {
        crate::context::assemble(&store, &p.target, &focus, budget)
    };
    match result {
        Ok(pack) => Json(json!(pack)).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

// Per document: the section tree and the coverage map, as stored.
pub async fn coverage(State(st): State<SharedState>) -> Json<Value> {
    let store = load_store(&st).await;
    Json(json!(store.docs))
}

// Viewer-style rollup: the status summary plus verification counts by status.
pub async fn overview(State(st): State<SharedState>) -> Json<Value> {
    let store = load_store(&st).await;
    let statuses = crate::verify::status_map(&store, &st.gs());
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    for v in statuses.values() {
        let s = v["status"].as_str().unwrap_or("unknown").to_string();
        *by_status.entry(s).or_default() += 1;
    }
    let mut v = status_value(&store);
    v["verification"] = json!(by_status);
    Json(v)
}

#[derive(Deserialize)]
pub struct JournalQ {
    from: Option<u64>,
    to: Option<u64>,
    limit: Option<u64>,
}

// Journal entries for a generation range, newest first. Missing files (GC'd or
// pre-journal builds) are skipped, never an error.
pub async fn journal(State(st): State<SharedState>, Query(p): Query<JournalQ>) -> Json<Value> {
    let out = st.out.clone();
    let entries = tokio::task::spawn_blocking(move || {
        let generation = Store::load(&out).status.generation;
        // Generations without a journal file exist (checks and GC entries are sparse),
        // so the default range walks all the way down and `limit` caps the count.
        let to = p.to.unwrap_or(generation).min(generation);
        let limit = p.limit.unwrap_or(50).clamp(1, 500);
        let from = p.from.unwrap_or(1).max(1);
        let mut entries: Vec<Value> = Vec::new();
        let mut g = to;
        while g >= from && (entries.len() as u64) < limit {
            let f = out.join("journal").join(format!("g{}.yaml", g));
            if let Ok(text) = std::fs::read_to_string(&f) {
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
        json!({ "generation": generation, "from": from, "to": to, "entries": entries })
    })
    .await
    .expect("journal task panicked");
    Json(entries)
}

// Relative doc path under the project root, forward slashes. The DocRecord key.
pub fn rel_doc(root: &std::path::Path, f: &std::path::Path) -> String {
    f.strip_prefix(root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| f.to_string_lossy().to_string())
}

// Open non-suppressed diagnostics counted per document. A diagnostic counts toward a
// document when a subject anchors there, the same mapping the LSP publishes:
// a requirement whose source is the document, an entity with a mention in it, or a
// section reference into it.
fn diag_counts_by_doc(store: &Store) -> BTreeMap<String, BTreeMap<&'static str, usize>> {
    let mut out: BTreeMap<String, BTreeMap<&'static str, usize>> = BTreeMap::new();
    for d in store.graph.diagnostics.values() {
        if d.lifecycle != "open" || d.triage.as_deref() == Some("suppressed") {
            continue;
        }
        let sev: &'static str = match d.severity.as_str() {
            "error" => "error",
            "warning" => "warning",
            "info" => "info",
            _ => "none",
        };
        let mut docs_hit: std::collections::BTreeSet<String> = Default::default();
        for subject in &d.subjects {
            let resolved = store.resolve_id(subject).to_string();
            if let Some(r) = store.graph.requirements.get(&resolved) {
                docs_hit.insert(r.source.doc.clone());
            } else if let Some(e) = store.graph.entities.get(&resolved) {
                for m in &e.mentions {
                    docs_hit.insert(m.doc.clone());
                }
            } else if let Some((sdoc, _)) = crate::model::split_section_ref(&resolved) {
                docs_hit.insert(sdoc);
            }
        }
        for doc in docs_hit {
            *out.entry(doc).or_default().entry(sev).or_default() += 1;
        }
    }
    out
}

// The matched documents with their on-disk hash, the reconciled hash, staleness, and
// open diagnostics by severity.
pub async fn docs(State(st): State<SharedState>) -> Json<Value> {
    let store = load_store(&st).await;
    let by_doc = diag_counts_by_doc(&store);
    let mut list: Vec<Value> = Vec::new();
    for f in st.proj().doc_files() {
        let rel = rel_doc(&st.proj().root, &f);
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        let hash = crate::model::hash_hex(&text);
        let graph_hash = store.docs.get(&rel).map(|r| r.content_hash.clone());
        let stale = graph_hash.as_deref() != Some(hash.as_str());
        list.push(json!({
            "path": rel,
            "contentHash": hash,
            "graphHash": graph_hash,
            "stale": stale,
            "diagnostics": by_doc.get(&rel).cloned().unwrap_or_default(),
        }));
    }
    Json(json!({ "docs": list }))
}

#[derive(Deserialize)]
pub struct DocQ {
    path: String,
}

pub async fn doc_content(State(st): State<SharedState>, Query(p): Query<DocQ>) -> Response {
    let Some(abs) = super::docs::safe_doc_path(&st.proj(), &p.path) else {
        return err(StatusCode::BAD_REQUEST, format!("invalid document path {}", p.path));
    };
    match std::fs::read_to_string(&abs) {
        Ok(text) => {
            let hash = crate::model::hash_hex(&text);
            Json(json!({ "path": p.path, "text": text, "hash": hash })).into_response()
        }
        Err(e) => err(StatusCode::NOT_FOUND, format!("cannot read {}: {}", p.path, e)),
    }
}

pub async fn gen_pending(State(st): State<SharedState>) -> Json<Value> {
    let store = load_store(&st).await;
    Json(json!({ "pending": crate::gen::pending(&store, &st.gs()) }))
}

pub async fn gen_task(State(st): State<SharedState>, UrlPath(id): UrlPath<String>) -> Response {
    let store = load_store(&st).await;
    match crate::gen::task_package(&store, &id, &st.gs()) {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Deserialize)]
pub struct VerifyPendingQ {
    filter: Option<String>,
    entity: Option<String>,
}

pub async fn verify_pending(
    State(st): State<SharedState>,
    Query(p): Query<VerifyPendingQ>,
) -> Json<Value> {
    let store = load_store(&st).await;
    let pending = crate::verify::pending(&store, &st.gs(), p.filter.as_deref(), p.entity.as_deref());
    Json(json!({ "pending": pending, "counts": crate::verify::pending_counts(&store, &st.gs()) }))
}

// Every requirement with its derived status, plus rollup counts. Rows are the
// status map enriched with the owning entity and a nested test object, so the
// matrix view can group by entity.
pub async fn verify_matrix(State(st): State<SharedState>) -> Json<Value> {
    let store = load_store(&st).await;
    let statuses = crate::verify::status_map(&store, &st.gs());
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    let mut rows: BTreeMap<String, Value> = BTreeMap::new();
    for (rid, v) in statuses {
        let s = v["status"].as_str().unwrap_or("unknown").to_string();
        *by_status.entry(s).or_default() += 1;
        let entity = store
            .graph
            .requirements
            .get(&rid)
            .and_then(|r| r.entities.first().cloned())
            .unwrap_or_default();
        let mut row = v.clone();
        row["entity"] = json!(entity);
        row["test"] = json!({ "kind": v["kind"], "run": v["run"], "label": v["label"] });
        rows.insert(rid, row);
    }
    Json(json!({ "rows": rows, "counts": by_status }))
}

pub async fn docsgen(State(st): State<SharedState>, UrlPath(slug): UrlPath<String>) -> Response {
    if slug.contains('/') || slug.contains("..") || slug.contains('\\') {
        return err(StatusCode::BAD_REQUEST, format!("invalid slug {}", slug));
    }
    let f = st.out.join("docsgen").join(format!("{}.md", slug.trim_end_matches(".md")));
    match std::fs::read_to_string(&f) {
        Ok(text) => Json(json!({ "slug": slug, "text": text })).into_response(),
        Err(_) => err(StatusCode::NOT_FOUND, format!("no requirements document for {}", slug)),
    }
}
