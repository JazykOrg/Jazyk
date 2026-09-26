// The deliverable browser's axum handlers: thin wrappers that read off the async
// runtime and answer the values crate::webapi::deliverable builds. Mirrors
// docs/frontends/gui.md#api (deliverable).
use super::api::refused;
use super::state::SharedState;
use crate::webapi::deliverable::{deliverable_baseline, deliverable_file, deliverable_value};
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;

pub async fn listing(State(st): State<SharedState>) -> Json<serde_json::Value> {
    let (out_dir, gs) = (st.out.clone(), st.gs());
    let v = tokio::task::spawn_blocking(move || deliverable_value(&out_dir, &gs))
        .await
        .expect("deliverable listing");
    Json(v)
}

#[derive(Deserialize)]
pub struct FileQ {
    path: String,
}

pub async fn baseline(State(st): State<SharedState>, Query(p): Query<FileQ>) -> Response {
    let out_dir = st.out.clone();
    let result = tokio::task::spawn_blocking(move || deliverable_baseline(&out_dir, &p.path))
        .await
        .expect("baseline read");
    match result {
        Ok(v) => Json(v).into_response(),
        Err(r) => refused(r),
    }
}

pub async fn file(State(st): State<SharedState>, Query(p): Query<FileQ>) -> Response {
    let (out_dir, gs) = (st.out.clone(), st.gs());
    let result = tokio::task::spawn_blocking(move || deliverable_file(&out_dir, &gs, &p.path))
        .await
        .expect("deliverable read");
    match result {
        Ok(v) => Json(v).into_response(),
        Err(r) => refused(r),
    }
}
