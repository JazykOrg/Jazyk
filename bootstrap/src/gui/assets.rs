// Frontend asset serving: the built SPA embedded at compile time, with a disk
// override for frontend development. Unknown non-API paths fall back to index.html
// so app routes are addressable URLs.
use super::state::SharedState;
use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use include_dir::{include_dir, Dir};

static DIST: Dir = include_dir!("$CARGO_MANIFEST_DIR/gui/dist");

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn load(st: &SharedState, path: &str) -> Option<Vec<u8>> {
    match &st.dist_dir {
        Some(dir) => {
            // Dev override. The path is normalized below; reject any residual traversal.
            if path.split('/').any(|c| c == "..") {
                return None;
            }
            std::fs::read(dir.join(path)).ok()
        }
        None => DIST.get_file(path).map(|f| f.contents().to_vec()),
    }
}

pub async fn serve(State(st): State<SharedState>, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let (path, body) = match load(&st, path) {
        Some(b) => (path.to_string(), b),
        // SPA fallback: anything outside the built asset tree is an app route (app
        // routes carry document paths like /docs/a.md, so extensions are no signal).
        None if !path.starts_with("assets/") => match load(&st, "index.html") {
            Some(b) => ("index.html".to_string(), b),
            None => return (StatusCode::NOT_FOUND, "not found").into_response(),
        },
        None => return (StatusCode::NOT_FOUND, "not found").into_response(),
    };
    ([(header::CONTENT_TYPE, mime(&path))], body).into_response()
}
