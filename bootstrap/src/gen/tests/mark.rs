use super::*;

#[test]
fn strip_markers_and_relocate() {
    let text = "// intro\n// req:main-js-5 hash:310b7c2e\n\nfunction trim(line) {\n  return line.trim()\n}\n";
    let (clean, sites) = strip_markers(text);
    assert!(!clean.contains("hash:"), "{}", clean);
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].rid, "req:main-js-5");
    // The head is the next significant line, past the blank one.
    assert_eq!(sites[0].head, "function trim(line) {");
    assert_eq!(sites[0].line, 3);

    // An inline marker beside code is never stripped: the harness must not mangle.
    let (kept, none) = strip_markers("let x = 1 // req:a-1 hash:deadbeef\n");
    assert_eq!(kept, "let x = 1 // req:a-1 hash:deadbeef\n");
    assert!(none.is_empty());

    // Relocation: whitespace-insensitive, nearest to the hint, lost when gone.
    let edited = "// new header\n\nfunction trim(line)   {\n  return line.trim()\n}\n";
    let (line, exact) = locate_head(edited, &sites[0].head, sites[0].line).unwrap();
    assert_eq!(line, 3);
    assert!(exact);
    let moved = "a\nb\nc\nd\nfunction trim(line) {\n";
    assert_eq!(locate_head(moved, &sites[0].head, 3), Some((5, false)));
    assert_eq!(locate_head("nothing here\n", &sites[0].head, 3), None);
}

#[test]
fn qualified_source_markers_anchor_only_their_document_set() {
    let _scope = crate::vfs::Scope::enter(&Default::default());
    let out = Path::new("/project/.jazyk/ds_main");
    let (store, gs) = fixture(out);
    let path = gs.deliverable.join("src/cart.rs");
    crate::vfs::create_dir_all(gs.deliverable.join("src")).unwrap();
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
    crate::vfs::write(&path, "// req:ds_main::shop-1 hash:deadbeef\nfn cart() {}\n// req:ds_other::shop-1 hash:deadbeef\nfn other() {}\n").unwrap();
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        "fn checks_cart() {}\n",
    )
    .unwrap();
    mark(&store, "ent:cart", None, &serde_json::json!({"files":["src/cart.rs"],"tests":[{
        "requirement":"req:shop-1","kind":"programmatic","artifact":"tests/cart.rs","name":"checks_cart","run":"true"
    }]}), &gs).unwrap();
    let ledger = Ledger::load(out);
    let sites = &ledger.requirements["req:shop-1"].sites;
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].head, "fn cart() {}");
    assert_eq!(sites[0].line, 1);
    let source = crate::vfs::read_to_string(path).unwrap();
    assert!(!source.contains("req:ds_main::"));
    assert!(source.contains("req:ds_other::shop-1"));
}

#[test]
fn mark_strips_markers_into_sites() {
    let out = std::env::temp_dir().join(format!("jazyk-sites-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
    let name = test_name("req:shop-1", "The Cart shall hold items.");
    crate::vfs::write(
        gs.deliverable.join("src/cart.rs"),
        "// req:shop-1 hash:12345678\nfn hold(i: Item) {}\n",
    )
    .ok();
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        format!("// req:shop-1 hash:12345678\nfn {}() {{}}\n", name),
    )
    .ok();
    let manifest = serde_json::json!({
        "files": ["src/cart.rs", "tests/cart.rs"],
        "tests": [{
            "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
            "artifact": "tests/cart.rs", "name": name,
            "run": format!("cargo test {}", name), "files": ["src/cart.rs"],
        }],
    });
    mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
    // The written files are clean; the ledger row carries the anchored sites.
    let product = crate::vfs::read_to_string(gs.deliverable.join("src/cart.rs")).unwrap();
    assert_eq!(product, "fn hold(i: Item) {}\n");
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.sites.len(), 2);
    assert_eq!(row.sites[0].file, "src/cart.rs");
    assert_eq!(row.sites[0].line, 1);
    assert_eq!(row.sites[0].head, "fn hold(i: Item) {}");
    // Hashes were computed over the stripped bytes: the row is not stale-code.
    let (status, _) = crate::verify::status_of(&s, "req:shop-1", row, &gs);
    assert_eq!(status, "unverified");
}

