// The project directory as a tree: every file under the folder of `jazyk.toml`,
// dot-folders and the out directory included, for the navigator's project tree; and
// any one of them read as bytes. Read only. Target-neutral: every read goes through
// crate::vfs. Mirrors docs/frontends/gui.md#api (the project files) and
// docs/frontends/gui.md#workspace.
use super::Refusal;
use crate::vfs::VPathExt;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// Folders the tree never walks: version control and build caches, which are not
// the project's content and can hold hundreds of thousands of files.
const SKIP: &[&str] = &[".git", "node_modules", "target"];

// The most files the tree lists; a bigger project lists its first ones.
const MOST: usize = 20_000;

fn walk(dir: &Path, base: &Path, acc: &mut Vec<(String, u64)>, seen: &mut std::collections::BTreeSet<PathBuf>) {
    let Ok(canonical)=dir.vcanonicalize() else {return;};
    if !canonical.starts_with(base.vcanonicalize().unwrap_or_else(|_|base.into())) || !seen.insert(canonical) {return;}
    let Ok(rd) = crate::vfs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if acc.len() >= MOST {
            return;
        }
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        if p.vis_dir() {
            walk(&p, base, acc, seen);
        } else if let Ok(rel) = p.strip_prefix(base) {
            if !p.vcanonicalize().is_ok_and(|p|p.starts_with(base.vcanonicalize().unwrap_or_else(|_|base.into()))) {continue;}
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            acc.push((rel.to_string_lossy().replace('\\', "/"), size));
        }
    }
}

// A directory relative to the project directory, or None when it lies outside.
fn rel_dir(root: &Path, dir: &Path) -> Option<String> {
    let root = root.vcanonicalize().unwrap_or_else(|_| root.to_path_buf());
    let dir = dir.vcanonicalize().unwrap_or_else(|_| dir.to_path_buf());
    dir.strip_prefix(&root).ok().map(|r| r.to_string_lossy().replace('\\', "/"))
}

// `GET /api/project/files`: every file under the project directory with its size,
// and where the product and the out directory sit in it.
pub fn project_files(root: &Path, deliverable: &Path, out: &Path, docs_glob: &[String]) -> Value {
    let mut acc = Vec::new();
    walk(root, root, &mut acc, &mut Default::default());
    let workspace=crate::document_sets::Workspace::load(root).ok();
    let files: Vec<Value> = acc.into_iter().map(|(path, size)| {
        let memberships=workspace.as_ref().map(|w|w.memberships(&path)).unwrap_or_default();
        json!({ "path": path, "size": size, "memberships":memberships })
    }).collect();
    json!({
        "files": files,
        "deliverable": rel_dir(root, deliverable),
        "out": rel_dir(root, out),
        "docsGlob": docs_glob,
    })
}

// A path relative to the project directory: no absolute, no empty or `.`/`..`
// component, no backslash, nothing under a skipped folder. Dot-named folders (the
// out directory) are allowed.
fn rel_ok(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel.split('/').any(|c| c.is_empty() || c == "." || c == ".." || SKIP.contains(&c))
}

fn safe(root: &Path, rel: &str) -> Option<PathBuf> {
    if !rel_ok(rel) {
        return None;
    }
    let base = root.vcanonicalize().ok()?;
    let canon = root.join(rel).vcanonicalize().ok()?;
    if !canon.starts_with(&base) || !canon.vexists() {
        return None;
    }
    Some(canon)
}

// `GET /api/project/file?path=`: one file under the project directory, as bytes. A
// path that leaves it is 400; a path that names nothing (or a folder) is 404.
pub fn project_file(root: &Path, rel: &str) -> Result<Vec<u8>, Refusal> {
    let Some(abs) = safe(root, rel) else {
        if !rel_ok(rel) {
            return Err((400, format!("invalid project path {}", rel)));
        }
        return Err((404, format!("no file {}", rel)));
    };
    if abs.vis_dir() {
        return Err((404, format!("{} is a folder", rel)));
    }
    crate::vfs::read(&abs).map_err(|e| (404, format!("cannot read {}: {}", rel, e)))
}

