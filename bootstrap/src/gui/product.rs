// The product surface's own endpoints: the capability descriptor the app renders
// against, the projects this server has opened, the Inbox rollup, and the trace
// matrix export. Mirrors docs/frontends/gui.md#backends-and-capabilities,
// docs/frontends/gui.md#home, docs/frontends/gui.md#inbox.
use super::api::err;
use super::state::SharedState;
use crate::store::Store;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// The server serves everything; another backend answers with what it can do.
pub async fn capabilities(State(st): State<SharedState>) -> Json<Value> {
    let acp = crate::project::load_global_acp();
    let proj = st.proj();
    let agent = match proj.acp.agent.as_deref().or(acp.agent.as_deref()) {
        Some("embedded") | None => "embedded",
        Some(_) => "external",
    };
    let llm = st.llm();
    let model = if llm.model.is_empty() {
        "none"
    } else if llm.api_key.is_empty()
        && !llm.base_url.contains("127.0.0.1")
        && !llm.base_url.contains("localhost")
    {
        "needs-key"
    } else {
        "configured"
    };
    Json(json!({
        "protocol": crate::protocol::EDITOR_PROTOCOL_VERSION,
        "backend": "server",
        "projects": "list",
        "docs": "write",
        "binaryDocuments": crate::webapi::binary_documents::available(),
        "events": "stream",
        "lsp": true,
        "jobs": ["workflow", "build", "compile", "gen", "verify", "audit", "decompile", "benchmark"],
        "agent": agent,
        "chatInputs": agent != "none",
        "testPaths": true,
        "selection": true,
        "model": model,
        "rendering": "server",
        "persistence": "disk",
        "dev": st.dev,
    }))
}

// ---------------------------------------------------------------------------
// ~/.jazyk/projects.yaml: the projects this machine's servers have opened.

#[derive(Serialize, Deserialize, Clone)]
struct Opened {
    root: String,
    #[serde(rename = "openedAt")]
    opened_at: u64,
}

const REGISTRY_CAP: usize = 12;

fn registry_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".jazyk").join("projects.yaml"))
}

fn load_registry() -> Vec<Opened> {
    let Some(p) = registry_path() else {
        return Vec::new();
    };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_norway::from_str::<Vec<Opened>>(&t).ok())
        .unwrap_or_default()
}

// Called at startup: the served project moves to the front of the list.
pub fn record_opened(root: &Path) {
    let Some(p) = registry_path() else {
        return;
    };
    let root_s = root.to_string_lossy().to_string();
    let mut list = load_registry();
    list.retain(|o| o.root != root_s);
    list.insert(
        0,
        Opened {
            root: root_s,
            opened_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        },
    );
    list.truncate(REGISTRY_CAP);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    if let Ok(text) = serde_norway::to_string(&list) {
        std::fs::write(p, text).ok();
    }
}

// docs-only | docs-and-product | product-only | empty: what the project holds, so
// the home screen and the wizard say what to do first.
pub fn shape(
    proj: &crate::project::Project,
    gs: &crate::gen::GenSettings,
    store: &Store,
) -> &'static str {
    let has_docs = !proj.doc_files().is_empty();
    let ledger = crate::gen::Ledger::load(&store.out);
    let bound = !ledger.entities.is_empty() || !ledger.requirements.is_empty();
    let deliverable_apart = gs.deliverable != proj.root && gs.deliverable.is_dir();
    let has_product = bound || deliverable_apart;
    match (has_docs, has_product) {
        (true, true) => "docs-and-product",
        (true, false) => "docs-only",
        (false, true) => "product-only",
        (false, false) => "empty",
    }
}

fn project_summary(root: &Path) -> Value {
    if !root.join(crate::document_sets::CONFIG).is_file() {
        return json!({ "root": root.to_string_lossy(), "missing": true });
    }
    let proj = crate::project::Project::load(root);
    let store = Store::load(&proj.out);
    let gs = crate::gen::GenSettings::resolve(&proj);
    let by_sev = open_by_severity(&store);
    json!({
        "root": proj.root.to_string_lossy(),
        "name": proj.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        "shape": shape(&proj, &gs, &store),
        "generation": store.status.generation,
        "verdict": store.status.verdict,
        "diagnostics": by_sev,
    })
}

pub async fn projects(State(st): State<SharedState>) -> Json<Value> {
    let current = st.root.clone();
    Json(
        tokio::task::spawn_blocking(move || {
            let mut list = load_registry();
            if !list.iter().any(|o| Path::new(&o.root) == current) {
                list.insert(
                    0,
                    Opened {
                        root: current.to_string_lossy().to_string(),
                        opened_at: 0,
                    },
                );
            }
            let projects: Vec<Value> = list
                .iter()
                .map(|o| {
                    let mut v = project_summary(Path::new(&o.root));
                    v["openedAt"] = json!(o.opened_at);
                    v["current"] = json!(Path::new(&o.root) == current);
                    v
                })
                .collect();
            json!({ "projects": projects })
        })
        .await
        .expect("projects task panicked"),
    )
}

fn open_by_severity(store: &Store) -> BTreeMap<String, usize> {
    let mut by_sev: BTreeMap<String, usize> = BTreeMap::new();
    for d in store.graph.diagnostics.values() {
        if d.lifecycle == "open" && d.triage.as_deref() != Some("suppressed") {
            *by_sev.entry(d.severity.clone()).or_default() += 1;
        }
    }
    by_sev
}

// ---------------------------------------------------------------------------
// The Inbox rollup: the value lives in crate::webapi::inbox.

pub async fn inbox(State(st): State<SharedState>) -> Json<Value> {
    let proj = st.proj();
    let gs = st.gs();
    let out = st.out.clone();
    Json(
        tokio::task::spawn_blocking(move || {
            let store = Store::load(&out);
            crate::webapi::inbox::inbox_value(&proj, &gs, &out, &store)
        })
        .await
        .expect("inbox task panicked"),
    )
}

// ---------------------------------------------------------------------------
// The trace matrix, as JSON or as a file: csv or md
// (docs/frontends/gui.md#trace-matrix). The values live in crate::webapi::product.

#[derive(Deserialize)]
pub struct MatrixQ {
    format: Option<String>,
}

pub async fn verify_matrix(State(st): State<SharedState>, Query(q): Query<MatrixQ>) -> Response {
    let store = super::api::load_store(&st).await;
    let gs = st.gs();
    match q.format.as_deref() {
        None | Some("json") => {
            Json(crate::webapi::product::matrix_value(&store, &gs)).into_response()
        }
        Some(f) => {
            let (rows, _) = crate::webapi::product::matrix_rows(&store, &gs);
            match crate::webapi::product::matrix_export(&rows, f) {
                Some((body, ctype, name)) => (
                    [
                        (header::CONTENT_TYPE, ctype.to_string()),
                        (
                            header::CONTENT_DISPOSITION,
                            format!("attachment; filename=\"{}\"", name),
                        ),
                    ],
                    body,
                )
                    .into_response(),
                None => err(
                    StatusCode::BAD_REQUEST,
                    format!("unknown format {}; csv, md, or json", f),
                ),
            }
        }
    }
}
