// The out directory's axum handlers: thin wrappers around crate::webapi::outfiles.
// The file read answers the bytes with the content type its extension names.
// Mirrors docs/frontends/gui.md#api (the out directory).
use super::api::refused;
use super::state::SharedState;
use crate::webapi::outfiles::{content_type, out_file, out_list};
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct OutQ {
    path: String,
}

// One file under the out directory, as bytes with its content type.
pub async fn file(State(st): State<SharedState>, Query(p): Query<OutQ>) -> Response {
    let out = st.out.clone();
    let path = p.path.clone();
    let result = tokio::task::spawn_blocking(move || out_file(&out, &path))
        .await
        .expect("out file read");
    match result {
        Ok(bytes) => file_bytes(&p.path, bytes),
        Err(r) => refused(r),
    }
}

// A file served as its bytes, with the type its extension names. The sandbox policy
// keeps a file opened directly in a tab (an SVG with a script) from running with the
// API's origin, and nosniff keeps the browser to the named type
// (docs/frontends/gui.md#serving).
pub(super) fn file_bytes(path: &str, bytes: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type(path)),
            (header::CACHE_CONTROL, "no-cache"),
            (header::CONTENT_SECURITY_POLICY, "sandbox"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response()
}

// The files under one directory of the out directory, recursively.
pub async fn list(State(st): State<SharedState>, Query(p): Query<OutQ>) -> Response {
    let out = st.out.clone();
    let result = tokio::task::spawn_blocking(move || out_list(&out, &p.path))
        .await
        .expect("out listing");
    match result {
        Ok(v) => Json(v).into_response(),
        Err(r) => refused(r),
    }
}
