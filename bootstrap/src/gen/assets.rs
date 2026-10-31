//! Local inputs named by authored facts. See docs/consumers/gen.md#referenced-assets.
use crate::model::{Provenance, ProvenanceRef, SourceRef};
use crate::store::Store;
use crate::vfs::VPathExt;
use pulldown_cmark::{Event, Parser, Tag};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::Path;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Reference {
    pub fact: String,
    pub doc: String,
    pub section: String,
    pub href: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReferencedAsset {
    pub path: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub references: BTreeSet<Reference>,
}

impl ReferencedAsset {
    fn new(path: String, status: &'static str, message: Option<&str>) -> Self {
        Self {
            path,
            status,
            sha256: None,
            message: message.map(String::from),
            references: BTreeSet::new(),
        }
    }
    pub(super) fn is_input(&self) -> bool {
        self.status != "output"
    }
}

struct Document {
    text: String,
    sections: BTreeMap<String, crate::model::Section>,
    links: Vec<(String, Range<usize>)>,
}

impl Document {
    fn read(path: &Path) -> Option<Self> {
        let text = crate::md::read_doc(path).ok()?;
        let sections = crate::md::parse_sections(&text);
        let links = Parser::new(&text)
            .into_offset_iter()
            .filter_map(|(event, range)| {
                let href = match event {
                    Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                        dest_url.into_string()
                    }
                    _ => return None,
                };
                Some((href, range))
            })
            .collect();
        Some(Self {
            text,
            sections,
            links,
        })
    }

    fn quoted_links(&self, source: &SourceRef) -> Vec<&str> {
        let Some(section) = self.sections.get(&source.section) else {
            return vec![];
        };
        let start = self
            .text
            .split_inclusive('\n')
            .take(section.lines[0])
            .map(str::len)
            .sum::<usize>();
        let end = self
            .text
            .split_inclusive('\n')
            .take(section.lines[1])
            .map(str::len)
            .sum::<usize>();
        let Some(section_text) = self.text.get(start..end) else {
            return vec![];
        };
        let Some((a, b)) = crate::md::locate_bytes(section_text, &source.quote) else {
            return vec![];
        };
        let quote = start + a..start + b;
        self.links
            .iter()
            .filter(|(_, range)| range.start < quote.end && range.end > quote.start)
            .map(|(href, _)| href.as_str())
            .collect()
    }
}

// Only the selected set can supply provenance documents and Source roles. Never
// resolve a workspace union or another set merely because its paths overlap.
fn project(store: &Store) -> Option<crate::project::Project> {
    let root = crate::project::find_root(&store.out)?;
    let workspace = crate::document_sets::Workspace::load(&root).ok()?;
    workspace
        .sets
        .iter()
        .filter_map(|set| workspace.resolve(Some(&set.id)).ok())
        .find(|project| project.out == store.out)
}

fn quotes<'a>(store: &'a Store, id: &'a str, rids: &[String]) -> Vec<(&'a str, &'a SourceRef)> {
    fn quoted(provenance: &Option<Provenance>) -> Option<&SourceRef> {
        match provenance {
            Some(Provenance::Quote(source)) => Some(source),
            _ => None,
        }
    }
    let mut out = Vec::new();
    if let Some(entity) = store.graph.entities.get(id) {
        out.extend(entity.mentions.iter().map(|source| (id, source)));
        if let Some(source) = quoted(&entity.provenance) {
            out.push((id, source));
        }
        for attribute in &entity.attributes {
            if let Provenance::Quote(source) = &attribute.provenance {
                out.push((id, source));
            }
        }
    }
    for rid in rids {
        let Some((rid, requirement)) = store.graph.requirements.get_key_value(rid) else {
            continue;
        };
        if let Some(ProvenanceRef::Quote(source)) = requirement.provenance() {
            out.push((rid.as_str(), source));
        }
        for (cid, criterion) in &store.graph.criteria {
            if store.resolve_id(&criterion.requirement) != rid {
                continue;
            }
            if !criterion.entities.is_empty()
                && !criterion
                    .entities
                    .iter()
                    .any(|entity| store.resolve_id(entity) == id)
            {
                continue;
            }
            if let Some(source) = criterion
                .source
                .as_ref()
                .or_else(|| quoted(&criterion.provenance))
            {
                out.push((cid.as_str(), source));
            }
        }
    }
    out
}

