// Router assembly, session-token check, CORS for the page's origin, bind within the
// discovery range, graceful shutdown. The server serves the API alone; the page is at
// jazyk.dev (docs/frontends/gui.md#serving).
use super::state::SharedState;
use super::{
    agents, api, board_api, card_api, chat, deliverable, diff, docs, events, facts, jobs, lsp_ws,
    outfiles, product, projfiles, views_api,
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
    if guarded && !origin_admitted(&req, &st.gui_origin) {
        return (StatusCode::FORBIDDEN, "origin not admitted").into_response();
    }
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
    let ok = super::state::token_matches(bearer, expect)
        || super::state::token_matches(query_token.as_deref(), expect);
    if ok {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, "missing or invalid session token").into_response()
    }
}

// The allow header stops a page from reading an answer, not from sending a request:
// a browser sends a cross-origin POST without reading the reply. So a request other
// than GET, HEAD, or OPTIONS must name an admitted origin, or come from the same
// site or no browser at all. The `/lsp` upgrade is a GET, but a browser opens a
// WebSocket to any origin without CORS, so it passes the same check. Token or not
// (docs/frontends/gui.md#serving).
fn origin_admitted(req: &Request, gui_origin: &str) -> bool {
    let upgrade = req.uri().path() == "/lsp";
    if !upgrade && matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return true;
    }
    let text = |name: &str| {
        req.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    if let Some(origin) = text("origin") {
        return admitted_origins(gui_origin).contains(&origin);
    }
    match text("sec-fetch-site") {
        Some(site) => site == "same-origin" || site == "none",
        None => true,
    }
}

// The name a request gives the server must be a loopback name with the bound port:
// a page that rebinds its own domain to 127.0.0.1 sends that domain as `Host` and is
// refused before anything reads the project. `port` is None only where no listener
// is bound (in-process tests); the name is checked either way. A request without
// `Host` passes (docs/frontends/gui.md#serving).
pub(crate) fn host_admitted(host: Option<&str>, port: Option<u16>) -> bool {
    let Some(host) = host else { return true };
    let Ok(authority) = host.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    let name = authority.host().to_ascii_lowercase();
    if !matches!(name.as_str(), "127.0.0.1" | "localhost" | "[::1]") {
        return false;
    }
    match port {
        Some(bound) => authority.port_u16().unwrap_or(80) == bound,
        None => true,
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
    for (a, b) in [
        ("://localhost", "://127.0.0.1"),
        ("://127.0.0.1", "://localhost"),
    ] {
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
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static("x-jazyk-agent-selection"),
        ])
        .allow_private_network(true)
        .max_age(std::time::Duration::from_secs(600))
}

// The API routes, built once per process: a request only binds its scoped state.
fn api_router(st: SharedState) -> Router {
    static ROUTES: std::sync::OnceLock<Router<SharedState>> = std::sync::OnceLock::new();
    ROUTES.get_or_init(api_routes).clone().with_state(st)
}

