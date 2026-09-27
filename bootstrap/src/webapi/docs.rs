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
        {
            return Err(format!("`{}` is a path component the doc walk skips", c));
        }
    }
    if !glob_included(proj, rel) {
        return Err(format!("outside the docs glob {:?}", proj.docs_glob));
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
    let mut anc: &Path = abs
        .parent()
        .ok_or_else(|| "the path has no parent".to_string())?;
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
    match crate::vfs::read_to_string(&abs) {
        Ok(text) => {
            let hash = crate::model::hash_hex(&text);
            DocRead::Found(json!({ "path": rel, "text": text, "hash": hash }))
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
    let abs = match safe_doc_path_why(proj, rel) {
        Ok(a) => a,
        Err(why) => return DocWrite::Invalid(format!("invalid document path {}: {}", rel, why)),
    };
    let on_disk = crate::vfs::read_to_string(&abs)
        .ok()
        .map(|t| crate::model::hash_hex(&t));
    if on_disk.as_deref() != base_hash {
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
    if let Err(e) = crate::vfs::write(&abs, text) {
        return DocWrite::Failed(format!("cannot write {}: {}", rel, e));
    }
    DocWrite::Written(json!({ "path": rel, "hash": crate::model::hash_hex(text) }))
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
    fn doc_path_safety_matrix() {
        let dir = std::env::temp_dir().join(format!("jazyk-gui-docs-test-{}", crate::sys::process_id()));
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
        let dir =
            std::env::temp_dir().join(format!("jazyk-gui-symlink-test-{}", crate::sys::process_id()));
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
