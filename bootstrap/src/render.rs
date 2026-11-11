use crate::vfs::VPathExt;
// Diagrams: one emitter per view kind, each a pure function of the store snapshot and
// the view, producing PlantUML text; lifting and collapse over the containment tree;
// over-limit auto-collapse; the render seam; the files under <out>/diagrams/.
// Mirrors docs/compiler/diagrams.md.
use crate::ids::entity_slug;
use crate::model::*;
use crate::store::Store;
use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct RenderError(pub String);

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RenderError {}

mod flow;
mod renderer;
mod scene;
mod structural;
#[cfg(test)]
mod tests;
mod text;

pub use flow::view_messages;
use flow::{
    emit_activity, emit_communication, emit_overview, emit_sequence, emit_state, emit_timing,
    emit_use_case,
};
pub use renderer::{render_png, render_svg};
pub use scene::view_relationships;
use scene::{scene, Scene};
use structural::{
    emit_class, emit_component, emit_composite, emit_deployment, emit_object, emit_package,
};
pub use text::card_link;
use text::empty_diagram;

// ---- dispatch ----

// The PlantUML text of a view over the snapshot. Deterministic: the same snapshot and
// view produce byte-identical output. Mirrors docs/compiler/diagrams.md#the-emitters.
pub fn emit(store: &Store, view_id: &str, view: &View) -> String {
    let scene = scene(store, view_id, view);
    let puml = emit_kind(&scene);
    // A diagram with nothing in it is not a diagram: draw the fact instead.
    let blank = puml
        .lines()
        .all(|l| l == "@startuml" || l == "@enduml" || l.starts_with("title "));
    if blank {
        return empty_diagram(view, "no members");
    }
    puml
}

fn emit_kind(scene: &Scene) -> String {
    let view = scene.view;
    match view.kind.as_str() {
        "class" => emit_class(scene),
        "object" => emit_object(scene),
        "package" => emit_package(scene),
        "component" => emit_component(scene),
        "composite" => emit_composite(scene),
        "deployment" => emit_deployment(scene),
        "use-case" => emit_use_case(scene),
        "activity" => emit_activity(scene),
        "state" => emit_state(scene),
        "sequence" => emit_sequence(scene),
        "communication" => emit_communication(scene),
        "timing" => emit_timing(scene),
        "overview" => emit_overview(scene),
        other => empty_diagram(view, &format!("view kind {} has no emitter", other)),
    }
}

// ---- output ----

