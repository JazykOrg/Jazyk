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
    } else if llm.api_key.is_empty() && !llm.base_url.contains("127.0.0.1") && !llm.base_url.contains("localhost")
    {
        "needs-key"
    } else {
        "configured"
    };
    Json(json!({
        "backend": "server",
        "projects": "list",
        "docs": "write",
        "events": "stream",
        "lsp": true,
        "jobs": ["compile", "gen", "verify", "audit", "decompile", "benchmark"],
        "agent": agent,
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
pub fn shape(proj: &crate::project::Project, gs: &crate::gen::GenSettings, store: &Store) -> &'static str {
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
    if !root.join("jazyk.toml").is_file() {
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
// The Inbox rollup: questions, proposals, problems, pending changes, drafts.

const PROPOSAL_RULES: [&str; 2] = ["ratification-pending", "invented-choice"];
const DRAFT_RULE: &str = "unratified";

pub fn inbox_value(proj: &crate::project::Project, gs: &crate::gen::GenSettings, out: &Path, store: &Store) -> Value {
    let mut questions = 0usize;
    let mut proposals = 0usize;
    let mut drafts = 0usize;
    let mut problems: BTreeMap<String, usize> = BTreeMap::new();
    for d in store.graph.diagnostics.values() {
        if d.lifecycle != "open" || d.triage.as_deref() == Some("suppressed") {
            continue;
        }
        if d.rule == DRAFT_RULE {
            drafts += 1;
        } else if PROPOSAL_RULES.contains(&d.rule.as_str()) {
            proposals += 1;
        } else if d.prompt.is_some() && d.answer.is_none() {
            questions += 1;
        } else {
            *problems.entry(d.severity.clone()).or_default() += 1;
        }
    }
    let control = crate::control::Control::load(proj, out);
    let mut pending_docs: Vec<String> = Vec::new();
    if control.compile == "manual" {
        for (doc, hash) in crate::control::current_doc_hashes(proj) {
            if control.released.compile.get(&doc) != Some(&hash) {
                pending_docs.push(doc);
            }
        }
    }
    let mut pending_entities = 0usize;
    if control.generate == "manual" && control.released.generate != store.status.generation {
        pending_entities = crate::gen::pending(store, gs).len();
    }
    let errors = problems.get("error").copied().unwrap_or(0);
    json!({
        "generation": store.status.generation,
        "questions": questions,
        "proposals": proposals,
        "problems": problems,
        "pending": { "docs": pending_docs, "entities": pending_entities },
        "drafts": drafts,
        "modes": { "compile": control.compile, "generate": control.generate },
        "total": questions + proposals + pending_docs.len() + errors,
    })
}

pub async fn inbox(State(st): State<SharedState>) -> Json<Value> {
    let proj = st.proj();
    let gs = st.gs();
    let out = st.out.clone();
    Json(
        tokio::task::spawn_blocking(move || {
            let store = Store::load(&out);
            inbox_value(&proj, &gs, &out, &store)
        })
        .await
        .expect("inbox task panicked"),
    )
}

// ---------------------------------------------------------------------------
// The trace matrix as a file: csv or md (docs/frontends/gui.md#trace-matrix).

#[derive(Deserialize)]
pub struct MatrixQ {
    format: Option<String>,
}

const COLUMNS: [&str; 8] = ["requirement", "entity", "status", "reason", "repair", "test", "lastRun", "evidence"];

fn cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn matrix_export(rows: &BTreeMap<String, Value>, format: &str) -> Option<(String, &'static str, &'static str)> {
    let field = |row: &Value, col: &str| -> String {
        match col {
            "test" => cell(&row["test"]["label"]),
            _ => cell(&row[col]),
        }
    };
    match format {
        "csv" => {
            let mut out = COLUMNS.join(",");
            out.push('\n');
            for (rid, row) in rows {
                let mut cells = vec![csv_escape(rid)];
                cells.extend(COLUMNS[1..].iter().map(|c| csv_escape(&field(row, c))));
                out.push_str(&cells.join(","));
                out.push('\n');
            }
            Some((out, "text/csv; charset=utf-8", "trace-matrix.csv"))
        }
        "md" => {
            let mut out = format!("| {} |\n|{}|\n", COLUMNS.join(" | "), COLUMNS.iter().map(|_| "---").collect::<Vec<_>>().join("|"));
            for (rid, row) in rows {
                let mut cells = vec![rid.clone()];
                cells.extend(COLUMNS[1..].iter().map(|c| field(row, c).replace('|', "\\|").replace('\n', " ")));
                out.push_str(&format!("| {} |\n", cells.join(" | ")));
            }
            Some((out, "text/markdown; charset=utf-8", "trace-matrix.md"))
        }
        _ => None,
    }
}

// The matrix rows the JSON endpoint serves, shared with the export.
pub fn matrix_rows(store: &Store, gs: &crate::gen::GenSettings) -> (BTreeMap<String, Value>, BTreeMap<String, usize>) {
    let statuses = crate::verify::status_map(store, gs);
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
    (rows, by_status)
}

pub async fn verify_matrix(State(st): State<SharedState>, Query(q): Query<MatrixQ>) -> Response {
    let store = super::api::load_store(&st).await;
    let (rows, counts) = matrix_rows(&store, &st.gs());
    match q.format.as_deref() {
        None | Some("json") => Json(json!({ "rows": rows, "counts": counts })).into_response(),
        Some(f) => match matrix_export(&rows, f) {
            Some((body, ctype, name)) => (
                [
                    (header::CONTENT_TYPE, ctype.to_string()),
                    (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", name)),
                ],
                body,
            )
                .into_response(),
            None => err(StatusCode::BAD_REQUEST, format!("unknown format {}; csv, md, or json", f)),
        },
    }
}
