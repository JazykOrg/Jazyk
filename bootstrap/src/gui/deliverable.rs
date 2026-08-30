// The deliverable browser: the generated product listed with the graph nodes the
// ledger binds to each file, and the ledger's anchored sites resolved against the
// current file text. Read-only. Mirrors docs/frontends/gui.md#api (deliverable).
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
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        // Same skip list as doc collection, so a deliverable at the project root
        // lists the product, not the compiler's output.
        if name.starts_with('.')
            || name == "node_modules"
            || name == "target"
            || name.starts_with("jazyk-out")
        {
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
            by_file
                .entry(row.test.artifact.clone())
                .or_default()
                .2
                .push(rid.clone());
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

// Resolve a client path strictly inside the given root directory.
fn safe_under(root_dir: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains('\\') {
        return None;
    }
    if rel
        .split('/')
        .any(|c| c.is_empty() || c == "." || c == ".." || c.starts_with('.'))
    {
        return None;
    }
    let abs = root_dir.join(rel);
    let root = root_dir.canonicalize().ok()?;
    let canon = abs.canonicalize().ok()?;
    if !canon.starts_with(&root) {
        return None;
    }
    Some(canon)
}

fn safe_path(gs: &crate::gen::GenSettings, rel: &str) -> Option<PathBuf> {
    safe_under(&gs.deliverable, rel)
}

#[derive(Deserialize)]
pub struct FileQ {
    path: String,
}

// Resolved sites for one file: the ledger's anchored sites located against the
// current text (exact, moved, or lost), plus each programmatic test whose artifact
// is this file, located by its embedded test name. Mirrors
// docs/frontends/gui.md#api (deliverable) and docs/consumers/gen.md#traceability.
fn sites(store: &Store, out_dir: &Path, path: &str, text: &str) -> Vec<Value> {
    let ledger = crate::gen::Ledger::load(out_dir);
    let mut out = Vec::new();
    for (rid, row) in &ledger.requirements {
        let exists = store.graph.requirements.contains_key(store.resolve_id(rid));
        for s in &row.sites {
            if s.file != path {
                continue;
            }
            let located = crate::gen::locate_head(text, &s.head, s.line);
            out.push(json!({
                "line": located.map(|(l, _)| l),
                "requirement": rid,
                "kind": "site",
                "located": match located {
                    Some((_, true)) => "exact",
                    Some(_) => "moved",
                    None => "lost",
                },
                "exists": exists,
            }));
        }
        if row.test.kind == "programmatic" && row.test.artifact == path && !row.test.name.is_empty()
        {
            let line = text
                .lines()
                .position(|l| l.contains(&row.test.name))
                .map(|i| i + 1);
            out.push(json!({
                "line": line,
                "requirement": rid,
                "kind": "test",
                "located": if line.is_some() { "exact" } else { "lost" },
                "exists": exists,
            }));
        }
    }
    out
}

// The file as it stood before the last generation run rewrote it, from the snapshot
// generation takes at write time. 404 when generation never rewrote the file.
// Mirrors docs/frontends/gui.md#api (deliverable) and
// docs/consumers/gen.md#incremental-regeneration.
pub async fn baseline(State(st): State<SharedState>, Query(p): Query<FileQ>) -> Response {
    let out_dir = st.out.clone();
    let path = p.path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, (StatusCode, String)> {
        let root = out_dir.join("deliverable-baseline");
        let Some(abs) = safe_under(&root, &path) else {
            return Err((StatusCode::NOT_FOUND, format!("no baseline for {}", path)));
        };
        let bytes = std::fs::read(&abs)
            .map_err(|_| (StatusCode::NOT_FOUND, format!("no baseline for {}", path)))?;
        match String::from_utf8(bytes) {
            Ok(text) => Ok(json!({ "path": path, "text": text })),
            Err(e) => Ok(json!({ "path": path, "binary": true, "size": e.as_bytes().len() })),
        }
    })
    .await
    .expect("baseline read");
    match result {
        Ok(v) => Json(v).into_response(),
        Err((code, msg)) => err(code, msg),
    }
}

pub async fn file(State(st): State<SharedState>, Query(p): Query<FileQ>) -> Response {
    let (out_dir, gs) = (st.out.clone(), st.gs());
    let path = p.path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, (StatusCode, String)> {
        let Some(abs) = safe_path(&gs, &path) else {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("invalid deliverable path {}", path),
            ));
        };
        let bytes = std::fs::read(&abs).map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                format!("cannot read {}: {}", path, e),
            )
        })?;
        let own = ownership(&out_dir, &gs);
        let empty = json!({ "entities": [], "requirements": [], "tests": [] });
        let owners = own.get(&path).unwrap_or(&empty).clone();
        match String::from_utf8(bytes) {
            Ok(text) => {
                let store = Store::load(&out_dir);
                let resolved = sites(&store, &out_dir, &path, &text);
                Ok(json!({ "path": path, "text": text, "sites": resolved, "owners": owners }))
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
