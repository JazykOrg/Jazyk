// The document values: the text of one document with its hash, the last reconciled
// baseline, and the conditional write. Target-neutral
// (docs/frontends/wasm.md#one-api-two-transports).
use crate::project::{self, Project};
use crate::store::Store;
use crate::vfs::VPathExt;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// Does the relative path match the docs glob? Last matching pattern wins, `!` negates.
// Mirrors Project::doc_files.
fn glob_included(proj: &Project, rel: &str) -> bool {
    if proj.set_id.is_none() && proj.root.join(crate::document_sets::CONFIG).vexists() {
        return crate::document_sets::Workspace::load(&proj.root).is_ok_and(|w| {
            w.sets
                .iter()
                .any(|s| crate::document_sets::included(&s.docs, rel))
        });
    }

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
    safe_doc_path_why(proj, rel).ok()
}

// The same check, naming the rule that rejected. A client's error is only useful if
// it says which of the six rules the path broke.
pub fn safe_doc_path_why(proj: &Project, rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains('\\') {
        return Err("not a relative path with forward slashes".to_string());
    }
    for c in rel.split('/') {
        // The same names the doc walk skips: hidden, build output, generated output.
        if c.is_empty()
            || c == "."
            || c == ".."
            || c == "target"
            || c == "node_modules"
            || c.starts_with('.')
            || c.starts_with(".jazyk")
            || c.starts_with("jazyk-out")
        {
            return Err(format!("`{}` is a path component the doc walk skips", c));
        }
    }
    if !glob_included(proj, rel) {
        return Err(format!("outside the docs glob {:?}", proj.docs_glob));
    }
    if !crate::document_sets::safe_relative(&proj.root, rel) {
        return Err("path crosses a nested project or a symlink boundary".into());
    }
    let abs = proj.root.join(rel);
    if abs.starts_with(&proj.out) {
        return Err("inside the out directory".to_string());
    }
    // Symlink guard: the deepest existing ancestor must canonicalize inside the root.
    let root = proj
        .root
        .vcanonicalize()
        .map_err(|e| format!("the project root does not resolve: {}", e))?;
    let mut anc: &Path = if abs.vexists() {
        &abs
    } else {
        abs.parent()
            .ok_or_else(|| "the path has no parent".to_string())?
    };
    while !anc.vexists() {
        anc = anc
            .parent()
            .ok_or_else(|| "no existing ancestor under the root".to_string())?;
    }
    let canon = anc
        .vcanonicalize()
        .map_err(|e| format!("{} does not resolve: {}", anc.display(), e))?;
    if !canon.starts_with(&root) {
        return Err(format!(
            "{} resolves outside the project root {}",
            canon.display(),
            root.display()
        ));
    }
    Ok(abs)
}

pub enum DocRead {
    Found(Value),
    Missing(String),
    Invalid(String),
}

pub fn doc_content(proj: &Project, rel: &str) -> DocRead {
    let abs = match safe_doc_path_why(proj, rel) {
        Ok(a) => a,
        Err(why) => return DocRead::Invalid(format!("invalid document path {}: {}", rel, why)),
    };
    match crate::md::read_doc(&abs) {
        Ok(text) => {
            let hash = crate::model::hash_hex(&text);
            let links = crate::md::missing_document_links(&proj.root, rel, &text);
            DocRead::Found(
                json!({ "path": rel, "text": text, "hash": hash, "linkDiagnostics": links }),
            )
        }
        Err(e) => DocRead::Missing(format!("cannot read {}: {}", rel, e)),
    }
}

// The last reconciled text, reconstructed from the stored section tree: sections
// ordered by their line spans, raw bodies joined, blank gap lines restored. The
// difference between this and the on-disk text is what the next build's dirty set
// sees.
pub fn doc_baseline(store: &Store, rel: &str) -> Option<Value> {
    let rec = store.docs.get(rel)?;
    let mut secs: Vec<_> = rec.sections.values().collect();
    secs.sort_by_key(|s| s.lines[0]);
    let mut parts: Vec<&str> = Vec::new();
    let mut expected = 0usize;
    for s in &secs {
        // Blank lines before the first heading belong to no section; restore them.
        for _ in expected..s.lines[0] {
            parts.push("");
        }
        parts.push(s.raw.as_str());
        expected = s.lines[1];
    }
    let text = parts.join("\n");
    Some(json!({ "path": rel, "text": text, "hash": rec.content_hash }))
}

pub enum DocWrite {
    Written(Value),
    Conflict(Value),
    Invalid(String),
    Failed(String),
}

// The write is conditional on the hash the client read, so two editors on one
// document never silently overwrite each other.
pub fn doc_write(proj: &Project, rel: &str, text: &str, base_hash: Option<&str>) -> DocWrite {
    if Path::new(rel).extension().and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("docx") || e.eq_ignore_ascii_case("pdf")) {
        return DocWrite::Invalid("binary documents require a format-specific editor".into());
    }
    let abs = match safe_doc_path_why(proj, rel) {
        Ok(a) => a,
        Err(why) => return DocWrite::Invalid(format!("invalid document path {}: {}", rel, why)),
    };
    let on_disk = crate::vfs::read_to_string(&abs)
        .ok()
        .map(|t| crate::model::hash_hex(&t));
    if base_hash.is_some() && on_disk.as_deref() != base_hash {
        return DocWrite::Conflict(json!({
            "error": "the document changed on disk since it was read",
            "diskHash": on_disk,
        }));
    }
    if let Some(parent) = abs.parent() {
        if !parent.vexists() {
            if let Err(e) = crate::vfs::create_dir_all(parent) {
                return DocWrite::Failed(format!("cannot create {}: {}", parent.display(), e));
            }
        }
    }
    let written = if base_hash.is_none() {
        // An exclusive create closes the missing-read/create race and refuses
        // unreadable existing files and symlinks without replacing their bytes.
        crate::vfs::create_new(&abs, text)
    } else {
        // A replacement lands whole: the watcher and a concurrent build never read
        // a half-written document.
        crate::store::write_atomic(&abs, text.as_bytes())
    };
    if let Err(e) = written {
        if base_hash.is_none() && e.kind() == std::io::ErrorKind::AlreadyExists {
            return DocWrite::Conflict(json!({
                "error": "the document already exists; read it before editing",
                "diskHash": crate::vfs::read_to_string(&abs).ok().map(|t| crate::model::hash_hex(&t)),
            }));
        }
        return DocWrite::Failed(format!("cannot write {}: {}", rel, e));
    }
    DocWrite::Written(json!({ "path": rel, "hash": crate::model::hash_hex(text) }))
}