// The shared component/credential policy applies to both the spelling and the
// resolved destination, including aliases into a hidden directory under root.
fn readable_path(path: &str) -> bool {
    crate::task_runtime::safe_path(path)
        && crate::document_sets::allowed_components(path)
        && !crate::task_runtime::protected(path)
}

fn resolved_relative(root: &Path, path: &str) -> Option<String> {
    root.join(path)
        .vcanonicalize()
        .ok()?
        .strip_prefix(root.vcanonicalize().ok()?)
        .ok()
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
}

fn contained_path(root: &Path, path: &str) -> bool {
    readable_path(path)
        && crate::document_sets::contained_relative(root, path)
        && resolved_relative(root, path)
            .as_deref()
            .is_none_or(readable_path)
}

enum Target {
    Ignore,
    Local(String),
    Blocked(String),
}

fn target(doc: &str, href: &str) -> Target {
    let raw = href.split(['?', '#']).next().unwrap_or("");
    if raw.is_empty() || raw.starts_with("//") {
        return Target::Ignore;
    }
    if raw.split('/').next().is_some_and(|part| part.contains(':')) {
        return Target::Ignore;
    }
    let bytes = raw.as_bytes();
    if bytes.iter().enumerate().any(|(i, byte)| {
        *byte == b'%'
            && !(bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit))
    }) {
        return Target::Blocked(href.to_string());
    }
    let Ok(decoded) = percent_encoding::percent_decode_str(raw).decode_utf8() else {
        return Target::Blocked(href.to_string());
    };
    if decoded.contains(['\\', ':'])
        || decoded.chars().any(char::is_control)
        || decoded.starts_with("//")
    {
        return Target::Blocked(href.to_string());
    }
    let path = if let Some(path) = decoded.strip_prefix('/') {
        crate::md::resolve_rel("", path)
    } else {
        crate::md::resolve_rel(doc, &decoded)
    };
    let Some(path) = path else {
        return Target::Blocked(href.to_string());
    };
    if decoded.ends_with('/') || matches!(decoded.rsplit('/').next(), Some("." | "..")) {
        return Target::Ignore;
    }
    if !readable_path(&path) {
        return Target::Blocked(path);
    }
    if crate::md::supported_format(Path::new(&path)) {
        return Target::Ignore;
    }
    Target::Local(path)
}

