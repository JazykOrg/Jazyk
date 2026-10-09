// The project files' axum handlers: thin wrappers around crate::webapi::projfiles.
// Mirrors docs/frontends/gui.md#api (the project files).
use super::api::refused;
use super::state::SharedState;
use crate::webapi::outfiles::content_type;
use crate::webapi::projfiles::{create_project_file, project_file, project_files, project_text, write_project_text};
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct FileQ {
    path: String,
}

// Every file under the project directory, with where the product and out sit.
pub async fn list(State(st): State<SharedState>) -> Response {
    let root = st.root.clone();
    let out = st.out.clone();
    let deliverable = st.gs.read().expect("settings").deliverable.clone();
    let glob = st.proj.read().expect("project").docs_glob.clone();
    let v = tokio::task::spawn_blocking(move || project_files(&root, &deliverable, &out, &glob))
        .await
        .expect("project listing");
    Json(v).into_response()
}

// One file under the project directory, as bytes with its content type.
pub async fn file(State(st): State<SharedState>, Query(p): Query<FileQ>) -> Response {
    let root = st.root.clone();
    let path = p.path.clone();
    let result = tokio::task::spawn_blocking(move || project_file(&root, &path))
        .await
        .expect("project file read");
    match result {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, content_type(&p.path)), (header::CACHE_CONTROL, "no-cache")],
            bytes,
        )
            .into_response(),
        Err(r) => refused(r),
    }
}

// Create one new file anywhere under the project directory; never an overwrite.
pub async fn create(State(st): State<SharedState>, Query(p): Query<FileQ>, Json(body): Json<serde_json::Value>) -> Response {
    let root = st.root.clone();
    let out = st.out.clone();
    match tokio::task::spawn_blocking(move || create_project_file(&root, &out, &p.path, &body)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(r)) => refused(r),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// An existing project file as text with its hash, for editing.
pub async fn text(State(st): State<SharedState>, Query(p): Query<FileQ>) -> Response {
    let root = st.root.clone();
    match tokio::task::spawn_blocking(move || project_text(&root, &p.path)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(r)) => refused(r),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// Replace an existing project file when its hash still matches.
pub async fn write_text(State(st): State<SharedState>, Query(p): Query<FileQ>, Json(body): Json<serde_json::Value>) -> Response {
    let root = st.root.clone();
    let out = st.out.clone();
    match tokio::task::spawn_blocking(move || write_project_text(&root, &out, &p.path, &body)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(r)) => refused(r),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
