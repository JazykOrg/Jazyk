// The MCP serving over HTTP: the same `McpServer` the stdio loop drives, reached
// through the MCP streamable HTTP transport by an agent that runs its MCP clients
// against URLs only (Claude Code through claude-code-acp). One server per session on
// a loopback port behind a per-session bearer token; every request dispatches through
// `McpServer::dispatch`, so the toolsets and the batch lifecycle are the same code.
// The async runtime is scoped to the serving thread, as the GUI scopes its own.
// Mirrors docs/frontends/mcp.md#mcp-over-http.
use crate::mcp::{BridgeFlags, McpServer};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// The endpoint path the URL names. One path, one session.
const PATH: &str = "/mcp";

// How long the listener may take to drain after the stop signal before the thread
// gives up waiting on lingering keep-alive connections.
const DRAIN: std::time::Duration = std::time::Duration::from_secs(5);

struct Shared {
    // Dispatch is serialized, as it is on stdio: the batch's staged state is one
    // thing and calls land on it one at a time.
    server: Mutex<McpServer>,
    token: String,
}

pub struct HttpServing {
    pub url: String,
    pub token: String,
    pub port: u16,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl HttpServing {
    // Bind a loopback port, mint the token, and serve `McpServer::with_bridge` for the
    // given toolsets and flags until `stop`. Binding happens on the caller's thread so
    // a failure is the caller's error, not a dead thread.
    pub fn start(
        project: crate::project::Project,
        out: PathBuf,
        modes: Vec<String>,
        flags: BridgeFlags,
    ) -> Result<HttpServing, String> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("mcp http serving: cannot bind 127.0.0.1: {}", e))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("mcp http serving: listener: {}", e))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("mcp http serving: listener: {}", e))?
            .port();
        let token = mint_token()?;
        let url = format!("http://127.0.0.1:{}{}", port, PATH);
        let server = McpServer::with_bridge(project, out, modes, false, flags);
        let shared = Arc::new(Shared {
            server: Mutex::new(server),
            token: token.clone(),
        });
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::Builder::new()
            .name(format!("mcp-http-{}", port))
            .spawn({
                let shared = shared.clone();
                move || serve_thread(listener, shared, stop_rx)
            })
            .map_err(|e| format!("mcp http serving: cannot spawn thread: {}", e))?;
        Ok(HttpServing {
            url,
            token,
            port,
            stop: Some(stop_tx),
            thread: Some(thread),
        })
    }

    // The header the session entry carries.
    pub fn header(&self) -> (String, String) {
        ("Authorization".to_string(), format!("Bearer {}", self.token))
    }

    // Stop listening and run the serving's end (the implicit finish for an ephemeral
    // serving, the transcript's close). Blocks until it has.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for HttpServing {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// The per-session bearer token: 128 bits from the OS entropy source, as hex.
fn mint_token() -> Result<String, String> {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).map_err(|e| format!("mcp http serving: entropy: {}", e))?;
    Ok(buf.iter().map(|b| format!("{:02x}", b)).collect())
}

fn serve_thread(
    listener: std::net::TcpListener,
    shared: Arc<Shared>,
    stop_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[acp] mcp http serving: cannot start runtime: {}", e);
            return;
        }
    };
    let state = shared.clone();
    rt.block_on(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[acp] mcp http serving: listener: {}", e);
                return;
            }
        };
        let app = Router::new()
            .route(
                PATH,
                post(post_mcp)
                    .get(|| async { StatusCode::METHOD_NOT_ALLOWED })
                    .delete(|| async { StatusCode::METHOD_NOT_ALLOWED }),
            )
            .with_state(state);
        // The stop signal ends accepting and starts the drain. A client that keeps a
        // connection alive past it must not hold the session's end hostage: the drain
        // is bounded, then the listener drops with the runtime. The bound starts at
        // the stop signal, never at the start of serving.
        let (drain_tx, drain_rx) = tokio::sync::oneshot::channel::<()>();
        let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = stop_rx.await;
            let _ = drain_tx.send(());
        });
        tokio::select! {
            _ = serve => {}
            _ = async {
                let _ = drain_rx.await;
                tokio::time::sleep(DRAIN).await;
            } => {}
        }
    });
    rt.shutdown_timeout(std::time::Duration::from_secs(1));
    // Every handler is gone with the runtime; the lock is uncontended.
    match shared.server.lock() {
        Ok(s) => s.finish(),
        Err(p) => p.into_inner().finish(),
    }
}

// POST: one JSON-RPC message (or a batch array), the token checked first.
async fn post_mcp(State(st): State<Arc<Shared>>, headers: HeaderMap, body: String) -> Response {
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| v.trim().to_string());
    if presented.as_deref() != Some(st.token.as_str()) {
        let st2 = st.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(s) = st2.server.lock() {
                s.note("http: refused a call without the session token");
            }
        })
        .await;
        return (StatusCode::UNAUTHORIZED, "session token required").into_response();
    }
    let req: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("invalid JSON-RPC body: {}", e),
            )
                .into_response();
        }
    };
    let out = tokio::task::spawn_blocking(move || dispatch(&st.server, &req)).await;
    match out {
        Ok(Some(v)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            v.to_string(),
        )
            .into_response(),
        Ok(None) => StatusCode::ACCEPTED.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("dispatch failed: {}", e),
        )
            .into_response(),
    }
}

