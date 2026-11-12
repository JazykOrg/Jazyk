// The project directory as a tree: every file under the folder of `jazyk.toml`,
// dot-folders and the out directory included, for the navigator's project tree; and
// any one of them read as bytes. Read only. Target-neutral: every read goes through
// crate::vfs. Mirrors docs/frontends/gui.md#api (`GET /api/project/files`); the
// navigator is jazyk-cloud docs/editor/editor.md#workspace.
use super::Refusal;
use crate::vfs::VPathExt;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// Version control is hidden. Build caches are hidden except for declared products.
// Prune unrelated cache directories before walking their contents.
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

fn product_patterns(root: &Path) -> Vec<String> {
    crate::task_runtime::load(root)
        .map(|(_, tasks)| tasks.into_iter().flat_map(|t| t.products).collect())
        .unwrap_or_default()
}

fn walk(
    dir: &Path,
    base: &Path,
    acc: &mut Vec<(String, u64)>,
    seen: &mut std::collections::BTreeSet<PathBuf>,
    products: &[String],
    product_only: bool,
) {
    let Ok(canonical) = dir.vcanonicalize() else {
        return;
    };
    if !canonical.starts_with(base.vcanonicalize().unwrap_or_else(|_| base.into()))
        || !seen.insert(canonical)
    {
        return;
    }
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
        if name == ".git" {
            continue;
        }
        if is_env_file(&name) && !p.vis_dir() {
            continue;
        }
        if p.vis_dir() {
            let product_only = product_only || SKIP.contains(&name.as_str());
            let relative = p
                .strip_prefix(base)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            if product_only && !crate::task_runtime::may_contain_product(products, &relative) {
                continue;
            }
            walk(&p, base, acc, seen, products, product_only);
        } else if let Ok(rel) = p.strip_prefix(base) {
            if !crate::vfs::is_symlink(&p)
                && !p.vcanonicalize().is_ok_and(|p| {
                    p.starts_with(base.vcanonicalize().unwrap_or_else(|_| base.into()))
                })
            {
                continue;
            }
            let relative = rel.to_string_lossy().replace('\\', "/");
            if product_only
                && (!crate::task_runtime::matches(products, &relative)
                    || !product_readable(base, &relative, &p))
            {
                continue;
            }
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            acc.push((rel.to_string_lossy().replace('\\', "/"), size));
        }
    }
}

// A directory relative to the project directory, or None when it lies outside.
fn rel_dir(root: &Path, dir: &Path) -> Option<String> {
    let root = root.vcanonicalize().unwrap_or_else(|_| root.to_path_buf());
    let dir = dir.vcanonicalize().unwrap_or_else(|_| dir.to_path_buf());
    dir.strip_prefix(&root)
        .ok()
        .map(|r| r.to_string_lossy().replace('\\', "/"))
}

// `GET /api/project/files`: every file under the project directory with its size,
// and where the product and the out directory sit in it.
pub fn project_files(root: &Path, deliverable: &Path, out: &Path, docs_glob: &[String]) -> Value {
    let mut acc = Vec::new();
    let products = product_patterns(root);
    walk(
        root,
        root,
        &mut acc,
        &mut Default::default(),
        &products,
        false,
    );
    let workspace = crate::document_sets::Workspace::load(root).ok();
    let files: Vec<Value> = acc
        .into_iter()
        .map(|(path, size)| {
            let memberships = workspace
                .as_ref()
                .map(|w| w.memberships(&path))
                .unwrap_or_default();
            let input = memberships.iter().any(|m| m["role"] == "docs")
                || crate::document_sets::included(docs_glob, &path);
            let product = !path.split('/').any(|p| p.starts_with('.'))
                && path != "jazyk.toml"
                && !root.join(&path).starts_with(out)
                && (crate::task_runtime::matches(&products, &path)
                    || (!input && root.join(&path).starts_with(deliverable)));
            let mut value=json!({ "path": path, "size": size, "memberships":memberships, "product":product, "writable": writable_path(root, out, &path).is_ok() });
            if let Ok(target)=crate::vfs::read_link(&root.join(&path)) {
                value["entryType"]=json!("symlink");
                value["symlinkTarget"]=json!(target.to_string_lossy());
                value["writable"]=json!(false);
            }
            value
        })
        .collect();
    json!({
        "files": files,
        "fileActions": true,
        "deliverable": rel_dir(root, deliverable),
        "out": rel_dir(root, out),
        "docsGlob": docs_glob,
    })
}

