use crate::{
    model::*,
    store::{Commit, Op, Store},
    tools::{ToolSession, WorkScope},
};
use serde_json::json;

fn fixture() -> (tempfile::TempDir, crate::project::Project, Store) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".jazyk")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join(".jazyk/jazyk.toml"), "version=1\n[[sets]]\nid='main'\nname='Main'\ndocs=['docs/**']\nsource=['deliverable/**']\n").unwrap();
    std::fs::write(
        root.join("docs/site.md"),
        "# Site\nThe site offers the [brochure](brochure.pdf) for download.\n",
    )
    .unwrap();
    std::fs::write(
        root.join("docs/brochure.pdf"),
        b"%PDF-deliberately-unparseable\0\xff",
    )
    .unwrap();
    let project = crate::document_sets::Workspace::load(&root)
        .unwrap()
        .resolve(Some("main"))
        .unwrap();
    let mut store = Store {
        out: project.out.clone(),
        ..Default::default()
    };
    let (parsed, _) = crate::reconcile::parse_all(&project);
    store.sync_docs(&parsed);
    (dir, project, store)
}

fn classify(store: &mut Store, path: &str, mode: &str) {
    let project = crate::input::project_for_store(store).unwrap();
    let sha = crate::task_runtime::digest(&std::fs::read(project.root.join(path)).unwrap());
    let decision = InputDecision {
        mode: mode.into(),
        sha256: sha,
        reason: "This supplied file is offered unchanged for download.".into(),
    };
    let report = store.apply(
        vec![Op::ClassifyInput {
            doc: path.into(),
            decision,
        }],
        &Commit::store("asset-test"),
    );
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
}

#[test]
fn asset_pdf_bypasses_parser_persists_and_reopens_whole_file() {
    let (_dir, project, mut store) = fixture();
    assert!(crate::input::scan(&project)["docs/brochure.pdf"].is_err());
    classify(&mut store, "docs/brochure.pdf", "asset");
    let before = crate::input::scan(&project)
        .remove("docs/brochure.pdf")
        .unwrap()
        .unwrap();
    assert_eq!(before.1.len(), 1);
    assert_eq!(before.1["/"].kind, "blob");
    let file = project.out.join("graph/decisions/inputs").join(format!(
        "{}.yaml",
        crate::store::node_stem("docs/brochure.pdf").0
    ));
    let bytes = std::fs::read(&file).unwrap();
    let mut loaded = Store::load(&project.out);
    loaded.save_for_test();
    assert_eq!(bytes, std::fs::read(file).unwrap());
    assert_eq!(loaded.decisions.inputs["docs/brochure.pdf"].mode, "asset");
    let no_change = crate::reconcile::parse_all(&project).0;
    assert!(loaded.sync_docs(&no_change).is_empty());
    std::fs::write(
        project.root.join("docs/brochure.pdf"),
        b"%PDF-new bytes\0\xfe",
    )
    .unwrap();
    let current = crate::reconcile::parse_all(&project).0;
    let dirty = loaded.sync_docs(&current);
    assert!(dirty
        .iter()
        .any(|d| d.doc == "docs/brochure.pdf" && d.dirty_sections == ["/"]));
    assert_ne!(before.0, current["docs/brochure.pdf"].0);
    assert_eq!(loaded.decisions.inputs["docs/brochure.pdf"].mode, "asset");
    let response = crate::webapi::docs::doc_content(&project, "docs/brochure.pdf");
    assert!(
        matches!(response, crate::webapi::docs::DocRead::Found(v) if v["asset"] == true && v["quotable"] == false)
    );
}

#[test]
fn classified_pdf_contributes_exact_bytes_to_generation() {
    let (_dir, project, mut store) = fixture();
    classify(&mut store, "docs/brochure.pdf", "asset");
    store.graph.requirements.insert(
        "req:download".into(),
        Requirement {
            statement: "Offer the brochure.".into(),
            source: Some(SourceRef {
                doc: "docs/site.md".into(),
                section: "/site".into(),
                quote: "The site offers the [brochure](brochure.pdf) for download.".into(),
            }),
            ..Default::default()
        },
    );
    let first = crate::gen::assets::referenced_assets(&store, "", &["req:download".into()]);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].status, "ready");
    let hash = crate::gen::assets::fingerprint("base", &first);
    std::fs::write(
        project.root.join("docs/brochure.pdf"),
        b"different PDF bytes",
    )
    .unwrap();
    let next = crate::gen::assets::referenced_assets(&store, "", &["req:download".into()]);
    assert_ne!(hash, crate::gen::assets::fingerprint("base", &next));
}

