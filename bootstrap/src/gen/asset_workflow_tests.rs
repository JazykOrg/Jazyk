#![cfg(not(target_arch = "wasm32"))]

use super::*;
use crate::hosted::{Execution, Snapshot};
use crate::model::{Coverage, Criterion, Entity, Provenance, Requirement, SourceRef};
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

const SITE: &str = "Site uses the supplied logo.";
const SITE_CRITERION: &str = "The displayed logo retains the supplied appearance.";
const FOOTER: &str = "Footer shows the copyright.";
const FOOTER_CRITERION: &str = "The footer includes Copyright.";
const LINKED: &str = "Site uses the supplied [logo](assets/logo.svg).";
const SVG_A: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path id="supplied-a" fill="#12ab34" d="M1 1h22v22H1z"/></svg>"##;
const SVG_B: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle id="supplied-b" fill="#ab3412" cx="12" cy="12" r="10"/></svg>"##;
const OLD_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg"><circle r="2"/></svg>"#;

fn source(section: &str, quote: &str) -> SourceRef {
    SourceRef {
        doc: "docs/main.md".into(),
        section: section.into(),
        quote: quote.into(),
    }
}

fn document(site: &str) -> String {
    format!("# Site\n\n{site}\n{SITE_CRITERION}\n\n# Footer\n\n{FOOTER}\n{FOOTER_CRITERION}\n")
}

fn project() -> crate::project::Project {
    crate::document_sets::Workspace::load(Path::new("/project"))
        .unwrap()
        .resolve(None)
        .unwrap()
}

fn cover(store: &mut Store) {
    store.status.changes.clear();
    for record in store.docs.values_mut() {
        for (section, body) in &record.sections {
            if body.kind == "blob" {
                continue;
            }
            record.coverage.insert(
                section.clone(),
                Coverage {
                    state: "covered".into(),
                    note: Some("The quoted requirements and examples are recorded.".into()),
                    claimed_by: None,
                },
            );
        }
    }
}

fn manifest(name: &str) -> Value {
    let files = if name == "site" {
        vec!["src/site.html", "src/logo.svg"]
    } else {
        vec!["src/footer.html"]
    };
    json!({"files":files,"tests":[{
        "requirement":format!("req:{name}"),"criterion":format!("crit:{name}"),
        "kind":"llm","artifact":format!("tests/{name}.md"),"name":name,"run":"llm","files":files
    }]})
}

// A settled, already generated project. Broad Docs contains the uploaded SVG,
// while the resolved Source exclusions keep the generated application separate.
fn fixture() -> Snapshot {
    let files = Snapshot::from([
        (".jazyk/jazyk.toml".into(), b"version=1\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['**','!src/**','!tests/**']\nroots=['docs/main.md']\nsource=['src/**','tests/**']\n".to_vec()),
        ("docs/main.md".into(), document(SITE).into_bytes()),
        ("src/site.html".into(), b"<!doctype html><img src=\"logo.svg\" alt=\"Old logo\">".to_vec()),
        ("src/logo.svg".into(), OLD_SVG.as_bytes().to_vec()),
        ("src/footer.html".into(), b"<footer>Copyright</footer>".to_vec()),
    ]);
    let fs = crate::vfs::Scope::enter(&files);
    let project = project();
    let mut store = Store {
        out: project.out.clone(),
        ..Default::default()
    };
    store.sync_docs(&crate::reconcile::parse_all(&project).0);
    store.status.generation = 33;
    cover(&mut store);
    for (name, label, statement, criterion) in [
        ("site", "Site", SITE, SITE_CRITERION),
        ("footer", "Footer", FOOTER, FOOTER_CRITERION),
    ] {
        let section = format!("/{name}");
        let quote = source(&section, statement);
        store.graph.entities.insert(
            format!("ent:{name}"),
            Entity {
                name: label.into(),
                mentions: vec![source(&section, label)],
                ..Default::default()
            },
        );
        store.graph.requirements.insert(
            format!("req:{name}"),
            Requirement {
                statement: statement.into(),
                entities: vec![format!("ent:{name}")],
                source: Some(quote.clone()),
                provenance: Some(Provenance::Quote(quote)),
                ..Default::default()
            },
        );
        store.graph.criteria.insert(
            format!("crit:{name}"),
            Criterion {
                requirement: format!("req:{name}"),
                statement: criterion.into(),
                kind: "positive".into(),
                provenance: Some(Provenance::Quote(source(&section, criterion))),
                ..Default::default()
            },
        );
        crate::vfs::write(
            format!("/project/tests/{name}.md"),
            criteria_front_matter(
                &format!("req:{name}"),
                Some(&format!("crit:{name}")),
                criterion,
            ) + &format!("\nInspect src/{name}.html. PASS when {criterion} FAIL otherwise.\n"),
        )
        .unwrap();
    }
    store.save_for_test();
    Ledger {
        medium: Some(Medium {
            form: "HTML page".into(),
            produced: "written".into(),
            toolchain: "HTML".into(),
            artifact: "src/site.html".into(),
        }),
        ..Default::default()
    }
    .save(&project.out)
    .unwrap();
    let gs = GenSettings::resolve(&project);
    for name in ["site", "footer"] {
        mark(&store, &format!("ent:{name}"), None, &manifest(name), &gs).unwrap();
    }
    assert!(pending(&store, &gs).is_empty());
    crate::vfs::write(
        project.out.join("control.yaml"),
        "compile: manual\ngenerate: manual\nreleased:\n  compile: {}\n  generate: 0\n",
    )
    .unwrap();
    fs.snapshot()
}

