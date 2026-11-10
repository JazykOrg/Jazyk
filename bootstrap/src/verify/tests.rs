use super::*;
use crate::gen::{mark as gen_mark, test_name, GenSettings};
use crate::model::*;

pub(super) fn fixture(out: &std::path::Path) -> (Store, GenSettings) {
    let mut s = Store {
        out: out.to_path_buf(),
        ..Default::default()
    };
    s.graph.entities.insert(
        "ent:cart".into(),
        Entity {
            name: "Cart".into(),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:shop-1".into(),
        Requirement {
            statement: "The Cart shall hold items.".into(),
            entities: vec!["ent:cart".into()],
            edges: vec![],
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop".into(),
                quote: "holds".into(),
            }),
            ..Default::default()
        },
    );
    let gs = GenSettings {
        configured: true,
        deliverable: out.join("product"),
        worker: "agentic".into(),
        code: Vec::new(),
    };
    crate::vfs::create_dir_all(gs.deliverable.join("src")).unwrap();
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
    let name = test_name("req:shop-1", "The Cart shall hold items.");
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// product").unwrap();
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        format!("// req:shop-1 hash:x\nfn {}() {{}}\n", name),
    )
    .unwrap();
    let manifest = serde_json::json!({
        "files": ["src/cart.rs", "tests/cart.rs"],
        "tests": [{
            "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
            "artifact": "tests/cart.rs", "name": name, "run": "true",
            "files": ["src/cart.rs"],
        }],
    });
    gen_mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
    (s, gs)
}

pub(super) fn multi_llm_fixture(out: &std::path::Path) -> (Store, GenSettings) {
    let (mut store, gs) = fixture(out);
    let mut ledger = Ledger::load(out);
    let row = ledger.requirements.get_mut("req:shop-1").unwrap();
    for id in ["crit:first", "crit:second"] {
        store.graph.criteria.insert(
            id.into(),
            Criterion {
                requirement: "req:shop-1".into(),
                statement: format!("Check {id}"),
                ..Default::default()
            },
        );
        let mut test = row.test.clone();
        test.kind = "llm".into();
        test.name = id.into();
        test.run = "llm".into();
        test.artifact = format!("tests/{}.md", id.replace(':', "-"));
        let hash = hash_hex(&store.graph.criteria[id].statement);
        crate::vfs::write(gs.deliverable.join(&test.artifact),format!("---\nrequirement: req:shop-1\ncriterion: {id}\nhash: {hash}\n---\nInspect source. PASS if satisfied. FAIL otherwise.\n")).unwrap();
        row.tests.insert(
            id.into(),
            crate::gen::CritTest {
                test_hash: hash_file(&artifact_path(out, &gs, &test)),
                test,
                criterion_hash: hash,
                verdict: "pass".into(),
                last_run: None,
                exit_code: None,
                evidence: None,
            },
        );
    }
    row.test = row.tests["crit:first"].test.clone();
    row.hashes.test = row.tests["crit:first"].test_hash.clone();
    row.verdict = "pass".into();
    ledger.save(out).unwrap();
    (store, gs)
}

#[test]
fn iso_clock_shape() {
    let t = now_iso();
    assert_eq!(t.len(), 20, "{}", t);
    assert!(t.ends_with('Z'));
    assert!(t.starts_with("20"));
}
