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

// A file whose name starts with `.env` holds secrets: never listed, never served
// (docs/frontends/gui.md#api).
fn is_env_file(name: &str) -> bool {
    name.starts_with(".env")
}

// A `jazyk.toml` is served with every `api_key` value replaced, whether the key
// sits on its own line or inside an inline table, in basic or literal quotes.
fn redact_api_keys(text: &str) -> String {
    let re = regex::Regex::new(r#"("?api_key"?\s*=\s*)(?:"(?:[^"\\]|\\.)*"|'[^']*')"#).unwrap();
    re.replace_all(text, "${1}\"<redacted>\"").into_owned()
}

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
        if is_env_file(&name) && !p.vis_dir() {
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
// component, no backslash, nothing under a skipped folder, no `.env` file.
// Dot-named folders (the out directory) are allowed.
fn rel_ok(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel.split('/').any(|c| c.is_empty() || c == "." || c == ".." || SKIP.contains(&c))
        && !rel.rsplit('/').next().is_some_and(is_env_file)
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
    let bytes = crate::vfs::read(&abs).map_err(|e| (404, format!("cannot read {}: {}", rel, e)))?;
    if rel.rsplit('/').next() == Some("jazyk.toml") {
        if let Ok(text) = std::str::from_utf8(&bytes) {
            return Ok(redact_api_keys(text).into_bytes());
        }
    }
    Ok(bytes)
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
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_files_are_neither_listed_nor_served_and_jazyk_toml_hides_its_key() {
        let dir = std::env::temp_dir().join(format!("jz-projfiles-secrets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (f, text) in [
            (".env", "JAZYK_API_KEY=sk-top-secret\n"),
            (".env.local", "X=1\n"),
            ("sub/.env.production", "X=2\n"),
            ("docs/a.md", "x"),
            (".jazyk/jazyk.toml", "version = 1\n[llm]\nmodel = \"m1\"\napi_key = \"sk-literal\"\napi_key_env = \"JAZYK_API_KEY\"\n\n[[sets]]\nid = \"ds_a\"\nname = \"A\"\ndocs = [\"docs/**\"]\nllm = {\"model\" = \"m2\", \"api_key\" = \"sk-inline\"}\n"),
        ] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
        }
        let v = project_files(&dir, &dir, &dir.join(".jazyk"), &[]);
        let paths: Vec<&str> = v["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"docs/a.md"));
        assert!(paths.contains(&".jazyk/jazyk.toml"));
        assert!(!paths.iter().any(|p| p.rsplit('/').next().unwrap().starts_with(".env")), "{:?}", paths);
        for f in [".env", ".env.local", "sub/.env.production"] {
            assert_eq!(project_file(&dir, f).unwrap_err().0, 400, "{}", f);
        }
        let served = String::from_utf8(project_file(&dir, ".jazyk/jazyk.toml").unwrap()).unwrap();
        assert!(!served.contains("sk-literal") && !served.contains("sk-inline"), "{}", served);
        assert!(served.contains("api_key = \"<redacted>\""), "{}", served);
        assert!(served.contains("\"api_key\" = \"<redacted>\""), "{}", served);
        assert!(served.contains("api_key_env = \"JAZYK_API_KEY\""), "{}", served);
        assert!(served.contains("model = \"m1\""), "{}", served);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
