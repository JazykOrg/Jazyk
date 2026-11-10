use super::*;

#[test]
fn task_build_requires_promised_products() {
    let _scope = crate::vfs::Scope::enter(&Default::default());
    let out = Path::new("/project/.jazyk");
    let (_, gs) = fixture(out);
    let build = Build {
        run: "task:build".into(),
        cwd: ".".into(),
        produces: vec!["product.txt".into()],
        last_run: None,
    };
    let mut ledger = Ledger::load(out);
    ledger.build = Some(build.clone());
    ledger.save(out).unwrap();
    assert!(check_build_products(out, &gs, &build)
        .unwrap_err()
        .contains("did not produce product.txt"));
    assert!(!Ledger::load(out).build.unwrap().last_run.unwrap().ok);
    crate::vfs::write(gs.deliverable.join("product.txt"), "ready").unwrap();
    check_build_products(out, &gs, &build).unwrap();
    assert!(Ledger::load(out).build.unwrap().last_run.unwrap().ok);
}

#[test]
fn recorded_medium_is_not_decided_again_before_the_first_entity_generation() {
    let out = std::env::temp_dir().join(format!("jazyk-medium-once-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (store, gs) = fixture(&out);
    Ledger {
        medium: Some(Medium {
            form: "Rust crate".into(),
            produced: "written".into(),
            toolchain: "cargo".into(),
            artifact: String::new(),
        }),
        ..Default::default()
    }
    .save(&out)
    .unwrap();
    assert!(Ledger::load(&out).entities.is_empty());
    let runner = StubRunner {
        record: true,
        failed: None,
        stopped: None,
        store: store.clone(),
        gs: gs.clone(),
    };
    let trace = crate::session::Trace::to_sink(
        crate::session::TraceLevel::Quiet,
        std::sync::Arc::new(|_| {}),
        Default::default(),
    );
    // StubRunner refuses every ask: deciding again would fail this run.
    let result = run_selected(
        &store,
        &runner,
        &gs,
        &["ent:cart".into()],
        false,
        &trace,
        false,
    )
    .unwrap();
    assert_eq!(result["regenerated"], 1, "{result}");
    assert_eq!(result["failures"], 0, "{result}");
    assert_eq!(Ledger::load(&out).medium.unwrap().form, "Rust crate");
    crate::vfs::remove_dir_all(&out).unwrap();
}

// The divergence check fires only between two known tool families.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
#[test]
fn medium_divergence_names_disjoint_tool_families() {
    let row = |run: &str| ReqRow {
        tests: std::collections::BTreeMap::new(),
        entity: "ent:cart".into(),
        files: Vec::new(),
        sites: Vec::new(),
        test: TestRef {
            kind: "programmatic".into(),
            label: "unit".into(),
            artifact: "tests/cart.rs".into(),
            name: "req_shop_1".into(),
            run: run.into(),
            cwd: ".".into(),
        },
        hashes: RowHashes::default(),
        verdict: "none".into(),
        last_run: None,
        exit_code: None,
        evidence: None,
    };
    let mut ledger = Ledger {
        medium: Some(Medium {
            form: "Python package".into(),
            produced: "written".into(),
            toolchain: "python3 with pytest".into(),
            artifact: String::new(),
        }),
        ..Default::default()
    };
    assert!(medium_divergence(&ledger).is_none(), "no rows, no verdict");
    ledger
        .requirements
        .insert("req:shop-1".into(), row("cargo test req_shop_1"));
    let w = medium_divergence(&ledger).expect("python beside cargo diverges");
    assert!(
        w.contains("python") && w.contains("rust") && w.contains("cargo test"),
        "{}",
        w
    );
    // The same family on both sides, or an unknown one on either, is no divergence.
    ledger.medium.as_mut().unwrap().toolchain = "rustc and cargo test".into();
    assert!(medium_divergence(&ledger).is_none());
    ledger.medium.as_mut().unwrap().toolchain = "a bespoke DSL".into();
    assert!(medium_divergence(&ledger).is_none());
    ledger.medium.as_mut().unwrap().toolchain = "python3 with pytest".into();
    ledger
        .requirements
        .insert("req:shop-1".into(), row("sh tests/shop.sh"));
    assert!(medium_divergence(&ledger).is_none());
}

// Once an entity is generated the medium stands: the record warns and keeps it.
// Mirrors docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated.
#[test]
fn mark_warns_when_run_commands_diverge_from_the_medium() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-diverge-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    crate::vfs::create_dir_all(&out).ok();
    let ledger = Ledger {
        medium: Some(Medium {
            form: "Python package".into(),
            produced: "written".into(),
            toolchain: "python3 with pytest".into(),
            artifact: String::new(),
        }),
        ..Default::default()
    };
    ledger.save(&out).unwrap();
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
    let name = test_name("req:shop-1", "The Cart shall hold items.");
    crate::vfs::write(gs.deliverable.join("cart.rs"), "fn hold() {}\n").ok();
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        format!("fn {}() {{}}\n", name),
    )
    .ok();
    let manifest = json!({
        "files": ["cart.rs", "tests/cart.rs"],
        "tests": [{
            "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
            "artifact": "tests/cart.rs", "name": name,
            "run": format!("cargo test {}", name), "files": ["cart.rs"],
        }],
    });
    let reply = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
    let w = reply["mediumWarning"].as_str().expect("the record warns");
    assert!(w.contains("python") && w.contains("rust"), "{}", w);
    let ledger = Ledger::load(&out);
    assert_eq!(
        ledger.medium.as_ref().map(|m| m.toolchain.as_str()),
        Some("python3 with pytest"),
        "an entity is generated, so the medium stands"
    );
    // Every later package carries the standing divergence.
    let pkg = task_package(&s, "ent:cart", &gs).unwrap();
    assert!(pkg["mediumWarning"].as_str().is_some(), "{}", pkg);
}