// The transport-neutral core, shared with the tests: a single message answers with
// its response (None for a notification); a batch answers with the array of responses
// its requests produced (None when it held notifications only).
pub fn dispatch(server: &Mutex<McpServer>, req: &Value) -> Option<Value> {
    let s = match server.lock() {
        Ok(s) => s,
        Err(p) => p.into_inner(),
    };
    match req {
        Value::Array(items) => {
            let replies: Vec<Value> = items.iter().filter_map(|r| s.dispatch(r)).collect();
            if replies.is_empty() {
                None
            } else {
                Some(Value::Array(replies))
            }
        }
        other => s.dispatch(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // A fixture project with a tiny store: one document, one entity, so a read tool
    // has something to answer with.
    fn fixture() -> (crate::project::Project, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "jazyk-mcp-http-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(
            dir.join("jazyk.toml"),
            "[docs]\nglob = [\"docs/**/*.md\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/main.md"),
            "# Main\n\nThe store keeps every order.\n",
        )
        .unwrap();
        let proj = crate::project::Project::load(&dir);
        let out = proj.out.clone();
        // A graph with one entity, committed, so the read tools have a store to answer
        // from rather than a no-build refusal.
        let (parsed, _) = crate::reconcile::parse_all(&proj);
        let mut s = crate::store::Store::load(&out);
        s.sync_docs(&parsed);
        s.apply(
            vec![crate::store::Op::CreateEntity {
                id: "ent:store".into(),
                entity: crate::model::Entity {
                    name: "Store".into(),
                    ..Default::default()
                },
            }],
            &crate::store::Commit::store("session"),
        );
        drop(s);
        (proj, out)
    }

    fn started() -> (HttpServing, PathBuf) {
        let (proj, out) = fixture();
        let root = proj.root.clone();
        let serving = HttpServing::start(proj, out, vec!["graph".into()], BridgeFlags::default())
            .expect("serving starts");
        (serving, root)
    }

    fn post(serving: &HttpServing, token: Option<&str>, body: &Value) -> (u16, Value) {
        let mut req = ureq::post(&serving.url).set("Content-Type", "application/json");
        if let Some(t) = token {
            req = req.set("Authorization", &format!("Bearer {}", t));
        }
        match req.send_string(&body.to_string()) {
            Ok(r) => {
                let status = r.status();
                let text = r.into_string().unwrap_or_default();
                (
                    status,
                    serde_json::from_str(&text).unwrap_or(Value::String(text)),
                )
            }
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                (code, Value::String(text))
            }
            Err(e) => panic!("request failed: {}", e),
        }
    }

    // The handler answers initialize, tools/list, and a read tool call, the same
    // replies the stdio serving gives. Mirrors docs/frontends/mcp.md#mcp-over-http.
    #[test]
    fn http_serving_answers_the_mcp_handshake_and_a_read_tool() {
        let (serving, root) = started();
        let token = serving.token.clone();
        let (status, init) = post(
            &serving,
            Some(&token),
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-03-26", "clientInfo": {"name": "test"}}}),
        );
        assert_eq!(status, 200);
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "jazyk");

        // A notification answers 202 with no body.
        let (status, _) = post(
            &serving,
            Some(&token),
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        );
        assert_eq!(status, 202);

        let (status, list) = post(
            &serving,
            Some(&token),
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
        );
        assert_eq!(status, 200);
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(names.contains(&"graph_status"), "{:?}", names);
        assert!(names.contains(&"search"), "{:?}", names);

        let (status, call) = post(
            &serving,
            Some(&token),
            &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "graph_status", "arguments": {}}}),
        );
        assert_eq!(status, 200);
        assert_eq!(call["id"], 3);
        assert!(call["result"]["content"][0]["text"].is_string(), "{}", call);
        assert_ne!(call["result"]["isError"], true, "{}", call);

        // GET is not a stream here.
        let r = ureq::get(&serving.url)
            .set("Authorization", &format!("Bearer {}", token))
            .call();
        assert!(matches!(r, Err(ureq::Error::Status(405, _))));

        serving.stop();
        std::fs::remove_dir_all(&root).ok();
    }

    // The token gate refuses a call without the token and one with another token,
    // before the handler sees it.
    #[test]
    fn http_serving_refuses_a_call_without_the_session_token() {
        let (serving, root) = started();
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}});
        let (status, _) = post(&serving, None, &body);
        assert_eq!(status, 401);
        let (status, _) = post(&serving, Some("not-the-token"), &body);
        assert_eq!(status, 401);
        let (status, ok) = post(&serving, Some(&serving.token.clone()), &body);
        assert_eq!(status, 200);
        assert!(ok["result"]["tools"].is_array());
        serving.stop();
        std::fs::remove_dir_all(&root).ok();
    }

    // The serving outlives the drain bound while nothing stopped it (a session runs
    // for minutes), and stopping closes the port: a call afterwards fails to connect.
    #[test]
    fn http_serving_stops_with_the_session() {
        let (serving, root) = started();
        let url = serving.url.clone();
        std::thread::sleep(DRAIN + std::time::Duration::from_secs(1));
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": "ping", "params": {}});
        let (status, _) = post(&serving, Some(&serving.token.clone()), &body);
        assert_eq!(status, 200, "still serving past the drain bound");
        serving.stop();
        let r = ureq::post(&url).send_string("{}");
        assert!(matches!(r, Err(ureq::Error::Transport(_))), "{:?}", r);
        std::fs::remove_dir_all(&root).ok();
    }
}
