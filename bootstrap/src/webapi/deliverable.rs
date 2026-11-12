// The deliverable browser: the generated product listed with the graph nodes the
// ledger binds to each file, and the ledger's anchored sites resolved against the
// current file text. Read-only, target-neutral: every read goes through crate::vfs,
// so a tab answers from the tree what the server answers from disk. Mirrors
// docs/frontends/gui.md#api (deliverable) and docs/frontends/wasm.md#the-product.
use super::Refusal;
use crate::gen::{GenSettings, Ledger};
use crate::store::Store;
use crate::vfs::VPathExt;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, u64)>) {
    walk_seen(dir, base, out, &mut Default::default());
}

fn walk_seen(
    dir: &Path,
    base: &Path,
    out: &mut Vec<(String, u64)>,
    seen: &mut std::collections::BTreeSet<PathBuf>,
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
    for e in rd.flatten() {
        let p = e.path();
        let Ok(relative) = p.strip_prefix(base) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        // Source inventories share the project policy, including scaffolding and
        // the boundary around a nested project.
        if !crate::document_sets::safe_relative(base, &relative) {
            continue;
        }
        if p.vis_dir() {
            walk_seen(&p, base, out, seen);
        } else if let Ok(rel) = p.strip_prefix(base) {
            if !p
                .vcanonicalize()
                .is_ok_and(|p| p.starts_with(base.vcanonicalize().unwrap_or_else(|_| base.into())))
            {
                continue;
            }
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((rel.to_string_lossy().replace('\\', "/"), size));
        }
    }
}

// The ledger reversed: file path -> the nodes bound to it.
fn ownership(ledger: &Ledger) -> BTreeMap<String, Value> {
    let mut by_file: BTreeMap<String, (Vec<String>, Vec<String>, Vec<String>)> = BTreeMap::new();
    for (eid, ent) in &ledger.entities {
        for f in &ent.files {
            by_file.entry(f.clone()).or_default().0.push(eid.clone());
        }
    }
    for (rid, row) in &ledger.requirements {
        for f in &row.files {
            by_file.entry(f.clone()).or_default().1.push(rid.clone());
        }
        // A programmatic test artifact lives under the deliverable too.
        if row.test.kind == "programmatic" && !row.test.artifact.is_empty() {
            by_file
                .entry(row.test.artifact.clone())
                .or_default()
                .2
                .push(rid.clone());
        }
    }
    by_file
        .into_iter()
        .map(|(f, (mut e, mut r, mut t))| {
            e.dedup();
            r.dedup();
            t.dedup();
            (f, json!({ "entities": e, "requirements": r, "tests": t }))
        })
        .collect()
}

fn no_owners() -> Value {
    json!({ "entities": [], "requirements": [], "tests": [] })
}

// `GET /api/deliverable`: every product file with its owners, and the recorded
// medium and build, the product form page's source (docs/frontends/gui.md#api).
pub fn deliverable_value(out_dir: &Path, gs: &GenSettings) -> Value {
    let mut files: Vec<(String, u64)> = Vec::new();
    walk(&gs.deliverable, &gs.deliverable, &mut files);
    files.retain(|(f, _)| gs.allows_source(f));
    files.sort();
    let ledger = Ledger::load(out_dir);
    let own = ownership(&ledger);
    let empty = no_owners();
    json!({
        "root": gs.deliverable.to_string_lossy(),
        "medium": ledger.medium,
        "build": ledger.build,
        "files": files
            .into_iter()
            .map(|(path, size)| {
                let o = own.get(&path).unwrap_or(&empty);
                json!({ "path": path, "size": size, "owners": o })
            })
            .collect::<Vec<_>>(),
    })
}

// The lexical half of the validation: relative, forward slashes, no empty, dot,
// dot-dot, or disallowed hidden component. A path that passes this but names nothing is
// absence (404), not an invalid path (400).
pub(crate) fn lexical_ok(rel: &str) -> bool {
    crate::document_sets::allowed_components(rel)
}

// Resolve a client path strictly inside the given root directory, to a path that
// exists. Natively the canonical form fails for a missing path; the in-memory tree
// canonicalizes any path, so existence is asked for explicitly.
pub(crate) fn safe_under(root_dir: &Path, rel: &str) -> Option<PathBuf> {
    if !lexical_ok(rel) {
        return None;
    }
    let abs = root_dir.join(rel);
    let root = root_dir.vcanonicalize().ok()?;
    let canon = abs.vcanonicalize().ok()?;
    if !canon.starts_with(&root) || !canon.vexists() {
        return None;
    }
    Some(canon)
}

