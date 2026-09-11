// JSON API handlers. Mirrors docs/frontends/gui.md#api.
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
        "budgets": {
            "sessionRounds": crate::limits::SESSION_ROUNDS,
            "sessionMutations": crate::limits::SESSION_MUTATIONS,
            "contextBudget": crate::limits::CONTEXT_BUDGET,
            "buildSessionFactor": crate::limits::BUILD_SESSION_FACTOR,
        },
        "executors": p.executors.by_kind,
        // Never the api key.
        "llm": { "model": st.llm().model, "baseUrl": st.llm().base_url },
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

// The `jazyk status` summary as JSON: status.yaml plus counts derived from the
// shards and the board counts. Mirrors docs/frontends/gui.md#api.
pub async fn status(State(st): State<SharedState>) -> Json<Value> {
    let proj = st.proj();
    let out = st.out.clone();
    Json(
        tokio::task::spawn_blocking(move || {
            let store = Store::load(&out);
            let mut v = status_value(&store);
            let board = crate::board::Board::compute(&proj, &out);
            v["board"] = json!(board.counts());
            v
        })
        .await
        .expect("status task panicked"),
    )
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
        "version": store.status.version,
        "generation": store.status.generation,
        "verdict": store.status.verdict,
        "spent": store.status.spent,
        "parked": store.status.parked,
        "failed": store.status.failed,
        "costs": store.status.costs,
        "counts": {
            "entities": store.graph.entities.len(),
            "requirements": store.graph.requirements.len(),
            "relationships": store.graph.relationships.len(),
            "views": store.graph.views.len(),
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
// the file hash for the conditional write. Mirrors docs/frontends/gui.md#api.
pub async fn settings_get(State(st): State<SharedState>) -> Json<Value> {
    let root = st.root.clone();
    Json(
        tokio::task::spawn_blocking(move || crate::project::settings_read(&root))
            .await
            .expect("settings read"),
    )
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

// The standing questions: open, unsuppressed, prompted, unanswered diagnostics.
// Mirrors docs/frontends/gui.md#questions.
pub async fn questions(State(st): State<SharedState>) -> Response {
    let out = st.out.clone();
    let v = tokio::task::spawn_blocking(move || {
        let store = Store::load(&out);
        let mut items: Vec<Value> = Vec::new();
        for (id, d) in &store.graph.diagnostics {
            if d.lifecycle != "open" || d.triage.as_deref() == Some("suppressed") {
                continue;
            }
            if d.prompt.is_none() {
                continue;
            }
            items.push(json!({
                "id": id, "rule": d.rule, "severity": d.severity, "message": d.message,
                "subjects": d.subjects, "prompt": d.prompt, "answer": d.answer,
            }));
        }
        json!({ "questions": items })
    })
    .await
    .expect("questions task panicked");
    Json(v).into_response()
}

// A human answer to a prompted diagnostic: {option} or {text}. Edit options apply
// and resolve synchronously; anything else records handling and a background
// answer session acts on it. Mirrors docs/compiler/model/diagnostic.md#answers.
pub async fn answer_question(
    State(st): State<SharedState>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let reply = if let Some(i) = body["option"].as_u64() {
        crate::answer::Reply::Choice(i as usize)
    } else if let Some(t) = body["text"].as_str() {
        crate::answer::Reply::Text(t.to_string())
    } else {
        return err(
            StatusCode::BAD_REQUEST,
            "pass option (an index) or text (a freeform reply)",
        );
    };
    let out = st.out.clone();
    let project = st.proj.read().unwrap().clone();
    let result = tokio::task::spawn_blocking(move || {
        let v = crate::answer::answer(&project, &out, &id, reply, None)?;
        if v["status"] == "handling" {
            crate::answer::spawn_handler(project, out, id);
        }
        Ok::<Value, String>(v)
    })
    .await
    .expect("answer task panicked");
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
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
            proposals: vec![],
        };
        store.apply(
            vec![crate::store::Op::TriageDiagnostic {
                id: rid.clone(),
                triage,
            }],
            &item.commit(0, 0),
        );
        Ok(json!({ "id": rid, "diagnostic": store.graph.diagnostics.get(&rid) }))
    })
    .await
    .expect("triage task panicked");
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, e),
    }
}

