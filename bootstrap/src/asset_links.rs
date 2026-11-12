//! Direct relative file links from deliverables to supplied assets.
use crate::vfs::VPathExt;
use std::path::{Path, PathBuf};

pub fn selected_asset(project: &crate::project::Project, path: &str) -> bool {
    crate::document_sets::safe_relative(&project.root, path)
        && project.doc_files().contains(&project.root.join(path))
        && (!crate::md::supported_format(Path::new(path))
            || crate::input::decisions(project)
                .get(path)
                .is_some_and(|d| d.mode == "asset"))
}

pub fn target(root: &Path, path: &str) -> Result<(String, String), String> {
    if !crate::task_runtime::safe_path(path) || !crate::document_sets::safe_relative(root, path) {
        return Err("link path is outside the eligible project files".into());
    }
    let file = root.join(path);
    let raw =
        crate::vfs::read_link(&file).map_err(|e| format!("cannot inspect link {path}: {e}"))?;
    if raw.is_absolute() || raw.to_string_lossy().contains('\\') {
        return Err("asset links must have relative targets".into());
    }
    let raw = raw.to_str().ok_or("link target is not UTF-8")?.to_string();
    let resolved = crate::md::resolve_rel(path, &raw).ok_or("link target leaves the project")?;
    let destination = root.join(&resolved);
    if crate::vfs::is_symlink(&destination)
        || !destination.vis_file()
        || !crate::document_sets::safe_relative(root, &resolved)
    {
        return Err("asset target is missing, unsafe, or another symlink".into());
    }
    let workspace = crate::document_sets::Workspace::load(root).map_err(|e| e.1)?;
    let allowed = workspace
        .sets
        .iter()
        .filter_map(|set| workspace.resolve(Some(&set.id)).ok())
        .any(|project| {
            crate::gen::GenSettings::resolve(&project).allows_source(path)
                && selected_asset(&project, &resolved)
        });
    if !allowed {
        return Err("link must connect selected Source to a selected asset in the same set".into());
    }
    Ok((raw, resolved))
}

pub fn relative_target(path: &str, target: &str) -> PathBuf {
    let parent: Vec<_> = path.split('/').collect();
    let parent = &parent[..parent.len().saturating_sub(1)];
    let target: Vec<_> = target.split('/').collect();
    let common = parent
        .iter()
        .zip(&target)
        .take_while(|(a, b)| a == b)
        .count();
    let mut relative = PathBuf::new();
    for _ in common..parent.len() {
        relative.push("..");
    }
    for part in &target[common..] {
        relative.push(part);
    }
    relative
}
