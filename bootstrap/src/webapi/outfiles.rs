// Read-only access to the out directory: the generated pages the markdown preview
// loads and the renderings it embeds as images. No write verb exists here; the out
// directory is build output. Target-neutral: every read goes through crate::vfs.
// Mirrors docs/frontends/gui.md#api (the out directory) and
// docs/frontends/wasm.md#the-product.
use super::deliverable::lexical_ok;
use super::Refusal;
use crate::vfs::VPathExt;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// Resolve a client path strictly inside the out directory: relative only, no
// traversal, no hidden component, and the canonical target inside the canonical
// out directory (a symlink pointing out is refused). None when anything fails,
// including a path that does not exist.
pub fn safe_out_path(out: &Path, rel: &str) -> Option<PathBuf> {
    super::deliverable::safe_under(out, rel)
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

// `GET /api/out/file?path=`: one file under the out directory, as bytes. A path
// that leaves the out directory (lexically, or through a symlink that exists) is
// 400; a path that validates but names nothing is 404.
pub fn out_file(out: &Path, path: &str) -> Result<Vec<u8>, Refusal> {
    let Some(abs) = safe_out_path(out, path) else {
        if !lexical_ok(path) || out.join(path).vexists() {
            return Err((400, format!("invalid out path {}", path)));
        }
        return Err((404, format!("no file {}", path)));
    };
    if abs.vis_dir() {
        return Err((404, format!("{} is a directory", path)));
    }
    crate::vfs::read(&abs).map_err(|e| (404, format!("cannot read {}: {}", path, e)))
}

pub(crate) fn walk(dir: &Path, base: &Path, acc: &mut Vec<(String, u64)>) {
    let Ok(rd) = crate::vfs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if p.vis_dir() {
            walk(&p, base, acc);
        } else if let Ok(rel) = p.strip_prefix(base) {
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            acc.push((rel.to_string_lossy().replace('\\', "/"), size));
        }
    }
}

// `GET /api/out/list?path=`: the files under one directory of the out directory,
// recursively, paths relative to the out directory. An absent directory lists as
// empty: the pages simply have not been generated yet. Every refusal is 400.
pub fn out_list(out: &Path, path: &str) -> Result<Value, Refusal> {
    let root = out
        .vcanonicalize()
        .map_err(|_| (400, "no out directory".to_string()))?;
    let mut files = Vec::new();
    match safe_out_path(out, path) {
        Some(dir) if dir.vis_dir() => walk(&dir, &root, &mut files),
        Some(_) => return Err((400, format!("{} is not a directory", path))),
        None => {
            if !lexical_ok(path) || out.join(path).vexists() {
                return Err((400, format!("invalid out path {}", path)));
            }
        }
    }
    files.sort();
    Ok(json!({
        "files": files
            .into_iter()
            .map(|(path, size)| json!({ "path": path, "size": size }))
            .collect::<Vec<_>>(),
    }))
}

// `GET /api/docsgen/{slug}`: one generated requirements document by its slug.
pub fn docsgen_value(out: &Path, slug: &str) -> Result<Value, Refusal> {
    if slug.contains('/') || slug.contains("..") || slug.contains('\\') {
        return Err((400, format!("invalid slug {}", slug)));
    }
    let f = out
        .join("docsgen")
        .join(format!("{}.md", slug.trim_end_matches(".md")));
    crate::vfs::read_to_string(&f)
        .map(|text| json!({ "slug": slug, "text": text }))
        .map_err(|_| (404, format!("no requirements document for {}", slug)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_out(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jazyk-webapi-out-{}-{}", name, crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join(".jazyk/docsgen/entities")).unwrap();
        crate::vfs::create_dir_all(dir.join(".jazyk/diagrams/class")).unwrap();
        crate::vfs::write(dir.join(".jazyk/docsgen/entities/funds.md"), "# Funds\n").unwrap();
        crate::vfs::write(dir.join(".jazyk/diagrams/class/funds.svg"), "<svg/>").unwrap();
        crate::vfs::write(dir.join(".jazyk/.hidden.md"), "x").unwrap();
        crate::vfs::write(dir.join("secret.md"), "outside\n").unwrap();
        dir
    }

    // Path confinement: a `..` that leaves the out directory is refused, and so are
    // absolute paths, hidden components, backslashes, and missing files.
    #[test]
    fn out_path_is_confined_to_the_out_directory() {
        let dir = temp_out("confine");
        let out = dir.join(".jazyk");
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
        crate::vfs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_the_out_directory_is_refused() {
        let dir = temp_out("symlink");
        let out = dir.join(".jazyk");
        std::os::unix::fs::symlink(dir.join("secret.md"), out.join("linked.md")).unwrap();
        std::os::unix::fs::symlink(&dir, out.join("up")).unwrap();
        assert!(safe_out_path(&out, "linked.md").is_none());
        assert!(safe_out_path(&out, "up/secret.md").is_none());
        assert_eq!(out_file(&out, "linked.md").unwrap_err().0, 400);
        crate::vfs::remove_dir_all(&dir).ok();
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
    // an absent directory as empty; a file read tells a refusal from an absence.
    #[test]
    fn listing_walks_a_directory_and_skips_hidden() {
        let dir = temp_out("list");
        let out = dir.join(".jazyk");
        let v = out_list(&out, "docsgen").unwrap();
        let files = v["files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["path"], "docsgen/entities/funds.md");
        assert_eq!(out_list(&out, "gen").unwrap()["files"], json!([]));
        assert_eq!(out_list(&out, "../x").unwrap_err().0, 400);
        let root = out.vcanonicalize().unwrap();
        let mut all = Vec::new();
        walk(&root, &root, &mut all);
        assert!(all.iter().all(|(p, _)| !p.contains("/.") && !p.starts_with('.')));
        assert_eq!(out_file(&out, "docsgen/entities/funds.md").unwrap(), b"# Funds\n");
        assert_eq!(out_file(&out, "docsgen/entities/missing.md").unwrap_err().0, 404);
        assert_eq!(out_file(&out, "docsgen").unwrap_err().0, 404);
        assert_eq!(out_file(&out, "../secret.md").unwrap_err().0, 400);
        crate::vfs::remove_dir_all(&dir).ok();
    }
}
