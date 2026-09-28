// POST /api/facts/{id}/edit on the server: a thin wrapper around the shared edit
// in crate::webapi::facts. Mirrors docs/frontends/gui.md#api (facts).
use super::state::SharedState;
use crate::webapi::facts::edit_fact_core;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::{json, Value};

pub async fn edit_fact(
    State(st): State<SharedState>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let proj = st.proj();
    let out = st.out.clone();
    let result = tokio::task::spawn_blocking(move || edit_fact_core(&proj, &out, &id, &body))
        .await
        .expect("facts edit task panicked");
    match result {
        Ok(mut v) => {
            // A committed edit moves the board; the watcher also catches it, but the
            // click deserves an immediate refresh.
            if v["committed"].as_bool().unwrap_or(false) {
                super::events::emit_board_changed(&st);
            }
            v["ok"] = json!(true);
            Json(v).into_response()
        }
        Err((code, msg)) => super::api::err(
            StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST),
            msg,
        ),
    }
}

