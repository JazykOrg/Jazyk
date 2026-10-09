use crate::vfs::VPathExt;
// Document read/write for the GUI: path validation and the conditional write.
// The write path is the security-sensitive surface; every rule here has a test.
use super::api::err;
use super::state::SharedState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
pub use crate::webapi::docs::safe_doc_path;
use serde::Deserialize;
use serde_json::json;


#[derive(Deserialize)]
pub struct DocPathQ {
    path: String,
}

#[derive(Deserialize)]
pub struct DocWrite {
    text: String,
    #[serde(rename = "baseHash")]
    base_hash: Option<String>,
}

// Conditional write: the caller names the hash it edited from; a mismatch means the
// file moved underneath it (another editor), and the client re-reads.
pub async fn doc_write(
    State(st): State<SharedState>,
    Query(p): Query<DocPathQ>,
    Json(body): Json<DocWrite>,
) -> Response {
    match crate::webapi::docs::doc_write(&st.proj(), &p.path, &body.text, body.base_hash.as_deref()) {
        crate::webapi::docs::DocWrite::Written(value) => Json(value).into_response(),
        crate::webapi::docs::DocWrite::Conflict(value) => (StatusCode::CONFLICT, Json(value)).into_response(),
        crate::webapi::docs::DocWrite::Invalid(message) => err(StatusCode::BAD_REQUEST, message),
        crate::webapi::docs::DocWrite::Failed(message) => err(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

#[derive(Deserialize)]
pub struct DocRename {
    from: String,
    to: String,
}

// Move a document. The graph is untouched: the next build's dirty set sees the move
// and the reconciler rewrites references mechanically.
pub async fn doc_rename(State(st): State<SharedState>, Json(body): Json<DocRename>) -> Response {
    let Some(from) = safe_doc_path(&st.proj(), &body.from) else {
        return err(
            StatusCode::BAD_REQUEST,
            format!("invalid document path {}", body.from),
        );
    };
    let Some(to) = safe_doc_path(&st.proj(), &body.to) else {
        return err(
            StatusCode::BAD_REQUEST,
            format!("invalid document path {}", body.to),
        );
    };
    if !from.vexists() {
        return err(StatusCode::NOT_FOUND, format!("no document {}", body.from));
    }
    if to.vexists() {
        return err(StatusCode::CONFLICT, format!("{} already exists", body.to));
    }
    if let Some(parent) = to.parent() {
        if let Err(e) = crate::vfs::create_dir_all(parent) {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("cannot create {}: {}", parent.display(), e),
            );
        }
    }
    if let Err(e) = crate::vfs::rename(&from, &to) {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot rename: {}", e),
        );
    }
    Json(json!({ "from": body.from, "to": body.to })).into_response()
}

// Delete a document. The graph is untouched: the next build reconciles the
// disappearance and garbage collection removes what nothing mentions anymore.
pub async fn doc_delete(State(st): State<SharedState>, Query(p): Query<DocPathQ>) -> Response {
    let Some(abs) = safe_doc_path(&st.proj(), &p.path) else {
        return err(
            StatusCode::BAD_REQUEST,
            format!("invalid document path {}", p.path),
        );
    };
    if !abs.vexists() {
        return err(StatusCode::NOT_FOUND, format!("no document {}", p.path));
    }
    if let Err(e) = crate::vfs::remove_file(&abs) {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot delete {}: {}", p.path, e),
        );
    }
    Json(json!({ "deleted": p.path })).into_response()
}

// Original bytes, projection and diagnostics share one revision.
pub async fn binary_read(State(st): State<SharedState>, Query(p): Query<DocPathQ>) -> Response {
    let project = st.proj();
    match tokio::task::spawn_blocking(move || crate::webapi::binary_documents::read(&project, &p.path)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(r)) => super::api::refused(r),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
pub async fn binary_write(State(st): State<SharedState>, Query(p): Query<DocPathQ>, Json(body): Json<serde_json::Value>) -> Response {
    let project = st.proj();
    match tokio::task::spawn_blocking(move || crate::webapi::binary_documents::write(&project, &p.path, &body)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(r)) => super::api::refused(r),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