fn api_routes() -> Router<SharedState> {
    let api = Router::new()
        .route("/agents", get(agents::list))
        .route("/agents/selections", post(agents::select))
        .route("/agents/setup", post(agents::setup))
        .route("/agents/cloud-challenge", post(agents::challenge))
        .route("/agents/bridge/{id}", get(agents::bridge_poll))
        .route("/agents/bridge/{id}/{request}", post(agents::bridge_reply))
        .route("/ping", get(api::ping))
        .route("/project", get(api::project))
        .route("/status", get(api::status))
        .route("/graph", get(api::graph))
        .route(
            "/board",
            get(board_api::board).post(board_api::selected_board),
        )
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
        .route("/find", get(api::find))
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
        .route(
            "/docs/document",
            get(docs::binary_read)
                .put(docs::binary_write)
                .layer(axum::extract::DefaultBodyLimit::max(45 * 1024 * 1024)),
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
        .route(
            "/chat/sessions/{id}",
            get(chat::get_session).delete(chat::delete_session),
        )
        .route("/chat/sessions/{id}/prompt", post(chat::post_prompt))
        .route("/chat/sessions/{id}/inputs", post(chat::post_input))
        .route("/chat/sessions/{id}/cancel", post(chat::cancel))
        .route("/chat/permissions", post(chat::answer_permission))
        .route("/jobs", get(jobs::list_jobs).post(jobs::post_job))
        .route("/jobs/{id}", get(jobs::get_job))
        .route("/jobs/{id}/cancel", post(jobs::cancel_job))
        .route("/project/files", get(projfiles::list))
        .route("/project/rename", post(projfiles::rename))
        .route(
            "/project/file",
            get(projfiles::file)
                .put(projfiles::create)
                .delete(projfiles::remove)
                .layer(axum::extract::DefaultBodyLimit::max(45 * 1024 * 1024)),
        )
        .route("/project/raw", get(projfiles::file))
        .route(
            "/project/text",
            get(projfiles::text).put(projfiles::write_text),
        )
        .route("/deliverable", get(deliverable::listing))
        .route("/deliverable/file", get(deliverable::file))
        .route("/deliverable/baseline", get(deliverable::baseline))
        .route("/deliverable/raw", get(deliverable::raw))
        .route("/product/overview", get(deliverable::overview))
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
    Router::new().nest("/api", api)
}

// Resolve the context once per request. The existing native handlers retain their
// streaming and binary transports; shared value functions serve aggregate reads.
async fn gateway(State(st): State<SharedState>, req: Request) -> Response {
    use serde_json::{json, Value};
    use tower::ServiceExt;
    let (mut parts, body) = req.into_parts();
    let limit = if parts.uri.path().ends_with("/docs/document")
        || parts.uri.path().ends_with("/project/file")
    {
        45 * 1024 * 1024
    } else {
        32 * 1024 * 1024
    };
    let bytes = match axum::body::to_bytes(body, limit).await {
        Ok(bytes) => bytes,
        Err(e) => return api::err(StatusCode::PAYLOAD_TOO_LARGE, e.to_string()),
    };
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(e) => return api::err(StatusCode::BAD_REQUEST, e.to_string()),
        }
    };
    let uri = parts.uri.to_string();
    let routed = match crate::webapi::sets::route(&uri, body) {
        Ok(r) => r,
        Err((code, e)) => return api::err(StatusCode::from_u16(code).unwrap(), e),
    };
    let route = routed.path.split('?').next().unwrap_or_default();
    let project_route = matches!(
        route,
        "/ping"
            | "/project"
            | "/project/files"
            | "/project/file"
            | "/project/raw"
            | "/project/text"
            | "/project/rename"
            | "/deliverable/raw"
            | "/docs/content"
            | "/docs/document"
            | "/docs/rename"
            | "/settings"
            | "/events"
            | "/shutdown"
            | "/capabilities"
            | "/projects"
            | "/benchmarks"
            | "/benchmarks/models"
            | "/jobs"
            | "/trace"
    ) || route == "/agents"
        || route.starts_with("/agents/")
        || route.starts_with("/chat/")
        || route.starts_with("/jobs/")
        || route.starts_with("/trace/");
    if route == "/tasks"
        || route == "/runs"
        || route.starts_with("/runs/")
        || route == "/sets"
        || route == "/sets/preview"
        || (parts.method == Method::GET
            && routed.selector.is_none()
            && crate::webapi::sets::aggregate_route(route))
    {
        let root = st.root.clone();
        let method = parts.method.to_string();
        let path = uri.clone();
        let body = routed.body.to_string();
        let result = tokio::task::spawn_blocking(move || {
            crate::webapi::dispatch::request_at(&root, &method, &path, &body)
        })
        .await;
        return match result {
            Ok(text) => {
                let v: Value = serde_json::from_str(&text).unwrap();
                if parts.method != Method::GET && v["status"].as_u64().unwrap_or(500) < 300 {
                    st.events.emit("sets.changed", json!({}));
                }
                (
                    StatusCode::from_u16(v["status"].as_u64().unwrap_or(500) as u16).unwrap(),
                    axum::Json(v["body"].clone()),
                )
                    .into_response()
            }
            Err(e) => api::err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
    }
    // The workspace and the scoped state both read disk (the configuration, the
    // global LLM config): resolve them off the runtime, once per request.
    let tolerant = matches!(
        route,
        "/ping" | "/settings" | "/project/files" | "/project/file" | "/project/raw"
    );
    let resolved = {
        let st = st.clone();
        let explicit = routed.selector.clone();
        tokio::task::spawn_blocking(move || {
            let workspace = crate::document_sets::Workspace::load(&st.root);
            let selector: Option<String> = if explicit.is_some() {
                explicit
            } else if project_route {
                None
            } else if let Ok(w) = &workspace {
                (w.sets.len() == 1).then(|| w.sets[0].id.clone())
            } else {
                None
            };
            if !project_route && selector.is_none() {
                return Err((
                    400u16,
                    "this operation requires setId or a qualified graph reference".to_string(),
                ));
            }
            let context = match &workspace {
                Ok(w) => st.scoped_in(w, selector.as_deref()),
                Err(e) => Err(e.clone()),
            };
            let context = match context {
                Ok(c) => c,
                Err(_) if tolerant => st.clone(),
                Err(e) => return Err(e),
            };
            Ok((workspace, selector, context))
        })
        .await
        .expect("request scope resolution")
    };
    let (workspace, selector, mut context) = match resolved {
        Ok(r) => r,
        Err((code, e)) => return api::err(StatusCode::from_u16(code).unwrap(), e),
    };
    if let Some(id) = parts
        .headers
        .get("x-jazyk-agent-selection")
        .filter(|_| !route.starts_with("/agents/"))
    {
        let selected = id.to_str().ok().and_then(|id| st.agent_choices.get(id));
        let Some(selected) = selected else {
            return api::err(
                StatusCode::CONFLICT,
                "Agent selection expired. Choose an agent again.",
            );
        };
        if let Some(context) = std::sync::Arc::get_mut(&mut context) {
            context.agent_selection = Some(selected);
        } else {
            return api::err(
                StatusCode::CONFLICT,
                "Project settings must be valid before choosing an agent.",
            );
        }
    }
    let selector = selector.as_deref();
    // The outer wildcard capture must not be appended to the inner route's captures.
    parts.extensions.clear();
    parts.uri = format!("/api{}", routed.path).parse().unwrap();
    parts.headers.remove(header::CONTENT_LENGTH);
    // A body no qualified reference changed travels as the bytes it came in, not
    // re-serialized for the inner router to parse again.
    let body = if routed.body.is_null() {
        axum::body::Body::empty()
    } else if !routed.body_changed {
        axum::body::Body::from(bytes)
    } else {
        axum::body::Body::from(routed.body.to_string())
    };
    let response = api_router(context.clone())
        .oneshot(Request::from_parts(parts, body))
        .await
        .unwrap();
    let is_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.starts_with("application/json"));
    if !is_json {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let limit = if route == "/docs/document" {
        128 * 1024 * 1024
    } else {
        32 * 1024 * 1024
    };
    let bytes = match axum::body::to_bytes(body, limit).await {
        Ok(b) => b,
        Err(e) => return api::err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    // Nothing to qualify or add: the reply travels as the inner router wrote it.
    if selector.is_none() && route != "/project" {
        return Response::from_parts(parts, axum::body::Body::from(bytes));
    }
    let mut value: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return Response::from_parts(parts, axum::body::Body::from(bytes)),
    };
    if route == "/project" {
        if let Ok(w) = workspace {
            // The summary loads every set's store and derives its board: off the
            // runtime like every other heavy read.
            let hash = w.hash.clone();
            let sets = tokio::task::spawn_blocking(move || w.summary()["sets"].take())
                .await
                .expect("workspace summary");
            value["sets"] = sets;
            value["configHash"] = json!(hash);
        }
    }
    if let Some(id) = selector {
        value = crate::webapi::sets::qualify_value(value, id);
    }
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, axum::body::Body::from(value.to_string()))
}