fn output_paths(store: &Store, project: &crate::project::Project) -> BTreeSet<String> {
    let settings = super::GenSettings::resolve(project);
    let ledger = super::Ledger::load(&store.out);
    let mut files = ledger
        .entities
        .values()
        .flat_map(|entity| entity.files.iter().cloned())
        .collect::<Vec<_>>();
    files.extend(ledger.support.iter().cloned());
    files.extend(super::build_outputs(&ledger));
    let mut paths = BTreeSet::new();
    for row in ledger.requirements.values() {
        files.extend(row.files.iter().cloned());
        for test in std::iter::once(&row.test).chain(row.tests.values().map(|test| &test.test)) {
            let path = super::artifact_path(&store.out, &settings, test);
            if let Ok(relative) = path.strip_prefix(&project.root) {
                paths.insert(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    paths.extend(files.into_iter().filter_map(|file| {
        settings
            .deliverable
            .join(file)
            .strip_prefix(&project.root)
            .ok()
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
    }));
    paths
}

fn inspect(
    root: &Path,
    path: &str,
    source: &[String],
    outputs: &BTreeSet<String>,
) -> ReferencedAsset {
    if !contained_path(root, path) {
        return ReferencedAsset::new(path.into(), "blocked", Some("The input must stay inside the project and outside private paths and nested projects."));
    }
    let file = root.join(path);
    let resolved = resolved_relative(root, path);
    let is_output =
        |path: &str| crate::document_sets::included(source, path) || outputs.contains(path);
    if is_output(path) || resolved.as_deref().is_some_and(is_output) {
        return ReferencedAsset::new(path.into(), "output", Some("A Source or recorded generated path is an output reference, not an input dependency."));
    }
    if !crate::document_sets::safe_relative(root, path)
        || resolved
            .as_deref()
            .is_some_and(|path| !crate::document_sets::eligible(path))
    {
        return ReferencedAsset::new(
            path.into(),
            "blocked",
            Some("Build and dependency caches cannot supply authored input assets."),
        );
    }
    match crate::vfs::read(&file) {
        Ok(bytes) => {
            let mut asset = ReferencedAsset::new(path.into(), "ready", None);
            asset.sha256 = Some(crate::task_runtime::digest(&bytes));
            asset
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound =>
            ReferencedAsset::new(path.into(), "missing", Some("The referenced input file is missing. Restore it or correct the source reference; do not invent a substitute.")),
        Err(_) => ReferencedAsset::new(path.into(), "unreadable", Some("The referenced input cannot be read. Resolve it before recording generation.")),
    }
}

pub fn referenced_assets(store: &Store, id: &str, rids: &[String]) -> Vec<ReferencedAsset> {
    let quotes = quotes(store, id, rids);
    // Local Markdown links and images require a bracket. Most authored facts have
    // none, so they keep the old cheap fingerprint without loading project files.
    if !quotes.iter().any(|(_, source)| source.quote.contains('[')) {
        return vec![];
    }
    let Some(project) = project(store) else {
        return vec![];
    };
    let selected_docs = project
        .doc_files()
        .into_iter()
        .filter_map(|path| {
            path.strip_prefix(&project.root)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        })
        .collect::<BTreeSet<_>>();
    let outputs = output_paths(store, &project);
    let mut documents = BTreeMap::<String, Option<Document>>::new();
    let mut assets = BTreeMap::<String, ReferencedAsset>::new();
    for (fact, source) in quotes {
        if !selected_docs.contains(&source.doc) || !contained_path(&project.root, &source.doc) {
            continue;
        }
        let document = documents
            .entry(source.doc.clone())
            .or_insert_with(|| Document::read(&project.root.join(&source.doc)));
        let Some(document) = document else { continue };
        for href in document.quoted_links(source) {
            let (path, blocked) = match target(&source.doc, href) {
                Target::Ignore => continue,
                Target::Local(path) => (path, false),
                Target::Blocked(path) => (path, true),
            };
            if !blocked
                && contained_path(&project.root, &path)
                && project.root.join(&path).vis_dir()
            {
                continue;
            }
            let asset = assets.entry(path.clone()).or_insert_with(|| {
                if blocked {
                    ReferencedAsset::new(
                        path,
                        "blocked",
                        Some("The reference is not a safe project-local input path."),
                    )
                } else {
                    inspect(&project.root, &path, &project.gen_code, &outputs)
                }
            });
            asset.references.insert(Reference {
                fact: fact.into(),
                doc: source.doc.clone(),
                section: source.section.clone(),
                href: href.into(),
            });
        }
    }
    assets.into_values().collect()
}

// Preserve the existing fingerprint exactly for facts without local input refs.
// Do not hash quote wording or offsets; only input identity, selectors and bytes.
pub fn fingerprint(base: &str, assets: &[ReferencedAsset]) -> String {
    let inputs = assets
        .iter()
        .filter(|asset| asset.is_input())
        .map(|asset| {
            let hrefs = asset
                .references
                .iter()
                .map(|reference| &reference.href)
                .collect::<BTreeSet<_>>();
            (&asset.path, asset.status, &asset.sha256, hrefs)
        })
        .collect::<Vec<_>>();
    if inputs.is_empty() {
        base.into()
    } else {
        crate::model::hash_hex(&format!(
            "{base}|referenced-assets:{}",
            serde_json::to_string(&inputs).expect("asset fingerprint serialization")
        ))
    }
}

pub fn ensure_available(assets: &[ReferencedAsset]) -> Result<(), String> {
    let unavailable = assets
        .iter()
        .filter(|asset| !matches!(asset.status, "ready" | "output"))
        .map(|asset| format!("{} ({})", asset.path, asset.status))
        .collect::<Vec<_>>();
    if unavailable.is_empty() {
        Ok(())
    } else {
        Err(format!("Referenced inputs are unavailable: {}. Restore the exact input or correct its quoted source reference; generation cannot substitute an invented asset.", unavailable.join(", ")))
    }
}