#[test]
fn classification_rejects_stale_bytes_and_stranded_provenance() {
    let (_dir, project, mut store) = fixture();
    let mut tools = ToolSession::new(store.clone(), WorkScope::serving("mcp-write"), 64, 24000);
    let inspected = tools
        .dispatch("inspect_input", &json!({"path":"docs/site.md"}))
        .unwrap();
    assert!(tools.dispatch("classify_input", &json!({"path":"docs/site.md","mode":"asset","sha256":"wrong","reason":"supplied content"})).is_err());
    store.graph.requirements.insert(
        "req:keep".into(),
        Requirement {
            source: Some(SourceRef {
                doc: "docs/site.md".into(),
                section: "/site".into(),
                quote: "The site offers the [brochure](brochure.pdf) for download.".into(),
            }),
            ..Default::default()
        },
    );
    let report = store.apply(
        vec![Op::ClassifyInput {
            doc: "docs/site.md".into(),
            decision: InputDecision {
                mode: "asset".into(),
                sha256: inspected["sha256"].as_str().unwrap().into(),
                reason: "supplied content".into(),
            },
        }],
        &Commit::store("test"),
    );
    assert!(report
        .skipped
        .iter()
        .any(|s| s.contains("relocate or retract")));
    assert!(!store.decisions.inputs.contains_key("docs/site.md"));
    assert!(project.root.join("docs/site.md").exists());
}

#[test]
fn long_nested_input_decisions_round_trip_and_merge() {
    let (_dir, project, mut store) = fixture();
    let path = format!("docs/{}/{}/download.pdf", "a".repeat(100), "b".repeat(100));
    std::fs::create_dir_all(project.root.join(&path).parent().unwrap()).unwrap();
    std::fs::write(project.root.join(&path), b"download").unwrap();
    classify(&mut store, &path, "asset");
    assert!(Store::load(&project.out)
        .decisions
        .inputs
        .contains_key(&path));
    let base = "id: docs/file.pdf\nmode: document\nsha256: old\nreason: old\n";
    let ours = "id: docs/file.pdf\nmode: asset\nsha256: ours\nreason: supplied download\n";
    let theirs = "id: docs/file.pdf\nmode: document\nsha256: theirs\nreason: specification\n";
    let merged = crate::merge_file::merge(
        base,
        ours,
        theirs,
        ".jazyk/main/graph/decisions/inputs/docs~file.pdf.yaml",
    )
    .unwrap();
    let value: serde_json::Value = serde_norway::from_str(&merged.text).unwrap();
    assert_eq!(value["mode"], "asset");
    assert!(!merged.conflicts.is_empty());
}

#[cfg(unix)]
#[test]
fn native_asset_links_share_bytes_protect_writes_and_export() {
    let (_dir, project, mut store) = fixture();
    classify(&mut store, "docs/brochure.pdf", "asset");
    let mut tools = ToolSession::new(store, WorkScope::serving("mcp-write"), 64, 24000);
    tools.gen = crate::gen::GenSettings::resolve(&project);
    let hash = tools
        .dispatch("inspect_input", &json!({"path":"docs/brochure.pdf"}))
        .unwrap()["sha256"]
        .clone();
    let reply=tools.dispatch("link_asset",&json!({"path":"deliverable/public/brochure.pdf","target":"docs/brochure.pdf","sha256":hash})).unwrap();
    let link = project.root.join("deliverable/public/brochure.pdf");
    assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
    assert_eq!(
        std::fs::read_link(&link).unwrap().to_str(),
        Some("../../docs/brochure.pdf")
    );
    assert!(tools
        .dispatch(
            "write_text_file",
            &json!({"path":"deliverable/public/brochure.pdf","content":"overwrite"})
        )
        .is_err());
    let first = crate::gen::hash_file(&link);
    std::fs::write(project.root.join("docs/brochure.pdf"), b"latest bytes").unwrap();
    assert_eq!(std::fs::read(&link).unwrap(), b"latest bytes");
    assert_ne!(first, crate::gen::hash_file(&link));
    let snapshot = crate::task_runtime::snapshot(&project.root).unwrap();
    use base64::Engine;
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(&snapshot["deliverable/public/brochure.pdf"])
            .unwrap(),
        b"latest bytes"
    );
    assert!(tools
        .dispatch(
            "remove_asset_link",
            &json!({"path":"deliverable/public/brochure.pdf","expectedTarget":"wrong"})
        )
        .is_err());
    tools
        .dispatch(
            "remove_asset_link",
            &json!({"path":"deliverable/public/brochure.pdf","expectedTarget":reply["target"]}),
        )
        .unwrap();
    assert!(!link.exists());
    assert!(project.root.join("docs/brochure.pdf").exists());
}