// A row whose requirement is a subject of an open error diagnostic: the package
// names the diagnostic, the record flags the row, and the reply says so.
// Mirrors docs/consumers/gen.md#rows-recorded-over-an-open-contradiction.
#[test]
fn a_row_over_an_open_error_diagnostic_is_flagged_contradicted() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-contra-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, gs) = fixture(&out);
    let diag = |severity: &str, lifecycle: &str| Diagnostic {
        goal: None,
        rule: "contradiction".into(),
        severity: severity.into(),
        subjects: vec!["req:shop-1".into(), "req:shop-9".into()],
        message: "21 days versus 30 days".into(),
        reasoning: None,
        lifecycle: lifecycle.into(),
        triage: None,
        prompt: None,
        answer: None,
        created: None,
        updated: None,
    };
    s.graph
        .diagnostics
        .insert("diag:contradiction-1".into(), diag("error", "open"));
    // A resolved error and an open warning on the same subject count for nothing.
    s.graph
        .diagnostics
        .insert("diag:contradiction-2".into(), diag("error", "resolved"));
    s.graph
        .diagnostics
        .insert("diag:contradiction-3".into(), diag("warning", "open"));
    let pkg = task_package(&s, "ent:cart", &gs).unwrap();
    let open = &pkg["requirementGroups"][0][0]["openDiagnostics"];
    assert_eq!(open.as_array().map(|a| a.len()), Some(1), "{}", pkg);
    assert_eq!(open[0]["id"], "diag:contradiction-1");
    assert_eq!(open[0]["rule"], "contradiction");
    assert_eq!(pkg["contradicted"], json!(["req:shop-1"]));

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
    assert_eq!(
        reply["contradicted"]["req:shop-1"],
        json!(["diag:contradiction-1"]),
        "{}",
        reply
    );
    assert_eq!(
        Ledger::load(&out).contradicted["req:shop-1"],
        vec!["diag:contradiction-1".to_string()]
    );
    // The dispute resolves; the next record clears the flag.
    s.graph
        .diagnostics
        .get_mut("diag:contradiction-1")
        .unwrap()
        .lifecycle = "resolved".into();
    let reply = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
    assert!(reply.get("contradicted").is_none(), "{}", reply);
    assert!(Ledger::load(&out).contradicted.is_empty());
}

// The extra files a reply wrote are sorted by what the manifest says, not by the
// order they arrived in: the build's entry and declared support are the
// deliverable's, a second module or test file is the entity's.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
#[test]
fn extra_files_are_classified_by_the_manifest() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-extra-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
    for f in ["src/cart.rs", "src/cart_util.rs", "build.rs", "Cargo.toml"] {
        crate::vfs::write(gs.deliverable.join(f), "// x").ok();
    }
    // The task wrote its part plus three more; the manifest calls two of them the
    // deliverable's (one as support, one as the build's entry).
    mark(
        &s,
        "ent:cart",
        None,
        &serde_json::json!({
            "files": ["src/cart.rs", "src/cart_util.rs"],
            "supportFiles": ["Cargo.toml"],
            "build": {"run": "cargo build --release", "cwd": "."},
            "tests": [],
        }),
        &gs,
    )
    .unwrap();
    let ledger = Ledger::load(&out);
    assert_eq!(ledger.support, vec!["Cargo.toml".to_string()]);
    let mine = &ledger.entities["cart"].files;
    assert!(mine.contains(&"src/cart_util.rs".to_string()), "{:?}", mine);
    assert!(!mine.contains(&"Cargo.toml".to_string()), "{:?}", mine);
    crate::vfs::remove_dir_all(&out).ok();
}