#[cfg(test)]
pub fn router(st: SharedState) -> Router {
    router_on(st, None)
}

// The router for a listener bound to `port`: the Host check runs first, before the
// token and the origin (docs/frontends/gui.md#serving).
pub fn router_on(st: SharedState, port: Option<u16>) -> Router {
    let cors = cors(&st.gui_origin);
    Router::new()
        .route("/api/{*path}", axum::routing::any(gateway))
        .route(
            "/model-bridge/{id}/chat/completions",
            post(agents::bridge_request),
        )
        .route("/lsp", get(lsp_ws::ws))
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(st.clone(), auth))
        .layer(middleware::from_fn(
            move |req: Request, next: Next| async move {
                let host = req
                    .headers()
                    .get(header::HOST)
                    .map(|v| v.to_str().unwrap_or("\u{0}"));
                if host_admitted(host, port) {
                    next.run(req).await
                } else {
                    (StatusCode::FORBIDDEN, "host not admitted").into_response()
                }
            },
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("cross-origin-resource-policy"),
            HeaderValue::from_static("cross-origin"),
        ))
        .layer(cors)
        .with_state(st)
}

pub const DEFAULT_PORT: u16 = 4680;
pub const DISCOVERY_PORT_END: u16 = 4711;

// Default launches stay discoverable even when earlier ports are busy. An
// explicit port is exact, including port zero's OS-assigned ephemeral behavior.
pub async fn bind(port: Option<u16>) -> Result<tokio::net::TcpListener, String> {
    bind_with(port, DEFAULT_PORT..=DISCOVERY_PORT_END, |addr| {
        tokio::net::TcpListener::bind(addr)
    })
    .await
}

// The binder seam keeps range and failure tests independent of machine ports.
async fn bind_with<T, F, Fut>(
    port: Option<u16>,
    default_ports: std::ops::RangeInclusive<u16>,
    mut binder: F,
) -> Result<T, String>
where
    F: FnMut(SocketAddr) -> Fut,
    Fut: std::future::Future<Output = std::io::Result<T>>,
{
    let ports = port.map_or_else(|| default_ports.clone(), |p| p..=p);
    for want in ports {
        let addr = SocketAddr::from(([127, 0, 0, 1], want));
        match binder(addr).await {
            Ok(listener) => return Ok(listener),
            Err(e) if port.is_none() && e.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(e) => return Err(format!("cannot bind {}: {}", addr, e)),
        }
    }
    Err(format!(
        "all local discovery ports 127.0.0.1:{}-{} are busy; stop an existing jazyk gui or choose --port N",
        default_ports.start(), default_ports.end()
    ))
}

