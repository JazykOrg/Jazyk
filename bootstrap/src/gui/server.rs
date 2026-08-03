// Router assembly, session-token check, bind with port fallback, graceful shutdown.
use super::state::SharedState;
use super::{api, assets, deliverable, diff, docs, events, jobs, lsp_ws};
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
    let guarded = (path.starts_with("/api") || path == "/lsp") && path != "/api/ping";
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
        .route("/ping", get(api::ping))
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
        .route("/feedback", get(api::feedback))
        .route("/benchmarks", get(api::benchmarks))
        .route("/benchmarks/models", get(api::benchmark_models))
        .route("/diff", get(diff::diff))
        .route("/diagnostics/{id}/triage", post(api::triage))
        .route("/docs", get(api::docs))
        .route("/docs/content", get(api::doc_content).put(docs::doc_write).delete(docs::doc_delete))
        .route("/docs/baseline", get(api::doc_baseline))
        .route("/docs/rename", post(docs::doc_rename))
        .route("/gen/pending", get(api::gen_pending))
        .route("/gen/task/{id}", get(api::gen_task))
        .route("/verify/pending", get(api::verify_pending))
        .route("/verify/matrix", get(api::verify_matrix))
        .route("/docsgen/{slug}", get(api::docsgen))
        .route("/jobs", get(jobs::list_jobs).post(jobs::post_job))
        .route("/jobs/{id}", get(jobs::get_job))
        .route("/jobs/{id}/cancel", post(jobs::cancel_job))
        .route("/deliverable", get(deliverable::listing))
        .route("/deliverable/file", get(deliverable::file))
        .route("/deliverable/baseline", get(deliverable::baseline))
        .route("/trace", get(jobs::list_traces))
        .route("/trace/{stem}", get(jobs::get_trace))
        .route("/trace/{stem}/{n}", get(jobs::get_trace_event))
        .route("/settings", get(api::settings_get).put(api::settings_put))
        .route("/watch", get(api::watch_get).put(api::watch_put))
        .route("/workers", get(api::workers))
        .route("/release", post(api::release))
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

// Serve until ctrl-c or /api/shutdown. No graceful drain: the event stream and the
// editor WebSocket stay open as long as a tab lives, so waiting on them would keep
// ctrl-c hanging until the browser closes. Dropping the server closes everything.
pub async fn serve(listener: tokio::net::TcpListener, st: SharedState) -> Result<(), String> {
    let app = router(st.clone());
    tokio::select! {
        r = axum::serve(listener, app) => r.map_err(|e| format!("server error: {}", e)),
        _ = tokio::signal::ctrl_c() => Ok(()),
        _ = st.shutdown.notified() => Ok(()),
    }
}
