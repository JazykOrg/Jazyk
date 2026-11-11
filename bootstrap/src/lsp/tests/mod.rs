mod annotations;
mod document;
mod navigation;

use super::document::{occurrences, path_to_uri, section_anchor, section_lead, uri_to_path};
use super::*;
use crate::gen::{GenSettings, Ledger, ReqRow, RowHashes, Site, TestRef};
use crate::md;
use crate::model::{Entity, Requirement, SourceRef};

fn lsp_at(root: std::path::PathBuf, out: std::path::PathBuf, store: Store) -> Lsp {
    // The store is in memory only: key the refresh to disk so it never reloads.
    let fingerprint = store_fingerprint(&root, &out);
    Lsp {
        root,
        out: out.clone(),
        store,
        generation: 0,
        fingerprint,
        gen: GenSettings {
            configured: true,
            deliverable: out.join("product"),
            worker: "agentic".into(),
            code: Vec::new(),
        },
        overlay: HashMap::new(),
        next_srv_id: 1,
        compiler: Compiler::Stdio {
            opts: Default::default(),
            running: None,
        },
    }
}

// The showcase graph with its derived data, its out directory under `tmp`.
fn shop_at(out: &std::path::Path) -> Store {
    let mut s = crate::derive::tests::showcase_store();
    s.out = out.to_path_buf();
    s.graph
        .entities
        .get_mut("ent:order-service")
        .unwrap()
        .definition = Some("Takes and fulfils orders.".into());
    let mut batch = crate::store::RecordBatch::new(1);
    crate::derive::recompute(&mut s, "g1", &mut batch);
    s
}

// The refresh keys on one line per set plus the configuration hash, and loads
// the graph only when one moved (docs/frontends/lsp.md#rebuilds-and-refresh).
#[test]
fn refresh_reloads_only_when_a_set_generation_or_the_configuration_moved() {
    let tmp = std::env::temp_dir().join(format!("jz-lsp-fingerprint-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join(".jazyk")).unwrap();
    let config = |extra: &str| {
        format!(
            "version = 1\n\n[[sets]]\nid = \"ds_a\"\nname = \"A\"\ndocs = [\"a/**\"]\n\n[[sets]]\nid = \"ds_b\"\nname = \"B\"\ndocs = [\"b/**\"]\n{}",
            extra
        )
    };
    std::fs::write(tmp.join(".jazyk/jazyk.toml"), config("")).unwrap();
    for id in ["ds_a", "ds_b"] {
        let mut s = crate::store::Store::load(&tmp.join(".jazyk").join(id));
        s.status.generation = 1;
        s.save_for_test();
    }
    let out = tmp.join(".jazyk");
    let first = super::store_fingerprint(&tmp, &out);
    assert_eq!(first, super::store_fingerprint(&tmp, &out));
    let gs = crate::gen::GenSettings {
        configured: false,
        deliverable: tmp.join("product"),
        worker: "agentic".into(),
        code: Vec::new(),
    };
    let mut lsp = super::Lsp::new(tmp.clone(), out.clone(), gs);
    let loaded = lsp.generation;
    let mut sink = Vec::new();
    lsp.refresh(&mut sink);
    assert_eq!(lsp.generation, loaded);
    assert!(sink.is_empty(), "no reload, nothing republished");
    // One set's generation moves: the fingerprint and the loaded store follow.
    let mut s = crate::store::Store::load(&out.join("ds_b"));
    s.status.generation = 2;
    s.save_for_test();
    let second = super::store_fingerprint(&tmp, &out);
    assert_ne!(first, second);
    lsp.refresh(&mut sink);
    assert_ne!(lsp.generation, loaded);
    assert_eq!(lsp.fingerprint, second);
    // The configuration moves without any generation: a reload too.
    std::fs::write(
        tmp.join(".jazyk/jazyk.toml"),
        config("\n[llm]\nmodel = \"m\"\n"),
    )
    .unwrap();
    assert_ne!(second, super::store_fingerprint(&tmp, &out));
    // A set's own out directory keys on its generation alone.
    assert_eq!(super::store_fingerprint(&tmp, &out.join("ds_b")), "2");
    let _ = std::fs::remove_dir_all(&tmp);
}

// `jazyk.compile` builds one set: the named one, or the only one; in a project of
// several sets a call without setId starts nothing. Inside the GUI it is a job
// (docs/frontends/lsp.md#commands).
#[test]
fn the_compile_command_addresses_one_set_through_the_frontends_build_path() {
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let add = |id: &str| {
        let w = crate::document_sets::Workspace::load(&root).unwrap();
        crate::document_sets::edit(&root, &json!({"baseHash": w.hash, "set": {"id": id, "name": id, "docs": [format!("{}.md", id)]}}), false).unwrap();
    };
    add("ds_one");
    let submitted: Arc<Mutex<Vec<(String, String)>>> = Default::default();
    let sink = submitted.clone();
    let mut lsp = Lsp::new(
        root.clone(),
        root.join(".jazyk"),
        GenSettings::resolve(&crate::project::Project::load(&root)),
    )
    .with_job_submitter(Box::new(move |id, hash| {
        sink.lock().unwrap().push((id.clone(), hash));
        json!({"started": [id], "jobId": 7})
    }));
    let call = |lsp: &mut Lsp, args: Value| {
        let mut out = Vec::new();
        lsp.on_execute_command(
            &json!({"command": "jazyk.compile", "arguments": [args]}),
            &mut out,
        )
    };
    let reply = call(&mut lsp, json!({}));
    assert_eq!(reply["started"], json!(["ds_one"]));
    assert_eq!(reply["jobId"], 7);
    add("ds_two");
    let reply = call(&mut lsp, json!({}));
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .starts_with("choose a document set"),
        "{}",
        reply
    );
    let reply = call(&mut lsp, json!({"setId": "ds_two"}));
    assert_eq!(reply["started"], json!(["ds_two"]));
    let calls = submitted.lock().unwrap().clone();
    assert_eq!(
        calls.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
        vec!["ds_one", "ds_two"]
    );
    assert_eq!(
        calls[1].1,
        crate::document_sets::Workspace::load(&root).unwrap().hash
    );
    // Over stdio the build runs in a thread the server waits for on exit.
    let mut stdio = Lsp::new(
        root.clone(),
        root.join(".jazyk"),
        GenSettings::resolve(&crate::project::Project::load(&root)),
    );
    assert!(call(&mut stdio, json!({})).get("error").is_some());
    let reply = call(&mut stdio, json!({"setId": "ds_one"}));
    assert_eq!(reply["started"], json!(["ds_one"]));
    stdio.compiler.finish();
    assert!(matches!(
        &stdio.compiler,
        Compiler::Stdio { running: None, .. }
    ));
}
