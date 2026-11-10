use super::*;

// Requirements per generation part for dense entities.
pub const GROUP: usize = 20;

// Resolved [gen] settings: where the deliverable lives. Nothing else: what the
// deliverable is comes from the documents through the graph, never from configuration.
// Mirrors docs/compiler/project-settings.md#generation.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct GenSettings {
    // Whether the project names a deliverable: the generation scope exists
    // (docs/compiler/goals/derive-criteria.md#created-when).
    pub configured: bool,
    pub deliverable: PathBuf,
    // The built-in worker: "agentic" (a generation turn with file and command tools,
    // the default) or "pipeline" (the fixed file-reply sequence).
    // Mirrors docs/compiler/project-settings.md#generation.
    pub worker: String,
    // Globs scoping which deliverable files count as implementation for the unclaimed
    // report and decompilation. Empty: everything minus the standard exclusions.
    pub code: Vec<String>,
}

impl GenSettings {
    pub(crate) fn current_for_store(&self, store: &Store) -> Self {
        let Some(root) = crate::project::find_root(&store.out) else {
            return self.clone();
        };
        let workspace = match crate::document_sets::Workspace::load(&root) {
            Ok(workspace) if workspace.exists => workspace,
            Ok(_) => return self.clone(),
            Err(_) => {
                return Self {
                    configured: false,
                    code: vec![],
                    ..self.clone()
                }
            }
        };
        // Match the selected store, never the project-wide union of Source roles.
        workspace
            .sets
            .iter()
            .filter_map(|set| workspace.resolve(Some(&set.id)).ok())
            .find(|project| project.out == store.out)
            .map(|project| Self::resolve(&project))
            .unwrap_or_else(|| Self {
                configured: false,
                code: vec![],
                ..self.clone()
            })
    }
    pub fn resolve(proj: &crate::project::Project) -> GenSettings {
        let deliverable = match &proj.gen_deliverable {
            Some(d) => proj.root.join(d),
            // Default: the project root itself. The docs glob keeps the source tree
            // out of the product's way (docs/compiler/project-settings.md#generation).
            None => proj.root.clone(),
        };
        GenSettings {
            configured: proj.gen_deliverable.is_some(),
            deliverable,
            worker: proj.gen_worker.clone().unwrap_or_else(|| "agentic".into()),
            code: proj.gen_code.clone(),
        }
    }

    // Placeholder for sessions with no project (benchmark cases). Gen tools are absent
    // from those toolsets, so the path is never read.
    pub fn allows_source(&self, relative: &str) -> bool {
        let Ok(w) = crate::document_sets::Workspace::load(&self.deliverable) else {
            return false;
        };
        if !w.exists {
            return true;
        }
        self.configured
            && crate::document_sets::safe_relative(&self.deliverable, relative)
            && crate::document_sets::included(&self.code, relative)
            && !w
                .sets
                .iter()
                .any(|s| crate::document_sets::included(&s.docs, relative))
    }
    pub fn from_out(out: &Path) -> GenSettings {
        GenSettings {
            configured: false,
            deliverable: out.join("gen").join("deliverable"),
            worker: "agentic".into(),
            code: Vec::new(),
        }
    }
}