fn reanchor(files: &Snapshot, quote: &str, extra: &[(&str, &[u8])]) -> Snapshot {
    let fs = crate::vfs::Scope::enter(files);
    crate::vfs::write("/project/docs/main.md", document(quote)).unwrap();
    for (path, bytes) in extra {
        crate::vfs::write(Path::new("/project").join(path), bytes).unwrap();
    }
    let project = project();
    let mut store = Store::load(&project.out);
    store.sync_docs(&crate::reconcile::parse_all(&project).0);
    let requirement = store.graph.requirements.get_mut("req:site").unwrap();
    requirement.source = Some(source("/site", quote));
    requirement.provenance = Some(Provenance::Quote(source("/site", quote)));
    assert_eq!(
        requirement.statement, SITE,
        "Only the verbatim provenance changes"
    );
    // The fixture directly re-anchors the fact above, settling its old proposal.
    store.status.alignment.clear();
    cover(&mut store);
    crate::derive::record_ledger_stale(&mut store, &GenSettings::resolve(&project));
    store.save_for_test();
    fs.snapshot()
}

fn package(files: &Snapshot) -> Value {
    let _fs = crate::vfs::Scope::enter(files);
    let project = project();
    task_package(
        &Store::load(&project.out),
        "ent:site",
        &GenSettings::resolve(&project),
    )
    .unwrap()
}

fn pending_ids(files: &Snapshot) -> Vec<String> {
    let _fs = crate::vfs::Scope::enter(files);
    let project = project();
    pending(&Store::load(&project.out), &GenSettings::resolve(&project))
        .iter()
        .map(|row| row["entity"].as_str().unwrap().to_string())
        .collect()
}

fn footer_row(files: &Snapshot) -> Value {
    let _fs = crate::vfs::Scope::enter(files);
    serde_json::to_value(&Ledger::load(&project().out).entities["footer"]).unwrap()
}

fn workflow(files: Snapshot, execution: &Execution, compile_sessions: u64) -> (Value, Snapshot) {
    let mut workspace = crate::hosted::Workspace::new(files).unwrap();
    let reply = workspace
        .request(
            "POST",
            "/api/jobs",
            &json!({"kind":"workflow", "setId":"ds_main",
        "steps":[{"action":"compile"},{"action":"generate"}]})
            .to_string(),
            execution,
        )
        .unwrap();
    let result: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(reply.status, 200, "{result}");
    assert_eq!(result["report"]["state"], "done", "{result}");
    assert_eq!(
        result["report"]["steps"][0]["result"]["sessions"], compile_sessions,
        "{result}"
    );
    (result, workspace.snapshot())
}

// GDK transports may wrap MCP replies in content blocks or JSON strings. Search
// only tool messages, preserving assertions on the actual tool result.
fn tool_object(value: &Value, key: &str, depth: usize) -> Option<Value> {
    if depth == 0 {
        return None;
    }
    match value {
        Value::Object(object) => {
            if object.contains_key(key) {
                return Some(value.clone());
            }
            object
                .values()
                .find_map(|value| tool_object(value, key, depth - 1))
        }
        Value::Array(values) => values
            .iter()
            .find_map(|value| tool_object(value, key, depth - 1)),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| tool_object(&value, key, depth - 1)),
        _ => None,
    }
}

fn previous_tool(request: &Value, key: &str) -> Value {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .filter(|message| message["role"] == "tool")
        .find_map(|message| tool_object(message, key, 12))
        .unwrap_or_else(|| panic!("No tool result containing {key}: {request}"))
}

