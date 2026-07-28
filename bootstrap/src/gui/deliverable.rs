// The deliverable browser: the generated product listed with the graph nodes the
// ledger binds to each file, and per-file traceability markers parsed against the
// live requirements. Read-only. Mirrors docs/frontends/gui.md#api (deliverable).
use super::api::err;
use super::state::SharedState;
use crate::store::Store;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        // Same skip list as doc collection, so a deliverable at the project root
        // lists the product, not the compiler's output.
        if name.starts_with('.') || name == "node_modules" || name == "target" || name.starts_with("jazyk-out") {
            continue;
        }
        if p.is_dir() {
            walk(&p, base, out);
        } else if let Ok(rel) = p.strip_prefix(base) {
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((rel.to_string_lossy().replace('\\', "/"), size));
        }
    }
}

// The ledger reversed: file path -> the nodes bound to it.
fn ownership(out_dir: &Path, gs: &crate::gen::GenSettings) -> BTreeMap<String, Value> {
    let ledger = crate::gen::Ledger::load(out_dir);
    let mut by_file: BTreeMap<String, (Vec<String>, Vec<String>, Vec<String>)> = BTreeMap::new();
    for (eid, ent) in &ledger.entities {
        for f in &ent.files {
            by_file.entry(f.clone()).or_default().0.push(eid.clone());
        }
    }
    for (rid, row) in &ledger.requirements {
        for f in &row.files {
            by_file.entry(f.clone()).or_default().1.push(rid.clone());
        }
        // A programmatic test artifact lives under the deliverable too.
        if row.test.kind == "programmatic" && !row.test.artifact.is_empty() {
            by_file.entry(row.test.artifact.clone()).or_default().2.push(rid.clone());
        }
    }
    let _ = gs;
    by_file
        .into_iter()
        .map(|(f, (mut e, mut r, mut t))| {
            e.dedup();
            r.dedup();
            t.dedup();
            (f, json!({ "entities": e, "requirements": r, "tests": t }))
        })
        .collect()
}

pub async fn listing(State(st): State<SharedState>) -> Json<Value> {
    let (out_dir, gs) = (st.out.clone(), st.gs());
    let v = tokio::task::spawn_blocking(move || {
        let mut files: Vec<(String, u64)> = Vec::new();
        walk(&gs.deliverable, &gs.deliverable, &mut files);
        files.sort();
        let own = ownership(&out_dir, &gs);
        let empty = json!({ "entities": [], "requirements": [], "tests": [] });
        json!({
            "root": gs.deliverable.to_string_lossy(),
            "files": files
                .into_iter()
                .map(|(path, size)| {
                    let o = own.get(&path).unwrap_or(&empty);
                    json!({ "path": path, "size": size, "owners": o })
                })
                .collect::<Vec<_>>(),
        })
    })
    .await
    .expect("deliverable listing");
    Json(v)
}

// Resolve a client path strictly inside the deliverable directory.
fn safe_path(gs: &crate::gen::GenSettings, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains('\\') {
        return None;
    }
    if rel.split('/').any(|c| c.is_empty() || c == "." || c == ".." || c.starts_with('.')) {
        return None;
    }
    let abs = gs.deliverable.join(rel);
    let root = gs.deliverable.canonicalize().ok()?;
    let canon = abs.canonicalize().ok()?;
    if !canon.starts_with(&root) {
        return None;
    }
    Some(canon)
}

#[derive(Deserialize)]
pub struct FileQ {
    path: String,
}

// Traceability markers: `req:<id> hash:<hash8>`, in any comment syntax. A marker is
// stale when its statement-hash prefix no longer matches the live requirement, and
// unresolved when the requirement is gone.
fn markers(store: &Store, text: &str) -> Vec<Value> {
    let re = regex::Regex::new(r"req:([A-Za-z0-9][A-Za-z0-9-]*)\s+hash:([0-9a-f]{4,16})").unwrap();
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        for c in re.captures_iter(line) {
            let rid = format!("req:{}", &c[1]);
            let marked = c[2].to_string();
            let resolved = store.resolve_id(&rid).to_string();
            let (exists, stale) = match store.graph.requirements.get(&resolved) {
                Some(r) => {
                    let live = crate::model::hash_hex(&r.ears);
                    (true, !live.starts_with(&marked))
                }
                None => (false, false),
            };
            out.push(json!({
                "line": i + 1,
                "requirement": resolved,
                "hash": marked,
                "exists": exists,
                "stale": stale,
            }));
        }
    }
    out
}

pub async fn file(State(st): State<SharedState>, Query(p): Query<FileQ>) -> Response {
    let (out_dir, gs) = (st.out.clone(), st.gs());
    let path = p.path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, (StatusCode, String)> {
        let Some(abs) = safe_path(&gs, &path) else {
            return Err((StatusCode::BAD_REQUEST, format!("invalid deliverable path {}", path)));
        };
        let bytes = std::fs::read(&abs)
            .map_err(|e| (StatusCode::NOT_FOUND, format!("cannot read {}: {}", path, e)))?;
        let own = ownership(&out_dir, &gs);
        let empty = json!({ "entities": [], "requirements": [], "tests": [] });
        let owners = own.get(&path).unwrap_or(&empty).clone();
        match String::from_utf8(bytes) {
            Ok(text) => {
                let store = Store::load(&out_dir);
                let marks = markers(&store, &text);
                Ok(json!({ "path": path, "text": text, "markers": marks, "owners": owners }))
            }
            Err(e) => Ok(json!({
                "path": path,
                "binary": true,
                "size": e.as_bytes().len(),
                "owners": owners,
            })),
        }
    })
    .await
    .expect("deliverable read");
    match result {
        Ok(v) => Json(v).into_response(),
        Err((code, msg)) => err(code, msg),
    }
}
