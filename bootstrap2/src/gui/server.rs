// Router assembly, session-token check, bind with port fallback, graceful shutdown.
use super::state::SharedState;
use super::{api, assets, diff, docs, events, jobs, lsp_ws};
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use std::net::SocketAddr;

// The token guards /api and /lsp: any local process could otherwise spend LLM budget
// or write documents. Assets are open; the token travels in the opened URL's fragment
// and the app sends it back as a bearer header (or `token` query for SSE/WS).
async fn auth(State(st): State<SharedState>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let guarded = path.starts_with("/api") || path == "/lsp";
    let Some(expect) = st.token.as_deref() else {
        return next.run(req).await;
    };
    if !guarded {
        return next.run(req).await;
    }
    let bearer = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let query_token = req.uri().query().and_then(|q| {
        q.split('&')
            .find_map(|kv| kv.strip_prefix("token=").map(|v| v.to_string()))
    });
    let ok = bearer == Some(expect) || query_token.as_deref() == Some(expect);
    if ok {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, "missing or invalid session token").into_response()
    }
}

pub fn router(st: SharedState) -> Router {
    let api = Router::new()
        .route("/project", get(api::project))
        .route("/status", get(api::status))
        .route("/graph", get(api::graph))
        .route("/entities/{id}", get(api::entity))
        .route("/requirements/{id}", get(api::requirement))
        .route("/search", get(api::search))
        .route("/context", get(api::context))
        .route("/coverage", get(api::coverage))
        .route("/overview", get(api::overview))
        .route("/journal", get(api::journal))
        .route("/diff", get(diff::diff))
        .route("/diagnostics/{id}/triage", post(api::triage))
        .route("/docs", get(api::docs))
        .route("/docs/content", get(api::doc_content).put(docs::doc_write))
        .route("/gen/pending", get(api::gen_pending))
        .route("/gen/task/{id}", get(api::gen_task))
        .route("/verify/pending", get(api::verify_pending))
        .route("/verify/matrix", get(api::verify_matrix))
        .route("/docsgen/{slug}", get(api::docsgen))
        .route("/jobs", get(jobs::list_jobs).post(jobs::post_job))
        .route("/jobs/{id}", get(jobs::get_job))
        .route("/jobs/{id}/cancel", post(jobs::cancel_job))
        .route("/watch", get(api::watch_get).put(api::watch_put))
        .route("/events", get(events::sse))
        .route("/shutdown", post(api::shutdown));
    Router::new()
        .nest("/api", api)
        .route("/lsp", get(lsp_ws::ws))
        .fallback(assets::serve)
        .layer(middleware::from_fn_with_state(st.clone(), auth))
        .with_state(st)
}

pub const DEFAULT_PORT: u16 = 4680;

// Bind: an explicit busy port is an error; the busy default falls back to ephemeral.
pub async fn bind(port: Option<u16>) -> Result<tokio::net::TcpListener, String> {
    let want = port.unwrap_or(DEFAULT_PORT);
    let addr = SocketAddr::from(([127, 0, 0, 1], want));
    match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => Ok(l),
        Err(e) if port.is_some() => Err(format!("cannot bind 127.0.0.1:{}: {}", want, e)),
        Err(_) => tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .map_err(|e| format!("cannot bind 127.0.0.1: {}", e)),
    }
}

pub async fn serve(listener: tokio::net::TcpListener, st: SharedState) -> Result<(), String> {
    let app = router(st.clone());
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = st.shutdown.notified() => {}
            }
        })
        .await
        .map_err(|e| format!("server error: {}", e))
}