fn logo_model(expected: &'static str) -> (Execution, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let generation_round = AtomicUsize::new(0);
    let hash = Mutex::new(String::new());
    let execution = Execution {
        model: "scripted-supplied-logo".into(),
        transport_async: Some(Arc::new(move |body| {
            let request: Value = serde_json::from_str(&body).unwrap();
            seen.fetch_add(1, Ordering::SeqCst);
            if let Some(reply) = crate::input::scripted_blob_reply(&request, Some("req:site")) {
                return Box::pin(async move { Ok(reply.to_string()) });
            }
            let round = generation_round.fetch_add(1, Ordering::SeqCst) + 1;
            let (tool, args) = match round {
                1 => ("begin_generation", json!({"entity":"ent:site"})),
                2 => {
                    let package = previous_tool(&request, "referencedAssets");
                    let asset = &package["referencedAssets"][0];
                    assert_eq!(asset["path"], "docs/assets/logo.svg");
                    assert_eq!(asset["status"], "ready");
                    assert_eq!(
                        asset["sha256"],
                        format!("{:x}", Sha256::digest(expected.as_bytes()))
                    );
                    *hash.lock().unwrap() = package["factHash"].as_str().unwrap().into();
                    ("read_text_file", json!({"path":"docs/assets/logo.svg"}))
                }
                3 => {
                    let reply = previous_tool(&request, "totalLines");
                    assert_eq!(
                        reply["content"], expected,
                        "Copy the actual uploaded bytes, not a reconstructed logo"
                    );
                    (
                        "write_text_file",
                        json!({"path":"src/logo.svg","content":reply["content"]}),
                    )
                }
                4 => (
                    "write_text_file",
                    json!({"path":"src/site.html","content":"<!doctype html><img src=\"logo.svg\" alt=\"Supplied logo\">"}),
                ),
                5 => (
                    "record_generation",
                    json!({"entity":"ent:site","factHash":*hash.lock().unwrap(),"manifest":manifest("site")}),
                ),
                6 => {
                    assert_eq!(
                        previous_tool(&request, "marked")["marked"],
                        "ent:site",
                        "The generation record must succeed before ending the session"
                    );
                    (
                        "done",
                        json!({"summary":"Used the exact uploaded SVG and kept its relative reference resolvable."}),
                    )
                }
                _ => panic!("Unexpected extra inference round {round}: {request}"),
            };
            let reply = json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{
            "id":format!("logo-{round}"),"type":"function","function":{"name":tool,"arguments":args.to_string()}
        }]}}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}}).to_string();
            Box::pin(async move { Ok(reply) })
        })),
        ..Default::default()
    };
    (execution, calls)
}

#[test]
fn uploaded_logo_survives_real_gdk_workflow_noop_and_same_path_replacement() {
    let original = fixture();
    let footer = footer_row(&original);
    let original_hash = package(&original)["factHash"].clone();
    let changed = reanchor(
        &original,
        LINKED,
        &[("docs/assets/logo.svg", SVG_A.as_bytes())],
    );
    assert_ne!(package(&changed)["factHash"], original_hash);
    assert_eq!(pending_ids(&changed), ["ent:site"]);
    let (execution, calls) = logo_model(SVG_A);
    let (report, generated) = workflow(changed, &execution, 1);
    assert_eq!(report["report"]["steps"][1]["result"]["regenerated"], 1);
    assert_eq!(calls.load(Ordering::SeqCst), 7);
    assert_eq!(generated["src/logo.svg"], SVG_A.as_bytes());
    assert_eq!(generated["docs/assets/logo.svg"], SVG_A.as_bytes());
    assert!(String::from_utf8_lossy(&generated["src/site.html"]).contains("src=\"logo.svg\""));
    assert_eq!(generated["src/footer.html"], original["src/footer.html"]);
    assert_eq!(footer_row(&generated), footer);
    assert!(pending_ids(&generated).is_empty());

    let no_calls = Arc::new(AtomicUsize::new(0));
    let observed = no_calls.clone();
    let noop = Execution {
        transport_async: Some(Arc::new(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err("A no-op rebuild must not infer.".into()) })
        })),
        ..Default::default()
    };
    let (report, unchanged) = workflow(generated.clone(), &noop, 0);
    assert_eq!(report["report"]["steps"][1]["result"]["regenerated"], 0);
    assert_eq!(no_calls.load(Ordering::SeqCst), 0);
    assert_eq!(unchanged["src/logo.svg"], SVG_A.as_bytes());

    let mut replaced = unchanged;
    replaced.insert("docs/assets/logo.svg".into(), SVG_B.as_bytes().to_vec());
    assert_ne!(
        package(&replaced)["factHash"],
        package(&generated)["factHash"]
    );
    assert_eq!(pending_ids(&replaced), ["ent:site"]);
    let (execution, calls) = logo_model(SVG_B);
    let (report, final_files) = workflow(replaced, &execution, 1);
    assert_eq!(report["report"]["steps"][1]["result"]["regenerated"], 1);
    assert_eq!(calls.load(Ordering::SeqCst), 7);
    assert_eq!(final_files["src/logo.svg"], SVG_B.as_bytes());
    assert_eq!(final_files["src/footer.html"], original["src/footer.html"]);
    assert_eq!(footer_row(&final_files), footer);
    assert!(pending_ids(&final_files).is_empty());
}