// Resolved sites for one file: the ledger's anchored sites located against the
// current text (exact, moved, or lost), plus each programmatic test whose artifact
// is this file, located by its embedded test name. Mirrors
// docs/frontends/gui.md#api (deliverable) and docs/consumers/gen.md#traceability.
fn sites(store: &Store, ledger: &Ledger, path: &str, text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for (rid, row) in &ledger.requirements {
        let exists = store.graph.requirements.contains_key(store.resolve_id(rid));
        for s in &row.sites {
            if s.file != path {
                continue;
            }
            let located = crate::gen::locate_head(text, &s.head, s.line);
            out.push(json!({
                "line": located.map(|(l, _)| l),
                "requirement": rid,
                "kind": "site",
                "located": match located {
                    Some((_, true)) => "exact",
                    Some(_) => "moved",
                    None => "lost",
                },
                "exists": exists,
            }));
        }
        if row.test.kind == "programmatic" && row.test.artifact == path && !row.test.name.is_empty()
        {
            let line = text
                .lines()
                .position(|l| l.contains(&row.test.name))
                .map(|i| i + 1);
            out.push(json!({
                "line": line,
                "requirement": rid,
                "kind": "test",
                "located": if line.is_some() { "exact" } else { "lost" },
                "exists": exists,
            }));
        }
    }
    out
}

// `GET /api/deliverable/file?path=`: one file's text with its owners and resolved
// sites, or its size when it is not text. A path that leaves the deliverable
// (lexically, or through a symlink that exists) is 400; one that validates but
// names nothing is 404.
pub fn deliverable_file(out_dir: &Path, gs: &GenSettings, path: &str) -> Result<Value, Refusal> {
    let Some(abs) = safe_under(&gs.deliverable, path) else {
        if !lexical_ok(path) || gs.deliverable.join(path).vexists() {
            return Err((400, format!("invalid deliverable path {}", path)));
        }
        return Err((404, format!("no file {}", path)));
    };
    if abs.vis_dir() {
        return Err((404, format!("{} is a directory", path)));
    }
    let bytes =
        crate::vfs::read(&abs).map_err(|e| (404, format!("cannot read {}: {}", path, e)))?;
    let ledger = Ledger::load(out_dir);
    let owners = ownership(&ledger).remove(path).unwrap_or_else(no_owners);
    match String::from_utf8(bytes) {
        Ok(text) => {
            let store = Store::load(out_dir);
            let resolved = sites(&store, &ledger, path, &text);
            Ok(json!({ "path": path, "text": text, "sites": resolved, "owners": owners }))
        }
        Err(e) => Ok(json!({
            "path": path,
            "binary": true,
            "size": e.as_bytes().len(),
            "owners": owners,
        })),
    }
}

// `GET /api/deliverable/raw?path=`: one product file's bytes, what the overview
// renders a built artifact from and downloads. The same confinement as the file
// read. Mirrors docs/frontends/gui.md#api (deliverable).
pub fn deliverable_raw(gs: &GenSettings, path: &str) -> Result<Vec<u8>, Refusal> {
    let Some(abs) = safe_under(&gs.deliverable, path) else {
        if !lexical_ok(path) || gs.deliverable.join(path).vexists() {
            return Err((400, format!("invalid deliverable path {}", path)));
        }
        return Err((404, format!("no file {}", path)));
    };
    if abs.vis_dir() {
        return Err((404, format!("{} is a directory", path)));
    }
    crate::vfs::read(&abs).map_err(|e| (404, format!("cannot read {}: {}", path, e)))
}

