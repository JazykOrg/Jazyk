// The project files' axum handlers: thin wrappers around crate::webapi::projfiles.
// Mirrors docs/frontends/gui.md#api (the project files).
use super::api::refused;
use super::state::SharedState;
use crate::webapi::outfiles::content_type;
use crate::webapi::projfiles::{project_file, project_files};
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
