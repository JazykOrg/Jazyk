// Document read/write for the GUI: path validation and the conditional write.
// The write path is the security-sensitive surface; every rule here has a test.
use super::api::err;
use super::state::SharedState;
use crate::project::{self, Project};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

// Does the relative path match the docs glob? Last matching pattern wins, `!` negates.
// Mirrors Project::doc_files.
fn glob_included(proj: &Project, rel: &str) -> bool {
    let mut included = false;
    for pat in &proj.docs_glob {
        let (neg, p) = match pat.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, pat.as_str()),
        };
        if project::glob_match(p, rel) {
            included = !neg;
        }
    }
    included
}

// Validate a client-supplied document path and resolve it under the project root.
// Rejects traversal, absolute paths, the out directory, anything the doc walk would
// skip, paths outside the docs glob, and symlink escapes. A path that validates but
// does not exist yet is fine: that is how a new document is created.
pub fn safe_doc_path(proj: &Project, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains('\\') {
        return None;
    }
    for c in rel.split('/') {
        // The same names the doc walk skips: hidden, build output, generated output.
        if c.is_empty()
            || c == "."
            || c == ".."
            || c == "target"
            || c == "node_modules"
            || c.starts_with('.')
            || c.starts_with("jazyk-out")
        {
            return None;
        }
    }
    if !glob_included(proj, rel) {
        return None;
    }
    let abs = proj.root.join(rel);
    if abs.starts_with(&proj.out) {
        return None;
    }
    // Symlink guard: the deepest existing ancestor must canonicalize inside the root.
    let root = proj.root.canonicalize().ok()?;
    let mut anc: &Path = abs.parent()?;
    while !anc.exists() {
        anc = anc.parent()?;
    }
    let canon = anc.canonicalize().ok()?;
    if !canon.starts_with(&root) {
        return None;
    }
    Some(abs)
}

#[derive(Deserialize)]
pub struct DocPathQ {
    path: String,
}

#[derive(Deserialize)]
pub struct DocWrite {
    text: String,
    #[serde(rename = "baseHash")]
    base_hash: Option<String>,
}

// Conditional write: the caller names the hash it edited from; a mismatch means the
// file moved underneath it (another editor), and the client re-reads.
pub async fn doc_write(
    State(st): State<SharedState>,
    Query(p): Query<DocPathQ>,
    Json(body): Json<DocWrite>,
) -> Response {
    let Some(abs) = safe_doc_path(&st.proj, &p.path) else {
        return err(StatusCode::BAD_REQUEST, format!("invalid document path {}", p.path));
    };
    let on_disk = std::fs::read_to_string(&abs).ok().map(|t| crate::model::hash_hex(&t));
    if on_disk != body.base_hash {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "the document changed on disk since it was read",
                "diskHash": on_disk,
            })),
        )
            .into_response();
    }
    if let Some(parent) = abs.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return err(StatusCode::INTERNAL_SERVER_ERROR, format!("cannot create {}: {}", parent.display(), e));
        }
    }
    if let Err(e) = std::fs::write(&abs, &body.text) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("cannot write {}: {}", p.path, e));
    }
    Json(json!({ "path": p.path, "hash": crate::model::hash_hex(&body.text) })).into_response()
}

#[derive(Deserialize)]
pub struct DocRename {
    from: String,
    to: String,
}

// Move a document. The graph is untouched: the next build's dirty set sees the move
// and the reconciler rewrites references mechanically.
pub async fn doc_rename(State(st): State<SharedState>, Json(body): Json<DocRename>) -> Response {
    let Some(from) = safe_doc_path(&st.proj, &body.from) else {
        return err(StatusCode::BAD_REQUEST, format!("invalid document path {}", body.from));
    };
    let Some(to) = safe_doc_path(&st.proj, &body.to) else {
        return err(StatusCode::BAD_REQUEST, format!("invalid document path {}", body.to));
    };
    if !from.exists() {
        return err(StatusCode::NOT_FOUND, format!("no document {}", body.from));
    }
    if to.exists() {
        return err(StatusCode::CONFLICT, format!("{} already exists", body.to));
    }
    if let Some(parent) = to.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return err(StatusCode::INTERNAL_SERVER_ERROR, format!("cannot create {}: {}", parent.display(), e));
        }
    }
    if let Err(e) = std::fs::rename(&from, &to) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("cannot rename: {}", e));
    }
    Json(json!({ "from": body.from, "to": body.to })).into_response()
}

// Delete a document. The graph is untouched: the next build reconciles the
// disappearance and garbage collection removes what nothing mentions anymore.
pub async fn doc_delete(State(st): State<SharedState>, Query(p): Query<DocPathQ>) -> Response {
    let Some(abs) = safe_doc_path(&st.proj, &p.path) else {
        return err(StatusCode::BAD_REQUEST, format!("invalid document path {}", p.path));
    };
    if !abs.exists() {
        return err(StatusCode::NOT_FOUND, format!("no document {}", p.path));
    }
    if let Err(e) = std::fs::remove_file(&abs) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("cannot delete {}: {}", p.path, e));
    }
    Json(json!({ "deleted": p.path })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proj(dir: &Path) -> Project {
        let mut p = Project::default();
        p.root = dir.to_path_buf();
        p.out = dir.join("jazyk-out");
        p.docs_glob = vec!["**/*.md".to_string(), "!fixtures/**".to_string()];
        p
    }

    #[test]
    fn doc_path_safety_matrix() {
        let dir = std::env::temp_dir().join(format!("jazyk-gui-docs-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/a.md"), "# A\n").unwrap();
        let p = proj(&dir);

        assert!(safe_doc_path(&p, "docs/a.md").is_some());
        // A new file under an existing dir, and under a new dir, both validate.
        assert!(safe_doc_path(&p, "docs/new.md").is_some());
        assert!(safe_doc_path(&p, "docs/sub/new.md").is_some());
        // Traversal, absolute, hidden, out-dir, wrong extension, excluded glob.
        assert!(safe_doc_path(&p, "../escape.md").is_none());
        assert!(safe_doc_path(&p, "docs/../../escape.md").is_none());
        assert!(safe_doc_path(&p, "/etc/passwd").is_none());
        assert!(safe_doc_path(&p, ".git/config.md").is_none());
        assert!(safe_doc_path(&p, "jazyk-out/docsgen/x.md").is_none());
        assert!(safe_doc_path(&p, "jazyk-out-backup/x.md").is_none());
        assert!(safe_doc_path(&p, "target/x.md").is_none());
        assert!(safe_doc_path(&p, "node_modules/x.md").is_none());
        assert!(safe_doc_path(&p, "docs/a.txt").is_none());
        assert!(safe_doc_path(&p, "fixtures/trap.md").is_none());
        assert!(safe_doc_path(&p, "").is_none());
        assert!(safe_doc_path(&p, "docs\\a.md").is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_parent_never_escapes() {
        let dir = std::env::temp_dir().join(format!("jazyk-gui-symlink-test-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("jazyk-gui-outside-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&outside).ok();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("linked")).unwrap();
        let p = proj(&dir);
        assert!(safe_doc_path(&p, "linked/escape.md").is_none());
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&outside).ok();
    }
}