// Fresh obligations can develop source and tests together. Fully current
// adopted rows stay unchanged without needing an entity generation entry.
#[test]
fn pending_diff_and_mark_lifecycle() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    assert_eq!(pending(&s, &gs)[0]["reason"], "incomplete-bindings");
    assert!(skip_reason(&s, &gs, "ent:cart").contains("generation work remains"));

    // Binding finds nothing: the row reads unimplemented and the entity is work.
    crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
    let name = test_name("req:shop-1", "The Cart shall hold items.");
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        format!("// req:shop-1\nfn {}() {{}}", name),
    )
    .ok();
    let bound = json!({"kind": "programmatic", "artifact": "tests/cart.rs", "name": name, "run": format!("cargo test {}", name)});
    crate::bind::record(&s, "req:shop-1", &[], &bound, &json!([]), "fail", None, &gs).unwrap();
    let p = pending(&s, &gs);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0]["reason"], "unimplemented-bindings");
    assert_eq!(p[0]["changed"][0], "req:shop-1");

    // Binding that found the code verified: adopted, nothing to generate.
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// product").ok();
    crate::bind::record(
        &s,
        "req:shop-1",
        &["src/cart.rs".into()],
        &bound,
        &json!([]),
        "pass",
        None,
        &gs,
    )
    .unwrap();
    assert!(
        pending(&s, &gs).is_empty(),
        "adopted code is not generation work"
    );
    assert!(skip_reason(&s, &gs, "ent:cart").contains("no ledger entry"));
    crate::bind::record(&s, "req:shop-1", &[], &bound, &json!([]), "fail", None, &gs).unwrap();

    // A mark with a manifest whose files exist makes it disappear from pending and
    // seeds a verification row.
    let manifest = serde_json::json!({
        "files": ["src/cart.rs", "tests/cart.rs"],
        "tests": [{
            "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
            "artifact": "tests/cart.rs", "name": name,
            "run": format!("cargo test {}", name), "files": ["src/cart.rs"],
        }],
    });
    let r = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
    assert_eq!(r["tests"], 1);
    assert!(pending(&s, &gs).is_empty());
    assert_eq!(skip_reason(&s, &gs, "ent:cart"), "unchanged");
    let ledger = Ledger::load(&out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.verdict, "none");
    assert_eq!(
        row.hashes.requirement,
        hash_hex("The Cart shall hold items.")
    );
    // A red verdict over the generated facts is a finding, never more generation
    // work: generation had its turn (docs/consumers/gen.md#status-is-derived-never-stored).
    crate::bind::record(&s, "req:shop-1", &[], &bound, &json!([]), "fail", None, &gs).unwrap();
    let ledger = Ledger::load(&out);
    let (st, why) =
        crate::verify::status_of(&s, "req:shop-1", &ledger.requirements["req:shop-1"], &gs);
    assert_eq!(
        (st.as_str(), why.as_str()),
        ("failing", "criteria-failing-after-generation")
    );
    assert!(pending(&s, &gs).is_empty(), "generation had its turn");

    // A recorded file deleted by hand is generation work under its own name.
    crate::vfs::remove_file(gs.deliverable.join("src/cart.rs")).unwrap();
    let p = pending(&s, &gs);
    assert_eq!(p[0]["reason"], "files-gone");
    assert_eq!(p[0]["changed"][0], "src/cart.rs");
    let rec = ledger_stale_records(&s, &gs);
    let g = rec.iter().find(|c| c.detail["goal"] == "generate").unwrap();
    assert_eq!(g.detail["reason"], "files-gone");
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "// product").ok();

    // A new requirement reappears as a precise diff.
    let mut s2 = s.clone();
    s2.graph.requirements.insert(
        "req:shop-2".into(),
        Requirement {
            statement: "The Cart shall empty on checkout.".into(),
            entities: vec!["ent:cart".into()],
            edges: vec![],
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop".into(),
                quote: "empty".into(),
            }),
            ..Default::default()
        },
    );
    let p2 = pending(&s2, &gs);
    assert_eq!(p2.len(), 1);
    assert_eq!(p2[0]["reason"], "changed");
    assert_eq!(p2[0]["changed"][0], "req:shop-2 (added)");

    let pkg = task_package(&s2, "ent:cart", &gs).unwrap();
    assert!(pkg["instructions"].as_str().unwrap().contains("manifest"));
    let g0 = pkg["requirementGroups"][0].as_array().unwrap();
    assert_eq!(g0.len(), 2);
    assert!(g0[0]["testName"]
        .as_str()
        .unwrap()
        .starts_with("req_shop_1_"));
}