#[test]
fn stale_asset_hash_cannot_record_generation_or_mutate_ledger() {
    let files = reanchor(
        &fixture(),
        LINKED,
        &[("docs/assets/logo.svg", SVG_A.as_bytes())],
    );
    let old_hash = package(&files)["factHash"].as_str().unwrap().to_owned();
    let fs = crate::vfs::Scope::enter(&files);
    let project = project();
    let store = Store::load(&project.out);
    crate::vfs::write("/project/docs/assets/logo.svg", SVG_B).unwrap();
    let before = fs.snapshot();
    let error = mark(
        &store,
        "ent:site",
        Some(&old_hash),
        &manifest("site"),
        &GenSettings::resolve(&project),
    )
    .unwrap_err();
    assert!(error.contains("referenced input bytes moved"), "{error}");
    assert_eq!(
        fs.snapshot(),
        before,
        "A stale completion cannot write the ledger or strip files"
    );
}

#[test]
fn adopted_bindings_without_entity_baseline_open_one_referenced_input_goal() {
    let files = fixture();
    let adopted = {
        let fs = crate::vfs::Scope::enter(&files);
        let project = project();
        let mut ledger = Ledger::load(&project.out);
        ledger.entities.remove("site");
        ledger.save(&project.out).unwrap();
        fs.snapshot()
    };
    assert!(
        pending_ids(&adopted).is_empty(),
        "Current adopted bindings alone do not regenerate source"
    );
    let changed = reanchor(
        &adopted,
        LINKED,
        &[("docs/assets/logo.svg", SVG_A.as_bytes())],
    );
    let _fs = crate::vfs::Scope::enter(&changed);
    let project = project();
    let store = Store::load(&project.out);
    let goals = pending(&store, &GenSettings::resolve(&project));
    assert_eq!(goals.len(), 1, "{goals:?}");
    assert_eq!(goals[0]["entity"], "ent:site");
    assert_eq!(goals[0]["reason"], "referenced-inputs");
    assert_eq!(store.graph.requirements["req:site"].statement, SITE);
}

#[test]
fn protected_escaping_nested_and_missing_inputs_never_enter_the_package_as_readable_bytes() {
    let original = fixture();
    for (href, expected_status) in [
        ("../.env", "blocked"),
        ("../.git/config", "blocked"),
        ("../.jazyk/jazyk.toml", "blocked"),
        ("../../outside.svg", "blocked"),
        ("../nested/logo.svg", "blocked"),
        ("assets/missing.svg", "missing"),
    ] {
        let quote = format!("Site uses the supplied [logo]({href}).");
        let files = reanchor(
            &original,
            &quote,
            &[
                (".env", b"NEVER_EXPOSE_CREDENTIAL"),
                (".git/config", b"NEVER_EXPOSE_GIT"),
                (
                    "nested/.jazyk/jazyk.toml",
                    b"version=1\n[[sets]]\nid='ds_child'\nname='Child'\ndocs=['**']\n",
                ),
                ("nested/logo.svg", b"NEVER_EXPOSE_NESTED"),
            ],
        );
        let package = package(&files);
        let assets = package["referencedAssets"].as_array().unwrap();
        assert_eq!(assets.len(), 1, "{href}: {package}");
        assert_eq!(assets[0]["status"], expected_status, "{href}: {package}");
        assert!(assets[0].get("sha256").is_none(), "{href}: {package}");
        assert!(!package.to_string().contains("NEVER_EXPOSE"));
        let fs = crate::vfs::Scope::enter(&files);
        let project = project();
        let store = Store::load(&project.out);
        let before = fs.snapshot();
        let hash = fact_hash(&store, "ent:site");
        let error = mark(
            &store,
            "ent:site",
            Some(&hash),
            &manifest("site"),
            &GenSettings::resolve(&project),
        )
        .unwrap_err();
        assert!(
            error.contains("Referenced inputs are unavailable"),
            "{href}: {error}"
        );
        assert_eq!(fs.snapshot(), before);
    }
}

#[test]
fn generated_output_reference_does_not_hash_its_own_changing_bytes() {
    let original = fixture();
    let quote = "Site uses the supplied [logo](../src/logo.svg).";
    let mut files = reanchor(&original, quote, &[]);
    let before = package(&files);
    assert_eq!(before["referencedAssets"][0]["path"], "src/logo.svg");
    assert_eq!(before["referencedAssets"][0]["status"], "output");
    assert_eq!(before["factHash"], package(&original)["factHash"]);
    files.insert("src/logo.svg".into(), SVG_B.as_bytes().to_vec());
    assert_eq!(package(&files)["factHash"], before["factHash"]);
    assert!(
        pending_ids(&files).is_empty(),
        "Generated bytes must not open a self-triggering rebuild"
    );
}