// A path relative to the project directory: no absolute, no empty or `.`/`..`
// component, no backslash, nothing under a skipped folder, no `.env` file.
// Dot-named folders (the out directory) are allowed.
fn product_readable(root: &Path, rel: &str, path: &Path) -> bool {
    if crate::task_runtime::protected(rel) {
        return false;
    }
    let Ok(base) = root.vcanonicalize() else {
        return false;
    };
    let Ok(canonical) = path.vcanonicalize() else {
        return false;
    };
    canonical.strip_prefix(base).is_ok_and(|p| {
        let p = p.to_string_lossy().replace('\\', "/");
        !crate::task_runtime::protected(&p) && !p.rsplit('/').next().is_some_and(is_env_file)
    })
}

fn rel_ok(root: &Path, rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == ".." || c == ".git")
        && !rel.rsplit('/').next().is_some_and(is_env_file)
        && (!rel.split('/').any(|c| SKIP.contains(&c))
            || (crate::task_runtime::matches(&product_patterns(root), rel)
                && product_readable(root, rel, &root.join(rel))))
}

fn safe(root: &Path, rel: &str) -> Option<PathBuf> {
    if !rel_ok(root, rel) {
        return None;
    }
    let base = root.vcanonicalize().ok()?;
    let canon = root.join(rel).vcanonicalize().ok()?;
    if !canon.starts_with(&base)
        || !canon.vexists()
        || !rel_ok(
            root,
            &canon
                .strip_prefix(&base)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/"),
        )
    {
        return None;
    }
    Some(canon)
}