// The example-sort livelock: rows for deleted requirements had no legal removal.
// Any record buries them. Mirrors docs/consumers/gen.md#deletion-prunes-the-ledger.
#[test]
fn mark_prunes_rows_whose_requirement_left_the_graph() {
    let out = std::env::temp_dir().join(format!("jazyk-prune-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
    let name = test_name("req:shop-1", "The Cart shall hold items.");
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        format!("// req:shop-1\nfn {}() {{}}", name),
    )
    .ok();
    // A leftover row for a requirement the graph no longer holds.
    let mut ledger = Ledger::load(&out);
    let gone: ReqRow = serde_json::from_value(serde_json::json!({
        "entity": "ent:cart",
        "test": {"kind": "programmatic", "label": "unit", "artifact": "tests/gone.rs",
                 "name": "req_gone_1_x", "run": "cargo test req_gone_1_x"},
        "hashes": {"requirement": "x", "test": "x", "files": "x"},
        "verdict": "fail",
    }))
    .unwrap();
    ledger.requirements.insert("req:gone-1".into(), gone);
    ledger.save(&out).unwrap();
    crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "pub struct Cart;\n").ok();
    let manifest = serde_json::json!({
        "files": ["src/cart.rs", "tests/cart.rs"],
        "tests": [{
            "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
            "artifact": "tests/cart.rs", "name": name, "run": format!("cargo test {}", name),
        }],
    });
    let r = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
    assert_eq!(r["prunedRows"], 1);
    let ledger = Ledger::load(&out);
    assert!(!ledger.requirements.contains_key("req:gone-1"));
    assert!(ledger.requirements.contains_key("req:shop-1"));
}