// One build per deliverable: the first task that needs one establishes it, later
// tasks receive it in their package and cannot replace it.
// Mirrors docs/consumers/gen.md#the-build.
#[test]
fn build_is_recorded_once_and_carried_into_later_packages() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-build-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    assert!(task_package(&s, "ent:cart", &gs).unwrap()["build"].is_null());

    crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// product").ok();
    mark(
        &s,
        "ent:cart",
        None,
        &serde_json::json!({
            "files": ["src/cart.rs"],
            "build": {"run": "python build_deck.py", "cwd": ".", "produces": ["deck.pptx"]},
            "tests": [],
        }),
        &gs,
    )
    .unwrap();
    let b = Ledger::load(&out).build.unwrap();
    assert_eq!(b.run, "python build_deck.py");
    assert_eq!(b.produces, vec!["deck.pptx".to_string()]);
    // The next task sees it and a second recording does not take.
    assert_eq!(
        task_package(&s, "ent:cart", &gs).unwrap()["build"]["run"],
        "python build_deck.py"
    );
    mark(
        &s,
        "ent:cart",
        None,
        &serde_json::json!({"files": ["src/cart.rs"], "build": {"run": "make all"}, "tests": []}),
        &gs,
    )
    .unwrap();
    assert_eq!(
        Ledger::load(&out).build.unwrap().run,
        "python build_deck.py"
    );
    crate::vfs::remove_dir_all(&out).ok();
}

#[test]
fn built_medium_demands_a_build_that_names_the_artifact() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-medium-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    let mut ledger = Ledger::load(&out);
    ledger.medium = Some(Medium {
        form: "Microsoft PowerPoint deck".into(),
        produced: "built".into(),
        toolchain: "python3 with python-pptx".into(),
        artifact: "jazyk.pptx".into(),
    });
    ledger.save(&out).unwrap();
    assert!(build_outputs(&ledger).contains("jazyk.pptx"));
    // The decision reaches the task package.
    let task = task_package(&s, "ent:cart", &gs).unwrap();
    assert_eq!(task["medium"]["produced"], "built");
    assert_eq!(task["medium"]["artifact"], "jazyk.pptx");

    crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
    // A marked file: the rejection must leave its markers for the retry.
    let marked = "// req:shop-1 hash:abcd1234\nfn hold() {}\n";
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), marked).ok();
    let no_build = serde_json::json!({"files": ["src/cart.rs"], "tests": []});
    let err = mark(&s, "ent:cart", None, &no_build, &gs).unwrap_err();
    assert!(err.contains("no build is recorded"), "{}", err);
    assert_eq!(
        crate::vfs::read_to_string(gs.deliverable.join("src/cart.rs")).unwrap(),
        marked
    );

    // A build that forgets the artifact still produces it: the decision fills it in.
    mark(
        &s,
        "ent:cart",
        None,
        &serde_json::json!({
            "files": ["src/cart.rs"],
            "build": {"run": "python3 build_deck.py", "cwd": "."},
            "tests": [],
        }),
        &gs,
    )
    .unwrap();
    let b = Ledger::load(&out).build.unwrap();
    assert_eq!(b.produces, vec!["jazyk.pptx".to_string()]);
    crate::vfs::remove_dir_all(&out).ok();
}