// `PUT /api/project/file?path=`: create one new file anywhere under the project
// directory, inside a document set or not (docs/frontends/gui.md#diagnostics). It
// never overwrites an existing path.
pub fn create_project_file(root: &Path, out: &Path, rel: &str, body: &serde_json::Value) -> Result<serde_json::Value, Refusal> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    if !crate::document_sets::safe_relative(root, rel) {
        return Err((400, format!("cannot create {rel}: choose a path inside the project, outside hidden, build, and dependency folders")));
    }
    let abs = root.join(rel);
    if abs.starts_with(out) {
        return Err((400, format!("cannot create {rel} inside the out directory")));
    }
    let bytes = if let Some(text) = body["text"].as_str() {
        text.as_bytes().to_vec()
    } else if let Some(encoded) = body["base64"].as_str() {
        STANDARD.decode(encoded).map_err(|_| (400, "invalid base64 content".to_string()))?
    } else {
        return Err((400, "text or base64 is required".into()));
    };
    if bytes.len() > 32 * 1024 * 1024 {
        return Err((413, "files must be at most 32 MiB".into()));
    }
    #[cfg(all(feature = "native-documents", not(target_arch = "wasm32")))]
    if crate::documents::supported(&abs) {
        crate::documents::validate(&abs, &bytes).map_err(|e| (422, e))?;
    }
    if abs.vexists() {
        return Err((409, format!("{rel} already exists")));
    }
    if let Some(parent) = abs.parent() {
        crate::vfs::create_dir_all(parent).map_err(|e| (500, e.to_string()))?;
    }
    crate::vfs::write(&abs, &bytes).map_err(|e| (500, e.to_string()))?;
    Ok(serde_json::json!({ "path": rel }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_everything_but_the_skipped_folders_and_refuses_leaving() {
        let dir = std::env::temp_dir().join(format!("jz-projfiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for f in ["jazyk.toml", "docs/a.md", ".jazyk/graph/entities.yaml", "src/x.js", "node_modules/m/i.js", ".git/HEAD"] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
        }
        let v = project_files(&dir, &dir.join("src"), &dir.join(".jazyk"), &["docs/**/*.md".into()]);
        let paths: Vec<&str> = v["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"jazyk.toml"));
        assert!(paths.contains(&".jazyk/graph/entities.yaml"));
        assert!(paths.contains(&"docs/a.md"));
        assert!(!paths.iter().any(|p| p.starts_with("node_modules") || p.starts_with(".git")));
        assert_eq!(v["deliverable"], "src");
        assert_eq!(v["out"], ".jazyk");
        assert_eq!(project_file(&dir, "jazyk.toml").unwrap(), b"x");
        assert_eq!(project_file(&dir, ".jazyk/graph/entities.yaml").unwrap(), b"x");
        assert_eq!(project_file(&dir, "../etc/passwd").unwrap_err().0, 400);
        assert_eq!(project_file(&dir, ".git/HEAD").unwrap_err().0, 400);
        assert_eq!(project_file(&dir, "nope.md").unwrap_err().0, 404);
        let out = dir.join(".jazyk");
        let text = serde_json::json!({"text": "hello"});
        assert_eq!(create_project_file(&dir, &out, "src/new/notes.txt", &text).unwrap()["path"], "src/new/notes.txt");
        assert_eq!(std::fs::read_to_string(dir.join("src/new/notes.txt")).unwrap(), "hello");
        assert_eq!(create_project_file(&dir, &out, "src/new/notes.txt", &text).unwrap_err().0, 409);
        assert_eq!(create_project_file(&dir, &out, ".jazyk/x.md", &text).unwrap_err().0, 400);
        assert_eq!(create_project_file(&dir, &out, "../x.md", &text).unwrap_err().0, 400);
        assert_eq!(create_project_file(&dir, &out, "node_modules/x.md", &text).unwrap_err().0, 400);
        assert_eq!(create_project_file(&dir, &out, "a.bin", &serde_json::json!({"base64": "aGk="})).unwrap()["path"], "a.bin");
        assert_eq!(std::fs::read(dir.join("a.bin")).unwrap(), b"hi");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