// The record applies the artifact gate binding applies: a row whose artifact is
// not there, or a programmatic row whose artifact lacks the declared name, is
// rejected naming the row and the fix. Mirrors docs/compiler/goals/generate.md#gate.
#[test]
fn record_generation_rejects_a_test_its_artifact_does_not_carry() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-artgate-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
    crate::vfs::write(gs.deliverable.join("cart.rs"), "fn a() {}\n").unwrap();
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        "fn something_else() {}\n",
    )
    .unwrap();
    let name = test_name("req:shop-1", "The Cart shall hold items.");
    let absent = json!({"files": ["cart.rs"], "tests": [{
        "requirement": "req:shop-1", "kind": "programmatic", "artifact": "tests/cart.rs",
        "name": name, "run": format!("cargo test {}", name)}]});
    let err = mark(&s, "ent:cart", None, &absent, &gs).unwrap_err();
    assert!(
        err.contains("does not contain that name") && err.contains(&name),
        "{}",
        err
    );
    let gone = json!({"files": ["cart.rs"], "tests": [{
        "requirement": "req:shop-1", "kind": "programmatic", "artifact": "tests/nope.rs",
        "name": name, "run": format!("cargo test {}", name)}]});
    let err = mark(&s, "ent:cart", None, &gone, &gs).unwrap_err();
    assert!(err.contains("no such file"), "{}", err);
    // A row without its requirement is refused by name, never dropped in silence.
    let unnamed = json!({"files": ["cart.rs"], "tests": [{
        "kind": "programmatic", "artifact": "tests/cart.rs", "name": "req_shop_1", "run": "cargo test req_shop_1"}]});
    let err = mark(&s, "ent:cart", None, &unnamed, &gs).unwrap_err();
    assert!(err.contains("`req_shop_1` names no requirement"), "{}", err);
    let no_criteria = json!({"files": ["cart.rs"], "tests": [{
        "requirement": "req:shop-1", "kind": "llm", "artifact": "criteria/req-shop-1.md", "name": name}]});
    let err = mark(&s, "ent:cart", None, &no_criteria, &gs).unwrap_err();
    assert!(err.contains("gen/"), "{}", err);
    assert!(mark(
        &s,
        "ent:cart",
        None,
        &json!({"files": ["cart.rs"], "tests": [{
        "requirement": "req:shop-1", "kind": "oracle", "artifact": "x", "name": "y", "run": "z"}]}),
        &gs
    )
    .unwrap_err()
    .contains("`programmatic`"));
    // The criteria file under the out directory satisfies an llm row.
    crate::vfs::create_dir_all(out.join("gen/criteria")).unwrap();
    crate::vfs::write(
        out.join("gen/criteria/req-shop-1.md"),
        format!(
            "{}\nInspect the product. PASS when satisfied. FAIL otherwise.\n",
            criteria_front_matter(
                "req:shop-1",
                None,
                &s.graph.requirements["req:shop-1"].statement
            )
        ),
    )
    .unwrap();
    mark(&s, "ent:cart", None, &no_criteria, &gs).unwrap();
    crate::vfs::remove_dir_all(&out).ok();
}

#[test]
fn qualified_report_selectors_record_and_stay_bound() {
    let out = std::env::temp_dir().join(format!(
        "jazyk-qualified-report-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    let (store, gs) = fixture(&out);
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
    crate::vfs::write(gs.deliverable.join("cart.rs"), "fn cart() {}\n").unwrap();
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        "fn qualified_case() {}\n",
    )
    .unwrap();
    let mut manifest = json!({"files":["cart.rs"],"tests":[{
        "requirement":"req:shop-1","kind":"programmatic","artifact":"tests/cart.rs",
        "name":"Suite::qualified_case","run":"task:check"
    }]});
    mark(&store, "ent:cart", None, &manifest, &gs).unwrap();
    assert!(crate::bind::pending(&store, &gs).is_empty());
    manifest["tests"][0]["name"] = json!("Suite::");
    assert!(mark(&store, "ent:cart", None, &manifest, &gs).is_err());
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        "fn renamed_case() {}\n",
    )
    .unwrap();
    assert_eq!(
        crate::bind::pending(&store, &gs)[0]["reason"],
        "artifact-gone"
    );
    crate::vfs::remove_dir_all(out).ok();
}

