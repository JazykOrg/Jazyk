//! Whole-file input inventory and optional prose projections.
//! See docs/compiler/parsing.md#format-handlers.
use crate::{
    md,
    model::{hash_hex, Section},
    project::Project,
};
use pulldown_cmark::{Event, Parser, Tag};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub type Input = Result<(String, BTreeMap<String, Section>), String>;

// All reads go through VFS, including opaque bytes in browser and hosted projects.
pub fn decisions(proj: &Project) -> BTreeMap<String, crate::model::InputDecision> {
    let dir = proj.out.join("graph/decisions/inputs");
    crate::vfs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry.path().extension().is_some_and(|e| e == "yaml")
                && !entry.file_name().to_string_lossy().starts_with('.')
        })
        .filter_map(|entry| {
            let value: serde_json::Value =
                serde_norway::from_slice(&crate::vfs::read(entry.path()).ok()?).ok()?;
            let id = value["id"].as_str()?.to_string();
            let decision = serde_json::from_value(value).ok()?;
            Some((id, decision))
        })
        .collect()
}

/// A project-level read bypasses parsing only when all claiming sets use assets.
pub fn asset_decision(project: &Project, path: &str) -> Option<crate::model::InputDecision> {
    if project.set_id.is_some() {
        return decisions(project)
            .remove(path)
            .filter(|d| d.mode == "asset");
    }
    let workspace = crate::document_sets::Workspace::load(&project.root).ok()?;
    let mut asset = None;
    for set in &workspace.sets {
        let context = workspace.resolve(Some(&set.id)).ok()?;
        if !context.doc_files().contains(&context.root.join(path)) {
            continue;
        }
        let decision = decisions(&context).remove(path)?;
        if decision.mode != "asset" {
            return None;
        }
        asset = Some(decision);
    }
    asset
}

pub fn project_for_store(store: &crate::store::Store) -> Option<Project> {
    let root = crate::project::find_root(&store.out)?;
    let workspace = crate::document_sets::Workspace::load(&root).ok()?;
    workspace
        .sets
        .iter()
        .filter_map(|set| workspace.resolve(Some(&set.id)).ok())
        .find(|project| project.out == store.out)
}

pub(crate) fn reference_links(doc: &str, text: &str) -> Vec<(String, std::ops::Range<usize>)> {
    if crate::rst::supported(std::path::Path::new(doc)) {
        crate::rst::references(text, true)
    } else if crate::asciidoc::supported(std::path::Path::new(doc)) {
        crate::asciidoc::references(text, true)
    } else {
        Parser::new(text)
            .into_offset_iter()
            .filter_map(|(event, range)| match event {
                Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                    Some((dest_url.into_string(), range))
                }
                _ => None,
            })
            .collect()
    }
}

pub fn scan(proj: &Project) -> BTreeMap<String, Input> {
    scan_with_decisions(proj, &decisions(proj))
}