// The workflow modes as the wire reports them. `mode` keeps the legacy select
// values (`queue` = manual, `watch` = auto) beside the control-plane names.
fn watch_state(st: &SharedState) -> Value {
    let c = st.control();
    json!({
        "mode": if c.compile == "auto" { "watch" } else { "queue" },
        "compile": c.compile,
        "gen": c.generate,
        "worker": c.worker,
    })
}

pub async fn watch_get(State(st): State<SharedState>) -> Json<Value> {
    Json(watch_state(&st))
}

// The workflow modes share the endpoint; either field alone leaves the other
// untouched. They persist in control.yaml, where every worker reads them.
// Mirrors docs/frontends/gui.md#workflow-modes.
pub async fn watch_put(State(st): State<SharedState>, Json(body): Json<Value>) -> Response {
    let mut c = st.control();
    // `compile` takes the control-plane names; `mode` keeps the legacy ones.
    if let Some(m) = body["compile"].as_str().or(match body["mode"].as_str() {
        Some("watch") => Some("auto"),
        Some("queue") | Some("off") => Some("manual"),
        Some(_) => {
            return err(
                StatusCode::BAD_REQUEST,
                "mode must be queue or watch (compile: manual or auto)",
            )
        }
        None => None,
    }) {
        if !["manual", "auto"].contains(&m) {
            return err(StatusCode::BAD_REQUEST, "compile must be manual or auto");
        }
        c.compile = m.to_string();
    }
    if let Some(gen) = body["gen"].as_str() {
        if !["manual", "auto"].contains(&gen) {
            return err(StatusCode::BAD_REQUEST, "gen must be manual or auto");
        }
        c.generate = gen.to_string();
    } else if !body["gen"].is_null() {
        return err(StatusCode::BAD_REQUEST, "gen must be a string");
    }
    if let Some(w) = body["worker"].as_str() {
        if !["internal", "agent", "any"].contains(&w) {
            return err(
                StatusCode::BAD_REQUEST,
                "worker must be internal, agent, or any",
            );
        }
        c.worker = w.to_string();
    }
    c.save(&st.out);
    let state = watch_state(&st);
    st.events.emit("watch.state", state.clone());
    Json(state).into_response()
}

// The control plane snapshot the workers strip renders: modes, registered workers,
// live leases, gated counts. Mirrors docs/frontends/gui.md#workers.
pub fn workers_snapshot(st: &SharedState) -> Value {
    let c = st.control();
    let board = crate::board::Board::compute(&st.proj(), &st.out);
    let graph_kinds = crate::board::Board::graph_kinds();
    let store = crate::store::Store::load(&st.out);
    let unclaimed = crate::bind::unclaimed(&st.proj(), &store, &st.gs());
    json!({
        "workflow": {"compile": c.compile, "generate": c.generate, "worker": c.worker},
        "workers": crate::control::workers(&st.out),
        "leases": crate::control::leases(&st.out).values().collect::<Vec<_>>(),
        "gated": {
            "compile": board.gated_of(&graph_kinds),
            // Bind goals gate under the generate release beside generation.
            "generate": board.gated_of(&["generate", "bind"]),
        },
        "actionable": {
            "compile": board.ready_of(&graph_kinds),
            "bind": board.ready_of(&["bind"]),
            "generate": board.ready_of(&["generate"]),
            "verify": board.ready_of(&["verify"]),
        },
        "board": board.counts(),
        // The unclaimed report: the decompile worklist.
        "unclaimed": unclaimed.len(),
        "decompileReleased": c.released.decompile,
    })
}

pub async fn workers(State(st): State<SharedState>) -> Json<Value> {
    Json(workers_snapshot(&st))
}

// Record a release without running anything: the workers strip's button. The wake
// happens through the control file every watcher watches.
// Mirrors docs/frontends/gui.md#workers.
pub async fn release(State(st): State<SharedState>, Json(body): Json<Value>) -> Response {
    let stage = body["stage"].as_str();
    if let Some(s) = stage {
        if s != "compile" && s != "generate" {
            return err(
                StatusCode::BAD_REQUEST,
                "stage must be compile or generate (or absent for both)",
            );
        }
    }
    crate::control::release(&st.proj(), &st.out, stage);
    let snap = workers_snapshot(&st);
    st.events.emit("control.changed", snap.clone());
    Json(snap).into_response()
}

