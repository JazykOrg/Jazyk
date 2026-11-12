//! Source and delivery projections. Mirrors docs/compiler/hosted-runtime.md#source-and-delivery-projections.
use super::{root, Projection, Snapshot};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(super) fn project_projection(
    files: &Snapshot,
    authored: &[String],
) -> Result<Projection, String> {
    let w = crate::document_sets::Workspace::load(&root()).map_err(|e| e.1)?;
    let mut combined = Projection {
        source: Default::default(),
        files: Default::default(),
        criteria: Default::default(),
        symlinks: Default::default(),
        graph_generation: 0,
    };
    let contexts: Vec<_> = if w.sets.is_empty() {
        vec![w.files_project()]
    } else {
        w.sets
            .iter()
            .map(|s| w.resolve(Some(&s.id)).unwrap())
            .collect()
    };
    for p in contexts {
        let projected = set_projection(files, authored, &p)?;
        combined.source.extend(projected.source);
        combined.files.extend(projected.files);
        combined.symlinks.extend(projected.symlinks);
        for (id, v) in projected.criteria {
            let id = p
                .set_id
                .as_deref()
                .map(|set| crate::webapi::sets::qualify(&id, set))
                .unwrap_or(id);
            combined.criteria.insert(
                id,
                p.set_id
                    .as_deref()
                    .map(|set| crate::webapi::sets::qualify_value(v.clone(), set))
                    .unwrap_or(v),
            );
        }
        combined.graph_generation += projected.graph_generation;
    }
    Ok(combined)
}
fn set_projection(
    files: &Snapshot,
    authored: &[String],
    p: &crate::project::Project,
) -> Result<Projection, String> {
    use crate::vfs::VPathExt;
    let store = crate::store::Store::load(&p.out);
    let gs = crate::gen::GenSettings::resolve(p);
    let ledger = crate::gen::Ledger::load(&p.out);
    // Resolve through the VFS so redirected projects and dot components use the
    // same paths as compilation. No absent or out-of-workspace file can enter.
    let relative = |path: &Path| -> Option<String> {
        if !path.vis_file() {
            return None;
        }
        let resolved = crate::vfs::canonicalize(path).ok()?;
        if !resolved.starts_with(root()) {
            return None;
        }
        Some(path.strip_prefix(root()).ok()?.to_str()?.replace('\\', "/"))
    };
    let internal = |name: &str| {
        !matches!(name, crate::document_sets::CONFIG | ".jazyk/.gitignore")
            && (name
                .split('/')
                .any(|c| c == ".git" || c.starts_with(".jazyk") || c.starts_with("jazyk-out"))
                || root().join(name).starts_with(&p.out))
    };
    let mut owned = BTreeSet::new();
    let mut accepted = BTreeSet::new();
    for path in ledger
        .support
        .iter()
        .chain(ledger.entities.values().flat_map(|e| e.files.iter()))
        .chain(ledger.requirements.values().flat_map(|r| r.files.iter()))
    {
        if let Some(name) = relative(&gs.deliverable.join(path)) {
            owned.insert(name.clone());
            accepted.insert(name);
        }
    }
    for row in ledger.requirements.values() {
        for test in std::iter::once(&row.test).chain(row.tests.values().map(|t| &t.test)) {
            if let Some(name) = relative(&crate::gen::artifact_path(&p.out, &gs, test)) {
                owned.insert(name.clone());
                if test.kind == "programmatic" {
                    accepted.insert(name);
                }
            }
        }
    }
    for path in crate::gen::build_outputs(&ledger) {
        if let Some(name) = relative(&gs.deliverable.join(path)) {
            owned.insert(name.clone());
            accepted.remove(&name);
        }
    }
    let mut source_paths = BTreeSet::new();
    for name in authored {
        if name.is_empty()
            || name.contains(['\\', '\0', ':'])
            || name
                .split('/')
                .any(|c| c.is_empty() || c == "." || c == "..")
        {
            return Err(format!("invalid authored path: {name}"));
        }
        if files.contains_key(name) {
            source_paths.insert(name.clone());
        }
    }
    for path in p.doc_files().into_iter().chain([
        root().join(crate::document_sets::CONFIG),
        p.root.join(crate::document_sets::CONFIG),
    ]) {
        if let Some(name) = relative(&path) {
            source_paths.insert(name);
        }
    }
    source_paths.retain(|name| !internal(name) && !owned.contains(name));
    let read_text = |names: BTreeSet<String>| -> Result<BTreeMap<String, String>, String> {
        names
            .into_iter()
            .filter(|name| !internal(name))
            .filter_map(|name| {
                let Some(bytes) = files.get(&name) else {
                    return Some(Err(format!("missing projected file: {name}")));
                };
                // Binary inputs and assets remain byte-for-byte in the snapshot.
                // These maps are only the host's text projections.
                std::str::from_utf8(bytes)
                    .ok()
                    .map(|content| Ok((name, content.to_owned())))
            })
            .collect()
    };
    let source = read_text(source_paths)?;
    let mut candidate = source.clone();
    candidate.extend(read_text(accepted)?);
    // The set's committed compiler state rides with the delivery, so the receiving
    // repository compiles incrementally from it. Local state stays in the snapshot.
    // Mirrors docs/compiler/hosted-runtime.md#source-and-delivery-projections.
    let out_prefix = p
        .out
        .strip_prefix(root())
        .ok()
        .and_then(|o| o.to_str())
        .map(|o| format!("{}/", o.replace('\\', "/")));
    if let Some(text) = files
        .get(".jazyk/.gitignore")
        .and_then(|b| std::str::from_utf8(b).ok())
    {
        candidate.insert(".jazyk/.gitignore".into(), text.to_owned());
    }
    if let Some(prefix) = out_prefix {
        for (name, bytes) in files.range(prefix.clone()..) {
            if !name.starts_with(&prefix) {
                break;
            }
            if crate::document_sets::committed(name) {
                let content = std::str::from_utf8(bytes)
                    .map_err(|_| format!("projected file is not UTF-8 text: {name}"))?;
                candidate.insert(name.clone(), content.to_owned());
            }
        }
    }
    let criteria = store
        .graph
        .criteria
        .iter()
        .map(|(id, criterion)| {
            serde_json::to_value(criterion)
                .map(|value| (id.clone(), value))
                .map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    let symlinks = candidate
        .keys()
        .filter(|path| files.is_symlink(path))
        .cloned()
        .collect();
    Ok(Projection {
        source,
        symlinks,
        files: candidate,
        criteria,
        graph_generation: store.status.generation,
    })
}
