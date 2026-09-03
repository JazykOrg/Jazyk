// Read-only access to the out directory: the generated pages the markdown preview
// loads and the renderings it embeds as images. No write verb exists here; the out
// directory is build output. Mirrors docs/frontends/gui.md#api (the out directory).
use super::api::err;
use super::state::SharedState;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

// The lexical half of the validation: relative, forward slashes, no empty, dot,
// dot-dot, or hidden component.
fn lexical_ok(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == ".." || c.starts_with('.'))
}

// Resolve a client path strictly inside the out directory: relative only, no
// traversal, no hidden component, and the canonical target inside the canonical
// out directory (a symlink pointing out is refused). None when anything fails,
// including a path that does not exist.
pub fn safe_out_path(out: &Path, rel: &str) -> Option<PathBuf> {
    if !lexical_ok(rel) {
        return None;
    }
    let root = out.canonicalize().ok()?;
    let canon = out.join(rel).canonicalize().ok()?;
    if !canon.starts_with(&root) {
        return None;
    }
    Some(canon)
}

// The content type by extension. Text kinds carry a charset so the browser and the
// app read them as UTF-8; anything unknown is opaque bytes.
pub fn content_type(rel: &str) -> &'static str {
    let ext = rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "md" => "text/markdown; charset=utf-8",
        "puml" | "yaml" | "yml" | "txt" | "json" | "jsonl" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[derive(Deserialize)]
pub struct OutQ {
    path: String,
}

// One file under the out directory, as bytes with its content type.
pub async fn file(State(st): State<SharedState>, Query(p): Query<OutQ>) -> Response {
    let out = st.out.clone();
    let path = p.path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, (StatusCode, String)> {
        // A path that leaves the out directory (lexically, or through a symlink that
        // exists) is 400; a path that validates but names nothing is 404.
        let Some(abs) = safe_out_path(&out, &path) else {
            if !lexical_ok(&path) || out.join(&path).exists() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("invalid out path {}", path),
                ));
            }
            return Err((StatusCode::NOT_FOUND, format!("no file {}", path)));
        };
        if abs.is_dir() {
            return Err((StatusCode::NOT_FOUND, format!("{} is a directory", path)));
        }
        std::fs::read(&abs).map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                format!("cannot read {}: {}", path, e),
            )
        })
    })
    .await
    .expect("out file read");
    match result {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type(&p.path)),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            bytes,
        )
            .into_response(),
        Err((code, msg)) => err(code, msg),
    }
}

fn walk(dir: &Path, base: &Path, acc: &mut Vec<(String, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            walk(&p, base, acc);
        } else if let Ok(rel) = p.strip_prefix(base) {
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            acc.push((rel.to_string_lossy().replace('\\', "/"), size));
        }
    }
}

// The files under one directory of the out directory, recursively, paths relative
// to the out directory. An absent directory lists as empty: the pages simply have
// not been generated yet.
pub async fn list(State(st): State<SharedState>, Query(p): Query<OutQ>) -> Response {
    let out = st.out.clone();
    let path = p.path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Vec<(String, u64)>, String> {
        let root = out
            .canonicalize()
            .map_err(|_| "no out directory".to_string())?;
        let mut files = Vec::new();
        match safe_out_path(&out, &path) {
            Some(dir) if dir.is_dir() => walk(&dir, &root, &mut files),
            Some(_) => return Err(format!("{} is not a directory", path)),
            None => {
                if !lexical_ok(&path) || out.join(&path).exists() {
                    return Err(format!("invalid out path {}", path));
                }
            }
        }
        files.sort();
        Ok(files)
    })
    .await
    .expect("out listing");
    match result {
        Ok(files) => Json(json!({
            "files": files
                .into_iter()
                .map(|(path, size)| json!({ "path": path, "size": size }))
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(msg) => err(StatusCode::BAD_REQUEST, msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_out(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jazyk-gui-out-{}-{}", name, std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("jazyk-out/docsgen/entities")).unwrap();
        std::fs::create_dir_all(dir.join("jazyk-out/diagrams/class")).unwrap();
        std::fs::write(dir.join("jazyk-out/docsgen/entities/funds.md"), "# Funds\n").unwrap();
        std::fs::write(dir.join("jazyk-out/diagrams/class/funds.svg"), "<svg/>").unwrap();
        std::fs::write(dir.join("jazyk-out/.hidden.md"), "x").unwrap();
        std::fs::write(dir.join("secret.md"), "outside\n").unwrap();
        dir
    }

    // Path confinement: a `..` that leaves the out directory is refused, and so are
    // absolute paths, hidden components, backslashes, and missing files.
    #[test]
    fn out_path_is_confined_to_the_out_directory() {
        let dir = temp_out("confine");
        let out = dir.join("jazyk-out");
        assert!(safe_out_path(&out, "docsgen/entities/funds.md").is_some());
        assert!(safe_out_path(&out, "diagrams/class/funds.svg").is_some());
        assert!(safe_out_path(&out, "docsgen").is_some());
        assert!(safe_out_path(&out, "../secret.md").is_none());
        assert!(safe_out_path(&out, "docsgen/../../secret.md").is_none());
        assert!(safe_out_path(&out, "docsgen/./entities/funds.md").is_none());
        assert!(safe_out_path(&out, "/etc/passwd").is_none());
        assert!(safe_out_path(&out, "docsgen\\entities\\funds.md").is_none());
        assert!(safe_out_path(&out, ".hidden.md").is_none());
        assert!(safe_out_path(&out, "").is_none());
        assert!(safe_out_path(&out, "docsgen/entities/missing.md").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_the_out_directory_is_refused() {
        let dir = temp_out("symlink");
        let out = dir.join("jazyk-out");
        std::os::unix::fs::symlink(dir.join("secret.md"), out.join("linked.md")).unwrap();
        std::os::unix::fs::symlink(&dir, out.join("up")).unwrap();
        assert!(safe_out_path(&out, "linked.md").is_none());
        assert!(safe_out_path(&out, "up/secret.md").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn content_types_follow_the_extension() {
        assert_eq!(content_type("diagrams/class/a.svg"), "image/svg+xml");
        assert_eq!(content_type("diagrams/class/a.PNG"), "image/png");
        assert_eq!(content_type("docsgen/entities/a.md"), "text/markdown; charset=utf-8");
        assert_eq!(content_type("diagrams/class/a.puml"), "text/plain; charset=utf-8");
        assert_eq!(content_type("status.yaml"), "text/plain; charset=utf-8");
        assert_eq!(content_type("graph/blob"), "application/octet-stream");
    }

    // The listing walks one directory recursively, skips hidden entries, and lists
    // an absent directory as empty.
    #[test]
    fn listing_walks_a_directory_and_skips_hidden() {
        let dir = temp_out("list");
        let out = dir.join("jazyk-out");
        let root = out.canonicalize().unwrap();
        let mut files = Vec::new();
        walk(&safe_out_path(&out, "docsgen").unwrap(), &root, &mut files);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "docsgen/entities/funds.md");
        let mut all = Vec::new();
        walk(&root, &root, &mut all);
        assert!(all.iter().all(|(p, _)| !p.contains("/.") && !p.starts_with('.')));
        std::fs::remove_dir_all(&dir).ok();
    }
}