// A record names the files that realize the entity's requirements: files that
// hold nothing but tests are refused naming the requirements nothing carries,
// while an entity whose requirement another entity's files carry records
// files: [] and its row keeps those files.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
#[test]
fn a_record_with_no_implementing_file_is_refused_unless_carried() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-no-impl-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, gs) = fixture(&out);
    s.graph.entities.insert(
        "ent:buyer".into(),
        Entity {
            name: "Buyer".into(),
            ..Default::default()
        },
    );
    s.graph.requirements.get_mut("req:shop-1").unwrap().entities =
        vec!["ent:cart".into(), "ent:buyer".into()];
    crate::vfs::create_dir_all(gs.deliverable.join("src")).unwrap();
    crate::vfs::create_dir_all(gs.deliverable.join("tests/criteria")).unwrap();
    crate::vfs::write(
        gs.deliverable.join("tests/criteria/req-shop-1.md"),
        format!(
            "{}\nInspect the product. PASS when satisfied. FAIL otherwise.\n",
            criteria_front_matter(
                "req:shop-1",
                None,
                &s.graph.requirements["req:shop-1"].statement
            )
        ),
    )
    .unwrap();
    let row = json!({"requirement": "req:shop-1", "kind": "llm", "label": "judged",
                     "artifact": "tests/criteria/req-shop-1.md", "name": "req_shop_1", "run": "llm"});
    // Nothing but the criteria file: refused, the requirement named.
    for files in [json!([]), json!(["tests/criteria/req-shop-1.md"])] {
        let err = mark(
            &s,
            "ent:cart",
            None,
            &json!({"files": files, "tests": [row.clone()]}),
            &gs,
        )
        .unwrap_err();
        assert!(err.contains("req:shop-1"), "{}", err);
        assert!(err.contains("not implementation"), "{}", err);
    }
    assert!(
        Ledger::load(&out).entities.is_empty(),
        "a refusal records nothing"
    );
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "pub struct Cart;\n").unwrap();
    mark(
        &s,
        "ent:cart",
        None,
        &json!({"files": ["src/cart.rs", "tests/criteria/req-shop-1.md"], "tests": [row.clone()]}),
        &gs,
    )
    .unwrap();
    assert_eq!(
        Ledger::load(&out).requirements["req:shop-1"].files,
        vec!["src/cart.rs".to_string()],
        "a test artifact is never a row's implementing file"
    );
    // The buyer's one requirement is carried by the cart's file: files: [] stands,
    // and listing the shared row unchanged keeps the file that carries it.
    mark(
        &s,
        "ent:buyer",
        None,
        &json!({"files": [], "tests": [row]}),
        &gs,
    )
    .unwrap();
    let ledger = Ledger::load(&out);
    assert!(ledger.entities["buyer"].files.is_empty());
    assert_eq!(
        ledger.requirements["req:shop-1"].files,
        vec!["src/cart.rs".to_string()]
    );
    assert!(gate(&s, &gs, "ent:buyer", false).is_ok());
    crate::vfs::remove_dir_all(&out).ok();
}