#[test]
fn virtual_links_survive_snapshots_follow_edits_and_keep_dangling_entries() {
    use crate::vfs::VPathExt;
    let mut files = crate::file_tree::Snapshot::from([
        (".jazyk/jazyk.toml".into(), b"version=1\n[[sets]]\nid='main'\nname='Main'\ndocs=['docs/**']\nsource=['deliverable/**']\n".to_vec()),
        ("docs/logo.svg".into(), b"<svg>original</svg>".to_vec()),
    ]);
    files.insert_symlink("deliverable/logo.svg".into(), "../docs/logo.svg".into());
    let round_trip: crate::file_tree::Snapshot =
        serde_json::from_slice(&serde_json::to_vec(&files).unwrap()).unwrap();
    assert_eq!(round_trip, files);
    {
        let scope = crate::vfs::Scope::enter(&files);
        let link = std::path::Path::new("/project/deliverable/logo.svg");
        assert!(crate::vfs::is_symlink(link));
        assert_eq!(crate::vfs::read(link).unwrap(), b"<svg>original</svg>");
        crate::vfs::write("/project/docs/logo.svg", b"<svg>changed</svg>").unwrap();
        assert_eq!(crate::vfs::read(link).unwrap(), b"<svg>changed</svg>");
        assert!(crate::vfs::write(link, b"overwrite").is_err());
        crate::vfs::rename(link, "/project/deliverable/brand.svg").unwrap();
        assert!(!link.vexists());
        let renamed = std::path::Path::new("/project/deliverable/brand.svg");
        assert_eq!(crate::vfs::read(renamed).unwrap(), b"<svg>changed</svg>");
        crate::vfs::remove_file("/project/docs/logo.svg").unwrap();
        assert!(!renamed.vexists());
        assert!(crate::vfs::is_symlink(renamed));
        files = scope.snapshot();
    }
    assert!(files.is_symlink("deliverable/brand.svg"));
    assert_eq!(files["deliverable/brand.svg"], b"../docs/logo.svg");
    let mut workspace = crate::hosted::Workspace::new(files).unwrap();
    workspace
        .request(
            "GET",
            "/api/project/files",
            "null",
            &crate::hosted::Execution::default(),
        )
        .unwrap();
    let snapshot = workspace.snapshot();
    assert!(snapshot.is_symlink("deliverable/brand.svg"));
    assert!(!snapshot
        .keys()
        .any(|p| p.ends_with("links.yaml") || p.ends_with("links.json")));
    let _scope = crate::vfs::Scope::enter(&snapshot);
    crate::vfs::remove_file(std::path::Path::new("/project/deliverable/brand.svg")).unwrap();
    assert!(!crate::vfs::is_symlink(std::path::Path::new(
        "/project/deliverable/brand.svg"
    )));
}

#[test]
fn virtual_link_resolution_rejects_cycles_and_escaping_snapshots() {
    let mut files = crate::file_tree::Snapshot::new();
    files.insert_symlink("a.svg".into(), "b.svg".into());
    files.insert_symlink("b.svg".into(), "a.svg".into());
    let _scope = crate::vfs::Scope::enter(&files);
    assert!(crate::vfs::read("/project/a.svg").is_err());
    let mut escaped = crate::file_tree::Snapshot::new();
    escaped.insert_symlink("outside".into(), "../private".into());
    assert!(crate::hosted::Workspace::new(escaped).is_err());
}

#[test]
fn reclassification_and_quote_retraction_share_one_atomic_changeset() {
    let (_dir, project, mut store) = fixture();
    store.graph.requirements.insert(
        "req:keep".into(),
        Requirement {
            statement: "Offer the brochure.".into(),
            source: Some(SourceRef {
                doc: "docs/site.md".into(),
                section: "/site".into(),
                quote: "The site offers the [brochure](brochure.pdf) for download.".into(),
            }),
            ..Default::default()
        },
    );
    let mut tools = ToolSession::new(store.clone(), WorkScope::serving("mcp-write"), 64, 24000);
    let sha =
        crate::task_runtime::digest(&std::fs::read(project.root.join("docs/site.md")).unwrap());
    tools.dispatch("classify_input",&json!({"path":"docs/site.md","mode":"asset","sha256":sha,"reason":"This file is supplied content rather than a specification."})).unwrap();
    tools
        .dispatch(
            "delete_requirement",
            &json!({"id":"req:keep","reason":"Retracted while classifying supplied content."}),
        )
        .unwrap();
    let result = store.apply(tools.staged, &Commit::store("asset-test"));
    assert!(result.skipped.is_empty(), "{:?}", result.skipped);
    assert!(!store.graph.requirements.contains_key("req:keep"));
    assert_eq!(store.docs["docs/site.md"].sections.len(), 1);
    assert_eq!(store.docs["docs/site.md"].sections["/"].kind, "blob");
}
