use super::*;

// The one-pass requirement index answers what the per-entity scan answers, for
// every entity, and the fact hash over it is the fact hash.
#[test]
fn reqs_by_entity_matches_the_per_entity_scan() {
    let mut synthetic = crate::derive::tests::synthetic_store(300, 700);
    synthetic
        .graph
        .redirects
        .insert("ent:old".into(), "ent:e1".into());
    synthetic.graph.requirements.insert(
        "req:redirected".into(),
        crate::model::Requirement {
            statement: "The old one, twice.".into(),
            entities: vec!["ent:old".into(), "ent:e1".into(), "ent:e2".into()],
            ..Default::default()
        },
    );
    for s in [synthetic, crate::board::tests::settled_store()] {
        let index = ReqsByEntity::build(&s);
        assert!(index.of("ent:nope").is_empty());
        for id in s.graph.entities.keys() {
            assert_eq!(index.of(id), reqs_of_sorted(&s, id).as_slice(), "{}", id);
            assert_eq!(
                fact_hash_over(&s, id, index.of(id)),
                fact_hash(&s, id),
                "{}",
                id
            );
        }
    }
}

// The generate gate is the ledger's word: no record, a stale factHash, or
// recorded files that are not there each bounce a done claim naming the repair;
// with run_tests required, a programmatic row never run bounces it too.
// Mirrors docs/compiler/goals/generate.md#gate.
#[test]
fn the_generate_gate_reads_the_landed_record() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-gate-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, gs) = fixture(&out);
    let err = gate(&s, &gs, "ent:cart", false).unwrap_err();
    assert!(err.contains("record_generation has not landed"), "{}", err);
    crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "fn a() {}\n").unwrap();
    let name = test_name("req:shop-1", "The Cart shall hold items.");
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        format!("fn {}() {{}}\n", name),
    )
    .unwrap();
    let manifest = json!({"files": ["src/cart.rs", "tests/cart.rs"], "tests": [{
        "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
        "artifact": "tests/cart.rs", "name": name, "run": "true", "files": ["src/cart.rs"]}]});
    mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
    assert_eq!(gate(&s, &gs, "ent:cart", false).unwrap(), 2);
    let err = gate(&s, &gs, "ent:cart", true).unwrap_err();
    assert!(
        err.contains("run_tests has not run") && err.contains("req:shop-1"),
        "{}",
        err
    );
    crate::verify::run_selected(&s, &gs, &[]).unwrap();
    gate(&s, &gs, "ent:cart", true).unwrap();
    crate::vfs::remove_file(gs.deliverable.join("src/cart.rs")).unwrap();
    let err = gate(&s, &gs, "ent:cart", true).unwrap_err();
    assert!(
        err.contains("do not exist") && err.contains("src/cart.rs"),
        "{}",
        err
    );
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "fn a() {}\n").unwrap();
    s.graph.entities.get_mut("ent:cart").unwrap().definition = Some("moved".into());
    let err = gate(&s, &gs, "ent:cart", false).unwrap_err();
    assert!(err.contains("facts moved"), "{}", err);
    crate::vfs::remove_dir_all(&out).ok();
}

// The fact hash covers everything the ledger documents: stereotype, attributes,
// and a statement's edges flip it, so those changes become generation work.
// Mirrors docs/consumers/gen.md#the-ledger.
#[test]
fn fact_hash_flips_on_stereotype_attributes_and_edges() {
    let out =
        std::env::temp_dir().join(format!("jazyk-facthash-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, _gs) = fixture(&out);
    let h0 = fact_hash(&s, "ent:cart");
    s.graph.entities.get_mut("ent:cart").unwrap().stereotype = Some("service".into());
    let h1 = fact_hash(&s, "ent:cart");
    assert_ne!(h0, h1, "a stereotype change flips the hash");
    s.graph
        .entities
        .get_mut("ent:cart")
        .unwrap()
        .attributes
        .push(Attribute {
            name: "capacity".into(),
            r#type: Some("u32".into()),
            value: None,
            provenance: Provenance::Quote(SourceRef {
                doc: "shop.md".into(),
                section: "/shop".into(),
                quote: "holds".into(),
            }),
        });
    let h2 = fact_hash(&s, "ent:cart");
    assert_ne!(h1, h2, "an attribute change flips the hash");
    s.graph
        .requirements
        .get_mut("req:shop-1")
        .unwrap()
        .edges
        .push(ReqEdge {
            a: "ent:cart".into(),
            b: "ent:item".into(),
            rel_type: Some("composition".into()),
            cardinality: Some("1..*".into()),
        });
    let h3 = fact_hash(&s, "ent:cart");
    assert_ne!(h2, h3, "an edge change flips the hash");
    crate::vfs::remove_dir_all(&out).ok();
}

// An unbound requirement yields a ledger-stale change record naming its goal,
// so the board derives bind (and generate for the entity) from it.
// Mirrors docs/compiler/goals/bind.md#created-when.
#[test]
fn an_unbound_requirement_yields_a_ledger_stale_record() {
    let out = std::env::temp_dir().join(format!("jazyk-stale-rec-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    let records = ledger_stale_records(&s, &gs);
    let bind = records
        .iter()
        .find(|r| r.detail["goal"] == "bind")
        .expect("a bind record");
    assert_eq!(bind.kind, "ledger-stale");
    assert_eq!(bind.subject, "req:shop-1");
    assert_eq!(bind.via, "ledger");
    assert_eq!(bind.detail["reason"], "unbound");
    assert!(bind.id.starts_with('c'), "{}", bind.id);
    let generation = records
        .iter()
        .find(|record| record.detail["goal"] == "generate")
        .unwrap();
    assert_eq!(generation.subject, "ent:cart");
    assert_eq!(generation.detail["reason"], "incomplete-bindings");
    // Bound and unimplemented: the entity is generation work under that reason.
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
    let name = test_name("req:shop-1", "The Cart shall hold items.");
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        format!("fn {}() {{}}", name),
    )
    .ok();
    let bound = json!({"kind": "programmatic", "artifact": "tests/cart.rs", "name": name, "run": format!("cargo test {}", name)});
    crate::bind::record(&s, "req:shop-1", &[], &bound, &json!([]), "fail", None, &gs).unwrap();
    let records = ledger_stale_records(&s, &gs);
    let g = records
        .iter()
        .find(|r| r.detail["goal"] == "generate")
        .expect("a generate record");
    assert_eq!(g.subject, "ent:cart");
    assert_eq!(g.detail["reason"], "unimplemented");
    assert_eq!(g.detail["changed"][0], "req:shop-1");
    crate::vfs::remove_dir_all(&out).ok();
}