// A regeneration replaces the file set through the record itself, whichever
// worker records: what the previous record listed and this manifest omits is
// removed (snapshotted first), while a file another entity records, a support
// file, or a row's test artifact stays.
// Mirrors docs/consumers/gen.md#incremental-regeneration.
#[test]
fn a_record_removes_the_files_its_manifest_omits() {
    let out = std::env::temp_dir().join(format!("jazyk-gen-remove-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, gs) = fixture(&out);
    // Shelf is part of Cart, one component group under the Shop system, so the
    // two share their files.
    s.graph.entities.insert(
        "ent:shop".into(),
        Entity {
            name: "Shop".into(),
            ..Default::default()
        },
    );
    s.graph.entities.get_mut("ent:cart").unwrap().parent = Some("ent:shop".into());
    s.graph.entities.insert(
        "ent:shelf".into(),
        Entity {
            name: "Shelf".into(),
            parent: Some("ent:cart".into()),
            ..Default::default()
        },
    );
    crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
    for f in [
        "src/cart.rs",
        "src/cart_old.rs",
        "src/shared.rs",
        "Cargo.toml",
        "src/cart_v2.rs",
    ] {
        crate::vfs::write(gs.deliverable.join(f), format!("// {}\n", f)).unwrap();
    }
    let name = test_name("req:shop-1", "The Cart shall hold items.");
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        format!("fn {}() {{}}\n", name),
    )
    .unwrap();
    let row = json!({
        "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
        "artifact": "tests/cart.rs", "name": name, "run": format!("cargo test {}", name),
    });
    mark(
        &s,
        "ent:shelf",
        None,
        &json!({"files": ["src/shared.rs"], "tests": []}),
        &gs,
    )
    .unwrap();
    mark(
        &s,
        "ent:cart",
        None,
        &json!({
            "files": ["src/cart.rs", "src/cart_old.rs", "src/shared.rs", "tests/cart.rs"],
            "supportFiles": ["Cargo.toml"], "tests": [row.clone()],
        }),
        &gs,
    )
    .unwrap();
    // The second record drops cart_old, shared (the other entity's too), and the
    // support file from its list.
    let r = mark(
        &s,
        "ent:cart",
        None,
        &json!({
            "files": ["src/cart_v2.rs", "tests/cart.rs"], "tests": [row],
        }),
        &gs,
    )
    .unwrap();
    assert_eq!(
        r["removed"],
        json!(["src/cart.rs", "src/cart_old.rs"]),
        "{}",
        r
    );
    assert!(!gs.deliverable.join("src/cart_old.rs").vexists());
    assert!(!gs.deliverable.join("src/cart.rs").vexists());
    assert!(
        gs.deliverable.join("src/shared.rs").vexists(),
        "another entity records it"
    );
    assert!(
        gs.deliverable.join("Cargo.toml").vexists(),
        "support files are the deliverable's"
    );
    assert!(gs.deliverable.join("tests/cart.rs").vexists());
    assert!(
        crate::layout::deliverable_baseline_dir(&out)
            .join("src/cart_old.rs")
            .vexists(),
        "snapshotted before removal"
    );
    crate::vfs::remove_dir_all(&out).ok();
}

// One invented-choice diagnostic per manifest entry, graded by the scope of the
// invention, with the proposal prompt and the measure in the message.
// Mirrors docs/consumers/gen.md#invented-choices.
#[test]
fn invented_choices_stage_one_graded_diagnostic_per_scope() {
    let out = std::env::temp_dir().join(format!("jazyk-choice-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (mut s, gs) = fixture(&out);
    s.docs.insert(
        "shop.md".into(),
        DocRecord {
            content_hash: hash_hex("# Shop\n\nholds\n"),
            sections: crate::md::parse_sections("# Shop\n\nholds\n"),
            coverage: Default::default(),
        },
    );
    s.graph.requirements.insert(
        "req:shop-2".into(),
        Requirement {
            statement: "The Cart shall print items.".into(),
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
    crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
    crate::vfs::write(gs.deliverable.join("src/cart.rs"), "fn hold() {}\n").ok();
    // Distinct subject sets: the store's sticky rule merges open diagnostics
    // with one rule and one subject set, so the requirements a choice fills in
    // are what keep its diagnostic its own.
    let manifest = serde_json::json!({
        "files": ["src/cart.rs"],
        "tests": [],
        "choices": [
            {"choice": "The product is a command-line cart simulator.", "scope": "product",
             "reasoning": "no statement names the medium"},
            {"choice": "The cart rejects a 21st item.", "scope": "behavior",
             "reasoning": "no stated limit", "requirements": ["req:shop-1"]},
            {"choice": "Items print in green.", "scope": "detail", "reasoning": "cosmetic",
             "requirements": ["req:shop-2"]},
        ],
    });
    let reply = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
    assert_eq!(reply["choices"], 3);
    let choices = parse_choices(&s, &manifest).unwrap();
    let ledger = Ledger::load(&out);
    let ops = choice_ops(
        &s,
        "ent:cart",
        &choices,
        ledger.entities["cart"].unattached.as_ref(),
    );
    assert_eq!(ops.len(), 3);
    let report = s.apply(ops, &crate::store::Commit::store("session"));
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let staged: Vec<&Diagnostic> = s
        .graph
        .diagnostics
        .values()
        .filter(|d| d.rule == "invented-choice")
        .collect();
    assert_eq!(staged.len(), 3);
    for want in ["error", "warning", "info"] {
        assert!(staged.iter().any(|d| d.severity == want), "{}", want);
    }
    let behavior = staged.iter().find(|d| d.severity == "warning").unwrap();
    assert!(behavior.subjects.contains(&"ent:cart".to_string()));
    assert!(behavior.subjects.contains(&"req:shop-1".to_string()));
    assert!(
        behavior.message.contains("Unattached remainder"),
        "{}",
        behavior.message
    );
    let p = behavior.prompt.as_ref().unwrap();
    let edit = p.options[0].edit.as_ref().unwrap();
    assert_eq!(edit.doc, "shop.md");
    assert_eq!(edit.section, "/shop");
    assert_eq!(edit.old_text, "");
    assert_eq!(edit.new_text, "The cart rejects a 21st item.");
    assert_eq!(p.options[1].answer.as_deref(), Some("keep unstated"));
    assert!(p.freeform);
    // A repeated choice keeps its diagnostic; an omitted one resolves.
    let again = parse_choices(
        &s,
        &serde_json::json!({"choices": [
            {"choice": "The cart rejects a 21st item.", "scope": "behavior", "reasoning": "still unstated"},
        ]}),
    )
    .unwrap();
    let ops = choice_ops(&s, "ent:cart", &again, None);
    let reports = ops
        .iter()
        .filter(|o| matches!(o, crate::store::Op::ReportDiagnostic { .. }))
        .count();
    let resolves = ops
        .iter()
        .filter(|o| matches!(o, crate::store::Op::ResolveDiagnostic { .. }))
        .count();
    assert_eq!(reports, 0);
    assert_eq!(resolves, 2);
    // A bad scope is rejected before any side effect.
    let bad = serde_json::json!({"files": [], "tests": [], "choices": [{"choice": "x", "scope": "huge"}]});
    assert!(mark(&s, "ent:cart", None, &bad, &gs)
        .unwrap_err()
        .contains("scope"));
    crate::vfs::remove_dir_all(&out).ok();
}

// The unattached remainder on a two-file fixture: one owned file no row names,
// and the significant lines outside every site's run.
// Mirrors docs/consumers/gen.md#the-unattached-remainder.
#[test]
fn unattached_remainder_counts_unnamed_files_and_lines_outside_site_runs() {
    let out = std::env::temp_dir().join(format!("jazyk-unattached-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let (s, gs) = fixture(&out);
    crate::vfs::create_dir_all(gs.deliverable.join("src")).ok();
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).ok();
    // Two significant lines sit before the first site's run.
    crate::vfs::write(
        gs.deliverable.join("src/cart.rs"),
        "// header comment\nuse std::fmt;\nfn helper() {}\n// req:shop-1 hash:12345678\nfn hold(i: Item) {}\nfn hold_more() {}\n",
    )
    .ok();
    // A second owned file no requirement row names: unattached whole.
    crate::vfs::write(
        gs.deliverable.join("src/extra.rs"),
        "fn extra() {}\n\n// note\nfn more() {}\n",
    )
    .ok();
    let name = test_name("req:shop-1", "The Cart shall hold items.");
    crate::vfs::write(
        gs.deliverable.join("tests/cart.rs"),
        format!("fn {}() {{}}\n", name),
    )
    .ok();
    let manifest = serde_json::json!({
        "files": ["src/cart.rs", "src/extra.rs", "tests/cart.rs"],
        "tests": [{
            "requirement": "req:shop-1", "kind": "programmatic", "label": "unit",
            "artifact": "tests/cart.rs", "name": name,
            "run": format!("cargo test {}", name), "files": ["src/cart.rs"],
        }],
    });
    let reply = mark(&s, "ent:cart", None, &manifest, &gs).unwrap();
    // src/extra.rs is the file slice; the lines are its two significant lines
    // plus src/cart.rs's two before the first site. The test artifact is claimed
    // by its row and never counts.
    assert_eq!(reply["unattached"]["files"], 1);
    assert_eq!(reply["unattached"]["lines"], 4);
    let ledger = Ledger::load(&out);
    let u = ledger.entities["cart"].unattached.as_ref().unwrap();
    assert_eq!(u.files, 1);
    assert_eq!(u.lines, 4);
    assert!(u.ratio > 0.5 && u.ratio < 1.0, "{}", u.ratio);
    crate::vfs::remove_dir_all(&out).ok();
}