// Every view, plus a state view per machine that has none. Sorted by id.
fn render_targets(store: &Store) -> Vec<(String, View)> {
    let mut out: Vec<(String, View)> = store
        .graph
        .views
        .iter()
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    for m in store.graph.state_machines.values() {
        let id = format!("view:state/{}", entity_slug(&m.subject));
        if store.graph.views.contains_key(&id) {
            continue;
        }
        let Some(subject) = store.graph.entities.get(&m.subject) else {
            continue;
        };
        out.push((
            id,
            View {
                kind: "state".to_string(),
                title: subject.name.clone(),
                members: vec![m.subject.clone()],
                ..Default::default()
            },
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn target(store: &Store, view_id: &str) -> Option<View> {
    render_targets(store)
        .into_iter()
        .find(|(id, _)| id == view_id)
        .map(|(_, v)| v)
}

// `<out>/diagrams/<kind>/<slug>` for a view id, without extension. None for an id that
// does not spell a kind and a slug.
fn diagram_base(out: &Path, view_id: &str) -> Option<PathBuf> {
    let rel = view_id.strip_prefix("view:")?;
    let (kind, slug) = rel.split_once('/')?;
    let clean = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    };
    if !clean(kind) || !clean(slug) {
        return None;
    }
    Some(crate::layout::diagrams_dir(out).join(kind).join(slug))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderReport {
    pub rendered: Vec<String>,
    pub skipped: Vec<String>,
    // (view id, error)
    pub failed: Vec<(String, String)>,
    // `<kind>/<slug>` of renders whose view is gone.
    pub removed: Vec<String>,
}

fn remove_if_present(path: &Path) {
    if path.vexists() {
        let _ = crate::vfs::remove_file(path);
    }
}

// Emit and render every view into <out>/diagrams/, skipping the renderer where the
// .puml on disk already equals the emission and its .svg exists, removing the files of
// views that are gone. A render error keeps the .puml, drops the stale .svg, and is
// reported. Mirrors docs/compiler/diagrams.md#rendering and #output-layout.
pub fn render_all(store: &Store, out: &Path) -> RenderReport {
    let mut report = RenderReport::default();
    // A newer store's output is never written (docs/compiler/graph.md#store-version).
    if crate::store::newer_version(out).is_some() {
        return report;
    }
    let mut keep: BTreeSet<PathBuf> = BTreeSet::new();
    for (id, view) in render_targets(store) {
        let Some(base) = diagram_base(out, &id) else {
            report
                .failed
                .push((id, "the id spells no kind and slug".to_string()));
            continue;
        };
        keep.insert(base.clone());
        let puml = emit(store, &id, &view);
        let puml_path = base.with_extension("puml");
        let svg_path = base.with_extension("svg");
        let png_path = base.with_extension("png");
        let unchanged = svg_path.vexists()
            && crate::vfs::read_to_string(&puml_path)
                .ok()
                .is_some_and(|on_disk| on_disk == puml);
        if unchanged {
            report.skipped.push(id);
            continue;
        }
        if let Some(dir) = base.parent() {
            if let Err(e) = crate::vfs::create_dir_all(dir) {
                report.failed.push((id, e.to_string()));
                continue;
            }
        }
        if let Err(e) = crate::vfs::write(&puml_path, &puml) {
            report.failed.push((id, e.to_string()));
            continue;
        }
        remove_if_present(&png_path);
        match render_svg(&puml) {
            Ok(svg) => match crate::vfs::write(&svg_path, svg) {
                Ok(()) => report.rendered.push(id),
                Err(e) => report.failed.push((id, e.to_string())),
            },
            Err(e) => {
                remove_if_present(&svg_path);
                report.failed.push((id, e.0));
            }
        }
    }
    let dir = crate::layout::diagrams_dir(out);
    let Ok(kinds) = crate::vfs::read_dir(&dir) else {
        return report;
    };
    for kind in kinds.flatten() {
        let kind_dir = kind.path();
        if !kind_dir.vis_dir() {
            continue;
        }
        let Ok(files) = crate::vfs::read_dir(&kind_dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !["puml", "svg", "png"].contains(&ext) {
                continue;
            }
            let base = path.with_extension("");
            if keep.contains(&base) {
                continue;
            }
            let _ = crate::vfs::remove_file(&path);
            let rel = format!(
                "{}/{}",
                kind.file_name().to_string_lossy(),
                base.file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default()
            );
            if !report.removed.contains(&rel) {
                report.removed.push(rel);
            }
        }
        if crate::vfs::read_dir(&kind_dir).is_ok_and(|mut d| d.next().is_none()) {
            let _ = crate::vfs::remove_dir(&kind_dir);
        }
    }
    report
}

// The PlantUML and SVG of one view, for surfaces that draw without touching the files.
pub fn render_view(store: &Store, view_id: &str) -> Result<(String, String), RenderError> {
    let view = target(store, view_id).ok_or_else(|| RenderError(format!("no view {}", view_id)))?;
    let puml = emit(store, view_id, &view);
    match render_svg(&puml) {
        Ok(svg) => Ok((puml, svg)),
        Err(e) => match rendered_on_disk(&store.out, view_id, &puml) {
            Some(svg) => Ok((puml, svg)),
            None => Err(e),
        },
    }
}

// The rendering the commit wrote, when it is this exact picture: the `.puml` beside
// it equals the emission. A tab cannot render, so this is how a seeded project's
// diagrams show there (docs/frontends/wasm.md#diagrams).
fn rendered_on_disk(out: &Path, view_id: &str, puml: &str) -> Option<String> {
    let base = diagram_base(out, view_id)?;
    let on_disk = crate::vfs::read_to_string(base.with_extension("puml")).ok()?;
    if on_disk != puml {
        return None;
    }
    crate::vfs::read_to_string(base.with_extension("svg")).ok()
}

// The .png of a rendered view, rasterized on first request and kept beside the .svg.
pub fn png_for(out: &Path, view_id: &str) -> Result<PathBuf, RenderError> {
    let base =
        diagram_base(out, view_id).ok_or_else(|| RenderError(format!("no view {}", view_id)))?;
    let png_path = base.with_extension("png");
    if png_path.vexists() {
        return Ok(png_path);
    }
    let svg = crate::vfs::read_to_string(base.with_extension("svg"))
        .map_err(|_| RenderError(format!("{} is not rendered", view_id)))?;
    let png = render_png(&svg)?;
    crate::vfs::write(&png_path, png).map_err(|e| RenderError(e.to_string()))?;
    Ok(png_path)
}