pub async fn graph(State(st): State<SharedState>) -> Response {
    let store = load_store(&st).await;
    Json(json!({
        "generation": store.status.generation,
        "entities": store.graph.entities,
        "requirements": store.graph.requirements,
        "views": store.graph.views,
        "relationships": store.graph.relationships,
        "stateMachines": store.graph.state_machines,
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
    let requirements: BTreeMap<&String, &crate::model::Requirement> = req_ids
        .iter()
        .filter_map(|r| store.graph.requirements.get(r).map(|q| (r, q)))
        .collect();
    let relationships: BTreeMap<&String, &crate::model::Relationship> = store
        .graph
        .relationships
        .iter()
        .filter(|(_, rel)| rel.members.contains(&id))
        .collect();
    let statuses = crate::verify::status_map(&store, &st.gs());
    let verify: BTreeMap<&String, &Value> = req_ids
        .iter()
        .filter_map(|r| statuses.get(r).map(|v| (r, v)))
        .collect();
    let children: Vec<&String> = store
        .graph
        .entities
        .iter()
        .filter(|(_, e)| e.parent.as_deref().map(|p| store.resolve_id(p)) == Some(id.as_str()))
        .map(|(cid, _)| cid)
        .collect();
    let views: Vec<&String> = store
        .graph
        .views
        .iter()
        .filter(|(_, v)| v.members.iter().any(|m| store.resolve_id(m) == id))
        .map(|(vid, _)| vid)
        .collect();
    let machine = store
        .graph
        .state_machines
        .iter()
        .find(|(_, m)| store.resolve_id(&m.subject) == id)
        .map(|(mid, m)| json!({ "id": mid, "machine": m }));
    Json(json!({
        "id": id,
        "entity": ent,
        "children": children,
        "views": views,
        "stateMachine": machine,
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
    depth: Option<u32>,
}

// What `load` renders for the target: the loaded set of that one load, with its
// expansion handles. Mirrors docs/compiler/context.md#tools.
pub async fn context(State(st): State<SharedState>, Query(p): Query<ContextQ>) -> Response {
    let store = load_store(&st).await;
    let depth = p.depth.unwrap_or(1);
    let mut set = crate::context::LoadedSet::new(crate::limits::CONTEXT_BUDGET);
    let result = if p.target.starts_with("h:") {
        crate::context::parse_handle(&p.target)
            .and_then(|(t, _, _)| set.load(&store, &t, depth))
            .and_then(|_| set.expand(&store, &p.target))
    } else {
        set.load(&store, &p.target, depth)
    };
    match result {
        Ok(text) => Json(json!({
            "target": p.target,
            "depth": depth,
            "pack": text,
            "handles": set.open_handles(),
        }))
        .into_response(),
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
pub struct FeedbackQ {
    limit: Option<usize>,
}

// The feedback log, newest first: what models found ambiguous, wrong, or confusing
// about jazyk's own prompts and tools. Mirrors docs/compiler/tools.md#feedback-tool.
pub async fn feedback(State(st): State<SharedState>, Query(p): Query<FeedbackQ>) -> Json<Value> {
    let out = st.out.clone();
    let limit = p.limit.unwrap_or(200).clamp(1, 2000);
    let entries = tokio::task::spawn_blocking(move || crate::feedback::read(&out, limit))
        .await
        .expect("feedback read task panicked");
    Json(json!({ "entries": entries }))
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
                if let Some(src) = r.source.as_ref() {
                    docs_hit.insert(src.doc.clone());
                }
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

// The matched documents with their on-disk hash, the reconciled hash, staleness,
// open diagnostics by severity, and open goals counted by kind.
pub async fn docs(State(st): State<SharedState>) -> Json<Value> {
    let proj = st.proj();
    let out = st.out.clone();
    Json(
        tokio::task::spawn_blocking(move || {
            let store = Store::load(&out);
            let by_doc = diag_counts_by_doc(&store);
            let board = crate::board::Board::compute(&proj, &out);
            let mut goals_by_doc: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
            for g in board.open_goals() {
                if let Some(doc) = crate::board::target_doc(&g.target) {
                    *goals_by_doc
                        .entry(doc)
                        .or_default()
                        .entry(g.kind.clone())
                        .or_default() += 1;
                }
            }
            let mut list: Vec<Value> = Vec::new();
            for f in proj.doc_files() {
                let rel = rel_doc(&proj.root, &f);
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
                    "goals": goals_by_doc.get(&rel).cloned().unwrap_or_default(),
                }));
            }
            json!({ "docs": list })
        })
        .await
        .expect("docs task panicked"),
    )
}

#[derive(Deserialize)]
pub struct DocQ {
    path: String,
}

pub async fn doc_content(State(st): State<SharedState>, Query(p): Query<DocQ>) -> Response {
    let Some(abs) = super::docs::safe_doc_path(&st.proj(), &p.path) else {
        return err(
            StatusCode::BAD_REQUEST,
            format!("invalid document path {}", p.path),
        );
    };
    match std::fs::read_to_string(&abs) {
        Ok(text) => {
            let hash = crate::model::hash_hex(&text);
            Json(json!({ "path": p.path, "text": text, "hash": hash })).into_response()
        }
        Err(e) => err(
            StatusCode::NOT_FOUND,
            format!("cannot read {}: {}", p.path, e),
        ),
    }
}

// The last reconciled text, reconstructed from the stored section tree: sections
// ordered by their line spans, raw bodies joined, blank gap lines restored. The
// difference between this and the on-disk text is what the next build's dirty set
// sees. Mirrors docs/frontends/gui.md#api.
pub async fn doc_baseline(State(st): State<SharedState>, Query(p): Query<DocQ>) -> Response {
    let store = load_store(&st).await;
    let Some(rec) = store.docs.get(&p.path) else {
        return err(
            StatusCode::NOT_FOUND,
            format!("{} has never reconciled", p.path),
        );
    };
    let mut secs: Vec<&crate::model::Section> = rec.sections.values().collect();
    secs.sort_by_key(|s| s.lines[0]);
    let mut parts: Vec<&str> = Vec::new();
    let mut expected = 0usize;
    for s in &secs {
        // Blank lines before the first heading belong to no section; restore them.
        for _ in expected..s.lines[0] {
            parts.push("");
        }
        parts.push(s.raw.as_str());
        expected = s.lines[1];
    }
    let text = parts.join("\n");
    Json(json!({ "path": p.path, "text": text, "hash": rec.content_hash })).into_response()
}

pub async fn gen_pending(State(st): State<SharedState>) -> Json<Value> {
    let store = load_store(&st).await;
    Json(json!({ "pending": crate::gen::pending(&store, &st.gs()) }))
}

// The per-entity generation package a session receives.
pub async fn gen_package(State(st): State<SharedState>, UrlPath(id): UrlPath<String>) -> Response {
    let store = load_store(&st).await;
    let id = store.resolve_id(&id).to_string();
    if !store.graph.entities.contains_key(&id) {
        return err(StatusCode::NOT_FOUND, format!("no entity {}", id));
    }
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
    let pending =
        crate::verify::pending(&store, &st.gs(), p.filter.as_deref(), p.entity.as_deref());
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
    let f = st
        .out
        .join("docsgen")
        .join(format!("{}.md", slug.trim_end_matches(".md")));
    match std::fs::read_to_string(&f) {
        Ok(text) => Json(json!({ "slug": slug, "text": text })).into_response(),
        Err(_) => err(
            StatusCode::NOT_FOUND,
            format!("no requirements document for {}", slug),
        ),
    }
}

// ---- benchmarks ----
// Mirrors docs/frontends/gui.md#benchmarks.

pub async fn benchmarks(State(st): State<SharedState>) -> Json<Value> {
    Json(crate::benchmark::all_results(&st.out))
}

#[derive(serde::Deserialize)]
pub struct ModelsQ {
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
}

// The endpoint's own model listing, for the run form's picker. A free-form model name
// works without this; an unreachable endpoint answers with the error, not a 500.
pub async fn benchmark_models(
    State(st): State<SharedState>,
    Query(p): Query<ModelsQ>,
) -> Json<Value> {
    let base = p.base_url.unwrap_or_else(|| st.llm().base_url.clone());
    let api_key = st.llm().api_key.clone();
    let url = format!(
        "{}/v1/models",
        base.trim_end_matches('/').trim_end_matches("/v1")
    );
    let listing = tokio::task::spawn_blocking(move || {
        let mut req = ureq::get(&url);
        if !api_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", api_key));
        }
        req.timeout(std::time::Duration::from_secs(5))
            .call()
            .map_err(|e| e.to_string())
            .and_then(|r| r.into_string().map_err(|e| e.to_string()))
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    match listing {
        Ok(v) => {
            let models: Vec<String> = v["data"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|m| m["id"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Json(serde_json::json!({"baseUrl": base, "models": models}))
        }
        Err(e) => Json(serde_json::json!({"baseUrl": base, "models": [], "error": e})),
    }
}
