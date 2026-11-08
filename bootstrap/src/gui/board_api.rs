// The axum transport for the board and causality values (`crate::webapi::board`).
use super::state::SharedState;
use crate::webapi::board::{board_value, explain_value, preview_value, ripple_value};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::Value;

pub async fn board(State(st): State<SharedState>) -> Json<Value> {
    let proj = st.proj();
    let out = st.out.clone();
    Json(
        tokio::task::spawn_blocking(move || board_value(&proj, &out))
            .await
            .expect("board task panicked"),
    )
}
pub async fn selected_board(
    State(st): State<SharedState>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let proj = st.proj();
    let out = st.out.clone();
    if proj.set_id.is_none() {
        return super::api::err(
            axum::http::StatusCode::BAD_REQUEST,
            "a selection needs a document set: send setId",
        );
    }
    match tokio::task::spawn_blocking(move || {
        crate::webapi::board::selected_board_value(&proj, &out, &body)
    })
    .await
    .expect("board task panicked")
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => super::api::err(axum::http::StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Deserialize)]
pub struct PreviewQ {
    goal: Option<String>,
}
pub async fn preview(State(st): State<SharedState>, Query(p): Query<PreviewQ>) -> Json<Value> {
    let proj = st.proj();
    let out = st.out.clone();
    let target = p.goal.unwrap_or_default();
    Json(
        tokio::task::spawn_blocking(move || preview_value(&proj, &out, &target))
            .await
            .expect("preview task panicked"),
    )
}
#[derive(Deserialize)]
pub struct ExplainQ {
    target: String,
}
// A goal: its change, cause, readiness, blockers. A node or section: the cone of
// goals a change to it would open. The same rendering as `jazyk explain`.
pub async fn explain(State(st): State<SharedState>, Query(p): Query<ExplainQ>) -> Response {
    let proj = st.proj();
    let out = st.out.clone();
    let target = p.target.clone();
    let result = tokio::task::spawn_blocking(move || explain_value(&proj, &out, &target))
        .await
        .expect("explain task panicked");
    match result {
        Some(v) => Json(v).into_response(),
        None => super::api::err(
            StatusCode::NOT_FOUND,
            format!("`{}` names no goal and no known target", p.target),
        ),
    }
}
#[derive(Deserialize)]
pub struct RippleQ {
    generation: Option<u64>,
    target: Option<String>,
    #[serde(default)]
    back: bool,
}
// The causality DAG over the journal, forward from a generation or the last cascade
// that touched a target; `back=true` walks causes instead. A generation's DAG
// doubles as the whole-build report.
pub async fn ripple(State(st): State<SharedState>, Query(p): Query<RippleQ>) -> Response {
    let root = match (&p.generation, &p.target) {
        (Some(g), _) => format!("g{}", g),
        (None, Some(t)) if !t.trim().is_empty() => t.trim().to_string(),
        _ => {
            return super::api::err(
                StatusCode::BAD_REQUEST,
                "pass generation= or target= to root the DAG",
            )
        }
    };
    let out = st.out.clone();
    let back = p.back;
    let result = tokio::task::spawn_blocking(move || ripple_value(&out, &root, back))
        .await
        .expect("ripple task panicked");
    match result {
        Some(v) => Json(v).into_response(),
        None => super::api::err(
            StatusCode::NOT_FOUND,
            "the journal holds no entry touching that root",
        ),
    }
}