pub fn doc_delete(proj: &Project, rel: &str) -> Result<Value, super::Refusal> {
    let abs = safe_doc_path_why(proj, rel).map_err(|e| (400, e))?;
    if !abs.vexists() {
        return Err((404, format!("no document {}", rel)));
    }
    crate::vfs::remove_file(abs).map_err(|e| (500, e.to_string()))?;
    Ok(json!({"deleted": rel}))
}

pub fn doc_rename(proj: &Project, from: &str, to: &str) -> Result<Value, super::Refusal> {
    let old = safe_doc_path_why(proj, from).map_err(|e| (400, e))?;
    let new = safe_doc_path_why(proj, to).map_err(|e| (400, e))?;
    if !old.vexists() {
        return Err((404, format!("no document {}", from)));
    }
    if new.vexists() {
        return Err((409, format!("{} already exists", to)));
    }
    if let Some(parent) = new.parent() {
        crate::vfs::create_dir_all(parent).map_err(|e| (500, e.to_string()))?;
    }
    crate::vfs::rename(old, new).map_err(|e| (500, e.to_string()))?;
    Ok(json!({"from": from, "to": to}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use std::path::Path;

    fn proj(dir: &Path) -> Project {
        let mut p = Project::default();
        p.root = dir.to_path_buf();
        p.out = dir.join(".jazyk");
        p.docs_glob = vec!["**/*.md".to_string(), "!fixtures/**".to_string()];
        p
    }

    #[test]
    fn text_writes_never_replace_binary_documents() {
        let dir = std::env::temp_dir().join(format!("jazyk-binary-write-{}", crate::sys::process_id()));
        crate::vfs::create_dir_all(&dir).unwrap();
        let p = proj(&dir);
        for name in ["contract.docx", "contract.DOCX", "contract.pdf", "contract.PDF"] {
            crate::vfs::write(dir.join(name), b"original binary bytes").unwrap();
            assert!(matches!(doc_write(&p, name, "replacement prose", None), DocWrite::Invalid(_)));
            assert_eq!(crate::vfs::read(dir.join(name)).unwrap(), b"original binary bytes");
        }
        crate::vfs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_link_read_and_exclusive_create_share_current_workspace() {
        let dir =
            std::env::temp_dir().join(format!("jazyk-doc-link-read-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(dir.join("docs/main.md"), "[Intro](intro.md)").unwrap();
        let p = proj(&dir);
        let DocRead::Found(read) = doc_content(&p, "docs/main.md") else {
            panic!("read")
        };
        assert_eq!(read["linkDiagnostics"][0]["target"], "docs/intro.md");
        assert!(matches!(
            doc_write(&p, "docs/intro.md", "original", None),
            DocWrite::Written(_)
        ));
        assert!(matches!(
            doc_write(&p, "docs/intro.md", "overwrite", None),
            DocWrite::Conflict(_)
        ));
        assert_eq!(
            crate::vfs::read_to_string(dir.join("docs/intro.md")).unwrap(),
            "original"
        );
        let DocRead::Found(read) = doc_content(&p, "docs/main.md") else {
            panic!("read")
        };
        assert_eq!(read["linkDiagnostics"], json!([]));
        crate::vfs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn doc_path_safety_matrix() {
        let dir =
            std::env::temp_dir().join(format!("jazyk-gui-docs-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(dir.join("docs/a.md"), "# A\n").unwrap();
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
        assert!(safe_doc_path(&p, ".jazyk/docsgen/x.md").is_none());
        assert!(safe_doc_path(&p, ".jazyk-backup/x.md").is_none());
        assert!(safe_doc_path(&p, "target/x.md").is_none());
        assert!(safe_doc_path(&p, "node_modules/x.md").is_none());
        assert!(safe_doc_path(&p, "docs/a.txt").is_none());
        assert!(safe_doc_path(&p, "fixtures/trap.md").is_none());
        assert!(safe_doc_path(&p, "").is_none());
        assert!(safe_doc_path(&p, "docs\\a.md").is_none());

        crate::vfs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_parent_never_escapes() {
        let dir = std::env::temp_dir().join(format!(
            "jazyk-gui-symlink-test-{}",
            crate::sys::process_id()
        ));
        let outside =
            std::env::temp_dir().join(format!("jazyk-gui-outside-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::remove_dir_all(&outside).ok();
        crate::vfs::create_dir_all(&dir).unwrap();
        crate::vfs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("linked")).unwrap();
        let p = proj(&dir);
        assert!(safe_doc_path(&p, "linked/escape.md").is_none());
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::remove_dir_all(&outside).ok();
    }
}