// `GET /api/project/file?path=`: one file under the project directory, as bytes. A
// path that leaves it is 400; a path that names nothing (or a folder) is 404.
pub fn project_file(root: &Path, rel: &str) -> Result<Vec<u8>, Refusal> {
    let Some(abs) = safe(root, rel) else {
        if !rel_ok(root, rel) {
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

// Where a project file may be written (docs/frontends/gui.md#api): under the project
// directory, never in `.git`, `.jazyk` (the settings go through /api/settings),
// `node_modules`, `target`, or the out directory, never across a symlink out of the
// project or into a nested project. Other dot-folders (`.github`) are fine.
pub fn writable_path(root: &Path, out: &Path, rel: &str) -> Result<PathBuf, Refusal> {
    let refuse = || {
        (400, format!("cannot write {rel}: choose a path inside the project, outside .git, .jazyk, build, and dependency folders"))
    };
    if rel.is_empty() || rel.starts_with('/') || rel.contains(['\\', '\0', ':']) {
        return Err(refuse());
    }
    if rel.split('/').any(|c| {
        c.is_empty()
            || c == "."
            || c == ".."
            || c == ".jazyk"
            || c.starts_with("jazyk-out")
            || SKIP.contains(&c)
    }) {
        return Err(refuse());
    }
    // Secrets stay out of reach: a `.env` file is never served, so never written; a
    // `jazyk.toml` reads with its keys redacted, so a write would lose them.
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if is_env_file(name) || name == "jazyk.toml" {
        return Err((
            400,
            format!("cannot write {rel} here; settings are edited through /api/settings"),
        ));
    }
    let abs = root.join(rel);
    if abs.starts_with(out) {
        return Err((400, format!("cannot write {rel} inside the out directory")));
    }
    let base = root.vcanonicalize().map_err(|e| (500, e.to_string()))?;
    for ancestor in abs.ancestors().take_while(|p| *p != root) {
        if ancestor != abs
            && ancestor.vis_dir()
            && ancestor.join(crate::document_sets::CONFIG).vexists()
        {
            return Err((
                400,
                format!("cannot write {rel}: it is inside a nested project"),
            ));
        }
        if ancestor.vexists() && !ancestor.vcanonicalize().is_ok_and(|p| p.starts_with(&base)) {
            return Err((
                400,
                format!("cannot write {rel}: it leaves the project through a symlink"),
            ));
        }
    }
    Ok(abs)
}

// `PUT /api/project/file?path=`: create one new file anywhere under the project
// directory, inside a document set or not (docs/frontends/gui.md#api). It never
// overwrites an existing path.
pub fn create_project_file(
    root: &Path,
    out: &Path,
    rel: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, Refusal> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let abs = writable_path(root, out, rel)?;
    if let Some(target) = body["symlinkTarget"].as_str() {
        if body.get("text").is_some() || body.get("base64").is_some() {
            return Err((
                400,
                "provide symlinkTarget or file contents, not both".into(),
            ));
        }
        let resolved = crate::md::resolve_rel(rel, target)
            .ok_or((400, "symlink target leaves the project".into()))?;
        if std::path::Path::new(target).is_absolute()
            || target.contains('\\')
            || !crate::document_sets::safe_relative(root, &resolved)
            || crate::vfs::is_symlink(&root.join(&resolved))
            || !root.join(&resolved).vis_file()
        {
            return Err((
                400,
                "symlink target must be a direct eligible project file".into(),
            ));
        }
        if abs.vexists() || crate::vfs::is_symlink(&abs) {
            return Err((409, format!("{rel} already exists")));
        }
        if abs
            .parent()
            .into_iter()
            .flat_map(|p| p.ancestors())
            .take_while(|p| *p != root)
            .any(crate::vfs::is_symlink)
        {
            return Err((400, "symlink parents are not supported".into()));
        }
        crate::vfs::create_dir_all(abs.parent().ok_or((400, "invalid symlink path".into()))?)
            .map_err(|e| (500, e.to_string()))?;
        crate::vfs::symlink_file(std::path::Path::new(target), &abs)
            .map_err(|e| (500, e.to_string()))?;
        return Ok(json!({"path":rel,"entryType":"symlink","symlinkTarget":target}));
    }
    let bytes = if let Some(text) = body["text"].as_str() {
        text.as_bytes().to_vec()
    } else if let Some(encoded) = body["base64"].as_str() {
        STANDARD
            .decode(encoded)
            .map_err(|_| (400, "invalid base64 content".to_string()))?
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

// `GET /api/project/text?path=`: an existing project file as text with its hash.
pub fn project_text(root: &Path, rel: &str) -> Result<serde_json::Value, Refusal> {
    let bytes = project_file(root, rel)?;
    let text = String::from_utf8(bytes).map_err(|_| (415, format!("{rel} is not UTF-8 text")))?;
    let hash = crate::model::hash_hex(&text);
    Ok(serde_json::json!({ "path": rel, "text": text, "hash": hash }))
}

// `PUT /api/project/text?path=` with `{text, baseHash}`: replace an existing project
// file when the hash still names what is on disk. The file keeps its permissions.
pub fn write_project_text(
    root: &Path,
    out: &Path,
    rel: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, Refusal> {
    let abs = writable_path(root, out, rel)?;
    if crate::vfs::is_symlink(&abs) {
        return Err((
            405,
            "remove or replace the symlink entry before writing; its target is an input".into(),
        ));
    }
    let text = body["text"]
        .as_str()
        .ok_or((400, "text is required".to_string()))?;
    let base = body["baseHash"].as_str().ok_or((
        400,
        "baseHash is required; create new files with PUT /api/project/file".to_string(),
    ))?;
    if !abs.vexists() || abs.vis_dir() {
        return Err((404, format!("no file {rel}")));
    }
    let current =
        crate::vfs::read_to_string(&abs).map_err(|_| (415, format!("{rel} is not UTF-8 text")))?;
    if crate::model::hash_hex(&current) != base {
        return Err((409, "the file changed on disk since it was read".into()));
    }
    crate::vfs::write(&abs, text).map_err(|e| (500, e.to_string()))?;
    Ok(serde_json::json!({ "path": rel, "hash": crate::model::hash_hex(text) }))
}

// File management shares the write boundary with project text edits.
pub fn delete_project_file(root: &Path, out: &Path, rel: &str) -> Result<Value, Refusal> {
    let abs = writable_path(root, out, rel)?;
    if !abs.vis_file() && !crate::vfs::is_symlink(&abs) {
        return Err((404, format!("no file {rel}")));
    }
    crate::vfs::remove_file(abs).map_err(|e| (500, e.to_string()))?;
    Ok(json!({"deleted": rel}))
}

pub fn rename_project_file(
    root: &Path,
    out: &Path,
    from: &str,
    to: &str,
) -> Result<Value, Refusal> {
    let old = writable_path(root, out, from)?;
    let new = writable_path(root, out, to)?;
    if !old.vis_file() {
        return Err((404, format!("no file {from}")));
    }
    if new.vexists() {
        return Err((409, format!("{to} already exists")));
    }
    let listing = project_files(root, root, out, &[]);
    let mut changes = Vec::new();
    for file in listing["files"].as_array().into_iter().flatten() {
        let path = file["path"].as_str().unwrap_or_default();
        if !matches!(
            Path::new(path).extension().and_then(|e| e.to_str()),
            Some("md" | "markdown" | "MD")
        ) {
            continue;
        }
        let Ok(abs) = writable_path(root, out, path) else {
            continue;
        };
        let before = crate::vfs::read_to_string(&abs).map_err(|e| (500, e.to_string()))?;
        let target = if path == from { to } else { path };
        let after = super::file_links::rewrite(&before, path, target, from, to);
        if before != after {
            changes.push((path.to_string(), before, after));
        }
    }
    if let Some(parent) = new.parent() {
        crate::vfs::create_dir_all(parent).map_err(|e| (500, e.to_string()))?;
    }
    crate::vfs::rename(&old, &new).map_err(|e| (500, e.to_string()))?;
    let mut written: Vec<(PathBuf, String)> = Vec::new();
    for (path, before, after) in &changes {
        let abs = root.join(if path == from { to } else { path });
        let result = crate::vfs::read_to_string(&abs).and_then(|current| {
            if current != *before {
                return Err(std::io::Error::other("file changed during move"));
            }
            crate::store::write_atomic(&abs, after.as_bytes())
        });
        if let Err(error) = result {
            let mut rollback = Vec::new();
            for (path, text) in written.iter().rev() {
                if let Err(e) = crate::store::write_atomic(path, text.as_bytes()) {
                    rollback.push(e.to_string());
                }
            }
            if let Err(e) = crate::vfs::rename(&new, &old) {
                rollback.push(e.to_string());
            }
            return Err((
                500,
                format!("move failed: {error}; rollback errors: {rollback:?}"),
            ));
        }
        written.push((abs, before.clone()));
    }
    Ok(
        json!({"from": from, "to": to, "updated": changes.iter().map(|(p,_,_)| if p == from { to } else { p }).collect::<Vec<_>>()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_everything_but_the_skipped_folders_and_refuses_leaving() {
        let dir = std::env::temp_dir().join(format!("jz-projfiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for f in [
            "jazyk.toml",
            "docs/a.md",
            ".jazyk/graph/entities.yaml",
            "src/x.js",
            "node_modules/m/i.js",
            ".git/HEAD",
        ] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
        }
        let v = project_files(
            &dir,
            &dir.join("src"),
            &dir.join(".jazyk"),
            &["docs/**/*.md".into()],
        );
        let paths: Vec<&str> = v["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap())
            .collect();
        assert!(paths.contains(&"jazyk.toml"));
        assert!(paths.contains(&".jazyk/graph/entities.yaml"));
        assert!(paths.contains(&"docs/a.md"));
        assert!(!paths
            .iter()
            .any(|p| p.starts_with("node_modules") || p.starts_with(".git")));
        assert_eq!(v["deliverable"], "src");
        assert_eq!(v["out"], ".jazyk");
        assert_eq!(project_file(&dir, "jazyk.toml").unwrap(), b"x");
        assert_eq!(
            project_file(&dir, ".jazyk/graph/entities.yaml").unwrap(),
            b"x"
        );
        assert_eq!(project_file(&dir, "../etc/passwd").unwrap_err().0, 400);
        assert_eq!(project_file(&dir, ".git/HEAD").unwrap_err().0, 400);
        assert_eq!(project_file(&dir, "nope.md").unwrap_err().0, 404);
        let out = dir.join(".jazyk");
        let text = serde_json::json!({"text": "hello"});
        assert_eq!(
            create_project_file(&dir, &out, "src/new/notes.txt", &text).unwrap()["path"],
            "src/new/notes.txt"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("src/new/notes.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            create_project_file(&dir, &out, "src/new/notes.txt", &text)
                .unwrap_err()
                .0,
            409
        );
        assert_eq!(
            create_project_file(&dir, &out, ".jazyk/x.md", &text)
                .unwrap_err()
                .0,
            400
        );
        assert_eq!(
            create_project_file(&dir, &out, "../x.md", &text)
                .unwrap_err()
                .0,
            400
        );
        assert_eq!(
            create_project_file(&dir, &out, "node_modules/x.md", &text)
                .unwrap_err()
                .0,
            400
        );
        assert_eq!(
            create_project_file(&dir, &out, "a.bin", &serde_json::json!({"base64": "aGk="}))
                .unwrap()["path"],
            "a.bin"
        );
        assert_eq!(std::fs::read(dir.join("a.bin")).unwrap(), b"hi");
        assert_eq!(
            create_project_file(&dir, &out, ".github/workflows/ci.yml", &text).unwrap()["path"],
            ".github/workflows/ci.yml"
        );
        assert_eq!(
            create_project_file(&dir, &out, ".git/x", &text)
                .unwrap_err()
                .0,
            400
        );
        let read = project_text(&dir, "src/new/notes.txt").unwrap();
        let edit = serde_json::json!({"text": "edited", "baseHash": read["hash"]});
        assert_eq!(
            write_project_text(&dir, &out, "src/new/notes.txt", &edit).unwrap()["hash"],
            crate::model::hash_hex("edited")
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("src/new/notes.txt")).unwrap(),
            "edited"
        );
        assert_eq!(
            write_project_text(&dir, &out, "src/new/notes.txt", &edit)
                .unwrap_err()
                .0,
            409
        );
        assert_eq!(
            write_project_text(&dir, &out, "src/none.txt", &edit)
                .unwrap_err()
                .0,
            404
        );
        assert_eq!(
            write_project_text(&dir, &out, ".jazyk/jazyk.toml", &edit)
                .unwrap_err()
                .0,
            400
        );
        assert_eq!(
            create_project_file(&dir, &out, "sub/.env.local", &text)
                .unwrap_err()
                .0,
            400
        );
        assert_eq!(
            create_project_file(&dir, &out, "jazyk.toml", &text)
                .unwrap_err()
                .0,
            400
        );
        std::fs::write(dir.join("img.bin"), [0xff, 0xfe]).unwrap();
        assert_eq!(project_text(&dir, "img.bin").unwrap_err().0, 415);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retained_build_products_are_listed_and_read_without_exposing_cache_or_secrets() {
        let config = "version=1\n[[tasks]]\nid='build'\nname='Build'\ndescription='Retain products'\ncommand='true'\nproducts=['target/*.txt','**/target/*.pdf']\n";
        let files: std::collections::BTreeMap<String, Vec<u8>> = [
            (".jazyk/jazyk.toml", config),
            ("target/book.txt", "product"),
            ("target/disposable.bin", "cache"),
            ("nested/target/book.pdf", "pdf"),
            ("node_modules/dependency.js", "dependency"),
            ("target/.env.txt", "secret"),
            (".env.local", "secret"),
        ]
        .into_iter()
        .map(|(p, v)| (p.into(), v.as_bytes().to_vec()))
        .collect();
        let check = |root: &Path| {
            let listing = project_files(root, root, &root.join(".jazyk"), &[]);
            let paths: Vec<_> = listing["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["path"].as_str().unwrap())
                .collect();
            assert!(paths.contains(&"target/book.txt"));
            assert!(listing["files"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["path"].as_str().unwrap().ends_with("book.txt")
                    || f["path"].as_str().unwrap().ends_with("book.pdf"))
                .all(|f| f["product"] == true));
            assert!(listing["files"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["path"].as_str().unwrap().starts_with(".jazyk/"))
                .all(|f| f["product"] == false));
            assert!(paths.contains(&"nested/target/book.pdf"));
            assert!(!paths.iter().any(|p| p.contains("disposable")
                || p.contains("dependency")
                || p.contains(".env")
                || p.contains("link")));
            assert_eq!(project_file(root, "target/book.txt").unwrap(), b"product");
            assert_eq!(
                project_file(root, "nested/target/book.pdf").unwrap(),
                b"pdf"
            );
            for path in [
                "target/disposable.bin",
                "target/.env.txt",
                "node_modules/dependency.js",
            ] {
                assert_eq!(project_file(root, path).unwrap_err().0, 400, "{path}");
            }
            assert!(writable_path(root, &root.join(".jazyk"), "target/book.txt").is_err());
        };
        {
            let _scope = crate::vfs::Scope::enter(&files);
            check(Path::new("/project"));
        }
        let root =
            std::env::temp_dir().join(format!("jazyk-product-tree-{}", crate::sys::now_millis()));
        for (path, data) in &files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, data).unwrap();
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("../.env.local", root.join("target/link.txt")).unwrap();
            assert_eq!(project_file(&root, "target/link.txt").unwrap_err().0, 400);
        }
        check(&root);
        std::fs::remove_dir_all(root).unwrap();
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
        let paths: Vec<&str> = v["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap())
            .collect();
        assert!(paths.contains(&"docs/a.md"));
        assert!(paths.contains(&".jazyk/jazyk.toml"));
        assert!(
            !paths
                .iter()
                .any(|p| p.rsplit('/').next().unwrap().starts_with(".env")),
            "{:?}",
            paths
        );
        for f in [".env", ".env.local", "sub/.env.production"] {
            assert_eq!(project_file(&dir, f).unwrap_err().0, 400, "{}", f);
        }
        let served = String::from_utf8(project_file(&dir, ".jazyk/jazyk.toml").unwrap()).unwrap();
        assert!(
            !served.contains("sk-literal") && !served.contains("sk-inline"),
            "{}",
            served
        );
        assert!(served.contains("api_key = \"<redacted>\""), "{}", served);
        assert!(
            served.contains("\"api_key\" = \"<redacted>\""),
            "{}",
            served
        );
        assert!(
            served.contains("api_key_env = \"JAZYK_API_KEY\""),
            "{}",
            served
        );
        assert!(served.contains("model = \"m1\""), "{}", served);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod file_action_tests {
    use super::*;
    #[test]
    fn move_preserves_bytes_updates_links_and_refuses_collisions_and_protected_paths() {
        let root = std::env::temp_dir().join(format!("jazyk-file-actions-{}", std::process::id()));
        crate::vfs::remove_dir_all(&root).ok();
        crate::vfs::create_dir_all(root.join("docs")).unwrap();
        let out = root.join(".jazyk");
        crate::vfs::write(root.join("docs/a.md"), "# A\n[x](image.png)").unwrap();
        crate::vfs::write(
            root.join("docs/b.md"),
            "[A](a.md#intro)\n[code](https://example.com/a.md)",
        )
        .unwrap();
        let v = rename_project_file(&root, &out, "docs/a.md", "guide/a.md").unwrap();
        assert_eq!(v["updated"].as_array().unwrap().len(), 2);
        assert!(!root.join("docs/a.md").vexists());
        assert_eq!(
            crate::vfs::read_to_string(root.join("guide/a.md")).unwrap(),
            "# A\n[x](../docs/image.png)"
        );
        assert!(crate::vfs::read_to_string(root.join("docs/b.md"))
            .unwrap()
            .contains("../guide/a.md#intro"));
        assert_eq!(
            rename_project_file(&root, &out, "guide/a.md", "docs/b.md")
                .unwrap_err()
                .0,
            409
        );
        for bad in [
            "../outside",
            ".env",
            ".jazyk/status.yaml",
            "node_modules/a",
            "jazyk.toml",
        ] {
            assert!(rename_project_file(&root, &out, "guide/a.md", bad).is_err());
            assert!(delete_project_file(&root, &out, bad).is_err());
        }
        assert!(delete_project_file(&root, &out, "docs").is_err());
        crate::vfs::write(root.join("image.png"), [0, 255, 13]).unwrap();
        rename_project_file(&root, &out, "image.png", "guide/image.png").unwrap();
        assert_eq!(
            crate::vfs::read(root.join("guide/image.png")).unwrap(),
            [0, 255, 13]
        );
        delete_project_file(&root, &out, "guide/a.md").unwrap();
        assert!(!root.join("guide/a.md").vexists());
        crate::vfs::remove_dir_all(root).unwrap();
    }
}
