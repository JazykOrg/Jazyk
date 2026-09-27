// Router assembly, session-token check, CORS for the page's origin, bind with port
// fallback, graceful shutdown. The server serves the API alone; the page is at
// jazyk.dev (docs/frontends/gui.md#serving).
use super::state::SharedState;
use super::{
    api, board_api, card_api, chat, deliverable, diff, docs, events, facts, jobs, lsp_ws, outfiles,
    product, views_api,
};
use axum::extract::{Request, State};
use axum::http::{header, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use std::net::SocketAddr;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

// The token guards /api and /lsp: any local process could otherwise spend LLM budget
// or write documents. The token travels in the opened URL's connect route and the
// app sends it back as a bearer header (or `token` query for SSE/WS). A preflight
// carries no token by design (the browser sends it before the request), so OPTIONS
// passes; the CORS layer answers it.
async fn auth(State(st): State<SharedState>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let guarded = (path.starts_with("/api") || path == "/lsp") && path != "/api/ping";
    let Some(expect) = st.token.as_deref() else {
        return next.run(req).await;
    };
    if !guarded || req.method() == Method::OPTIONS {
        return next.run(req).await;
    }
    let bearer = req
        .headers()
        .get(header::AUTHORIZATION)
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

// The server serves no page: a path the API does not serve is 404.
async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

// The origins the CORS layer admits: the page's origin, and when the page is a
// local dev server, its loopback twin (`localhost` and `127.0.0.1` on the same
// port name the same server, and a browser may reach it by either).
pub fn admitted_origins(gui_origin: &str) -> Vec<String> {
    let mut list = vec![gui_origin.to_string()];
    for (a, b) in [("://localhost", "://127.0.0.1"), ("://127.0.0.1", "://localhost")] {
        if gui_origin.contains(a) {
            list.push(gui_origin.replacen(a, b, 1));
        }
    }
    list
}

// Every response carries the allow header for the admitted origin (and no other),
// the methods and headers the app uses, `Access-Control-Allow-Private-Network` on
// the preflight, and `Cross-Origin-Resource-Policy: cross-origin`, since the page
// is cross-origin isolated and would otherwise refuse the response
// (docs/frontends/gui.md#serving).
fn cors(gui_origin: &str) -> CorsLayer {
    let origins: Vec<HeaderValue> = admitted_origins(gui_origin)
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .allow_private_network(true)
        .max_age(std::time::Duration::from_secs(600))
}

pub fn router(st: SharedState) -> Router {
    let api = Router::new()
        .route("/ping", get(api::ping))
        .route("/project", get(api::project))
        .route("/status", get(api::status))
        .route("/graph", get(api::graph))
        .route("/board", get(board_api::board))
        .route("/preview", get(board_api::preview))
        .route("/explain", get(board_api::explain))
        .route("/ripple", get(board_api::ripple))
        .route("/views", get(views_api::views))
        .route("/views/{id}", get(views_api::view))
        .route("/views/{id}/page", get(card_api::view_page_handler))
        .route("/tree", get(views_api::tree))
        .route("/facts/{id}/edit", post(facts::edit_fact))
        .route("/entities/{id}", get(api::entity))
        .route("/entities/{id}/card", get(card_api::entity_card_handler))
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
        .route("/questions", get(api::questions))
        .route("/questions/{id}/answer", post(api::answer_question))
        .route("/docs", get(api::docs))
        .route(
            "/docs/content",
            get(api::doc_content)
                .put(docs::doc_write)
                .delete(docs::doc_delete),
        )
        .route("/docs/baseline", get(api::doc_baseline))
        .route("/docs/rename", post(docs::doc_rename))
        .route("/gen/pending", get(api::gen_pending))
        .route("/gen/package/{id}", get(api::gen_package))
        .route("/verify/pending", get(api::verify_pending))
        .route("/verify/matrix", get(product::verify_matrix))
        .route("/capabilities", get(product::capabilities))
        .route("/projects", get(product::projects))
        .route("/inbox", get(product::inbox))
        .route("/docsgen/{slug}", get(api::docsgen))
        .route(
            "/chat/sessions",
            get(chat::list_sessions).post(chat::post_session),
        )
        .route("/chat/sessions/{id}", get(chat::get_session))
        .route("/chat/sessions/{id}/prompt", post(chat::post_prompt))
        .route("/chat/sessions/{id}/cancel", post(chat::cancel))
        .route("/chat/permissions", post(chat::answer_permission))
        .route("/jobs", get(jobs::list_jobs).post(jobs::post_job))
        .route("/jobs/{id}", get(jobs::get_job))
        .route("/jobs/{id}/cancel", post(jobs::cancel_job))
        .route("/deliverable", get(deliverable::listing))
        .route("/deliverable/file", get(deliverable::file))
        .route("/deliverable/baseline", get(deliverable::baseline))
        .route("/out/file", get(outfiles::file))
        .route("/out/list", get(outfiles::list))
        .route("/trace", get(jobs::list_traces))
        .route("/trace/{stem}", get(jobs::get_trace))
        .route("/trace/{stem}/{n}", get(jobs::get_trace_event))
        .route("/settings", get(api::settings_get).put(api::settings_put))
        .route("/watch", get(api::watch_get).put(api::watch_put))
        .route("/workers", get(api::workers))
        .route("/release", post(api::release))
        .route("/events", get(events::sse))
        .route("/shutdown", post(api::shutdown));
    let cors = cors(&st.gui_origin);
    // Layers wrap outward in the order added: the token check sits inside the
    // header layers, so a 401 and a 404 carry the CORS headers too and the page can
    // read them.
    Router::new()
        .nest("/api", api)
        .route("/lsp", get(lsp_ws::ws))
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(st.clone(), auth))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("cross-origin-resource-policy"),
            HeaderValue::from_static("cross-origin"),
        ))
        .layer(cors)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::state::AppState;
    use axum::body::Body;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn state(token: Option<&str>, gui_origin: &str) -> SharedState {
        let root = std::env::temp_dir().join(format!(
            "jazyk-gui-server-test-{}-{}",
            crate::sys::process_id(),
            gui_origin.replace(|c: char| !c.is_alphanumeric(), "_")
        ));
        std::fs::create_dir_all(&root).unwrap();
        let proj = crate::project::Project::load(&root);
        let out = proj.out.clone();
        let gs = crate::gen::GenSettings::resolve(&proj);
        Arc::new(AppState {
            root,
            proj: std::sync::RwLock::new(proj),
            llm: std::sync::RwLock::new(crate::llm::Llm {
                base_url: String::new(),
                model: String::new(),
                api_key: String::new(),
                temperature: None,
                trace: None,
            }),
            out,
            gs: std::sync::RwLock::new(gs),
            cli_opts: crate::cli::Options::default(),
            token: token.map(str::to_string),
            gui_origin: gui_origin.to_string(),
            dev: false,
            shutdown: tokio::sync::Notify::new(),
            events: events::EventHub::new(),
            last_pending: std::sync::Mutex::new(serde_json::Value::Null),
            jobs: jobs::JobManager::new(),
            chat: chat::ChatManager::default(),
            backoff: std::sync::atomic::AtomicU64::new(30),
        })
    }

    async fn send(st: SharedState, req: Request) -> Response {
        router(st).oneshot(req).await.unwrap()
    }

    fn get_from(path: &str, origin: &str) -> Request {
        Request::builder()
            .uri(path)
            .header("origin", origin)
            .body(Body::empty())
            .unwrap()
    }

    fn hdr<'a>(r: &'a Response, name: &str) -> Option<&'a str> {
        r.headers().get(name).and_then(|v| v.to_str().ok())
    }

    #[tokio::test]
    async fn the_page_origin_gets_the_allow_header_and_the_resource_policy() {
        let r = send(state(None, "https://jazyk.dev"), get_from("/api/ping", "https://jazyk.dev")).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(hdr(&r, "access-control-allow-origin"), Some("https://jazyk.dev"));
        assert_eq!(hdr(&r, "cross-origin-resource-policy"), Some("cross-origin"));
    }

    #[tokio::test]
    async fn a_foreign_origin_gets_no_allow_header() {
        let r = send(state(None, "https://jazyk.dev"), get_from("/api/ping", "https://other.example")).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(hdr(&r, "access-control-allow-origin"), None);
    }

    #[tokio::test]
    async fn a_dev_server_is_admitted_under_both_loopback_names() {
        assert_eq!(
            admitted_origins("http://localhost:5173"),
            vec!["http://localhost:5173".to_string(), "http://127.0.0.1:5173".to_string()]
        );
        assert_eq!(admitted_origins("https://jazyk.dev"), vec!["https://jazyk.dev".to_string()]);
        let r = send(state(None, "http://localhost:5173"), get_from("/api/ping", "http://127.0.0.1:5173")).await;
        assert_eq!(hdr(&r, "access-control-allow-origin"), Some("http://127.0.0.1:5173"));
        let r = send(state(None, "http://localhost:5173"), get_from("/api/ping", "https://jazyk.dev")).await;
        assert_eq!(hdr(&r, "access-control-allow-origin"), None);
    }

    #[tokio::test]
    async fn a_preflight_passes_without_the_token_and_names_what_the_app_uses() {
        let req = Request::builder()
            .method(Method::OPTIONS)
            .uri("/api/status")
            .header("origin", "https://jazyk.dev")
            .header("access-control-request-method", "PUT")
            .header("access-control-request-headers", "authorization, content-type")
            .header("access-control-request-private-network", "true")
            .body(Body::empty())
            .unwrap();
        let r = send(state(Some("secret"), "https://jazyk.dev"), req).await;
        assert!(r.status().is_success(), "{}", r.status());
        assert_eq!(hdr(&r, "access-control-allow-origin"), Some("https://jazyk.dev"));
        let methods = hdr(&r, "access-control-allow-methods").unwrap_or("").to_ascii_uppercase();
        for m in ["GET", "POST", "PUT", "DELETE", "OPTIONS"] {
            assert!(methods.contains(m), "{} not in {}", m, methods);
        }
        let headers = hdr(&r, "access-control-allow-headers").unwrap_or("").to_ascii_lowercase();
        assert!(headers.contains("authorization") && headers.contains("content-type"), "{}", headers);
        assert_eq!(hdr(&r, "access-control-allow-private-network"), Some("true"));
    }

    #[tokio::test]
    async fn the_token_still_guards_every_request_and_the_server_serves_no_page() {
        let st = state(Some("secret"), "https://jazyk.dev");
        let r = send(st.clone(), get_from("/api/status", "https://jazyk.dev")).await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        // The refusal is readable by the page: it carries the allow header too.
        assert_eq!(hdr(&r, "access-control-allow-origin"), Some("https://jazyk.dev"));
        let r = send(st.clone(), get_from("/api/ping", "https://jazyk.dev")).await;
        assert_eq!(r.status(), StatusCode::OK);
        for path in ["/", "/index.html", "/assets/app.js", "/docs/a.md"] {
            let r = send(st.clone(), Request::builder().uri(path).body(Body::empty()).unwrap()).await;
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{}", path);
        }
    }
}