#[test]
fn encoded_case_sensitive_binary_reference_hashes_exact_bytes_and_ignores_unquoted_links() {
    let bytes = [0, 255, 17, 128, 13, 10, 42];
    let quote = "Site uses the supplied [logo](assets/My%20Logo.PnG?rev=3#icon).";
    let mut files = reanchor(&fixture(), quote, &[("docs/assets/My Logo.PnG", &bytes)]);
    let mut text = String::from_utf8(files["docs/main.md"].clone()).unwrap();
    text.push_str("\nAn unrelated [asset](assets/unrelated.svg) belongs only to the footer.\n");
    files.insert("docs/main.md".into(), text.into_bytes());
    files.insert(
        "docs/assets/unrelated.svg".into(),
        SVG_A.as_bytes().to_vec(),
    );
    let before = package(&files);
    assert_eq!(before["referencedAssets"].as_array().unwrap().len(), 1);
    let asset = &before["referencedAssets"][0];
    assert_eq!(asset["path"], "docs/assets/My Logo.PnG");
    assert_eq!(asset["status"], "ready");
    assert_eq!(asset["sha256"], format!("{:x}", Sha256::digest(bytes)));
    // The requirement and, by reference, its entity's derived mention.
    assert!(
        asset["references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["fact"] == "req:site"),
        "{}",
        asset
    );
    files.insert(
        "docs/assets/unrelated.svg".into(),
        SVG_B.as_bytes().to_vec(),
    );
    assert_eq!(package(&files)["factHash"], before["factHash"]);
}

#[test]
fn declared_build_output_is_not_an_input_but_undeclared_cache_files_are_blocked() {
    let files = fixture();
    let files = {
        let fs = crate::vfs::Scope::enter(&files);
        let project = project();
        let mut ledger = Ledger::load(&project.out);
        ledger.build = Some(Build {
            run: "make product".into(),
            cwd: ".".into(),
            produces: vec!["target/product.svg".into()],
            last_run: None,
        });
        ledger.save(&project.out).unwrap();
        fs.snapshot()
    };
    let mut generated = reanchor(
        &files,
        "Site uses the supplied [logo](../target/product.svg).",
        &[("target/product.svg", SVG_A.as_bytes())],
    );
    let before = package(&generated);
    assert_eq!(before["referencedAssets"][0]["status"], "output");
    assert!(before["referencedAssets"][0].get("sha256").is_none());
    generated.insert("target/product.svg".into(), SVG_B.as_bytes().to_vec());
    assert_eq!(package(&generated)["factHash"], before["factHash"]);

    let input = reanchor(
        &files,
        "Site uses the supplied [logo](../target/input.svg).",
        &[("target/input.svg", SVG_A.as_bytes())],
    );
    let package = package(&input);
    assert_eq!(package["referencedAssets"][0]["status"], "blocked");
    assert!(package["referencedAssets"][0].get("sha256").is_none());
    let fs = crate::vfs::Scope::enter(&input);
    let project = project();
    let store = Store::load(&project.out);
    let before = fs.snapshot();
    let error = mark(
        &store,
        "ent:site",
        Some(&fact_hash(&store, "ent:site")),
        &manifest("site"),
        &GenSettings::resolve(&project),
    )
    .unwrap_err();
    assert!(
        error.contains("Referenced inputs are unavailable"),
        "{error}"
    );
    assert_eq!(fs.snapshot(), before);
}

#[test]
fn reference_style_images_and_links_use_remote_definitions_but_ignore_code_and_raw_html() {
    let quote = "Site uses the supplied ![logo][brand] and [download][brand].\n\n`[inline](assets/inline.svg)`\n\n```markdown\n[fenced](assets/fenced.svg)\n```\n\n<img src=\"assets/raw.svg\">";
    let mut files = reanchor(
        &fixture(),
        quote,
        &[
            ("docs/assets/logo.svg", SVG_A.as_bytes()),
            ("docs/assets/second.svg", SVG_A.as_bytes()),
            ("docs/assets/inline.svg", SVG_B.as_bytes()),
            ("docs/assets/fenced.svg", SVG_B.as_bytes()),
            ("docs/assets/raw.svg", SVG_B.as_bytes()),
        ],
    );
    let text = String::from_utf8(files["docs/main.md"].clone()).unwrap()
        + "\n# References\n\n[brand]: assets/logo.svg \"Uploaded brand\"\n";
    files.insert("docs/main.md".into(), text.into_bytes());
    let before = package(&files);
    let assets = before["referencedAssets"].as_array().unwrap();
    assert_eq!(
        assets.len(),
        1,
        "Only parsed Markdown links in the quoted range are inputs: {before}"
    );
    assert_eq!(assets[0]["path"], "docs/assets/logo.svg");
    assert_eq!(assets[0]["status"], "ready");
    assert_eq!(assets[0]["references"][0]["href"], "assets/logo.svg");

    // The target path is part of the fingerprint even when both files contain
    // byte-identical uploads and the quote's reference label stays unchanged.
    let text = String::from_utf8(files["docs/main.md"].clone())
        .unwrap()
        .replace("[brand]: assets/logo.svg", "[brand]: assets/second.svg");
    files.insert("docs/main.md".into(), text.into_bytes());
    let after = package(&files);
    assert_eq!(
        after["referencedAssets"][0]["path"],
        "docs/assets/second.svg"
    );
    assert_eq!(after["referencedAssets"][0]["sha256"], assets[0]["sha256"]);
    assert_ne!(after["factHash"], before["factHash"]);
}

#[cfg(unix)]
#[test]
fn native_symlinks_cannot_import_outside_or_private_input_bytes() {
    let baseline = fixture();
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    for (path, bytes) in baseline {
        let target = root.path().join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, bytes).unwrap();
    }
    std::fs::create_dir_all(root.path().join("docs/assets")).unwrap();
    std::fs::write(outside.path().join("logo.svg"), "NEVER_READ_OUTSIDE").unwrap();
    std::fs::write(root.path().join(".env"), "NEVER_READ_CREDENTIAL").unwrap();
    std::fs::create_dir_all(root.path().join("nested/.jazyk")).unwrap();
    std::fs::write(root.path().join("nested/.jazyk/jazyk.toml"), "version=1\n").unwrap();
    std::fs::write(root.path().join("nested/logo.svg"), "NEVER_READ_NESTED").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("logo.svg"),
        root.path().join("docs/assets/outside.svg"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        root.path().join("nested/logo.svg"),
        root.path().join("docs/assets/nested.svg"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        root.path().join(".env"),
        root.path().join("docs/assets/alias.svg"),
    )
    .unwrap();
    let project = crate::document_sets::Workspace::load(root.path())
        .unwrap()
        .resolve(None)
        .unwrap();
    for name in ["outside", "alias", "nested"] {
        let quote = format!("Site uses the supplied [logo](assets/{name}.svg).");
        std::fs::write(root.path().join("docs/main.md"), document(&quote)).unwrap();
        let mut store = Store::load(&project.out);
        let requirement = store.graph.requirements.get_mut("req:site").unwrap();
        requirement.source = Some(source("/site", &quote));
        requirement.provenance = Some(Provenance::Quote(source("/site", &quote)));
        let gs = GenSettings::resolve(&project);
        let package = task_package(&store, "ent:site", &gs).unwrap();
        assert_eq!(
            package["referencedAssets"][0]["status"], "blocked",
            "{name}: {package}"
        );
        assert!(package["referencedAssets"][0].get("sha256").is_none());
        assert!(!package.to_string().contains("NEVER_READ"));
        let before = std::fs::read(project.out.join("gen/ledger.yaml")).unwrap();
        let error = mark(
            &store,
            "ent:site",
            Some(&fact_hash(&store, "ent:site")),
            &manifest("site"),
            &gs,
        )
        .unwrap_err();
        assert!(
            error.contains("Referenced inputs are unavailable"),
            "{error}"
        );
        assert_eq!(
            std::fs::read(project.out.join("gen/ledger.yaml")).unwrap(),
            before
        );
        let mut tools = crate::tools::ToolSession::new(
            store,
            crate::tools::WorkScope::serving("generate"),
            20,
            8000,
        );
        tools.gen = gs;
        assert!(
            tools
                .dispatch(
                    "read_text_file",
                    &json!({"path":format!("docs/assets/{name}.svg")})
                )
                .is_err(),
            "The worker must not read the same unsafe alias by guessing its path"
        );
    }
}

fn generation_request(
    files: &Snapshot,
    execution: &Execution,
) -> (Result<Value, String>, Snapshot) {
    use crate::runner::SessionRunner;
    let fs = crate::vfs::Scope::enter(files);
    let _state = crate::scoped_state::Scope::enter();
    let _execution = crate::hosted::test_execution_scope(execution.clone());
    let _recovery = crate::session_recovery::Scope::enter(execution.resume.as_ref());
    let project = project();
    let store = Store::load(&project.out);
    let guard = crate::control::begin_internal_build(&project, &project.out, "generate").unwrap();
    let llm = crate::llm::Llm {
        api: "chat_completions".into(),
        reasoning_summary: None,
        base_url: "http://127.0.0.1:1/v1".into(),
        model: "asset-release-recovery".into(),
        api_key: String::new(),
        temperature: None,
        trace: None,
    };
    let runner = crate::inprocess::InProcessRunner::start(&project, &llm, &project.out).unwrap();
    runner.set_build_token(Some(guard.token().to_owned()));
    let trace = crate::session::Trace::to_sink(
        crate::session::TraceLevel::Quiet,
        Arc::new(|_| {}),
        Default::default(),
    );
    let result = run_selected(
        &store,
        &runner,
        &GenSettings::resolve(&project),
        &[],
        false,
        &trace,
        false,
    );
    drop(guard);
    (result, fs.snapshot())
}

// Keep the actual captured GDK model/tool/checkpoint state and express only the
// previous release's wire fields and fingerprint algorithm. There is no alternate
// production runtime or test-only dependency switch.
fn previous_release_capsule(
    mut capsule: Value,
    files: &Snapshot,
    current_hash: &str,
) -> (Value, Snapshot) {
    let fs = crate::vfs::Scope::enter(files);
    let project = project();
    let store = Store::load(&project.out);
    let base = fact_hash_base(&store, "ent:site", &reqs_of_sorted(&store, "ent:site"));
    fn old_wire(value: &mut Value, current: &str, base: &str) {
        match value {
            Value::Object(object) => {
                object.remove("before_sequences");
                object.remove("recordSequence");
                object.remove("referencedAssets");
                for value in object.values_mut() {
                    old_wire(value, current, base);
                }
            }
            Value::Array(values) => {
                for value in values {
                    old_wire(value, current, base);
                }
            }
            Value::String(text) => {
                if let Ok(mut json) = serde_json::from_str::<Value>(text) {
                    old_wire(&mut json, current, base);
                    *text = json.to_string();
                } else {
                    *text = text.replace(current, base);
                }
            }
            _ => {}
        }
    }
    old_wire(&mut capsule, current_hash, &base);
    // A previous release recorded no argument digest on its tool receipts.
    fn strip_digests(value: &mut Value) {
        match value {
            Value::Object(object) => {
                if let Some(Value::Object(receipts)) = object.get_mut("tool_receipts") {
                    for receipt in receipts.values_mut() {
                        if let Some(receipt) = receipt.as_object_mut() {
                            receipt.remove("args");
                        }
                    }
                }
                object.values_mut().for_each(strip_digests);
            }
            Value::Array(values) => values.iter_mut().for_each(strip_digests),
            _ => {}
        }
    }
    strip_digests(&mut capsule);
    // Parent keys were derived from the signature, so recompute exactly the old
    // key while preserving every captured sequence, frame and request receipt.
    let parents = capsule["parent"].as_object_mut().unwrap();
    let old_entry = parents
        .iter()
        .find(|(_, entry)| entry["data"]["session"]["batch"]["id"] == "g:generate:ent:site")
        .map(|(key, value)| (key.clone(), value.clone()))
        .unwrap();
    parents.remove(&old_entry.0);
    let key = progress::command_key(
        "generation-session",
        &project.out,
        &old_entry.1["signature"],
    );
    parents.insert(key, old_entry.1);
    let mut ledger = Ledger::load(&project.out);
    let row = ledger.entities.get_mut("site").unwrap();
    row.fact_hash = base;
    row.record_sequence = 0;
    ledger.save(&project.out).unwrap();
    (capsule, fs.snapshot())
}

#[test]
fn pre_input_release_capsules_resume_saved_calls_and_refresh_committed_placeholder_records() {
    use std::sync::atomic::AtomicBool;
    for (phase, stop_round) in [
        ("before_tool", 2usize),
        ("after_tool", 2),
        ("after_tool", 3),
    ] {
        let files = reanchor(
            &fixture(),
            LINKED,
            &[("docs/assets/logo.svg", SVG_A.as_bytes())],
        );
        let current_hash = package(&files)["factHash"].as_str().unwrap().to_owned();
        let files = {
            let fs = crate::vfs::Scope::enter(&files);
            let mut ledger = Ledger::load(&project().out);
            ledger.entities.get_mut("site").unwrap().recorded_at =
                Some("2000-01-01T00:00:00Z".into());
            ledger.save(&project().out).unwrap();
            fs.snapshot()
        };
        let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
        let observed = requests.clone();
        let seen_hash = Mutex::new(current_hash.clone());
        let model = Arc::new(move |body: String| {
            let request: Value = serde_json::from_str(&body).unwrap();
            let mut requests = observed.lock().unwrap();
            requests.push(request.clone());
            let round = requests.len();
            let repair_start = if phase == "before_tool" { 3 } else { 4 };
            let (tool, args) = if round == 1 {
                ("begin_generation", json!({"entity":"ent:site"}))
            } else if round == 2 {
                // The old worker records an existing placeholder. The actual
                // captured receipt is converted to the prior wire hash below.
                (
                    "record_generation",
                    json!({"entity":"ent:site", "factHash":*seen_hash.lock().unwrap(), "manifest":manifest("site")}),
                )
            } else if round == 3 && phase != "before_tool" {
                (
                    "done",
                    json!({"summary":"The previous worker ended its generation."}),
                )
            } else {
                match round - repair_start {
                    0 => ("begin_generation", json!({"entity":"ent:site"})),
                    1 => {
                        let package = previous_tool(&request, "referencedAssets");
                        assert_eq!(package["referencedAssets"][0]["status"], "ready");
                        *seen_hash.lock().unwrap() = package["factHash"].as_str().unwrap().into();
                        ("read_text_file", json!({"path":"docs/assets/logo.svg"}))
                    }
                    2 => {
                        let read = previous_tool(&request, "totalLines");
                        assert_eq!(read["content"], SVG_A);
                        (
                            "write_text_file",
                            json!({"path":"src/logo.svg", "content":read["content"]}),
                        )
                    }
                    3 => (
                        "write_text_file",
                        json!({"path":"src/site.html", "content":"<!doctype html><img src=\"logo.svg\" alt=\"Supplied logo\">"}),
                    ),
                    4 => (
                        "record_generation",
                        json!({"entity":"ent:site", "factHash":*seen_hash.lock().unwrap(), "manifest":manifest("site")}),
                    ),
                    5 => {
                        assert_eq!(previous_tool(&request, "marked")["marked"], "ent:site");
                        (
                            "done",
                            json!({"summary":"The supplied input is now used exactly."}),
                        )
                    }
                    _ => panic!(
                        "A saved call was rerequested or a completed component replayed: {request}"
                    ),
                }
            };
            let reply = json!({"choices":[{"message":{"role":"assistant", "content":null,"tool_calls":[{
                "id":format!("release-{round}"),"type":"function","function":{"name":tool,"arguments":args.to_string()}
            }]}}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}}).to_string();
            Box::pin(async move { Ok(reply) })
                as std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<String, String>> + Send>,
                >
        });
        let checkpoint = Arc::new(Mutex::new(None::<(Value, Snapshot)>));
        let saved = checkpoint.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let sink = Arc::new(move |mut capsule: Value, files: Snapshot| {
            let next = capsule["sequence"].as_u64().unwrap_or(0) + 1;
            capsule["sequence"] = json!(next);
            let matches = capsule["frames"]
                .as_array()
                .and_then(|frames| frames.last())
                .is_some_and(|frame| {
                    frame["key"]
                        .as_str()
                        .is_some_and(|key| key.ends_with("g:generate:ent:site"))
                        && frame["boundary"]["phase"] == phase
                        && frame["harness"]["rounds"] == stop_round
                });
            let crash = matches && !stop.swap(true, Ordering::SeqCst);
            if crash {
                *saved.lock().unwrap() = Some((capsule, files));
            }
            Box::pin(async move {
                if crash {
                    Err("checkpoint-failed: test suspends an old release boundary".into())
                } else {
                    Ok(next)
                }
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64, String>> + Send>>
        });
        let mut execution = Execution {
            model: "asset-release-recovery".into(),
            transport_async: Some(model),
            checkpoint: Some(sink),
            ..Default::default()
        };
        let (stopped, _) = generation_request(&files, &execution);
        assert!(
            stopped.unwrap_err().contains("checkpoint-failed:"),
            "{phase}/{stop_round}"
        );
        assert_eq!(requests.lock().unwrap().len(), stop_round);
        let (capsule, protected) = checkpoint.lock().unwrap().clone().unwrap();
        assert_eq!(
            protected["src/logo.svg"],
            OLD_SVG.as_bytes(),
            "The old placeholder has not been repaired"
        );
        let (capsule, protected) = previous_release_capsule(capsule, &protected, &current_hash);
        crate::session_recovery::validate(&capsule).unwrap();
        execution.resume = Some(capsule);
        let (complete, final_files) = generation_request(&protected, &execution);
        let complete = complete.unwrap();
        assert_eq!(
            complete["regenerated"], 1,
            "{phase}/{stop_round}: {complete}"
        );
        assert_eq!(complete["failures"], 0, "{phase}/{stop_round}: {complete}");
        assert_eq!(final_files["src/logo.svg"], SVG_A.as_bytes());
        assert_eq!(final_files["src/footer.html"], files["src/footer.html"]);
        assert!(pending_ids(&final_files).is_empty());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), if phase == "before_tool" { 8 } else { 9 });
        let request_ids: std::collections::BTreeSet<_> = requests
            .iter()
            .map(|request| request["_jazyk"]["requestId"].as_str().unwrap())
            .collect();
        assert_eq!(
            request_ids.len(),
            requests.len(),
            "Restoring a retained call must not readmit inference"
        );
    }
}