pub fn scan_with_decisions(
    proj: &Project,
    modes: &BTreeMap<String, crate::model::InputDecision>,
) -> BTreeMap<String, Input> {
    let mut inputs = BTreeMap::new();
    let mut blobs = BTreeMap::new();
    for file in proj.doc_files() {
        let path = file
            .strip_prefix(&proj.root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let input = crate::vfs::read(&file)
            .map_err(|e| format!("cannot read the file: {e}"))
            .and_then(|bytes| {
                let sha = crate::task_runtime::digest(&bytes);
                if md::supported_format(&file)
                    && !modes.get(&path).is_some_and(|d| d.mode == "asset")
                {
                    md::project_bytes(&file, &bytes).and_then(|text| {
                        // UTF-8 prose already hashes all original bytes. Native package
                        // projections must retain the original package byte identity.
                        let hash = if matches!(
                            file.extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_ascii_lowercase()
                                .as_str(),
                            "docx" | "pdf"
                        ) {
                            sha
                        } else {
                            hash_hex(&text)
                        };
                        Ok((hash, md::parse_document_checked(&file, &text)?))
                    })
                } else {
                    let mut envelope = json!({"path": path, "size": bytes.len(), "sha256": sha});
                    if let Some(decision) = modes.get(&path) {
                        envelope["classification"] = json!(decision);
                    }
                    blobs.insert(path.clone(), envelope);
                    Ok((sha, BTreeMap::new()))
                }
            });
        inputs.insert(path, input);
    }
    let mut incoming: BTreeMap<String, BTreeMap<(String, String), Value>> = BTreeMap::new();
    for (doc, input) in &inputs {
        let Ok((_, sections)) = input else { continue };
        // Parse the full projection to retain reference-style link definitions.
        let text = projection_text(sections);
        let references = reference_links(doc, &text);
        for (dest_url, range) in references {
            let crate::gen::assets::Target::Local(path) =
                crate::gen::assets::target_input(doc, &dest_url)
            else {
                continue;
            };
            if !blobs.contains_key(&path) {
                continue;
            }
            let line = text[..range.start].bytes().filter(|b| *b == b'\n').count();
            // Prefer the innermost prose section; fenced blocks are not references.
            if let Some((section, body)) = sections
                .iter()
                .filter(|(_, s)| {
                    s.kind != "code-block"
                        && s.kind != "diagram"
                        && s.lines[0] <= line
                        && line < s.lines[1]
                })
                .min_by_key(|(_, s)| s.lines[1] - s.lines[0])
            {
                incoming.entry(path).or_default().insert(
                    (doc.clone(), section.clone()),
                    json!({"doc":doc,"section":section,"context":body.raw}),
                );
            }
        }
    }
    for (path, mut envelope) in blobs {
        envelope["references"] = json!(incoming
            .remove(&path)
            .unwrap_or_default()
            .into_values()
            .collect::<Vec<_>>());
        let raw = serde_json::to_string_pretty(&envelope).expect("input envelope");
        // Context participates in dirtiness, but sha256 remains the exact byte digest.
        let revision = crate::task_runtime::digest(raw.as_bytes());
        let section = Section {
            reflected: None,
            title: path.clone(),
            kind: "blob".into(),
            order: 0,
            parent: None,
            hash: hash_hex(&raw),
            raw,
            lines: [0, 0],
        };
        inputs.insert(
            path,
            Ok((revision, BTreeMap::from([("/".into(), section)]))),
        );
    }
    inputs
}

pub(crate) fn projection_text(sections: &BTreeMap<String, Section>) -> String {
    let mut sections: Vec<_> = sections
        .values()
        .filter(|s| !matches!(s.kind.as_str(), "code-block" | "diagram" | "blob"))
        .collect();
    sections.sort_by_key(|s| s.lines[0]);
    let mut text = String::new();
    let mut line = 0;
    for section in sections {
        while line < section.lines[0] {
            text.push('\n');
            line += 1;
        }
        text.push_str(&section.raw);
        if !section.raw.ends_with('\n') {
            text.push('\n');
        }
        line = section.lines[1];
    }
    text
}

pub fn is_blob(store: &crate::store::Store, doc: &str, section: &str) -> bool {
    store
        .docs
        .get(doc)
        .and_then(|r| r.sections.get(section))
        .is_some_and(|s| s.kind == "blob")
}

pub fn referring_documents(section: &Section) -> BTreeSet<String> {
    serde_json::from_str::<Value>(&section.raw)
        .ok()
        .and_then(|v| v["references"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|r| r["doc"].as_str().map(String::from))
        .collect()
}

// Use the existing asset resolver, including quote scoping and path safety.
pub fn requirement_links(store: &crate::store::Store, path: &str) -> BTreeSet<String> {
    let rids = store.graph.requirements.keys().cloned().collect::<Vec<_>>();
    crate::gen::assets::referenced_assets(store, "", &rids)
        .into_iter()
        .filter(|a| a.path == path && a.status == "ready")
        .flat_map(|a| a.references.into_iter().map(|r| r.fact))
        .filter(|id| store.graph.requirements.contains_key(id))
        .collect()
}

// Coverage may refer to a requirement authored in the same atomic changeset.
// Project only quote-bearing mutations here; the normal graph gates validate them.
pub fn staged_requirement_links(
    store: &crate::store::Store,
    path: &str,
    staged: &[crate::store::Op],
) -> BTreeSet<String> {
    if staged.is_empty() {
        return requirement_links(store, path);
    }
    let mut projected = store.clone();
    for op in staged {
        match op {
            crate::store::Op::CreateRequirement { id, requirement } => {
                projected
                    .graph
                    .requirements
                    .insert(id.clone(), requirement.clone());
            }
            crate::store::Op::UpdateRequirement {
                id,
                source,
                provenance,
                ..
            } => {
                if let Some(requirement) = projected.graph.requirements.get_mut(id) {
                    if let Some(source) = source {
                        requirement.source = Some(source.clone());
                        requirement.provenance =
                            Some(crate::model::Provenance::Quote(source.clone()));
                    }
                    if let Some(provenance) = provenance {
                        requirement.provenance = Some(provenance.clone());
                        requirement.source = match provenance {
                            crate::model::Provenance::Quote(source) => Some(source.clone()),
                            _ => None,
                        };
                    }
                }
            }
            crate::store::Op::DeleteRequirement { id, .. } => {
                projected.graph.requirements.remove(id);
            }
            _ => {}
        }
    }
    requirement_links(&projected, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        goals::GoalKind,
        model::{Coverage, Entity, Requirement, SourceRef},
        store::{Op, Store},
    };
    const DOC: &str = "# Site\n\nThe site uses the supplied [logo][brand].\n\n[brand]: logo.svg\n";
    fn fixture() -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([
            (".jazyk/jazyk.toml".into(), b"version=1\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['**','!src/**']\nroots=['main.md']\nsource=['src/**']\n".to_vec()),
            ("main.md".into(), DOC.as_bytes().to_vec()),
            ("logo.svg".into(), b"<svg>this is not prose</svg>".to_vec()),
            ("photo.png".into(), vec![137, 80, 78, 71, 0, 255]),
            ("src/logo.svg".into(), b"<svg>this is not prose</svg>".to_vec()),
        ])
    }
    #[test]
    fn asciidoc_inventory_uses_native_sections() {
        let mut files = fixture();
        files.insert(
            "guide.ADOC".into(),
            b"= Guide\n\n== Policy\nOrders must persist.\n".to_vec(),
        );
        let _scope = crate::vfs::Scope::enter(&files);
        let inputs = scan(&project());
        let (_, sections) = inputs["guide.ADOC"].as_ref().unwrap();
        assert_eq!(sections["/guide/policy"].kind, "heading");
        assert!(sections["/guide/policy"]
            .raw
            .contains("Orders must persist."));
    }

    #[test]
    fn asciidoc_images_attach_original_reference_context_to_blobs() {
        let mut files = fixture();
        files.insert(
            "guide.adoc".into(),
            b"= Guide\n\n== Brand\nThe logo must be used.\n\nimage::logo.svg[Logo]\n".to_vec(),
        );
        let _scope = crate::vfs::Scope::enter(&files);
        let inputs = scan(&project());
        let (_, sections) = inputs["logo.svg"].as_ref().unwrap();
        let value: Value = serde_json::from_str(&sections["/"].raw).unwrap();
        assert!(value["references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["doc"] == "guide.adoc" && r["section"] == "/guide/brand"));
    }

    fn project() -> Project {
        crate::document_sets::Workspace::load(std::path::Path::new("/project"))
            .unwrap()
            .resolve(None)
            .unwrap()
    }
    fn store(project: &Project) -> Store {
        let mut store = Store {
            out: project.out.clone(),
            ..Default::default()
        };
        store.sync_docs(&crate::reconcile::parse_all(project).0);
        store
    }
    fn link(store: &mut Store) {
        store.graph.entities.insert(
            "ent:site".into(),
            Entity {
                name: "Site".into(),
                ..Default::default()
            },
        );
        store.graph.requirements.insert(
            "req:logo".into(),
            Requirement {
                statement: "The site uses the supplied logo.".into(),
                entities: vec!["ent:site".into()],
                source: Some(SourceRef {
                    doc: "main.md".into(),
                    section: "/site".into(),
                    quote: "The site uses the supplied [logo][brand].".into(),
                }),
                ..Default::default()
            },
        );
    }
    #[test]
    fn whole_files_compile_without_text_decoding_or_quotable_envelopes() {
        let _scope = crate::vfs::Scope::enter(&fixture());
        let project = project();
        let store = store(&project);
        for path in ["logo.svg", "photo.png"] {
            let section = &store.docs[path].sections["/"];
            assert_eq!(section.kind, "blob");
            assert!(crate::goals::section_has_body(section));
            assert_eq!(section.lines, [0, 0]);
            assert!(!store.quote_locates(path, "/", "sha256"));
            assert!(!section.raw.contains("this is not prose"));
            let value: Value = serde_json::from_str(&section.raw).unwrap();
            assert_eq!(
                value["sha256"],
                crate::task_runtime::digest(&crate::vfs::read(project.root.join(path)).unwrap())
            );
        }
        assert!(referring_documents(&store.docs["logo.svg"].sections["/"]).contains("main.md"));
        assert!(referring_documents(&store.docs["photo.png"].sections["/"]).is_empty());
        assert_eq!(
            crate::control::current_doc_hashes(&project),
            store
                .docs
                .iter()
                .map(|(p, r)| (p.clone(), r.content_hash.clone()))
                .collect()
        );
        assert!(!crate::reconcile::checks(&store, &project)
            .iter()
            .any(|f| f.0 == "unsupported-format" || f.0 == "parse-error"));
    }
    #[test]
    fn byte_and_reference_changes_reopen_only_relevant_inputs_and_noop_stays_clean() {
        let _scope = crate::vfs::Scope::enter(&fixture());
        let project = project();
        let mut store = store(&project);
        link(&mut store);
        for rec in store.docs.values_mut() {
            for sec in rec.sections.keys() {
                rec.coverage.insert(
                    sec.clone(),
                    Coverage {
                        state: "covered".into(),
                        note: Some("req:logo".into()),
                        claimed_by: None,
                    },
                );
            }
        }
        store.status.changes.clear();
        let before = crate::control::current_doc_hashes(&project);
        assert!(store
            .sync_docs(&crate::reconcile::parse_all(&project).0)
            .is_empty());
        crate::vfs::write(project.root.join("logo.svg"), [0, 255, 42]).unwrap();
        let after = crate::control::current_doc_hashes(&project);
        assert_ne!(before["logo.svg"], after["logo.svg"]);
        assert_eq!(before["photo.png"], after["photo.png"]);
        store.sync_docs(&crate::reconcile::parse_all(&project).0);
        let goals = crate::goals::ReconcileSection
            .derive_goals(&store, &crate::gen::GenSettings::resolve(&project));
        assert!(goals.iter().any(|g| g.target == "logo.svg#/"));
        assert!(!goals.iter().any(|g| g.target == "photo.png#/"));
        let before = after;
        crate::vfs::write(
            project.root.join("main.md"),
            DOC.replace("supplied", "official"),
        )
        .unwrap();
        let after = crate::control::current_doc_hashes(&project);
        assert_ne!(before["logo.svg"], after["logo.svg"]);
        assert_eq!(before["photo.png"], after["photo.png"]);
        crate::vfs::write(project.root.join("main.md"), "# Site\nNo reference.\n").unwrap();
        let parsed = crate::reconcile::parse_all(&project).0;
        assert!(referring_documents(&parsed["logo.svg"].1["/"]).is_empty());
    }
    #[test]
    fn covered_blob_requires_real_external_requirement_and_cannot_be_dismissed() {
        let _scope = crate::vfs::Scope::enter(&fixture());
        let project = project();
        let mut store = store(&project);
        let covered = |note: &str| Op::SetCoverage {
            doc: "logo.svg".into(),
            section: "/".into(),
            state: "covered".into(),
            note: Some(note.into()),
        };
        assert!(!crate::goals::ReconcileSection
            .gates(&store, &[covered("req:invented")])
            .is_empty());
        link(&mut store);
        assert_eq!(
            requirement_links(&store, "logo.svg"),
            BTreeSet::from(["req:logo".into()])
        );
        assert!(crate::goals::ReconcileSection
            .gates(
                &store,
                &[covered("Supplied logo for req:logo; retain its bytes.")]
            )
            .is_empty());
        assert!(!crate::goals::ReconcileSection
            .gates(&store, &[covered("req:logo-other")])
            .is_empty());
        let dismiss = Op::SetCoverage {
            doc: "logo.svg".into(),
            section: "/".into(),
            state: "non-normative".into(),
            note: Some("Only an image".into()),
        };
        assert!(!crate::goals::ReconcileSection
            .gates(&store, &[dismiss])
            .is_empty());
        // A generated copy does not turn the authored input into an output reference.
        assert_ne!(
            project.root.join("src/logo.svg"),
            project.root.join("logo.svg")
        );
        assert_eq!(requirement_links(&store, "logo.svg").len(), 1);
    }
    #[test]
    fn blob_session_repairs_reference_and_rejects_invalid_coverage_before_staging() {
        let _scope = crate::vfs::Scope::enter(&fixture());
        let project = project();
        let mut store = store(&project);
        link(&mut store);
        store
            .graph
            .requirements
            .get_mut("req:logo")
            .unwrap()
            .source
            .as_mut()
            .unwrap()
            .quote = "The site uses the supplied".into();
        let goal = crate::model::Goal {
            id: "g:reconcile-section:logo.svg#/".into(),
            kind: "reconcile-section".into(),
            target: "logo.svg#/".into(),
            mandatory: true,
            ..Default::default()
        };
        let mut session = crate::tools::ToolSession::new(
            store,
            crate::tools::WorkScope::for_batch("blob-repair", &[goal]),
            64,
            24_000,
        );
        let coverage = json!({"section":"logo.svg#/", "state":"covered", "note":"req:logo"});
        let err = session.dispatch("set_coverage", &coverage).unwrap_err();
        assert_eq!(err.rule, "uncovered-claim");
        assert!(err.message.contains("no requirement quote selects"));
        assert!(session.staged.is_empty());
        let mut failed = crate::tools::ToolSession::new(
            session.snapshot.clone(),
            session.scope.clone(),
            64,
            24_000,
        );
        failed.dispatch("set_coverage", &coverage).unwrap_err();
        failed.dispatch("mark_goal_failed", &json!({
            "goal":"g:reconcile-section:logo.svg#/", "reason":"The referring requirement does not yet quote the linked input."
        })).unwrap();
        failed
            .dispatch(
                "done",
                &json!({"summary":"Recorded the missing reference without a coverage claim."}),
            )
            .unwrap();
        // Unrelated prose still cannot be quoted by this blob batch.
        let err = session
            .dispatch(
                "update_requirement",
                &json!({
                    "id":"req:logo", "section":"main.md#/site", "quote":"The site uses the supplied"
                }),
            )
            .unwrap_err();
        assert_eq!(err.rule, "wrong-document");
        session.dispatch("update_requirement", &json!({
            "id":"req:logo", "section":"main.md#/site", "quote":"The site uses the supplied [logo][brand]."
        })).unwrap();
        session.dispatch("set_coverage", &coverage).unwrap();
        assert!(crate::goals::ReconcileSection
            .gates(&session.snapshot, &session.staged)
            .is_empty());
    }

    #[test]
    fn legacy_unsupported_warning_resolves_and_missing_input_invalidates_its_requirement() {
        let _scope = crate::vfs::Scope::enter(&fixture());
        let project = project();
        let mut store = store(&project);
        link(&mut store);
        store.reconcile_check_diags(vec![(
            "unsupported-format".into(),
            vec!["logo.svg".into()],
            "warning".into(),
            "legacy warning".into(),
            None,
        )]);
        store.reconcile_check_diags(crate::reconcile::checks(&store, &project));
        assert!(store
            .graph
            .diagnostics
            .values()
            .filter(|d| d.rule == "unsupported-format")
            .all(|d| d.lifecycle == "resolved"));
        let before = crate::gen::fact_hash(&store, "ent:site");
        crate::vfs::remove_file(project.root.join("logo.svg")).unwrap();
        assert_ne!(before, crate::gen::fact_hash(&store, "ent:site"));
        let assets =
            crate::gen::assets::referenced_assets(&store, "ent:site", &["req:logo".into()]);
        assert_eq!(assets[0].status, "missing");
        let parsed = crate::reconcile::parse_all(&project).0;
        assert!(!parsed.contains_key("logo.svg"));
    }

    #[test]
    fn blob_reconciliation_follows_referring_prose() {
        let _scope = crate::vfs::Scope::enter(&fixture());
        let project = project();
        let store = store(&project);
        let board = crate::board::Board::derive(
            &store,
            &project,
            &crate::control::Control::load(&project, &project.out),
        );
        let blob = board
            .goals
            .iter()
            .find(|g| g.target == "logo.svg#/")
            .unwrap();
        assert!(matches!(
            blob.state,
            crate::model::GoalState::Blocked { .. }
        ));
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use crate::{
        goals::GoalKind,
        model::{Entity, Requirement, SourceRef},
        store::Store,
        tools::{ToolSession, WorkScope},
    };
    #[test]
    fn blob_coverage_completes_through_real_tool_session_without_a_blob_quote() {
        let _scope = crate::vfs::Scope::enter(&BTreeMap::from([
            (
                ".jazyk/jazyk.toml".into(),
                b"version=1\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['**']\n".to_vec(),
            ),
            (
                "main.md".into(),
                b"# Site\nThe site uses [logo](logo.svg).\n".to_vec(),
            ),
            ("logo.svg".into(), vec![0, 255, 42]),
        ]));
        let project = crate::document_sets::Workspace::load(std::path::Path::new("/project"))
            .unwrap()
            .resolve(None)
            .unwrap();
        let mut store = Store {
            out: project.out.clone(),
            ..Default::default()
        };
        store.sync_docs(&crate::reconcile::parse_all(&project).0);
        store.graph.entities.insert(
            "ent:site".into(),
            Entity {
                name: "Site".into(),
                ..Default::default()
            },
        );
        store.graph.requirements.insert(
            "req:logo".into(),
            Requirement {
                statement: "The site uses the logo.".into(),
                entities: vec!["ent:site".into()],
                source: Some(SourceRef {
                    doc: "main.md".into(),
                    section: "/site".into(),
                    quote: "The site uses [logo](logo.svg).".into(),
                }),
                ..Default::default()
            },
        );
        let goal = crate::goals::ReconcileSection
            .derive_goals(&store, &crate::gen::GenSettings::resolve(&project))
            .into_iter()
            .find(|g| g.target == "logo.svg#/")
            .unwrap();
        let mut session = ToolSession::new(
            store,
            WorkScope::for_batch("blob-proof", std::slice::from_ref(&goal)),
            100,
            20000,
        );
        session.dispatch("set_coverage",&json!({"section":"logo.svg#/","state":"covered","note":"req:logo uses the supplied bytes; generation retains or copies them."})).unwrap();
        session.dispatch("done",&json!({"summary":"Reconciled logo input against req:logo.","goals":[{"goal":goal.id,"justification":"req:logo identifies this whole-file input as the supplied logo."}]})).unwrap();
    }
}

// Shared scripted-provider fixture: exercise actual blob sessions before generation.
#[cfg(test)]
pub(crate) fn scripted_blob_reply(request: &Value, requirement: Option<&str>) -> Option<Value> {
    let payload = request["messages"]
        .as_array()?
        .iter()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| serde_json::from_str::<Value>(m["content"].as_str()?).ok())
        .find(|v| v["goals"].is_array())?;
    let goals = payload["goals"].as_array()?;
    if goals.is_empty()
        || !goals.iter().all(|g| {
            g["kind"] == "reconcile-section"
                && g["target"].as_str().is_some_and(|t| t.ends_with("#/"))
        })
    {
        return None;
    }
    let note = requirement
        .map(|id| {
            format!(
                "{id} identifies the supplied logo; preserve its exact bytes in the deliverable."
            )
        })
        .unwrap_or_else(|| {
            "No referring document or requirement assigns an obligation to this input.".into()
        });
    let mut actions = goals.iter().map(|g| ("set_coverage", json!({"section":g["target"],"state":if requirement.is_some() {"covered"} else {"non-normative"},"note":note}))).collect::<Vec<_>>();
    actions.push(("done",json!({"summary":"Reconciled whole-file inputs.","goals":goals.iter().map(|g| json!({"goal":g["id"],"justification":note})).collect::<Vec<_>>()})));
    Some(
        json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(i,(tool,args))|json!({"id":format!("blob-{i}"),"type":"function","function":{"name":tool,"arguments":args.to_string()}})).collect::<Vec<_>>()}}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}}),
    )
}
