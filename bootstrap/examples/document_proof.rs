//! Fixture-only API for the binary format evaluation, not a production server.
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use jazyk::{documents, lsp::Lsp, project::Project};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
struct App {
    root: PathBuf,
    saves: Arc<Mutex<()>>,
}
type Refusal = (StatusCode, String);

fn file(app: &App, name: &str) -> Result<PathBuf, Refusal> {
    if !["eigenpal.docx", "docxodus.docx", "sample.pdf"].contains(&name) {
        return Err((StatusCode::NOT_FOUND, "unknown proof fixture".into()));
    }
    let path = app.root.join(name);
    let m = std::fs::symlink_metadata(&path).map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err((
            StatusCode::BAD_REQUEST,
            "fixture must be a regular file".into(),
        ));
    }
    Ok(path)
}

async fn bytes(State(app): State<App>, Path(name): Path<String>) -> Result<Response, Refusal> {
    let p = file(&app, &name)?;
    let b = std::fs::read(p).map_err(internal)?;
    let ct = if name.ends_with("pdf") {
        "application/pdf"
    } else {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    };
    Ok(([("content-type", ct), ("cache-control", "no-store")], b).into_response())
}

fn internal(e: impl ToString) -> Refusal {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn utf16_byte(line: &str, units: usize) -> Option<usize> {
    let mut n = 0;
    for (i, c) in line.char_indices() {
        if n == units {
            return Some(i);
        }
        n += c.len_utf16();
    }
    (n == units).then_some(line.len())
}

fn position(text: &str, p: &Value) -> Option<usize> {
    let line = p["line"].as_u64()? as usize;
    let col = p["character"].as_u64()? as usize;
    let mut base = 0;
    for (i, s) in text.split('\n').enumerate() {
        if i == line {
            return Some(base + utf16_byte(s, col)?);
        }
        base += s.len() + 1;
    }
    None
}

async fn projection(
    State(app): State<App>,
    Path(name): Path<String>,
) -> Result<Json<Value>, Refusal> {
    let path = file(&app, &name)?;
    let p = documents::read(&path).map_err(internal)?;
    let proj = Project::load(&app.root);
    let gen = jazyk::gen::GenSettings::resolve(&proj);
    let lsp = Lsp::new(proj.root.clone(), proj.root.join(".jazyk"), gen);
    let mut diagnostics = lsp.document_diagnostics(&name);
    for d in &mut diagnostics {
        let range = &d["range"];
        let start = position(&p.text, &range["start"]);
        let end = position(&p.text, &range["end"]);
        let quote = start
            .zip(end)
            .and_then(|(s, e)| p.text.get(s..e))
            .unwrap_or("");
        d["quote"] = json!(quote);
        d["revision"] = json!(p.revision);
        d["locations"] = if !quote.is_empty() && p.text.match_indices(quote).count() == 1 {
            if p.format == "pdf" {
                json!(documents::pdf_quote(&path, quote).map_err(internal)?)
            } else {
                json!(p
                    .blocks
                    .iter()
                    .filter(|b| b.text.contains(quote))
                    .map(|b| &b.location)
                    .collect::<Vec<_>>())
            }
        } else {
            json!([])
        };
    }
    Ok(Json(json!({"projection": p, "diagnostics": diagnostics})))
}

async fn save(
    State(app): State<App>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, Refusal> {
    let path = file(&app, &name)?;
    if !name.ends_with(".docx") {
        return Err((StatusCode::METHOD_NOT_ALLOWED, "PDF is read-only".into()));
    }
    let _guard = app.saves.lock().map_err(internal)?;
    let expected = headers
        .get("if-match")
        .and_then(|v| v.to_str().ok())
        .ok_or((
            StatusCode::PRECONDITION_REQUIRED,
            "If-Match revision is required".into(),
        ))?;
    let before = std::fs::read(&path).map_err(internal)?;
    if documents::revision(&before) != expected {
        return Err((
            StatusCode::CONFLICT,
            "file changed; reopen before saving".into(),
        ));
    }
    let p =
        documents::from_bytes(&path, &body).map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e))?;
    // Validate before creating the replacement. A failed serializer never clobbers input.
    let temp = app.root.join(format!(".{name}.proof-save"));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(internal)?;
    use std::io::Write;
    let result = f
        .write_all(&body)
        .and_then(|_| f.sync_all())
        .and_then(|_| std::fs::rename(&temp, &path));
    if let Err(e) = result {
        std::fs::remove_file(temp).ok();
        return Err(internal(e));
    }
    Ok(Json(json!({"revision":p.revision})))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let root = PathBuf::from(
        args.get(1)
            .ok_or("usage: document_proof FIXTURE_ROOT [PORT]")?,
    )
    .canonicalize()?;
    let port: u16 = args.get(2).map(|s| s.parse()).transpose()?.unwrap_or(4328);
    let app = Router::new()
        .route("/file/{name}", get(bytes).put(save))
        .route("/projection/{name}", get(projection))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(App {
            root,
            saves: Arc::new(Mutex::new(())),
        });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    eprintln!("Document proof API: http://127.0.0.1:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
