// The deliverable browser's axum handlers: thin wrappers that read off the async
// runtime and answer the values crate::webapi::deliverable builds. Mirrors
// docs/frontends/gui.md#api (deliverable).
use super::api::refused;
use super::state::SharedState;
use crate::webapi::deliverable::{
    deliverable_baseline, deliverable_file, deliverable_raw, deliverable_value,
};
use crate::webapi::overview::overview_value;
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

// One product file as bytes with its content type: a built artifact the overview
// renders or downloads (docs/frontends/gui.md#api, deliverable).
pub async fn raw(State(st): State<SharedState>, Query(p): Query<FileQ>) -> Response {
    let gs = st.gs();
    let path = p.path.clone();
    let result = tokio::task::spawn_blocking(move || deliverable_raw(&gs, &path))
        .await
        .expect("deliverable raw read");
    match result {
        Ok(bytes) => super::outfiles::file_bytes(&p.path, bytes),
        Err(r) => refused(r),
    }
}

// The Product lens's overview (docs/frontends/gui.md#api, `GET /api/product/overview`).
pub async fn overview(State(st): State<SharedState>) -> Json<serde_json::Value> {
    let (proj, gs) = (st.proj(), st.gs());
    let v = tokio::task::spawn_blocking(move || overview_value(&proj, &gs))
        .await
        .expect("product overview");
    Json(v)
}