// Serve until ctrl-c or /api/shutdown. No graceful drain: the event stream and the
// editor WebSocket stay open as long as a tab lives, so waiting on them would keep
// ctrl-c hanging until the browser closes. Dropping the server closes everything.
pub async fn serve(listener: tokio::net::TcpListener, st: SharedState) -> Result<(), String> {
    let port = listener.local_addr().ok().map(|a| a.port());
    let app = router_on(st.clone(), port);
    tokio::select! {
        r = axum::serve(listener, app) => r.map_err(|e| format!("server error: {}", e)),
        _ = tokio::signal::ctrl_c() => Ok(()),
        _ = st.shutdown.notified() => Ok(()),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::gui::state::AppState;
    use axum::body::Body;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[tokio::test]
    async fn default_binding_skips_occupied_prefix_and_stops_at_first_available_port() {
        let mut attempted = Vec::new();
        let selected = bind_with(None, 12000..=12003, |addr| {
            attempted.push(addr);
            std::future::ready(if addr.port() < 12002 {
                Err(std::io::Error::from(std::io::ErrorKind::AddrInUse))
            } else {
                Ok(addr)
            })
        })
        .await
        .unwrap();
        assert_eq!(selected, SocketAddr::from(([127, 0, 0, 1], 12002)));
        assert_eq!(
            attempted,
            (12000..=12002)
                .map(|port| SocketAddr::from(([127, 0, 0, 1], port)))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn default_binding_stops_on_errors_other_than_address_in_use() {
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::AddrNotAvailable,
        ] {
            let mut attempted = Vec::new();
            let error = bind_with(None, 12000..=12003, |addr| {
                attempted.push(addr.port());
                std::future::ready(Err::<(), _>(std::io::Error::new(kind, "bind refused")))
            })
            .await
            .unwrap_err();
            assert_eq!(attempted, vec![12000]);
            assert_eq!(error, "cannot bind 127.0.0.1:12000: bind refused");
        }
    }

    #[tokio::test]
    async fn default_binding_exhaustion_is_bounded_and_never_uses_an_ephemeral_port() {
        let mut attempted = Vec::new();
        let error = bind_with(None, DEFAULT_PORT..=DISCOVERY_PORT_END, |addr| {
            attempted.push(addr.port());
            std::future::ready(Err::<(), _>(std::io::Error::from(
                std::io::ErrorKind::AddrInUse,
            )))
        })
        .await
        .unwrap_err();
        assert_eq!(attempted, (4680..=4711).collect::<Vec<_>>());
        assert!(error.contains("127.0.0.1:4680-4711"));
        assert!(error.contains("--port N"));
    }

    #[tokio::test]
    async fn explicit_binding_never_retries_even_within_the_discovery_range() {
        let mut attempted = Vec::new();
        let error = bind_with(Some(12001), 12000..=12003, |addr| {
            attempted.push(addr.port());
            std::future::ready(Err::<(), _>(std::io::Error::from(
                std::io::ErrorKind::AddrInUse,
            )))
        })
        .await
        .unwrap_err();
        assert_eq!(attempted, vec![12001]);
        assert!(error.starts_with("cannot bind 127.0.0.1:12001:"));
    }

    #[tokio::test]
    async fn explicit_zero_binds_loopback_and_the_actual_busy_port_is_an_error() {
        let listener = bind(Some(0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        assert_eq!(addr.ip(), std::net::Ipv4Addr::LOCALHOST);
        assert_ne!(addr.port(), 0);
        let error = bind(Some(addr.port())).await.unwrap_err();
        assert!(
            error.starts_with(&format!("cannot bind {}:", addr)),
            "{}",
            error
        );
    }

    pub(crate) fn state(token: Option<&str>, gui_origin: &str) -> SharedState {
        // One root per call: tests sharing an origin run in parallel and each removes
        // its own root when done.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "jazyk-gui-server-test-{}-{}-{}",
            crate::sys::process_id(),
            gui_origin.replace(|c: char| !c.is_alphanumeric(), "_"),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let proj = crate::project::Project::load(&root);
        let out = proj.out.clone();
        let gs = crate::gen::GenSettings::resolve(&proj);
        Arc::new(AppState {
            agent_choices: Arc::new(super::agents::Choices::default()),
            agent_selection: None,
            root,
            proj: std::sync::RwLock::new(proj),
            llm: std::sync::RwLock::new(crate::llm::Llm {
                api: "chat_completions".into(),
                reasoning_summary: None,
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
            shutdown: Arc::new(tokio::sync::Notify::new()),
            events: events::EventHub::new(),
            last_pending: Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new())),
            jobs: Arc::new(jobs::JobManager::new()),
            chat: Arc::new(chat::ChatManager::default()),
            board: Arc::new(events::BoardCoalescer::default()),
            backoff: Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new())),
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
        let r = send(
            state(None, "https://jazyk.dev"),
            get_from("/api/ping", "https://jazyk.dev"),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            hdr(&r, "access-control-allow-origin"),
            Some("https://jazyk.dev")
        );
        assert_eq!(
            hdr(&r, "cross-origin-resource-policy"),
            Some("cross-origin")
        );
    }

    #[tokio::test]
    async fn a_foreign_origin_gets_no_allow_header() {
        let r = send(
            state(None, "https://jazyk.dev"),
            get_from("/api/ping", "https://other.example"),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(hdr(&r, "access-control-allow-origin"), None);
    }

    #[tokio::test]
    async fn a_dev_server_is_admitted_under_both_loopback_names() {
        assert_eq!(
            admitted_origins("http://localhost:5173"),
            vec![
                "http://localhost:5173".to_string(),
                "http://127.0.0.1:5173".to_string()
            ]
        );
        assert_eq!(
            admitted_origins("https://jazyk.dev"),
            vec!["https://jazyk.dev".to_string()]
        );
        let r = send(
            state(None, "http://localhost:5173"),
            get_from("/api/ping", "http://127.0.0.1:5173"),
        )
        .await;
        assert_eq!(
            hdr(&r, "access-control-allow-origin"),
            Some("http://127.0.0.1:5173")
        );
        let r = send(
            state(None, "http://localhost:5173"),
            get_from("/api/ping", "https://jazyk.dev"),
        )
        .await;
        assert_eq!(hdr(&r, "access-control-allow-origin"), None);
    }

    #[tokio::test]
    async fn a_preflight_passes_without_the_token_and_names_what_the_app_uses() {
        let req = Request::builder()
            .method(Method::OPTIONS)
            .uri("/api/status")
            .header("origin", "https://jazyk.dev")
            .header("access-control-request-method", "PUT")
            .header(
                "access-control-request-headers",
                "authorization, content-type",
            )
            .header("access-control-request-private-network", "true")
            .body(Body::empty())
            .unwrap();
        let r = send(state(Some("secret"), "https://jazyk.dev"), req).await;
        assert!(r.status().is_success(), "{}", r.status());
        assert_eq!(
            hdr(&r, "access-control-allow-origin"),
            Some("https://jazyk.dev")
        );
        let methods = hdr(&r, "access-control-allow-methods")
            .unwrap_or("")
            .to_ascii_uppercase();
        for m in ["GET", "POST", "PUT", "DELETE", "OPTIONS"] {
            assert!(methods.contains(m), "{} not in {}", m, methods);
        }
        let headers = hdr(&r, "access-control-allow-headers")
            .unwrap_or("")
            .to_ascii_lowercase();
        assert!(
            headers.contains("authorization") && headers.contains("content-type"),
            "{}",
            headers
        );
        assert_eq!(
            hdr(&r, "access-control-allow-private-network"),
            Some("true")
        );
    }

    #[tokio::test]
    async fn the_token_still_guards_every_request_and_the_server_serves_no_page() {
        let st = state(Some("secret"), "https://jazyk.dev");
        let r = send(st.clone(), get_from("/api/status", "https://jazyk.dev")).await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        // The refusal is readable by the page: it carries the allow header too.
        assert_eq!(
            hdr(&r, "access-control-allow-origin"),
            Some("https://jazyk.dev")
        );
        let r = send(st.clone(), get_from("/api/ping", "https://jazyk.dev")).await;
        assert_eq!(r.status(), StatusCode::OK);
        for path in ["/", "/index.html", "/assets/app.js", "/docs/a.md"] {
            let r = send(
                st.clone(),
                Request::builder().uri(path).body(Body::empty()).unwrap(),
            )
            .await;
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{}", path);
        }
    }

    // Discovery precedes the authenticated project reads. An independently
    // released editor can reject an incompatible engine without touching its
    // graph, and knowing the protocol does not bypass the session token.
    #[tokio::test]
    async fn protocol_discovery_is_read_only_and_capabilities_stay_guarded() {
        let st = state(Some("protocol-secret"), "https://protocol.test");
        assert!(!st.out.exists());

        let r = send(st.clone(), get_from("/api/ping", "https://protocol.test")).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            hdr(&r, "access-control-allow-origin"),
            Some("https://protocol.test")
        );
        let body = axum::body::to_bytes(r.into_body(), usize::MAX)
            .await
            .unwrap();
        let ping: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(ping["app"], "jazyk-gui");
        assert_eq!(ping["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(ping["protocol"], crate::protocol::EDITOR_PROTOCOL_VERSION);
        assert_eq!(ping["root"], st.root.to_string_lossy().as_ref());

        let r = send(
            st.clone(),
            get_from("/api/capabilities", "https://protocol.test"),
        )
        .await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        let req = Request::builder()
            .uri("/api/capabilities")
            .header("origin", "https://protocol.test")
            .header("authorization", "Bearer protocol-secret")
            .body(Body::empty())
            .unwrap();
        let r = send(st.clone(), req).await;
        assert_eq!(r.status(), StatusCode::OK);
        let body = axum::body::to_bytes(r.into_body(), usize::MAX)
            .await
            .unwrap();
        let caps: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(caps["protocol"], ping["protocol"]);
        assert_eq!(caps["backend"], "server");
        assert_eq!(caps["docs"], "write");
        assert_eq!(caps["lsp"], true);
        assert_eq!(caps["persistence"], "disk");
        assert!(caps["jobs"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("compile")));
        assert!(
            !st.out.exists(),
            "discovery must not create or migrate a graph"
        );
        std::fs::remove_dir_all(&st.root).ok();
    }

    // A built artifact is served as its bytes with its content type, a download link
    // authorized by the token in the query (docs/frontends/gui.md#api, deliverable).
    #[tokio::test]
    async fn a_product_file_is_served_raw_with_its_content_type() {
        let st = state(Some("secret"), "https://raw.test");
        crate::document_sets::edit(&st.root,&serde_json::json!({"baseHash":crate::document_sets::Workspace::load(&st.root).unwrap().hash,"set":{"name":"Deck","docs":["docs/**"],"source":["dist/**"]}}),false).unwrap();
        let deck = st.root.join("dist/deck.pptx");
        std::fs::create_dir_all(deck.parent().unwrap()).unwrap();
        let bytes: Vec<u8> = (0..=255u8).chain([0x50, 0x4b]).collect();
        std::fs::write(&deck, &bytes).unwrap();
        let r = send(
            st.clone(),
            get_from(
                "/api/deliverable/raw?path=dist/deck.pptx&token=secret",
                "https://raw.test",
            ),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            hdr(&r, "content-type"),
            Some("application/vnd.openxmlformats-officedocument.presentationml.presentation")
        );
        assert_eq!(hdr(&r, "content-security-policy"), Some("sandbox"));
        assert_eq!(hdr(&r, "x-content-type-options"), Some("nosniff"));
        let body = axum::body::to_bytes(r.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), bytes.as_slice());
        let r = send(
            st.clone(),
            get_from(
                "/api/deliverable/raw?path=../x&token=secret",
                "https://raw.test",
            ),
        )
        .await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let r = send(
            st.clone(),
            get_from("/api/product/overview?token=secret", "https://raw.test"),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        // Every file served as its bytes is sandboxed and typed exactly: an SVG with
        // a script opened in a tab runs nothing with the API's origin
        // (docs/frontends/gui.md#serving).
        std::fs::write(
            st.root.join("dist/pic.svg"),
            "<svg><script>alert(1)</script></svg>",
        )
        .unwrap();
        for path in [
            "/api/project/raw?path=dist/pic.svg&token=secret",
            "/api/project/file?path=dist/pic.svg&token=secret",
            "/api/deliverable/raw?path=dist/pic.svg&token=secret",
        ] {
            let r = send(st.clone(), get_from(path, "https://raw.test")).await;
            assert_eq!(r.status(), StatusCode::OK, "{}", path);
            assert_eq!(hdr(&r, "content-type"), Some("image/svg+xml"), "{}", path);
            assert_eq!(
                hdr(&r, "content-security-policy"),
                Some("sandbox"),
                "{}",
                path
            );
            assert_eq!(
                hdr(&r, "x-content-type-options"),
                Some("nosniff"),
                "{}",
                path
            );
        }
        let set = crate::document_sets::Workspace::load(&st.root)
            .unwrap()
            .sets[0]
            .id
            .clone();
        let diagrams = st.root.join(".jazyk").join(&set).join("diagrams");
        std::fs::create_dir_all(&diagrams).unwrap();
        std::fs::write(diagrams.join("v.svg"), "<svg/>").unwrap();
        let r = send(
            st.clone(),
            get_from(
                "/api/out/file?path=diagrams/v.svg&token=secret",
                "https://raw.test",
            ),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(hdr(&r, "content-security-policy"), Some("sandbox"));
        assert_eq!(hdr(&r, "x-content-type-options"), Some("nosniff"));
        std::fs::remove_dir_all(&st.root).ok();
    }
    // A page on another site can send a POST it never reads; the origin check
    // refuses it, with or without a token (docs/frontends/gui.md#serving).
    #[tokio::test]
    async fn a_write_from_a_foreign_origin_is_refused_even_without_a_token() {
        let post = |origin: Option<&str>, site: Option<&str>| {
            let mut b = Request::builder()
                .method(Method::POST)
                .uri("/api/jobs/1/cancel");
            if let Some(o) = origin {
                b = b.header("origin", o);
            }
            if let Some(s) = site {
                b = b.header("sec-fetch-site", s);
            }
            b.body(Body::empty()).unwrap()
        };
        let st = state(None, "https://jazyk.dev");
        let r = send(
            st.clone(),
            post(Some("https://evil.example"), Some("cross-site")),
        )
        .await;
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        let r = send(st.clone(), post(Some("null"), None)).await;
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        let r = send(st.clone(), post(None, Some("cross-site"))).await;
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        // The page, a same-site request, and a local tool all pass to the route.
        let r = send(
            st.clone(),
            post(Some("https://jazyk.dev"), Some("cross-site")),
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let r = send(st.clone(), post(None, Some("same-origin"))).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let r = send(st.clone(), post(None, Some("none"))).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let r = send(st.clone(), post(None, None)).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        // A read from a foreign origin still reaches the route: the allow header
        // keeps the browser from showing the answer.
        let r = send(st.clone(), get_from("/api/ping", "https://evil.example")).await;
        assert_eq!(r.status(), StatusCode::OK);
        // With a token the origin check comes first.
        let st = state(Some("secret"), "https://jazyk.dev");
        let r = send(st.clone(), post(Some("https://evil.example"), None)).await;
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        let r = send(st.clone(), post(None, None)).await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        std::fs::remove_dir_all(&st.root).ok();
    }

    // A page that rebinds its own domain to 127.0.0.1 names that domain as Host:
    // refused before the route, token or not; loopback names with the bound port
    // pass (docs/frontends/gui.md#serving).
    #[tokio::test]
    async fn a_request_naming_a_foreign_host_is_refused() {
        assert!(host_admitted(Some("127.0.0.1:4680"), Some(4680)));
        assert!(host_admitted(Some("localhost:4680"), Some(4680)));
        assert!(host_admitted(Some("LOCALHOST:4680"), Some(4680)));
        assert!(host_admitted(Some("[::1]:4680"), Some(4680)));
        assert!(host_admitted(None, Some(4680)));
        assert!(!host_admitted(Some("127.0.0.1:4681"), Some(4680)));
        assert!(!host_admitted(Some("127.0.0.1"), Some(4680)));
        assert!(!host_admitted(Some("evil.example:4680"), Some(4680)));
        assert!(!host_admitted(
            Some("127.0.0.1.evil.example:4680"),
            Some(4680)
        ));
        assert!(!host_admitted(Some("localhost@evil.example"), None));
        let st = state(None, "https://jazyk.dev");
        let get = |host: &str| {
            Request::builder()
                .uri("/api/ping")
                .header("host", host)
                .body(Body::empty())
                .unwrap()
        };
        let app = |st: SharedState| router_on(st, Some(4680));
        let r = app(st.clone())
            .oneshot(get("evil.example:4680"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        let r = app(st.clone())
            .oneshot(get("127.0.0.1:4680"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let r = app(st.clone())
            .oneshot(get("localhost:4680"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        std::fs::remove_dir_all(&st.root).ok();
    }

    // A browser opens a WebSocket to any origin without CORS: the `/lsp` upgrade
    // needs an admitted origin, or no browser at all, with or without a token
    // (docs/frontends/gui.md#serving).
    #[tokio::test]
    async fn the_lsp_upgrade_from_a_foreign_origin_is_refused_even_without_a_token() {
        let upgrade = |origin: Option<&str>| {
            let mut b = Request::builder().uri("/lsp");
            if let Some(o) = origin {
                b = b.header("origin", o);
            }
            b.body(Body::empty()).unwrap()
        };
        for token in [None, Some("secret")] {
            let st = state(token, "https://jazyk.dev");
            let r = send(st.clone(), upgrade(Some("https://evil.example"))).await;
            assert_eq!(r.status(), StatusCode::FORBIDDEN);
            let r = send(st.clone(), upgrade(Some("null"))).await;
            assert_eq!(r.status(), StatusCode::FORBIDDEN);
        }
        // The page and a local tool reach the route (a plain GET is no upgrade, so
        // the WebSocket extractor refuses it past the origin check).
        let st = state(None, "https://jazyk.dev");
        for origin in [Some("https://jazyk.dev"), None] {
            let r = send(st.clone(), upgrade(origin)).await;
            assert_ne!(r.status(), StatusCode::FORBIDDEN, "{:?}", origin);
        }
        std::fs::remove_dir_all(&st.root).ok();
    }

    // A burst of causes derives the board once and emits one event; a cause landing
    // after the derivation started derives again (docs/frontends/gui.md#events).
    #[tokio::test]
    async fn board_derivations_coalesce_per_set() {
        let st = state(None, "https://coalesce.test");
        let mut rx = st.events.subscribe();
        for _ in 0..5 {
            events::emit_board_changed(&st);
        }
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        let mut seen = 0;
        while let Ok(ev) = rx.try_recv() {
            if ev["type"] == "board.changed" {
                seen += 1;
                assert!(ev["counts"].is_object(), "{}", ev);
                assert!(ev["verdict"].is_string(), "{}", ev);
            }
        }
        assert_eq!(seen, 1);
        events::emit_board_changed(&st);
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        let mut seen = 0;
        while let Ok(ev) = rx.try_recv() {
            if ev["type"] == "board.changed" {
                seen += 1;
            }
        }
        assert_eq!(seen, 1);
        std::fs::remove_dir_all(&st.root).ok();
    }

    #[tokio::test]
    async fn all_sets_are_visible_and_colliding_ids_route_to_their_own_store() {
        use serde_json::{json, Value};
        let st = state(None, "https://sets.test");
        for (id, name) in [("ds_one", "Orders"), ("ds_two", "Billing")] {
            let w = crate::document_sets::Workspace::load(&st.root).unwrap();
            crate::document_sets::edit(&st.root,&json!({"baseHash":w.hash,"set":{"id":id,"name":name,"docs":[format!("{}.md",id)]}}),false).unwrap();
            let mut store = crate::store::Store::load(&st.root.join(".jazyk").join(id));
            store.graph.entities.insert(
                "ent:shared".into(),
                crate::model::Entity {
                    name: name.into(),
                    ..Default::default()
                },
            );
            store.graph.diagnostics.insert("diag:shared".into(),serde_json::from_value(json!({"rule":"example","severity":"warning","lifecycle":"open","subjects":[],"message":"Example finding"})).unwrap());
            store.save_for_test();
        }
        let response = send(st.clone(), get_from("/api/graph", "https://sets.test")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let v: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(v["entities"]["ent:ds_one::shared"]["name"], "Orders");
        assert_eq!(v["entities"]["ent:ds_two::shared"]["name"], "Billing");
        let response = send(
            st.clone(),
            get_from(
                "/api/entities/ent%3Ads_two%3A%3Ashared",
                "https://sets.test",
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let request = Request::builder()
            .method("POST")
            .uri("/api/diagnostics/diag%3Ads_two%3A%3Ashared/triage")
            .header("content-type", "application/json")
            .body(Body::from(json!({"triage":"suppressed"}).to_string()))
            .unwrap();
        assert_eq!(send(st.clone(), request).await.status(), StatusCode::OK);
        let one = crate::store::Store::load(&st.root.join(".jazyk/ds_one"));
        let two = crate::store::Store::load(&st.root.join(".jazyk/ds_two"));
        assert!(one.graph.diagnostics["diag:shared"].triage.is_none());
        assert_eq!(
            two.graph.diagnostics["diag:shared"].triage.as_deref(),
            Some("suppressed")
        );
        let request = Request::builder()
            .method("POST")
            .uri("/api/diagnostics/diag%3Ads_two%3A%3Ashared/triage")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"setId":"ds_one","triage":"suppressed"}).to_string(),
            ))
            .unwrap();
        assert_eq!(
            send(st.clone(), request).await.status(),
            StatusCode::BAD_REQUEST
        );
        std::fs::remove_dir_all(&st.root).unwrap();
    }
    #[test]
    fn stop_clears_followups_and_blocks_automatic_jobs_until_explicit_build() {
        let st = state(None, "https://stop-jobs.test");
        let first = st.jobs.submit(&st, jobs::JobKind::Compile);
        let followup = st.jobs.submit(
            &st,
            jobs::JobKind::Gen {
                entities: vec![],
                force: false,
            },
        );
        let stopped = st.jobs.cancel(&st, first).unwrap();
        assert_eq!(stopped["state"], "cancelled");
        assert_eq!(st.jobs.get(followup).unwrap()["state"], "cancelled");
        assert!(st.jobs.submit_auto(&st, jobs::JobKind::Compile).is_none());
        let next = st.jobs.submit(&st, jobs::JobKind::Compile);
        assert_eq!(st.jobs.get(next).unwrap()["state"], "queued");
        assert!(st.jobs.submit_auto(&st, jobs::JobKind::Compile).is_some());
        std::fs::remove_dir_all(&st.root).unwrap();
    }
}
