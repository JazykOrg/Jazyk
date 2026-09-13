// The axum transport for the card values (`crate::webapi::card`).
use super::state::SharedState;
use crate::webapi::card::{card_value, page_value};
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};

pub async fn entity_card_handler(
    State(st): State<SharedState>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    let store = super::api::load_store(&st).await;
    match card_value(&store, &id) {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no entity {}", id)),
    }
}
pub async fn view_page_handler(
    State(st): State<SharedState>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    let store = super::api::load_store(&st).await;
    match page_value(&store, &id) {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no view {}", id)),
    }
}