// `GET /api/deliverable/baseline?path=`: the file as it stood before the last
// generation run rewrote it, from the snapshot generation takes at write time. 404
// when generation never rewrote the file. Mirrors docs/frontends/gui.md#api
// (deliverable) and docs/consumers/gen.md#incremental-regeneration.
pub fn deliverable_baseline(out_dir: &Path, path: &str) -> Result<Value, Refusal> {
    let root = crate::layout::deliverable_baseline_dir(out_dir);
    let missing = || (404, format!("no baseline for {}", path));
    let abs = safe_under(&root, path).ok_or_else(missing)?;
    let bytes = crate::vfs::read(&abs).map_err(|_| missing())?;
    match String::from_utf8(bytes) {
        Ok(text) => Ok(json!({ "path": path, "text": text })),
        Err(e) => Ok(json!({ "path": path, "binary": true, "size": e.as_bytes().len() })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_deliv(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jazyk-webapi-deliv-{}-{}",
            name,
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("product/src")).unwrap();
        crate::vfs::write(dir.join("product/src/lib.rs"), "fn main() {}\n").unwrap();
        crate::vfs::write(dir.join("product/.env"), "SECRET=1\n").unwrap();
        crate::vfs::write(dir.join("outside.txt"), "outside\n").unwrap();
        dir
    }

    // The lexical rule separates a refusal (400) from an absence (404): a
    // traversal, a hidden component, or an absolute path never validates; a
    // well-formed path that names nothing simply does not resolve.
    #[test]
    fn deliverable_paths_are_confined_and_absence_is_distinct() {
        let dir = temp_deliv("confine");
        let root = dir.join("product");
        assert!(safe_under(&root, "src/lib.rs").is_some());
        assert!(safe_under(&root, "../outside.txt").is_none());
        assert!(!lexical_ok("../outside.txt"));
        assert!(safe_under(&root, ".env").is_none());
        assert!(!lexical_ok(".env"));
        assert!(safe_under(&root, "/etc/passwd").is_none());
        assert!(safe_under(&root, "src\\lib.rs").is_none());
        // Missing but well formed: lexically fine, resolves to nothing.
        assert!(lexical_ok("src/missing.rs"));
        assert!(safe_under(&root, "src/missing.rs").is_none());
        assert!(!root.join("src/missing.rs").vexists());
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // The listing skips hidden entries and the compiler's own output directories.
    #[test]
    fn listing_skips_hidden_and_output_directories() {
        let dir = temp_deliv("list");
        let root = dir.join("product");
        crate::vfs::create_dir_all(root.join(".jazyk/graph")).unwrap();
        crate::vfs::write(root.join(".jazyk/graph/entities.yaml"), "x").unwrap();
        crate::vfs::create_dir_all(root.join("target")).unwrap();
        crate::vfs::write(root.join("target/bin"), "x").unwrap();
        let mut files = Vec::new();
        walk(&root, &root, &mut files);
        files.sort();
        assert_eq!(
            files.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            vec!["src/lib.rs"]
        );
        crate::vfs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn source_scaffolding_is_listed_and_read_without_exposing_other_hidden_paths() {
        let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let root = Path::new("/project");
        let out = root.join(".jazyk/ds_main");
        crate::vfs::create_dir_all(root.join(".jazyk")).unwrap();
        crate::vfs::write(root.join(crate::document_sets::CONFIG), "version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Site'\ndocs = ['docs/**']\nsource = ['deliverable/**']\n").unwrap();
        let project = crate::document_sets::Workspace::load(root)
            .unwrap()
            .resolve(Some("ds_main"))
            .unwrap();
        let gs = GenSettings::resolve(&project);
        for path in [
            "deliverable/.github/workflows/deploy.yml",
            "deliverable/.gitignore",
            "deliverable/.env",
            "deliverable/.github/.aws/config",
        ] {
            crate::vfs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            crate::vfs::write(root.join(path), "fixture\n").unwrap();
        }
        let files = deliverable_value(&out, &gs);
        let paths: Vec<_> = files["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            [
                "deliverable/.github/workflows/deploy.yml",
                "deliverable/.gitignore"
            ]
        );
        for path in paths {
            assert_eq!(
                deliverable_file(&out, &gs, path).unwrap()["text"],
                "fixture\n"
            );
            assert_eq!(deliverable_raw(&gs, path).unwrap(), b"fixture\n");
        }
        for path in [
            "deliverable/.env",
            "deliverable/.github/.aws/config",
            "deliverable/.gitignore/child",
        ] {
            assert_eq!(deliverable_file(&out, &gs, path).unwrap_err().0, 400);
            assert_eq!(deliverable_raw(&gs, path).unwrap_err().0, 400);
        }
        // This lexical helper also serves generated report files. Its callers
        // retain the existing ability to read non-hidden build/cache paths.
        assert!(lexical_ok("target/reports/check.xml"));
    }
}
