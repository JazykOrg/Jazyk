// The axum transport for the view values. Every handler is a thin wrapper around
// `crate::webapi::views`; the browser backend wraps the same functions its own way.
use super::state::SharedState;
use crate::webapi::views::{tree_value, view_value};
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::Value;

pub async fn views(
    State(st): State<SharedState>,
    Query(q): Query<std::collections::BTreeMap<String, String>>,
) -> Json<Value> {
    let store = super::api::load_store(&st).await;
    let args = serde_json::to_value(q).unwrap_or_default();
    Json(crate::webapi::views::list_value(&store, &args))
}
#[derive(Deserialize)]
pub struct ViewQ {
    // Levels of detail beneath the members (docs/frontends/gui.md#api, `?detail=N`).
    detail: Option<usize>,
}
pub async fn view(
    State(st): State<SharedState>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<ViewQ>,
) -> Response {
    let store = super::api::load_store(&st).await;
    match view_value(&store, &id, q.detail.unwrap_or(0)) {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no view {}", id)),
    }
}
pub async fn tree(State(st): State<SharedState>) -> Json<Value> {
    let store = super::api::load_store(&st).await;
    Json(tree_value(&store))
}

pub async fn diagram_suppressions(State(st): State<SharedState>) -> Json<Value> {
    let store = super::api::load_store(&st).await;
    Json(crate::diagrams::suppressions(&store))
}

pub async fn diagram_coverage(State(st): State<SharedState>) -> Json<Value> {
    let store = super::api::load_store(&st).await;
    Json(crate::webapi::diagrams::coverage(&store))
}

pub async fn edit_diagram(State(st): State<SharedState>, Json(body): Json<Value>) -> Response {
    let proj = st.proj();
    let out = st.out.clone();
    let result =
        tokio::task::spawn_blocking(move || crate::webapi::diagrams::edit(&proj, &out, &body))
            .await
            .expect("diagram edit task panicked");
    match result {
        Ok(v) => {
            super::events::emit_board_changed(&st);
            Json(v).into_response()
        }
        Err(e) => super::api::refused(e),
    }
}
