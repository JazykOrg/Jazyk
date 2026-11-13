use super::Lsp;
use crate::gen::{artifact_path, locate_head, Ledger};
use serde_json::{json, Value};
use std::collections::BTreeSet;

impl Lsp {
    // Resolve only recorded anchors, against the current editor buffer. No syntax
    // inference or file-wide attribution. Lines returned here are zero-based.
    pub(super) fn code_requirements(&self, doc: &str) -> Vec<(String, usize)> {
        let path = self.root.join(doc);
        let text = self
            .overlay
            .get(doc)
            .cloned()
            .unwrap_or_else(|| crate::vfs::read_to_string(&path).unwrap_or_default());
        let mut found = BTreeSet::new();
        let mut collect = |out: &std::path::Path,
                           gen: &crate::gen::GenSettings,
                           set: Option<&str>| {
            for (rid, row) in Ledger::load(out).requirements {
                let rid =
                    set.map_or_else(|| rid.clone(), |id| crate::webapi::sets::qualify(&rid, id));
                if !self.store.graph.requirements.contains_key(&rid) {
                    continue;
                }
                for site in &row.sites {
                    if gen.deliverable.join(&site.file) == path {
                        if let Some((line, _)) = locate_head(&text, &site.head, site.line) {
                            found.insert((rid.clone(), line - 1));
                        }
                    }
                }
                for test in std::iter::once(&row.test).chain(row.tests.values().map(|t| &t.test)) {
                    if test.kind == "programmatic"
                        && !test.name.is_empty()
                        && artifact_path(out, gen, test) == path
                    {
                        if let Some(line) = text.lines().position(|l| l.contains(&test.name)) {
                            found.insert((rid.clone(), line));
                        }
                    }
                }
            }
        };
        if self.serves_one_set() {
            collect(&self.out, &self.gen, None);
        } else if let Some(workspace) = crate::document_sets::Workspace::load(&self.root)
            .ok()
            .filter(|w| w.exists)
        {
            for set in &workspace.sets {
                if let Ok(project) = workspace.resolve(Some(&set.id)) {
                    collect(
                        &project.out,
                        &crate::gen::GenSettings::resolve(&project),
                        Some(&set.id),
                    );
                }
            }
        } else {
            collect(&self.out, &self.gen, None);
        }
        found.into_iter().collect()
    }

    pub(super) fn requirement_document_location(&self, rid: &str) -> Option<Value> {
        let (set, local) = crate::webapi::sets::unqualify(rid);
        let out = match set {
            Some(id) => crate::project::Project::load_set(&self.root, &id).ok()?.out,
            None => self.out.clone(),
        };
        let r = self.store.graph.requirements.get(rid)?;
        let entity = self.store.resolve_id(r.entities.first()?);
        let (_, entity) = crate::webapi::sets::unqualify(entity);
        let path = crate::layout::docsgen_dir(&out)
            .join(format!("{}.md", crate::ids::entity_slug(&entity)));
        let text = crate::vfs::read_to_string(&path).ok()?;
        let heading = format!("### `{}`", local);
        let line = text.lines().position(|l| l.trim() == heading)?;
        Some(json!({"uri": super::document::path_to_uri(&path),
            "range": self.range((line, 0, line, 0))}))
    }

    pub(super) fn code_definition(&self, doc: &str, line: usize) -> Option<Value> {
        let targets: Vec<Value> = self.code_requirements(doc).into_iter()
            .filter(|(_, l)| *l == line)
            .filter_map(|(rid, _)| {
                let r = self.store.graph.requirements.get(&rid)?;
                if let Some(source) = r.source.as_ref() {
                    if let Some(range) = crate::md::locate(&self.doc_text(&source.doc), &source.quote) {
                        return Some(json!({"uri": self.doc_to_uri(&source.doc), "range": self.range(range)}));
                    }
                }
                self.requirement_document_location(&rid)
            }).collect();
        (!targets.is_empty()).then(|| json!(targets))
    }
}
